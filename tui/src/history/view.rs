//! The history view's state, keys and drawing: the lines of the Session's
//! file under a top line, scrolled like the Worker view's (`worker/view.rs`).

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::worker::render::wrap;

/// The rows one turn of the mouse wheel scrolls.
const WHEEL: usize = 3;

/// The top line: `<name> (archived, read-only)`, or `<name> (read-only)` for a Session that is not archived.
pub(super) fn top_line(name: &str, archived: bool) -> String {
    if archived { format!("{name} (archived, read-only)") } else { format!("{name} (read-only)") }
}

pub(super) struct View {
    title: String,
    lines: Vec<Line<'static>>,
    /// `lines` wrapped to the width they were last drawn at.
    wrapped: Option<(u16, Vec<Line<'static>>)>,
    /// The first row shown; [`usize::MAX`] follows the end.
    scroll: usize,
    /// The rows and the body height of the last draw.
    rows: usize,
    height: usize,
}

impl View {
    pub fn new(name: &str, archived: bool) -> View {
        View { title: top_line(name, archived), lines: Vec::new(), wrapped: None, scroll: usize::MAX, rows: 0, height: 0 }
    }

    pub fn set_lines(&mut self, lines: Vec<Line<'static>>) {
        self.lines = lines;
        self.wrapped = None;
    }

    /// Whether the view ends. Up and `k`, Down and `j`, PageUp, PageDown scroll; End and `G` follow the end again.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        let page = self.height;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-(page as isize)),
            KeyCode::PageDown => self.scroll_by(page as isize),
            KeyCode::End | KeyCode::Char('G') => self.scroll = usize::MAX,
            _ => {}
        }
        false
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollUp => self.scroll_by(-(WHEEL as isize)),
            MouseEventKind::ScrollDown => self.scroll_by(WHEEL as isize),
            _ => {}
        }
    }

    fn max(&self) -> usize {
        self.rows.saturating_sub(self.height)
    }

    /// Moves `by` rows from where the view is; reaching the end follows it.
    fn scroll_by(&mut self, by: isize) {
        let top = self.scroll.min(self.max()).saturating_add_signed(by);
        self.scroll = if top >= self.max() { usize::MAX } else { top };
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(f.area());
        if self.wrapped.as_ref().is_none_or(|(width, _)| *width != body.width) {
            self.wrapped = Some((body.width, wrap(&self.lines, usize::from(body.width))));
        }
        let rows = &self.wrapped.as_ref().expect("wrapped above").1;
        self.rows = rows.len();
        self.height = usize::from(body.height);
        let top = self.scroll.min(self.max());
        f.render_widget(Paragraph::new(Line::from(self.title.as_str()).bold()), head);
        f.render_widget(Paragraph::new(rows.iter().skip(top).take(self.height).cloned().collect::<Vec<_>>()), body);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyEventState, KeyModifiers};

    use super::*;
    use crate::worker::render::render;

    fn lines(n: usize) -> Vec<Line<'static>> {
        (1..=n).map(|i| Line::from(format!("line {i}"))).collect()
    }

    fn view(n: usize) -> View {
        let mut view = View::new("lead", true);
        view.set_lines(lines(n));
        view
    }

    fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.draw(f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn screen(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned())
            .collect()
    }

    fn press(view: &mut View, code: KeyCode) -> bool {
        view.key(KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE })
    }

    fn wheel(view: &mut View, kind: MouseEventKind) {
        view.mouse(MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE });
    }

    #[test]
    fn the_top_line_names_the_session_and_says_it_is_read_only() {
        assert_eq!(top_line("pay", true), "pay (archived, read-only)");
        assert_eq!(top_line("pay", false), "pay (read-only)");
    }

    #[test]
    fn the_top_line_stays_while_the_body_starts_at_the_end() {
        let mut v = view(10);
        assert_eq!(screen(&draw(&mut v, 30, 4)), ["lead (archived, read-only)", "line 8", "line 9", "line 10"]);
    }

    #[test]
    fn a_fixture_session_file_is_drawn_under_the_top_line() {
        let mut v = View::new("Plan lead", true);
        v.set_lines(render(include_str!("../worker/fixtures/session.jsonl")));
        let rows = screen(&draw(&mut v, 200, 30));
        assert_eq!(rows[0], "Plan lead (archived, read-only)");
        assert_eq!(rows[1..=23].to_vec(), render(include_str!("../worker/fixtures/session.jsonl")).iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_owned()).collect::<Vec<_>>());
    }

    #[test]
    fn scrolling_leaves_the_end_and_reaching_it_follows_again() {
        let mut v = view(10);
        draw(&mut v, 24, 4);
        press(&mut v, KeyCode::Up);
        assert_eq!(screen(&draw(&mut v, 24, 4))[1..], ["line 7", "line 8", "line 9"]);
        press(&mut v, KeyCode::Char('k'));
        press(&mut v, KeyCode::PageUp);
        press(&mut v, KeyCode::PageUp);
        assert_eq!(screen(&draw(&mut v, 24, 4))[1], "line 1", "it stops at the top");
        wheel(&mut v, MouseEventKind::ScrollDown);
        assert_eq!(screen(&draw(&mut v, 24, 4))[1], "line 4");
        press(&mut v, KeyCode::Char('G'));
        assert_eq!(screen(&draw(&mut v, 24, 4))[3], "line 10");
        wheel(&mut v, MouseEventKind::ScrollUp);
        assert_eq!(screen(&draw(&mut v, 24, 4))[1], "line 5", "the wheel moves 3 rows");
        press(&mut v, KeyCode::PageDown);
        press(&mut v, KeyCode::Char('j'));
        assert_eq!(screen(&draw(&mut v, 24, 4))[3], "line 10", "Down to the end follows it");
        press(&mut v, KeyCode::Char('k'));
        press(&mut v, KeyCode::End);
        assert_eq!(screen(&draw(&mut v, 24, 4))[3], "line 10", "End follows the end");
    }

    #[test]
    fn only_q_and_escape_end_the_view() {
        let mut v = view(1);
        assert!(!press(&mut v, KeyCode::Char('x')) && !press(&mut v, KeyCode::Enter));
        assert!(press(&mut v, KeyCode::Char('q')) && press(&mut v, KeyCode::Esc));
    }
}
