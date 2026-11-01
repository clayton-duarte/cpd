//! The `chat` view (commands.md §10, §13.2, §17; amendment 2): the chat slot's
//! placeholder. It reads nothing and draws one static line,
//! [`PLACEHOLDER_TEXT`], dim like the spike's other empty states, until the
//! pane is killed (`session open` kills it when a Lead takes the slot). Keys
//! are ignored, so a stray Ctrl-C cannot empty the slot.

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event};
use ratatui::text::Line;
use ratatui::widgets::{Block, Padding, Paragraph, Wrap};

use super::{Capture, run_view};
use cpd_core::error::CpdError;
use cpd_core::host::layout::PLACEHOLDER_TEXT;
use cpd_core::output::Output;

pub(super) fn run() -> Result<Output, CpdError> {
    run_view(
        "chat",
        Capture::Keys,
        || Ok(()),
        |terminal, ()| Err(show(terminal).into()),
    )
}

/// Draws the placeholder until the terminal fails: any event, a resize
/// included, redraws, and nothing ends the view.
fn show(terminal: &mut ratatui::DefaultTerminal) -> std::io::Error {
    loop {
        if let Err(e) = terminal
            .draw(draw)
            .map(drop)
            .and_then(|()| event::read().map(|_: Event| ()))
        {
            return e;
        }
    }
}

/// Draws the placeholder: [`PLACEHOLDER_TEXT`], muted, inside the chat slot's
/// uniform 1-cell pad, so the text starts at row 1, column 1 and the pane's
/// first and last row and column stay blank.
fn draw(frame: &mut Frame) {
    let text = Paragraph::new(Line::styled(PLACEHOLDER_TEXT, crate::palette::muted_text()))
        .wrap(Wrap { trim: true });
    frame.render_widget(
        text.block(Block::new().padding(Padding::uniform(1))),
        frame.area(),
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn render(width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(draw).unwrap();
        terminal.backend().buffer().clone()
    }

    fn rows(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn draws_the_placeholder_text_dim() {
        let buffer = render(80, 3);
        let rows = rows(&buffer);
        assert_eq!(rows[0].trim(), "", "the first row is blank: {rows:?}");
        assert_eq!(
            rows[1].trim_end(),
            " No Lead here: pick a Plan in the session tree",
            "{rows:?}"
        );
        assert_eq!(rows[2].trim(), "", "the last row is blank: {rows:?}");
        assert!(
            buffer[(0, 1)].symbol().trim().is_empty(),
            "the first column is blank"
        );
        assert!(
            buffer[(79, 1)].symbol().trim().is_empty(),
            "the last column is blank"
        );
        assert!(crate::palette::check::is_muted(&buffer[(1, 1)]));
    }

    #[test]
    fn wraps_the_text_in_a_narrow_pane() {
        let rows = rows(&render(20, 5));
        let words: Vec<String> = rows
            .iter()
            .flat_map(|r| r.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
            .collect();
        assert_eq!(words.join(" "), PLACEHOLDER_TEXT, "{rows:?}");
    }
}
