//! The Inbox section at the bottom of the tree column (commands.md §13.2, §17; contract §2.2 "Inbox item"): a header
//! ` Inbox <n> `, up to [`SHOWN`] item rows `● <label>` with the age right-aligned, and, with more items, a row
//! `+<n> more` that expands the list inline and becomes `− less`. Whether it is expanded is view state only.
//!
//! A click on an item row jumps to its cause ([`jump`]): a command, the logs popup at the item's Event, or nothing,
//! when the row's label is an OSC 8 link to the item's `url` (`crate::link`, which the terminal opens with its
//! modifier-click). Everything here is pure but [`draw`], which only writes to the frame.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::actions::Action;
use super::rows::truncate;
use crate::link::link;
use crate::palette::{self, Meaning};
use crate::read::{Dot, Tree};
use crate::widgets::section_header;
use cpd_core::ids::SessionId;
use cpd_core::inbox::model::{Item, Kind};

/// How many item rows the section shows before `+<n> more`.
pub const SHOWN: usize = 3;

/// `● ` before the label.
const DOT_WIDTH: usize = 2;

/// What a row of the section is, by its offset from the header's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Header,
    /// The item at this index of the snapshot's Inbox.
    Item(usize),
    /// `+<n> more` or `− less`.
    Toggle,
}

/// The rows of the section for `count` visible items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    count: usize,
    expanded: bool,
}

impl Shape {
    pub fn new(count: usize, expanded: bool) -> Shape {
        Shape { count, expanded }
    }

    /// The item rows: three, or all when expanded.
    fn shown(self) -> usize {
        if self.expanded { self.count } else { self.count.min(SHOWN) }
    }

    fn has_toggle(self) -> bool {
        self.count > SHOWN
    }

    /// The rows the section wants: the header, the items, the toggle.
    pub fn height(self) -> u16 {
        u16::try_from(1 + self.shown() + usize::from(self.has_toggle())).unwrap_or(u16::MAX)
    }

    /// The row `row` rows below the header's.
    pub fn hit(self, row: u16) -> Option<Hit> {
        let row = usize::from(row);
        match row {
            0 => Some(Hit::Header),
            r if r <= self.shown() => Some(Hit::Item(r - 1)),
            r if r == self.shown() + 1 && self.has_toggle() => Some(Hit::Toggle),
            _ => None,
        }
    }
}

/// An age in the largest whole unit: `42s`, `5m`, `3h`, `2d`. A negative age (a clock that went back) is `0s`.
pub fn age(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

/// The age of an item opened at `opened_at` (§1.3's RFC 3339), at `now`; `?` when the stored time does not parse.
fn age_of(opened_at: &str, now: OffsetDateTime) -> String {
    OffsetDateTime::parse(opened_at, &Rfc3339).map_or_else(|_| "?".to_owned(), |t| age((now - t).whole_seconds()))
}

/// What a click on an item does besides the OSC 8 link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jump {
    /// Run this command and focus the chat slot (a click on a Session row).
    Run(Action),
    /// The logs popup, at this Event; at the bottom with none.
    Logs(Option<i64>),
}

/// `session open <id>`, or `session history <id>` for an archived Session (as a click on its tree row).
fn open_or_history(tree: &Tree, id: &SessionId) -> Action {
    let archived = tree
        .contexts
        .iter()
        .flat_map(|c| &c.plans)
        .flat_map(|p| &p.sessions)
        .find(|s| &s.id == id)
        .is_some_and(|s| s.is_archived());
    if archived { Action::History(id.clone()) } else { Action::Open(id.clone()) }
}

/// Where a click on `item` goes (commands.md §13.2, "Clicks on an Inbox item"); `None` for the kinds that are a link
/// (the terminal opens it), and for an item without what its jump needs.
pub fn jump(tree: &Tree, item: &Item) -> Option<Jump> {
    match item.kind {
        Kind::Dialog | Kind::Unread => item.session.as_ref().map(|s| Jump::Run(open_or_history(tree, s))),
        Kind::JobStuck => {
            let plan = item.plan.as_ref()?;
            let plan = tree.contexts.iter().flat_map(|c| &c.plans).find(|p| &p.head.id == plan)?;
            Some(Jump::Run(open_or_history(tree, &plan.head.lead_session)))
        }
        Kind::EngineError | Kind::EngineDown => Some(Jump::Logs(item.event)),
        Kind::Question => item.context.as_ref().map(|c| Jump::Run(Action::OpenManager(c.clone()))),
        Kind::PlanReview | Kind::PrFailing | Kind::PrChanges | Kind::PrReady | Kind::ReviewRequested => None,
    }
}

/// The kinds whose label is an OSC 8 link to the item's `url`.
fn is_link(kind: Kind) -> bool {
    matches!(kind, Kind::PlanReview | Kind::PrFailing | Kind::PrChanges | Kind::PrReady | Kind::ReviewRequested)
}

/// An item's row text: `● <label>`, the label cut with `…` so that it never reaches the age, and the age. Returns the
/// line and the number of cells of the label.
fn item_line(item: &Item, age: &str, width: usize, frame: usize) -> (Line<'static>, usize) {
    let room = width.saturating_sub(DOT_WIDTH + 1 + age.chars().count());
    let label = truncate(&item.label, room);
    let label_width = label.chars().count();
    let gap = width.saturating_sub(DOT_WIDTH + label_width + age.chars().count());
    let line = Line::from(vec![
        palette::dot_span(Dot::from(item.urgency), frame, false),
        " ".into(),
        label.into(),
        " ".repeat(gap).into(),
        Span::styled(age.to_owned(), palette::fg(Meaning::Nothing)),
    ]);
    (line, label_width)
}

/// Draws the section into `area` (its first row is the header; rows that do not fit are cut): `items` are the
/// snapshot's visible open items, `now` the clock the ages are taken from.
pub fn draw(f: &mut Frame, area: Rect, items: &[Item], expanded: bool, now: OffsetDateTime, frame: usize) {
    let shape = Shape::new(items.len(), expanded);
    for dy in 0..area.height.min(shape.height()) {
        let row = Rect::new(area.x, area.y + dy, area.width, 1);
        match shape.hit(dy) {
            Some(Hit::Header) => {
                let title = format!("Inbox {}", items.len());
                f.render_widget(Paragraph::new(section_header(&title, usize::from(row.width))), row);
            }
            Some(Hit::Item(i)) => {
                let item = &items[i];
                let (line, label_width) = item_line(item, &age_of(&item.opened_at, now), usize::from(row.width), frame);
                f.render_widget(Paragraph::new(line), row);
                if let (true, Some(url)) = (is_link(item.kind), &item.url) {
                    let label = Rect::new(row.x + DOT_WIDTH as u16, row.y, label_width as u16, 1);
                    link(f.buffer_mut(), label, url);
                }
            }
            Some(Hit::Toggle) => {
                let text = if expanded { "− less".to_owned() } else { format!("+{} more", items.len() - SHOWN) };
                f.render_widget(Paragraph::new(Span::styled(text, palette::fg(Meaning::Nothing))), row);
            }
            None => {}
        }
    }
}
