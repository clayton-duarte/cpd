//! The wrapped, scrolled body of the history view ([`Scroller`]), and the wrapping it draws with
//! ([`wrap`]). The scroll position is the dialog kit's [`Scroll`], the one scroll module of the views (tui-G1, hs-d297);
//! the Event log scrolls with it too.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::cells;
use crate::dialog::scroll::Scroll;

/// The indent of a continuation row made by [`wrap`].
const INDENT: usize = 6;

/// Styled lines, scrolled, following the end until the User scrolls.
pub(crate) struct Scroller {
    lines: Vec<Line<'static>>,
    /// `lines` wrapped to the width they were last drawn at.
    wrapped: Option<(u16, Vec<Line<'static>>)>,
    scroll: Scroll,
    /// The view shows the end, so new lines scroll in; scrolling decides it again (at the end it follows).
    follow: bool,
}

impl Scroller {
    pub fn new() -> Scroller {
        Scroller {
            lines: Vec::new(),
            wrapped: None,
            scroll: Scroll::default(),
            follow: true,
        }
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
        self.scroll.key(key);
        self.settle();
        false
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        self.scroll.mouse(m);
        self.settle();
    }

    /// Whether the view shows the end, and so follows new lines.
    #[cfg(test)]
    pub fn follows(&self) -> bool {
        self.follow
    }

    /// Scrolling decides whether the view follows: it does at the end.
    fn settle(&mut self) {
        self.follow = self.scroll.offset() == self.scroll.max_offset();
    }

    /// Draws the rows in `body`, wrapped to its width.
    pub fn draw(&mut self, f: &mut Frame, body: Rect) {
        if self
            .wrapped
            .as_ref()
            .is_none_or(|(width, _)| *width != body.width)
        {
            self.wrapped = Some((body.width, wrap(&self.lines, usize::from(body.width))));
        }
        let rows = &self.wrapped.as_ref().expect("wrapped above").1;
        self.scroll.resize(rows.len(), usize::from(body.height));
        if self.follow {
            self.scroll.by(isize::MAX);
        }
        let shown = rows
            .iter()
            .skip(self.scroll.offset())
            .take(usize::from(body.height))
            .cloned()
            .collect::<Vec<_>>();
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
        let styled: Vec<(char, Style)> = line
            .spans
            .iter()
            .flat_map(|s| s.content.chars().map(|c| (c, line.style.patch(s.style))))
            .collect();
        let columns: usize = styled
            .iter()
            .map(|(c, _)| cells::width(c.encode_utf8(&mut [0; 4])))
            .sum();
        if columns <= width {
            rows.push(line.clone());
            continue;
        }
        let mut start = 0;
        let mut first = true;
        while start < styled.len() {
            let end = chunk_end(&styled, start, if first { width } else { width - indent });
            rows.push(row(if first { 0 } else { indent }, &styled[start..end]));
            start = end;
            first = false;
        }
    }
    rows
}

/// The end index of the row of `styled` starting at `start` that fits `columns` display columns, at least one
/// character: a wide character that does not fit goes to the next row, never split in two.
fn chunk_end(styled: &[(char, Style)], start: usize, columns: usize) -> usize {
    let mut used = 0;
    let mut end = start;
    while end < styled.len() {
        let w = cells::width(styled[end].0.encode_utf8(&mut [0; 4]));
        if used + w > columns {
            break;
        }
        used += w;
        end += 1;
    }
    if end == start { start + 1 } else { end }
}

/// One row of styled characters after `indent` blanks, neighbours of one style merged into one span.
fn row(indent: usize, cells: &[(char, Style)]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = if indent > 0 {
        vec![Span::raw(" ".repeat(indent))]
    } else {
        Vec::new()
    };
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
        let lines = vec![Line::from(vec![
            "ab".bold(),
            "cdefghijklmnopqrstuvwxyz".into(),
        ])];
        let rows: Vec<String> = wrap(&lines, 16).iter().map(text).collect();
        assert_eq!(rows, ["abcdefghijklmnop", "      qrstuvwxyz"]);
        let wrapped = wrap(&lines, 16);
        assert!(
            wrapped[0].spans[0]
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD),
            "styles survive"
        );
        assert_eq!(
            wrap(&lines, 40).iter().map(text).collect::<Vec<_>>(),
            ["abcdefghijklmnopqrstuvwxyz"]
        );
        let narrow: Vec<String> = wrap(&lines, 8).iter().map(text).collect();
        assert_eq!(
            narrow,
            ["abcdefgh", "ijklmnop", "qrstuvwx", "yz"],
            "no indent in a narrow pane"
        );
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, ratatui::crossterm::event::KeyModifiers::NONE)
    }

    /// The rows a 4-row body shows of `n` one-row lines.
    fn drawn(s: &mut Scroller, n: usize) -> Vec<String> {
        s.set_lines((0..n).map(|i| Line::from(format!("line {i}"))).collect());
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 4)).unwrap();
        terminal.draw(|f| s.draw(f, f.area())).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..4)
            .map(|y| {
                (0..20)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// tui-G1: the body follows the end until it moves up, and follows again at the end (End or G).
    #[test]
    fn the_body_follows_the_end_until_it_moves_up_and_follows_again_at_the_end() {
        let mut s = Scroller::new();
        assert_eq!(drawn(&mut s, 10)[3], "line 9");
        assert!(!s.key(press(KeyCode::Up)));
        assert!(!s.follows());
        assert_eq!(
            drawn(&mut s, 11)[3],
            "line 8",
            "a new line does not move it"
        );
        s.key(press(KeyCode::Char('G')));
        assert!(s.follows());
        assert_eq!(drawn(&mut s, 12)[3], "line 11");
        s.key(press(KeyCode::PageUp));
        s.key(press(KeyCode::End));
        assert!(s.follows(), "End follows again");
        assert!(s.key(press(KeyCode::Char('q'))), "q ends the view");
    }
}
