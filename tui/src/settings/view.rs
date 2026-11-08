//! The drawing and the mouse hit-testing of the Agent Tiers view: the title, the scrolled body (the rows, the open
//! list inline under its row, the buttons) and the footer. The body is built as lines that know what a click on
//! each column lands on, so what is drawn and what is hit are one thing.

use super::model::{Feature, Level, Model, Row, RowView};
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
    /// An Experimental row: a click flips it.
    Flip(Feature),
    /// An entry of the open list.
    Entry(usize),
    /// An entry of the menu.
    Menu(usize),
    /// The Workers count's value.
    Concurrency,
    Load,
    Save,
    Cancel,
}

/// The index of the first row of the Tiers section: after Preset, Provider and the five letters O, S, H, G, J.
const TIERS_AT: usize = 7;
/// The column of a row's value, and of an open list's entries.
const VALUE_COL: usize = 15;
/// The column of the first button of the buttons row.
const BUTTONS_COL: usize = 24;

/// The title of each level (commands.md §13.2).
fn title(level: Level) -> &'static str {
    match level {
        Level::Menu => "Settings",
        Level::Tiers => "Agent Tiers",
        Level::Workers => "Workers",
    }
}

/// The buttons row, shared by every level.
fn buttons_line(model: &Model) -> BodyLine {
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
    b.done()
}

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

fn header(name: &str, hint: &str) -> BodyLine {
    let mut b = Builder::default();
    b.text(
        format!(" {name}"),
        Style::default().add_modifier(Modifier::BOLD),
    )
    .text(format!("  {hint}"), palette::muted_text());
    b.done()
}

/// An Experimental row: the key padded to the longest key and two spaces, then `on` or `off`; no list, no arrow.
fn switch_line(rv: &RowView, feature: Feature) -> BodyLine {
    let width = Feature::ALL
        .iter()
        .map(|f| f.key().len())
        .max()
        .unwrap_or(0);
    let mut b = Builder::default();
    b.text(if rv.focused { "   ▸ " } else { "     " }, Style::default());
    b.text(format!("{:<width$}  ", rv.label), Style::default());
    let suffix = if rv.changed { "*" } else { "" };
    b.hit(
        format!("{}{suffix}", rv.value),
        Style::default(),
        Hit::Flip(feature),
    );
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
    match model.level() {
        Level::Menu => menu_body(model),
        Level::Workers => workers_body(model),
        Level::Tiers => tiers_body(model),
    }
}

/// The menu: one hit line per entry, the focused one marked `▸` in column 3.
fn menu_body(model: &Model) -> Body {
    let mut lines = Vec::new();
    let mut focus = (0, 0);
    for (i, name) in super::model::MENU.iter().enumerate() {
        let focused = i == model.menu_focus();
        let mark = if focused { "   ▸ " } else { "     " };
        let mut b = Builder::default();
        b.hit(format!("{mark}{name}"), Style::default(), Hit::Menu(i));
        let at = lines.len();
        lines.push(b.done());
        if focused {
            focus = (at, at);
        }
    }
    Body { lines, focus }
}

/// The Workers screen: the header, the count's row (no list, so no arrow), the buttons.
fn workers_body(model: &Model) -> Body {
    let mut lines = vec![header("Workers", "the Attempts the machine runs at once")];
    let suffix = if model.concurrency_changed() { "*" } else { "" };
    let mut b = Builder::default();
    b.text("   ▸ ", Style::default());
    b.text("concurrency", Style::default());
    b.hit(
        format!("  {}{suffix}", model.concurrency_value()),
        Style::default(),
        Hit::Concurrency,
    );
    lines.push(b.done());
    lines.push(blank());
    lines.push(buttons_line(model));
    Body {
        lines,
        focus: (1, 1),
    }
}

fn tiers_body(model: &Model) -> Body {
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
    // an open Tier list hides the section, as it hides the Tier rows below it
    if hidden_to < rows.len() {
        lines.push(blank());
        let mut b = Builder::default();
        b.text(
            " Experimental",
            Style::default().add_modifier(Modifier::BOLD),
        );
        lines.push(b.done());
        for rv in model.switches() {
            let Row::Experimental(f) = rv.row else {
                continue;
            };
            let first = lines.len();
            lines.push(switch_line(&rv, f));
            if rv.focused {
                focus = (first, first);
            }
        }
    }
    lines.push(blank());
    lines.push(buttons_line(model));
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
        SectionTitle::new(title(model.level()), None),
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
