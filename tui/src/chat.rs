//! The `chat` view (commands.md §10, §13.2, §17; amendment 2): the chat slot's
//! placeholder. It reads nothing and draws one static line,
//! [`PLACEHOLDER_TEXT`], dim like the spike's other empty states, until the
//! pane is killed (`session open` kills it when a Lead takes the slot). Keys
//! are ignored, so a stray Ctrl-C cannot empty the slot.

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};

use super::{not_a_terminal, on_a_terminal};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::layout::PLACEHOLDER_TEXT;
use cpd_core::output::Output;

pub(super) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view chat"));
    }
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let error = show(&mut terminal);
    ratatui::restore();
    Err(CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view chat failed: {error}."))
        .with("command", "tui view chat"))
}

/// Draws the placeholder until the terminal fails: any event, a resize
/// included, redraws, and nothing ends the view.
fn show(terminal: &mut ratatui::DefaultTerminal) -> std::io::Error {
    loop {
        if let Err(e) = terminal.draw(draw).map(drop).and_then(|()| event::read().map(|_: Event| ())) {
            return e;
        }
    }
}

/// Draws the placeholder: [`PLACEHOLDER_TEXT`], dim, wrapped to the pane one
/// column in from its left edge.
fn draw(frame: &mut Frame) {
    let text = Paragraph::new(Line::from(PLACEHOLDER_TEXT).dim()).wrap(Wrap { trim: true });
    frame.render_widget(text.block(Block::new().padding(Padding::left(1))), frame.area());
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;

    use super::*;

    fn render(width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(draw).unwrap();
        terminal.backend().buffer().clone()
    }

    fn rows(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
        let area = buffer.area;
        (0..area.height).map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>()).collect()
    }

    #[test]
    fn draws_the_placeholder_text_dim() {
        let buffer = render(80, 3);
        let rows = rows(&buffer);
        assert_eq!(rows[0].trim_end(), " No Lead here: pick a Plan in the session tree", "{rows:?}");
        assert!(rows[1..].iter().all(|r| r.trim().is_empty()), "{rows:?}");
        assert!(buffer[(1, 0)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn wraps_the_text_in_a_narrow_pane() {
        let rows = rows(&render(20, 5));
        let words: Vec<String> = rows.iter().flat_map(|r| r.split_whitespace().map(str::to_owned).collect::<Vec<_>>()).collect();
        assert_eq!(words.join(" "), PLACEHOLDER_TEXT, "{rows:?}");
    }
}
