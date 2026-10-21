//! The worker view's state, keys and drawing: the lines of the Worker's
//! session file, scrolled, following the end until the User scrolls.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::palette;
use crate::scroll::Scroller;

pub(super) struct View {
    body: Scroller,
    /// The Attempt's and its Session's state, the foot's text.
    pub status: String,
    /// A failure to read, shown at the foot instead of the status.
    pub note: String,
}

impl View {
    pub fn new() -> View {
        View { body: Scroller::new(), status: String::new(), note: String::new() }
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
        let [body, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
        self.body.draw(f, body);
        let line = if self.note.is_empty() {
            Line::styled(format!(" {}", self.status), palette::muted_text())
        } else {
            Line::styled(format!(" {}", self.note), palette::error())
        };
        f.render_widget(Paragraph::new(line), foot);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers, MouseEventKind};

    use super::*;

    fn lines(n: usize) -> Vec<Line<'static>> {
        (1..=n).map(|i| Line::from(format!("line {i}"))).collect()
    }

    fn view(n: usize) -> View {
        let mut view = View::new();
        view.set_lines(lines(n));
        view.status = "a-a1 running · working".to_owned();
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
    fn it_follows_the_end_and_shows_the_status_at_the_foot() {
        let mut v = view(10);
        assert_eq!(screen(&draw(&mut v, 24, 4)), ["line 8", "line 9", "line 10", " a-a1 running · working"]);
        v.set_lines(lines(12));
        assert_eq!(screen(&draw(&mut v, 24, 4))[2], "line 12", "a new line keeps the end in view");
    }

    #[test]
    fn scrolling_leaves_the_end_and_reaching_it_follows_again() {
        let mut v = view(10);
        draw(&mut v, 24, 4);
        press(&mut v, KeyCode::Up);
        assert_eq!(screen(&draw(&mut v, 24, 4))[..3], ["line 7", "line 8", "line 9"]);
        v.set_lines(lines(12));
        assert_eq!(screen(&draw(&mut v, 24, 4))[0], "line 7", "the view stays where the User put it");
        press(&mut v, KeyCode::Char('k'));
        press(&mut v, KeyCode::PageUp);
        press(&mut v, KeyCode::PageUp);
        assert_eq!(screen(&draw(&mut v, 24, 4))[0], "line 1", "it stops at the top");
        wheel(&mut v, MouseEventKind::ScrollDown);
        assert_eq!(screen(&draw(&mut v, 24, 4))[0], "line 4");
        press(&mut v, KeyCode::Char('G'));
        assert_eq!(screen(&draw(&mut v, 24, 4))[2], "line 12");
        wheel(&mut v, MouseEventKind::ScrollUp);
        for _ in 0..3 {
            press(&mut v, KeyCode::Down);
        }
        v.set_lines(lines(14));
        assert_eq!(screen(&draw(&mut v, 24, 4))[2], "line 14", "Down to the end follows it");
    }

    #[test]
    fn only_q_and_escape_end_the_view() {
        let mut v = view(1);
        assert!(!press(&mut v, KeyCode::Char('x')) && !press(&mut v, KeyCode::Enter));
        assert!(press(&mut v, KeyCode::Char('q')) && press(&mut v, KeyCode::Esc));
    }

    #[test]
    fn long_lines_wrap_and_scroll_by_rows() {
        let mut v = View::new();
        v.set_lines(vec![Line::from("0123456789abcdefghijklmnopqrstuvwxyz")]);
        assert_eq!(screen(&draw(&mut v, 16, 4)), ["0123456789abcdef", "      ghijklmnop", "      qrstuvwxyz", ""]);
        assert_eq!(screen(&draw(&mut v, 30, 3))[..2], ["0123456789abcdefghijklmnopqrst", "      uvwxyz"], "a new width wraps again");
    }

    #[test]
    fn a_failure_replaces_the_status() {
        let mut v = view(1);
        v.note = "The database is locked.".to_owned();
        assert_eq!(screen(&draw(&mut v, 30, 2))[1], " The database is locked.");
    }
}
