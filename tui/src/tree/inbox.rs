//! The Inbox section at the bottom of the tree column (commands.md §13.2, §17; contract §2.2 "Inbox item"): a header
//! ` Inbox <n> `, up to [`SHOWN`] item rows `<dot> <label>` with the age right-aligned, and, with more items, a row
//! `+<n> more` that expands the list inline and becomes `− less`. Whether it is expanded is view state only.
//!
//! A click on an item row jumps to its cause ([`jump`]): a command, the logs popup at the item's Event, or nothing,
//! when the row's label is an OSC 8 link to the item's `url` (`crate::link`, which the terminal opens with its
//! modifier-click). Everything here is pure but [`draw`], which only writes to the frame.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
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

/// The dot and a space before the label.
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
    pub fn shown(self) -> usize {
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

/// An item's row text: `<dot> <label>`, the label cut with `…` so that it never reaches the age, and the age. Returns the
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
/// snapshot's visible open items, `now` the clock the ages are taken from, `cursor` the item the keyboard cursor
/// is on (its row is inverted across its width).
pub fn draw(f: &mut Frame, area: Rect, items: &[Item], expanded: bool, cursor: Option<usize>, now: OffsetDateTime, frame: usize) {
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
                if cursor == Some(i) {
                    f.buffer_mut().set_style(row, Modifier::REVERSED);
                }
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

/// An open item for the tests: `kind` and `urgency`, its `label`, opened at `opened_at`; the rest is none.
#[cfg(test)]
pub(super) fn item(id: i64, kind: Kind, urgency: cpd_core::inbox::model::Urgency, label: &str, opened_at: &str) -> Item {
    Item {
        id,
        kind,
        urgency,
        subject: format!("subject:{id}"),
        label: label.to_owned(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: opened_at.to_owned(),
        closed_at: None,
        snoozed_until: None,
        dismissed_at: None,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{self, Write};
    use std::rc::Rc;

    use ratatui::backend::CrosstermBackend;
    use ratatui::{Terminal, TerminalOptions, Viewport};

    use super::super::fixture::{archived, sample};
    use super::*;
    use cpd_core::ids::{ContextId, PlanId};
    use cpd_core::inbox::model::Urgency;

    const OPENED: &str = "2026-01-01T00:00:00.000Z";

    fn at(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    #[test]
    fn an_age_is_the_largest_whole_unit() {
        let cases = [
            (0, "0s"),
            (42, "42s"),
            (59, "59s"),
            (60, "1m"),
            (299, "4m"),
            (300, "5m"),
            (3599, "59m"),
            (3600, "1h"),
            (3 * 3600 + 59 * 60, "3h"),
            (86399, "23h"),
            (86400, "1d"),
            (2 * 86400 + 86399, "2d"),
            (-5, "0s"),
        ];
        for (seconds, text) in cases {
            assert_eq!(age(seconds), text, "{seconds} s");
        }
    }

    #[test]
    fn an_age_is_taken_from_opened_at_and_an_unreadable_time_is_a_question_mark() {
        assert_eq!(age_of(OPENED, at("2026-01-01T00:00:42.900Z")), "42s");
        assert_eq!(age_of(OPENED, at("2026-01-03T05:00:00.000Z")), "2d");
        assert_eq!(age_of("yesterday", at("2026-01-03T05:00:00.000Z")), "?");
    }

    #[test]
    fn the_rows_are_the_header_up_to_three_items_and_a_toggle_only_with_more() {
        let rows = |count, expanded| {
            let shape = Shape::new(count, expanded);
            (0..shape.height() + 1).map(|r| shape.hit(r)).collect::<Vec<_>>()
        };
        assert_eq!(rows(0, false), [Some(Hit::Header), None]);
        assert_eq!(rows(2, false), [Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), None]);
        assert_eq!(rows(3, true), rows(3, false), "three items have nothing to expand");
        assert_eq!(rows(5, false), [Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2)), Some(Hit::Toggle), None]);
        let expanded: Vec<_> = (0..7).map(|r| Shape::new(5, true).hit(r)).collect();
        assert_eq!(
            expanded,
            [Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2)), Some(Hit::Item(3)), Some(Hit::Item(4)), Some(Hit::Toggle)]
        );
        assert_eq!((Shape::new(0, false).height(), Shape::new(5, false).height(), Shape::new(5, true).height()), (1, 5, 7));
    }

    #[test]
    fn each_kind_jumps_to_its_cause_or_is_a_link() {
        let tree = sample();
        let with = |kind, f: &dyn Fn(&mut Item)| {
            let mut item = item(1, kind, Urgency::Red, "x", OPENED);
            f(&mut item);
            item
        };
        let session = |item: &mut Item| item.session = Some(sid("m-alpha"));
        let plan = |item: &mut Item| item.plan = Some(PlanId::parse("ship").unwrap());
        let context = |item: &mut Item| item.context = Some(ContextId::parse("alpha").unwrap());
        let event = |item: &mut Item| item.event = Some(7);
        let open = |id: &str| Some(Jump::Run(Action::Open(sid(id))));
        assert_eq!(jump(&tree, &with(Kind::Dialog, &session)), open("m-alpha"));
        assert_eq!(jump(&tree, &with(Kind::Unread, &session)), open("m-alpha"));
        assert_eq!(jump(&tree, &with(Kind::JobStuck, &plan)), open("l-ship"), "the Lead of the item's Plan");
        assert_eq!(jump(&tree, &with(Kind::EngineError, &event)), Some(Jump::Logs(Some(7))));
        assert_eq!(jump(&tree, &with(Kind::EngineDown, &|_| {})), Some(Jump::Logs(None)), "engine-down has no Event: the bottom");
        assert_eq!(jump(&tree, &with(Kind::Question, &context)), Some(Jump::Run(Action::OpenManager(ContextId::parse("alpha").unwrap()))));
        for kind in [Kind::PlanReview, Kind::PrFailing, Kind::PrChanges, Kind::PrReady, Kind::ReviewRequested] {
            assert_eq!(jump(&tree, &with(kind, &|i| i.url = Some("https://example.com".into()))), None, "{kind:?} is a link");
        }
        // an item without what its jump needs jumps nowhere
        assert_eq!(jump(&tree, &with(Kind::Dialog, &|_| {})), None);
        assert_eq!(jump(&tree, &with(Kind::JobStuck, &|i| i.plan = Some(PlanId::parse("nope").unwrap()))), None);
        assert_eq!(jump(&tree, &with(Kind::Question, &|_| {})), None);
        // an archived Session shows its history, as a click on its tree row
        assert_eq!(
            jump(&archived(), &with(Kind::Unread, &|i| i.session = Some(sid("f-blocked")))),
            Some(Jump::Run(Action::History(sid("f-blocked"))))
        );
        assert_eq!(
            jump(&archived(), &with(Kind::JobStuck, &|i| i.plan = Some(PlanId::parse("old").unwrap()))),
            Some(Jump::Run(Action::History(sid("l-old"))))
        );
    }

    /// A writer the test keeps a handle on.
    #[derive(Clone, Default)]
    struct Sink(Rc<RefCell<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// `bytes` as text without the CSI sequences (cursor moves and colours): what is left is what is printed, and the
    /// OSC 8 sequences.
    fn printed(bytes: &[u8]) -> String {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' && chars.peek() == Some(&'[') {
                for c in chars.by_ref().skip(1) {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn a_link_kinds_label_is_written_as_an_osc_8_link_and_its_dot_and_age_are_not() {
        let sink = Sink::default();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(sink.clone()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 41, 2)) },
        )
        .unwrap();
        let url = "https://github.com/acme/api/pull/12";
        let mut linked = item(1, Kind::PrFailing, Urgency::Red, "api#12 failing", OPENED);
        linked.url = Some(url.into());
        terminal.draw(|f| draw(f, f.area(), &[linked], false, None, at("2026-01-01T00:03:30.000Z"), 0)).unwrap();
        let bytes = sink.0.borrow().clone();
        let pieces: String = ["ap", "i#", "12", " f", "ai", "li", "ng"]
            .iter()
            .map(|p| format!("\x1b]8;;{url}\x07{p}\x1b]8;;\x07"))
            .collect();
        let text = printed(&bytes);
        assert!(text.contains(&format!("{}{pieces}3m", palette::NEEDS_YOU)), "the dot, the label in pieces of two cells, the age: {text:?}");
        assert_eq!(text.matches("\x1b]8;;https").count(), 7, "one link per piece, none around the dot or the age");
        // an item with no url, or of a kind that jumps, has no link
        let sink = Sink::default();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(sink.clone()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 41, 3)) },
        )
        .unwrap();
        let mut dialog = item(2, Kind::Dialog, Urgency::Red, "dialog", OPENED);
        dialog.url = Some(url.into());
        let no_url = item(3, Kind::PlanReview, Urgency::Yellow, "review", OPENED);
        terminal.draw(|f| draw(f, f.area(), &[dialog, no_url], false, None, at("2026-01-01T00:03:30.000Z"), 0)).unwrap();
        assert!(!printed(&sink.0.borrow()).contains("\x1b]8;;"));
    }
}
