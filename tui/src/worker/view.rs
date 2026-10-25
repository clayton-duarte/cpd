//! The worker view's state, keys and drawing: the lines of the Worker's
//! session file, scrolled, following the end until the User scrolls, under the pinned reason header.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::cells;
use crate::palette;
use crate::scroll::Scroller;

/// The most rows the header takes (commands.md §13.2).
const HEADER_ROWS: usize = 5;

pub(super) struct View {
    body: Scroller,
    /// Why the Job is in its state, empty for none: wrapped at draw, pinned above the body, a blank row after it.
    pub header: String,
    /// The Attempt's and its Session's state, the foot's text.
    pub status: String,
    /// A failure to read, shown at the foot instead of the status.
    pub note: String,
}

impl View {
    pub fn new() -> View {
        View { body: Scroller::new(), header: String::new(), status: String::new(), note: String::new() }
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
        let width = usize::from(f.area().width);
        let header: Vec<Line> = wrap_header(&self.header, width).into_iter().map(|r| Line::styled(r, palette::muted_text())).collect();
        let rows = if header.is_empty() { 0 } else { header.len() as u16 + 1 };
        let [top, body, foot] = Layout::vertical([Constraint::Length(rows), Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
        f.render_widget(Paragraph::new(header), top);
        self.body.draw(f, body);
        let line = if self.note.is_empty() {
            Line::styled(format!(" {}", self.status), palette::muted_text())
        } else {
            Line::styled(format!(" {}", self.note), palette::error())
        };
        f.render_widget(Paragraph::new(line), foot);
    }
}

/// `text` as rows of at most `width` columns, each after one blank, broken at spaces (a longer word is cut); at most
/// [`HEADER_ROWS`], the last ending ` …` when text is left out. No rows for no text.
fn wrap_header(text: &str, width: usize) -> Vec<String> {
    let room = width.saturating_sub(1).max(1);
    let mut rows: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let mut rest = word;
        while !rest.is_empty() {
            let mut piece = cells::take(rest, room);
            if piece.is_empty() {
                // A character wider than the room is taken whole: it cannot fit any row.
                piece = &rest[..rest.chars().next().map_or(0, char::len_utf8)];
            }
            rest = &rest[piece.len()..];
            match rows.last_mut() {
                Some(row) if cells::width(row) + 1 + cells::width(piece) <= width => {
                    row.push(' ');
                    row.push_str(piece);
                }
                _ => rows.push(format!(" {piece}")),
            }
        }
    }
    if rows.len() > HEADER_ROWS {
        rows.truncate(HEADER_ROWS);
        let last = rows.last_mut().expect("truncated to HEADER_ROWS");
        while cells::width(last) + 2 > width && last.rfind(' ').is_some_and(|i| i > 0) {
            last.truncate(last.rfind(' ').expect("checked"));
        }
        last.push_str(" …");
    }
    rows
}

#[cfg(test)]
#[path = "width_tests.rs"]
mod width_tests;

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyEventState, KeyModifiers, MouseEventKind};

    use super::*;
    use crate::test_text::rows;

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

    fn press(view: &mut View, code: KeyCode) -> bool {
        view.key(KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE })
    }

    fn wheel(view: &mut View, kind: MouseEventKind) {
        view.mouse(MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE });
    }

    #[test]
    fn it_follows_the_end_and_shows_the_status_at_the_foot() {
        let mut v = view(10);
        assert_eq!(rows(&draw(&mut v, 24, 4)), ["line 8", "line 9", "line 10", " a-a1 running · working"]);
        v.set_lines(lines(12));
        assert_eq!(rows(&draw(&mut v, 24, 4))[2], "line 12", "a new line keeps the end in view");
    }

    #[test]
    fn scrolling_leaves_the_end_and_reaching_it_follows_again() {
        let mut v = view(10);
        draw(&mut v, 24, 4);
        press(&mut v, KeyCode::Up);
        assert_eq!(rows(&draw(&mut v, 24, 4))[..3], ["line 7", "line 8", "line 9"]);
        v.set_lines(lines(12));
        assert_eq!(rows(&draw(&mut v, 24, 4))[0], "line 7", "the view stays where the User put it");
        press(&mut v, KeyCode::Char('k'));
        press(&mut v, KeyCode::PageUp);
        press(&mut v, KeyCode::PageUp);
        assert_eq!(rows(&draw(&mut v, 24, 4))[0], "line 1", "it stops at the top");
        wheel(&mut v, MouseEventKind::ScrollDown);
        assert_eq!(rows(&draw(&mut v, 24, 4))[0], "line 4");
        press(&mut v, KeyCode::Char('G'));
        assert_eq!(rows(&draw(&mut v, 24, 4))[2], "line 12");
        wheel(&mut v, MouseEventKind::ScrollUp);
        for _ in 0..3 {
            press(&mut v, KeyCode::Down);
        }
        v.set_lines(lines(14));
        assert_eq!(rows(&draw(&mut v, 24, 4))[2], "line 14", "Down to the end follows it");
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
        assert_eq!(rows(&draw(&mut v, 16, 4)), ["0123456789abcdef", "      ghijklmnop", "      qrstuvwxyz", ""]);
        assert_eq!(rows(&draw(&mut v, 30, 3))[..2], ["0123456789abcdefghijklmnopqrst", "      uvwxyz"], "a new width wraps again");
    }

    #[test]
    fn the_header_is_pinned_wrapped_and_stops_at_five_rows() {
        let mut v = view(10);
        v.header = "failed: crashed".to_owned();
        assert_eq!(rows(&draw(&mut v, 24, 5)), [" failed: crashed", "", "line 9", "line 10", " a-a1 running · working"]);
        v.header = (1..=20).map(|i| format!("w{i:02}")).collect::<Vec<_>>().join(" ");
        let rows = wrap_header(&v.header, 12);
        assert_eq!(rows, [" w01 w02 w03", " w04 w05 w06", " w07 w08 w09", " w10 w11 w12", " w13 w14 …"]);
        assert_eq!(wrap_header("", 12), Vec::<String>::new());
        assert_eq!(wrap_header("abcdefghij", 6), [" abcde", " fghij"], "a long word is cut");
    }

    #[test]
    fn a_failure_replaces_the_status() {
        let mut v = view(1);
        v.note = "The database is locked.".to_owned();
        assert_eq!(rows(&draw(&mut v, 30, 2))[1], " The database is locked.");
    }
}
