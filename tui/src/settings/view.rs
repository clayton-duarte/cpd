//! The drawing and the mouse hit-testing of the Agent Tiers view (stubs: the implementation fills them in).

use super::model::{Model, Row};
use ratatui::Frame;
use ratatui::crossterm::event::MouseEvent;

/// What a click lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A row's value.
    Value(Row),
    /// An entry of the open list.
    Entry(usize),
    Load,
    Save,
    Cancel,
}

/// Draws the title, the scrolled body, the open list and the footer of `model`.
pub fn draw(_f: &mut Frame, _model: &Model) {
    todo!("draw: the implementation Job")
}

/// What `mouse` lands on in the last drawn frame.
pub fn hit(_model: &Model, _mouse: MouseEvent) -> Option<Hit> {
    todo!("hit: the implementation Job")
}
