//! The input box (contract §6.1.1): a labelled rounded box every text input is drawn in. Stubs for now.

use ratatui::text::Line;

/// The box's rows: the top border with the label as a pill, the wrapped text rows, the bottom border; each `width` cells.
#[allow(dead_code)]
pub(crate) fn lines(label: &str, text: &[char], cursor: usize, focused: bool, width: usize) -> Vec<Line<'static>> {
    let _ = (label, text, cursor, focused, width);
    todo!()
}

/// The number of text rows for `len` characters: `len / tw + 1`, `tw = width - 6` (1 for `width <= 6`).
#[allow(dead_code)]
pub(crate) fn text_rows(len: usize, width: usize) -> usize {
    let _ = (len, width);
    todo!()
}
