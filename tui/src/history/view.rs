//! The history view's state, keys and drawing: the lines of the Session's
//! file under a top line, scrolled with the kit's `Scroller`.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::{Constraint, Layout, Margin};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::scroll::Scroller;

/// The top line: `<name> (archived, read-only)`, or `<name> (read-only)` for a Session that is not archived.
pub(super) fn top_line(name: &str, archived: bool) -> String {
    if archived { format!("{name} (archived, read-only)") } else { format!("{name} (read-only)") }
}

pub(super) struct View {
    title: String,
    body: Scroller,
}

impl View {
    pub fn new(name: &str, archived: bool) -> View {
        View { title: top_line(name, archived), body: Scroller::new() }
    }

    pub fn set_lines(&mut self, lines: Vec<Line<'static>>) {
        self.body.set_lines(lines);
    }

    /// Whether the view ends (see [`Scroller::key`]).
    pub fn key(&mut self, key: KeyEvent) -> bool {
        self.body.key(key)
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        self.body.mouse(m);
    }

    pub fn draw(&mut self, f: &mut Frame) {
        // The chat slot's uniform 1-cell pad insets the whole view: the title sits at the top
        // of the inset, the body below it, and the pane's first and last row and column stay blank.
        let area = f.area().inner(Margin { horizontal: 1, vertical: 1 });
        let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        f.render_widget(Paragraph::new(Line::styled(self.title.as_str(), crate::palette::read_only_text().add_modifier(ratatui::style::Modifier::BOLD))), head);
        self.body.draw(f, body);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers, MouseEventKind};

    use super::*;
    use crate::test_text::rows;
    use crate::history::render::render;

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
        assert_eq!(rows(&draw(&mut v, 30, 5)), ["", " lead (archived, read-only)", " line 9", " line 10", ""]);
    }

    #[test]
    fn a_fixture_session_file_is_drawn_under_the_top_line() {
        let mut v = View::new("Plan lead", true);
        v.set_lines(render(include_str!("fixtures/session.jsonl"), &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(&cpd_core::clock::ManualClock::new())));
        let rows = rows(&draw(&mut v, 200, 30));
        assert_eq!(rows[0], "", "the pad's first row is blank");
        assert_eq!(rows[1], " Plan lead (archived, read-only)");
        assert_eq!(rows[2..=24].to_vec(), render(include_str!("fixtures/session.jsonl"), &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(&cpd_core::clock::ManualClock::new())).iter().map(|l| {
            let content = l.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
            let content = content.trim_end();
            if content.is_empty() { String::new() } else { format!(" {content}") }
        }).collect::<Vec<_>>());
    }

    #[test]
    fn scrolling_leaves_the_end_and_reaching_it_follows_again() {
        let mut v = view(10);
        draw(&mut v, 24, 5);
        press(&mut v, KeyCode::Up);
        assert_eq!(rows(&draw(&mut v, 24, 5))[2..], [" line 8", " line 9", ""]);
        press(&mut v, KeyCode::Char('k'));
        press(&mut v, KeyCode::PageUp);
        press(&mut v, KeyCode::PageUp);
        press(&mut v, KeyCode::PageUp);
        assert_eq!(rows(&draw(&mut v, 24, 5))[2], " line 1", "it stops at the top");
        wheel(&mut v, MouseEventKind::ScrollDown);
        assert_eq!(rows(&draw(&mut v, 24, 5))[2], " line 4");
        press(&mut v, KeyCode::Char('G'));
        assert_eq!(rows(&draw(&mut v, 24, 5))[3], " line 10");
        wheel(&mut v, MouseEventKind::ScrollUp);
        assert_eq!(rows(&draw(&mut v, 24, 5))[2], " line 6", "the wheel moves 3 rows");
        press(&mut v, KeyCode::PageDown);
        press(&mut v, KeyCode::Char('j'));
        assert_eq!(rows(&draw(&mut v, 24, 5))[3], " line 10", "Down to the end follows it");
        press(&mut v, KeyCode::Char('k'));
        press(&mut v, KeyCode::End);
        assert_eq!(rows(&draw(&mut v, 24, 5))[3], " line 10", "End follows the end");
    }

    #[test]
    fn only_q_and_escape_end_the_view() {
        let mut v = view(1);
        assert!(!press(&mut v, KeyCode::Char('x')) && !press(&mut v, KeyCode::Enter));
        assert!(press(&mut v, KeyCode::Char('q')) && press(&mut v, KeyCode::Esc));
    }
}
