//! The worker view's state, keys and drawing: the lines of the Worker's
//! session file, scrolled, following the end until the User scrolls.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use super::render::wrap;
use crate::palette::{self, Meaning};

/// The rows one turn of the mouse wheel scrolls.
const WHEEL: usize = 3;

pub(super) struct View {
    lines: Vec<Line<'static>>,
    /// `lines` wrapped to the width they were last drawn at.
    wrapped: Option<(u16, Vec<Line<'static>>)>,
    /// The first row shown; [`usize::MAX`] follows the end.
    scroll: usize,
    /// The rows and the body height of the last draw.
    rows: usize,
    height: usize,
    /// The Attempt's and its Session's state, the foot's text.
    pub status: String,
    /// A failure to read, shown at the foot instead of the status.
    pub note: String,
}

impl View {
    pub fn new() -> View {
        View { lines: Vec::new(), wrapped: None, scroll: usize::MAX, rows: 0, height: 0, status: String::new(), note: String::new() }
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
        let [body, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
        if self.wrapped.as_ref().is_none_or(|(width, _)| *width != body.width) {
            self.wrapped = Some((body.width, wrap(&self.lines, usize::from(body.width))));
        }
        let rows = &self.wrapped.as_ref().expect("wrapped above").1;
        self.rows = rows.len();
        self.height = usize::from(body.height);
        let top = self.scroll.min(self.max());
        f.render_widget(Paragraph::new(rows.iter().skip(top).take(self.height).cloned().collect::<Vec<_>>()), body);
        let line = if self.note.is_empty() {
            Line::from(format!(" {}", self.status)).dim()
        } else {
            Line::styled(format!(" {}", self.note), palette::fg(Meaning::Failed))
        };
        f.render_widget(Paragraph::new(line), foot);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyEventState, KeyModifiers, MouseEventKind};

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
