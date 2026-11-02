//! The drawing and the mouse hit-testing of the Agent Tiers view: the title, the scrolled body (the rows, the open
//! list inline under its row, the buttons) and the footer. The body is built as lines that know what a click on
//! each column lands on, so what is drawn and what is hit are one thing.

use super::model::{Model, Row, RowView};
use crate::box_widget::SectionTitle;
use crate::dialog::scroll::Scroll;
use crate::palette;
use ratatui::Frame;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

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

/// The index of the first row of the Tiers section.
const TIERS_AT: usize = 6;
/// The column of a row's value, and of an open list's entries.
const VALUE_COL: usize = 15;
/// The column of the first button of the buttons row.
const BUTTONS_COL: usize = 24;
const TITLE: &str = "Agent Tiers";

/// One line of the body and what a click on its columns `start..end` lands on.
struct BodyLine {
    line: Line<'static>,
    hits: Vec<(usize, usize, Hit)>,
}

/// A line under construction, tracking its column.
#[derive(Default)]
struct Builder {
    spans: Vec<Span<'static>>,
    hits: Vec<(usize, usize, Hit)>,
    col: usize,
}

impl Builder {
    fn text(&mut self, text: impl Into<String>, style: Style) -> &mut Self {
        let text = text.into();
        self.col += text.chars().count();
        self.spans.push(Span::styled(text, style));
        self
    }

    /// `text`, a click on which is `hit`.
    fn hit(&mut self, text: impl Into<String>, style: Style, hit: Hit) -> &mut Self {
        let start = self.col;
        self.text(text, style);
        self.hits.push((start, self.col, hit));
        self
    }

    fn done(&mut self) -> BodyLine {
        BodyLine {
            line: Line::from(std::mem::take(&mut self.spans)),
            hits: std::mem::take(&mut self.hits),
        }
    }
}

fn blank() -> BodyLine {
    Builder::default().done()
}

fn header(name: &str, note: &str) -> BodyLine {
    let mut b = Builder::default();
    b.text(
        format!(" {name}"),
        Style::default().add_modifier(Modifier::BOLD),
    )
    .text(format!("  {note}"), palette::muted_text());
    b.done()
}

fn row_line(rv: &RowView, arrow: &str) -> BodyLine {
    let mut b = Builder::default();
    let mark = if rv.focused { "   ▸ " } else { "     " };
    b.text(mark, Style::default());
    b.text(format!("{:<10}", rv.label), Style::default());
    let suffix = if rv.invalid {
        "!"
    } else if rv.changed {
        "*"
    } else {
        ""
    };
    let style = if rv.invalid {
        palette::error()
    } else if rv.dimmed {
        palette::muted_text()
    } else {
        Style::default()
    };
    b.hit(
        format!("{}{suffix} {arrow}", rv.value),
        style,
        Hit::Value(rv.row),
    );
    if rv.row == Row::Preset {
        b.text("  ", Style::default());
        b.hit("[ Load ]", palette::clickable_text(), Hit::Load);
    } else if !rv.range.is_empty() {
        b.text(format!("  {}", rv.range), palette::muted_text());
    }
    b.done()
}

/// The body: its lines, and the lines `focus` that scrolling keeps in view (the focused row, its open list and the
/// two lines after the list).
struct Body {
    lines: Vec<BodyLine>,
    focus: (usize, usize),
}

fn body(model: &Model) -> Body {
    let mut lines = Vec::new();
    let mut focus = (0, 0);
    let list = model.list();
    let rows = model.rows();
    // an open list takes the place of the rows below its row, in its section
    let mut hidden_to = 0;
    for (at, rv) in rows.iter().enumerate() {
        if at < hidden_to {
            continue;
        }
        match at {
            0 => lines.push(header("Models", "one provider per machine")),
            TIERS_AT => {
                lines.push(blank());
                lines.push(header(
                    "Tiers",
                    "the default of each role; its range follows",
                ));
            }
            _ => {}
        }
        let first = lines.len();
        lines.push(row_line(rv, if rv.open { "▴" } else { "▾" }));
        let mut last = first;
        if let (true, Some(list)) = (rv.open, &list) {
            let pad = " ".repeat(VALUE_COL);
            let mut b = Builder::default();
            b.text(format!("{pad}filter: {}", list.filter), Style::default());
            lines.push(b.done());
            for (i, entry) in list.entries.iter().enumerate() {
                let dot = if list.current == Some(i) {
                    "(•)"
                } else {
                    "( )"
                };
                let style = if list.highlight == i {
                    palette::selected()
                } else {
                    Style::default()
                };
                let mut b = Builder::default();
                b.text(pad.clone(), Style::default());
                b.hit(format!("{dot} {entry}"), style, Hit::Entry(i));
                lines.push(b.done());
            }
            last = lines.len() + 1;
            hidden_to = if at < TIERS_AT { TIERS_AT } else { rows.len() };
        }
        if rv.focused {
            focus = (first, last);
        }
    }
    lines.push(blank());
    let mut b = Builder::default();
    let save = if model.blocked().is_some() {
        palette::muted_text()
    } else {
        palette::clickable_text()
    };
    b.text(" ".repeat(BUTTONS_COL), Style::default());
    b.hit("[ Save ]", save, Hit::Save);
    b.text("  ", Style::default());
    b.hit("[ Cancel ]", palette::clickable_text(), Hit::Cancel);
    lines.push(b.done());
    Body { lines, focus }
}

/// The rows of the body inside `area`: under the title, above the footer.
fn body_area(area: Rect) -> Rect {
    let top = SectionTitle::body(area);
    Rect::new(top.x, top.y, top.width, top.height.saturating_sub(1))
}

/// Draws the title, the scrolled body, the open list and the footer of `model`; `follow` scrolls to keep the focused
/// row and its list in view (not after the wheel).
pub fn draw(f: &mut Frame, model: &Model, scroll: &mut Scroll, follow: bool) {
    let area = f.area();
    f.render_widget(
        SectionTitle::new(TITLE, None),
        Rect::new(
            area.x,
            area.y,
            area.width,
            SectionTitle::ROWS.min(area.height),
        ),
    );
    let body_rect = body_area(area);
    let body = body(model);
    scroll.resize(body.lines.len(), usize::from(body_rect.height));
    if follow {
        scroll.show(body.focus.0, body.focus.1);
    }
    for (i, bl) in body
        .lines
        .iter()
        .skip(scroll.offset())
        .take(usize::from(body_rect.height))
        .enumerate()
    {
        let rect = Rect::new(body_rect.x, body_rect.y + i as u16, body_rect.width, 1);
        f.render_widget(Paragraph::new(bl.line.clone()), rect);
    }
    if let (Some(text), true) = (model.footer(), area.height > 0) {
        let rect = Rect::new(area.x, area.bottom() - 1, area.width, 1);
        f.render_widget(Paragraph::new(Span::styled(text, palette::error())), rect);
    }
}

/// What `mouse` lands on in the frame of `area` drawn with `scroll`.
pub fn hit(model: &Model, scroll: &Scroll, area: Rect, mouse: MouseEvent) -> Option<Hit> {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    let rect = body_area(area);
    if mouse.row < rect.y || mouse.row >= rect.bottom() || mouse.column < rect.x {
        return None;
    }
    let body = body(model);
    let line = body
        .lines
        .get(scroll.offset() + usize::from(mouse.row - rect.y))?;
    let col = usize::from(mouse.column - rect.x);
    line.hits
        .iter()
        .find(|(start, end, _)| (*start..*end).contains(&col))
        .map(|(_, _, hit)| *hit)
}
