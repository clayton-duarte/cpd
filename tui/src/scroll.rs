//! The one scroll and wrap module of the views (tui-G1): a scroll position that follows the end until the User scrolls,
//! with the keys and the wheel that move it ([`Scroll`]); and a body of styled lines wrapped to the width it is drawn
//! at ([`Scroller`], the Worker and history views). The Event log scrolls its rows with a [`Scroll`] too.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// The rows one turn of the mouse wheel scrolls.
const WHEEL: usize = 3;
/// The indent of a continuation row made by [`wrap`].
const INDENT: usize = 6;

/// Where a view of `rows` rows, `height` of them on screen, is: at row `top`, or following the end.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scroll {
    /// The first row shown; [`usize::MAX`] follows the end.
    top: usize,
    rows: usize,
    height: usize,
}

impl Default for Scroll {
    fn default() -> Scroll {
        Scroll { top: usize::MAX, rows: 0, height: 0 }
    }
}

impl Scroll {
    /// Sets the size, as of the last draw. A view that shows the end follows it from now on.
    pub fn fit(&mut self, rows: usize, height: usize) {
        (self.rows, self.height) = (rows, height);
        if self.top >= self.max() {
            self.top = usize::MAX;
        }
    }

    fn max(&self) -> usize {
        self.rows.saturating_sub(self.height)
    }

    /// The first row shown.
    pub fn top(&self) -> usize {
        self.top.min(self.max())
    }

    /// Whether the view shows the end, and so follows new rows.
    pub fn follows(&self) -> bool {
        self.top == usize::MAX
    }

    /// The rows on screen.
    pub fn height(&self) -> usize {
        self.height
    }

    /// The rows on screen: a page.
    pub fn page(&self) -> usize {
        self.height.max(1)
    }

    /// Moves `by` rows from where the view is; reaching the end follows it.
    pub fn by(&mut self, by: isize) {
        self.to(self.top().saturating_add_signed(by));
    }

    /// Shows from row `top` on; reaching the end follows it.
    pub fn to(&mut self, top: usize) {
        self.top = if top >= self.max() { usize::MAX } else { top };
    }

    /// Up and `k`, Down and `j`, PageUp, PageDown scroll; End and `G` follow the end again.
    pub fn key(&mut self, code: KeyCode) {
        let page = self.page() as isize;
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.by(1),
            KeyCode::PageUp => self.by(-page),
            KeyCode::PageDown => self.by(page),
            KeyCode::End | KeyCode::Char('G') => self.top = usize::MAX,
            _ => {}
        }
    }

    /// The wheel moves [`WHEEL`] rows.
    pub fn mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollUp => self.by(-(WHEEL as isize)),
            MouseEventKind::ScrollDown => self.by(WHEEL as isize),
            _ => {}
        }
    }
}

/// Styled lines, scrolled, following the end until the User scrolls.
pub(crate) struct Scroller {
    lines: Vec<Line<'static>>,
    /// `lines` wrapped to the width they were last drawn at.
    wrapped: Option<(u16, Vec<Line<'static>>)>,
    scroll: Scroll,
}

impl Scroller {
    pub fn new() -> Scroller {
        Scroller { lines: Vec::new(), wrapped: None, scroll: Scroll::default() }
    }

    pub fn set_lines(&mut self, lines: Vec<Line<'static>>) {
        self.lines = lines;
        self.wrapped = None;
    }

    /// Whether the view ends: `q` and Esc end it; the other keys are [`Scroll::key`]'s.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
            return true;
        }
        self.scroll.key(key.code);
        false
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        self.scroll.mouse(m);
    }

    /// Draws the rows in `body`, wrapped to its width.
    pub fn draw(&mut self, f: &mut Frame, body: Rect) {
        if self.wrapped.as_ref().is_none_or(|(width, _)| *width != body.width) {
            self.wrapped = Some((body.width, wrap(&self.lines, usize::from(body.width))));
        }
        let rows = &self.wrapped.as_ref().expect("wrapped above").1;
        self.scroll.fit(rows.len(), usize::from(body.height));
        let shown = rows.iter().skip(self.scroll.top()).take(usize::from(body.height)).cloned().collect::<Vec<_>>();
        f.render_widget(Paragraph::new(shown), body);
    }
}

/// `lines` broken into rows of at most `width` characters, a continuation row
/// indented by [`INDENT`] (nothing when the pane is narrower than twice that).
pub(crate) fn wrap(lines: &[Line<'static>], width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let indent = if width >= 2 * INDENT { INDENT } else { 0 };
    let mut rows = Vec::new();
    for line in lines {
        let cells: Vec<(char, Style)> =
            line.spans.iter().flat_map(|s| s.content.chars().map(|c| (c, line.style.patch(s.style)))).collect();
        if cells.len() <= width {
            rows.push(line.clone());
            continue;
        }
        let (first, rest) = cells.split_at(width);
        rows.push(row(0, first));
        rows.extend(rest.chunks(width - indent).map(|chunk| row(indent, chunk)));
    }
    rows
}

/// One row of styled characters after `indent` blanks, neighbours of one style merged into one span.
fn row(indent: usize, cells: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = if indent > 0 { vec![Span::raw(" ".repeat(indent))] } else { Vec::new() };
    for &(c, style) in cells {
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push(c),
            _ => spans.push(Span::styled(c.to_string(), style)),
        }
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use ratatui::style::Stylize;

    use super::*;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn long_lines_wrap_with_the_body_indent() {
        let lines = vec![Line::from(vec!["ab".bold(), "cdefghijklmnopqrstuvwxyz".into()])];
        let rows: Vec<String> = wrap(&lines, 16).iter().map(text).collect();
        assert_eq!(rows, ["abcdefghijklmnop", "      qrstuvwxyz"]);
        let wrapped = wrap(&lines, 16);
        assert!(wrapped[0].spans[0].style.add_modifier.contains(ratatui::style::Modifier::BOLD), "styles survive");
        assert_eq!(wrap(&lines, 40).iter().map(text).collect::<Vec<_>>(), ["abcdefghijklmnopqrstuvwxyz"]);
        let narrow: Vec<String> = wrap(&lines, 8).iter().map(text).collect();
        assert_eq!(narrow, ["abcdefgh", "ijklmnop", "qrstuvwx", "yz"], "no indent in a narrow pane");
    }

    /// A position over 10 rows, 4 on screen.
    fn scroll() -> Scroll {
        let mut s = Scroll::default();
        s.fit(10, 4);
        s
    }

    #[test]
    fn a_position_follows_the_end_until_it_moves_up_and_follows_again_at_the_end() {
        let mut s = scroll();
        assert!((s.follows(), s.top()) == (true, 6));
        s.key(KeyCode::Up);
        assert!((s.follows(), s.top()) == (false, 5));
        s.fit(11, 4);
        assert_eq!(s.top(), 5, "a new row does not move it");
        s.key(KeyCode::PageUp);
        s.key(KeyCode::PageUp);
        assert_eq!(s.top(), 0, "it stops at the top");
        s.key(KeyCode::Char('j'));
        s.key(KeyCode::PageDown);
        assert_eq!(s.top(), 5);
        s.key(KeyCode::Char('G'));
        assert!(s.follows());
        s.key(KeyCode::Down);
        assert!(s.follows(), "it stops at the end");
    }
}
