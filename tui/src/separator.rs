//! The one separator line of the cockpit: a dim rule across a one-row area. The Plan view's tabs, the Context's bottom
//! row and the Dispatch sections all separate areas with it, and with nothing else.

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders},
};

use crate::palette::{self, Meaning};

/// The rows a separator takes.
pub const HEIGHT: u16 = 1;

/// Draw the separator across `area` (one row), in the dim "nothing" colour.
pub fn draw(f: &mut Frame, area: Rect) {
    // `Borders::TOP` of a one-row block is the line itself
    f.render_widget(Block::new().borders(Borders::TOP).border_style(palette::fg(Meaning::Nothing)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn the_line_runs_across_its_row_in_the_dim_colour_and_leaves_the_rest() {
        let mut terminal = Terminal::new(TestBackend::new(6, 3)).unwrap();
        terminal.draw(|f| draw(f, Rect::new(0, 1, 6, HEIGHT))).unwrap();
        let buf = terminal.backend().buffer();
        for x in 0..6 {
            assert_eq!(buf[(x, 1)].symbol(), "─");
            assert_eq!(buf[(x, 1)].fg, palette::color(Meaning::Nothing));
            assert_eq!(buf[(x, 0)].symbol(), " ");
            assert_eq!(buf[(x, 2)].symbol(), " ");
        }
    }
}
