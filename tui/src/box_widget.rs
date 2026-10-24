//! The one box widget: rounded walls (or a left rail), a title, padding and a
//! body. Stateless; the spacing rule lives in [`spacing`].

use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Widget;

/// The box spacing rule (hs-d578, hs-d476).
pub mod spacing {
    use ratatui::layout::Rect;

    pub const MARGIN_X: u16 = 1;
    pub const PAD_X: u16 = 1;
    pub const FOLD_MARGIN_Y: u16 = 1;
    /// The padding and the wall column a rail leaves blank: content ends at W-4.
    pub const RIGHT_PAD: u16 = PAD_X + 1;
    /// Wall, pad, one cell, pad, wall.
    pub const MIN_WIDTH: u16 = 2 + 2 * PAD_X + 1;

    /// `area` less [`MARGIN_X`] on each side.
    pub fn contain(_area: Rect) -> Rect {
        unimplemented!()
    }
}

/// How the walls are drawn.
pub enum Walls<'a> {
    /// Rounded walls all round.
    Full,
    /// A left rail only, with a closing row.
    Rail { closing: Line<'a> },
}

/// A box: walls, an optional title, a body and an optional highlight.
#[allow(dead_code, reason = "the stub reads no field yet")]
pub struct BoxWidget<'a, B: Widget> {
    walls: Walls<'a>,
    border: Style,
    title: Option<Line<'a>>,
    body: B,
    highlight: Option<Range<u16>>,
}

impl<'a, B: Widget> BoxWidget<'a, B> {
    pub fn new(walls: Walls<'a>, border: Style, body: B) -> Self {
        Self { walls, border, title: None, body, highlight: None }
    }

    pub fn title(mut self, title: Line<'a>) -> Self {
        self.title = Some(title);
        self
    }

    pub fn highlight(mut self, rows: Range<u16>) -> Self {
        self.highlight = Some(rows);
        self
    }
}

/// Where the parts of a box sit in an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub wall: Rect,
    pub inner: Rect,
    pub title_row: u16,
    pub closing_row: Option<u16>,
}

/// Pure: the widget and the views both call it.
pub fn geometry(_walls: &Walls, _area: Rect) -> Geometry {
    unimplemented!()
}

impl<B: Widget> Widget for BoxWidget<'_, B> {
    fn render(self, _area: Rect, _buf: &mut Buffer) {
        unimplemented!()
    }
}
