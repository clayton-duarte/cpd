//! The one box widget: rounded walls (or a left rail), a title, padding and a
//! body. Stateless; the spacing rule lives in [`spacing`].

use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Widget};

use crate::palette;

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
    pub fn contain(area: Rect) -> Rect {
        Rect::new(
            area.x + MARGIN_X,
            area.y,
            area.width.saturating_sub(2 * MARGIN_X),
            area.height,
        )
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
pub struct BoxWidget<'a, B: Widget> {
    walls: Walls<'a>,
    border: Style,
    title: Option<Line<'a>>,
    body: B,
    highlight: Option<Range<u16>>,
}

impl<'a, B: Widget> BoxWidget<'a, B> {
    pub fn new(walls: Walls<'a>, border: Style, body: B) -> Self {
        Self {
            walls,
            border,
            title: None,
            body,
            highlight: None,
        }
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
pub fn geometry(walls: &Walls, area: Rect) -> Geometry {
    let (min_rows, top) = match walls {
        Walls::Full => (2, 0),
        Walls::Rail { .. } => (4, spacing::FOLD_MARGIN_Y),
    };
    let fits = area.width >= spacing::MIN_WIDTH && area.height >= min_rows;
    let inner = if !fits {
        Rect::ZERO
    } else {
        Rect::new(
            area.x + 1 + spacing::PAD_X,
            area.y + 1 + top,
            area.width - 2 - 2 * spacing::PAD_X,
            area.height - 2 - 2 * top,
        )
    };
    Geometry {
        wall: area,
        inner,
        title_row: area.y + top,
        closing_row: match walls {
            Walls::Full => None,
            Walls::Rail { .. } => Some(area.y + area.height.saturating_sub(2)),
        },
    }
}

impl<B: Widget> Widget for BoxWidget<'_, B> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let g = geometry(&self.walls, area);
        if g.inner == Rect::ZERO {
            return;
        }
        let area = area.intersection(buf.area);
        match &self.walls {
            Walls::Full => {
                let mut block = Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(self.border);
                if let Some(title) = self.title {
                    let mut spans = vec![Span::styled("─", self.border)];
                    spans.extend(title.spans);
                    block = block.title(Line::from(spans).style(title.style));
                }
                block.render(area, buf);
            }
            Walls::Rail { closing } => {
                let right = area.x + area.width;
                let cap = |buf: &mut Buffer, y: u16, glyph: &str, line: Option<&Line>| {
                    buf[(area.x, y)].set_symbol(glyph).set_style(self.border);
                    if let Some(line) = line {
                        line.render(Rect::new(area.x + 1, y, right - area.x - 1, 1), buf);
                    }
                };
                cap(buf, g.title_row, "╭", self.title.as_ref());
                let closing_row = g.closing_row.unwrap_or(g.title_row);
                for y in g.title_row + 1..closing_row {
                    cap(buf, y, "│", None);
                }
                cap(buf, closing_row, "╰", Some(closing));
            }
        }
        self.body.render(g.inner, buf);
        if let Some(rows) = self.highlight {
            let end = rows.end.min(g.inner.height);
            let (x, width) = match self.walls {
                Walls::Full => (area.x + 1, area.width - 2),
                Walls::Rail { .. } => (area.x, area.width),
            };
            for r in rows.start..end {
                buf.set_style(Rect::new(x, g.inner.y + r, width, 1), palette::selected());
            }
        }
    }
}

/// A section title (hs-d476): three rows, a blank one, the title row (`section::title`, from the area's first column to its
/// last) and a blank one. Stateless; it writes nothing on the blank rows nor outside its area.
pub struct SectionTitle<'a> {
    title: &'a str,
    count: Option<usize>,
}

impl<'a> SectionTitle<'a> {
    /// The rows a section title takes: blank, title, blank.
    pub const ROWS: u16 = 3;

    pub fn new(title: &'a str, count: Option<usize>) -> Self {
        SectionTitle { title, count }
    }

    /// The area under the three rows (zero height when `area` is shorter).
    pub fn body(area: Rect) -> Rect {
        let rows = Self::ROWS.min(area.height);
        Rect::new(area.x, area.y + rows, area.width, area.height - rows)
    }
}

impl Widget for SectionTitle<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width == 0 {
            return;
        }
        let row = Rect::new(area.x, area.y + 1, area.width, 1);
        ratatui::widgets::Paragraph::new(crate::section::title(
            self.title,
            self.count,
            usize::from(area.width),
        ))
        .render(row, buf);
    }
}
