//! The Inbox section at the bottom of the tree column (commands.md §13.2, §17; contract §2.2 "Inbox item"): a header
//! ` Inbox <n> `, up to [`SHOWN`] item rows `<dot> <label>` with the age right-aligned, and, with more items, a row
//! `+<n> more` that expands the list inline and becomes `− less`. Whether it is expanded is view state only. Expanded, it is
//! capped at half the column and its items scroll inside the cap, with ` ▲ <n> more` / ` ▼ <n> more` cue rows ([`Shape`]).
//!
//! A click on an item row jumps to its cause ([`jump`]): a command, the logs popup at the item's Event, the merge popup of
//! its pull request, or the copy of its `url`. The label of a link kind is also an OSC 8 link to the `url` (`crate::link`, which the terminal opens with its
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
use crate::merge::Pull;
use crate::palette::{self, Meaning};
use crate::read::{Dot, Tree};
use crate::widgets::{Window, dim, max_scroll, section_header, window};
use cpd_core::ids::{PlanId, SessionId};
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

/// The rows of the section for `count` visible items. Expanded, its height is capped at `cap` rows (never below the
/// collapsed section's) and the items scroll in it from the item `scroll`, with a ` ▲ <n> more` and a ` ▼ <n> more` cue row
/// where some are hidden; the scrolling is the Deploy column's PR list's ([`window`]), one row an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    count: usize,
    expanded: bool,
    cap: u16,
    scroll: usize,
}

impl Shape {
    /// The section as tall as its items want (no cap), at the top.
    pub fn new(count: usize, expanded: bool) -> Shape {
        Shape { count, expanded, cap: u16::MAX, scroll: 0 }
    }

    /// The same, expanded at most `cap` rows tall.
    pub fn capped(self, cap: u16) -> Shape {
        Shape { cap, ..self }
    }

    /// The same, scrolled to the item `scroll` (clamped on use).
    pub fn scrolled(self, scroll: usize) -> Shape {
        Shape { scroll, ..self }
    }

    /// The item rows the keyboard cursor reaches: three, or all when expanded (they scroll).
    pub fn shown(self) -> usize {
        if self.expanded { self.count } else { self.count.min(SHOWN) }
    }

    fn has_toggle(self) -> bool {
        self.count > SHOWN
    }

    /// The rows the section wants: the header, the items, the toggle; expanded, at most the cap, but not less than collapsed.
    pub fn height(self) -> u16 {
        let collapsed = 1 + self.count.min(SHOWN) + usize::from(self.has_toggle());
        let full = 1 + self.count + usize::from(self.has_toggle());
        let height = if self.expanded { full.min(usize::from(self.cap)).max(collapsed) } else { collapsed };
        u16::try_from(height).unwrap_or(u16::MAX)
    }

    /// The rows between the header and the toggle: the items and their cues.
    fn list_rows(self) -> usize {
        usize::from(self.height()) - 1 - usize::from(self.has_toggle())
    }

    /// The items drawn and the cues: all of the first three when collapsed, else the window at the scroll offset.
    fn window(self) -> Window {
        if self.expanded {
            window(self.count, self.list_rows(), 1, self.scroll)
        } else {
            Window { first: 0, shown: self.shown(), cue_above: false, below: 0 }
        }
    }

    /// The largest scroll offset: the last item reaches the bottom of the list. 0 when collapsed or when all fit.
    pub fn max_scroll(self) -> usize {
        if self.expanded { max_scroll(self.count, self.list_rows(), 1) } else { 0 }
    }

    /// The row, below the header's, of the item `i`, when it is drawn.
    pub fn row_of(self, i: usize) -> Option<u16> {
        let win = self.window();
        (win.first..win.first + win.shown).contains(&i).then(|| (1 + usize::from(win.cue_above) + i - win.first) as u16)
    }

    /// The scroll offset nearest `scroll` at which the item `i` is drawn.
    pub fn scroll_to(self, i: usize) -> usize {
        let mut scroll = self.scroll.min(self.max_scroll());
        if i < self.scrolled(scroll).window().first {
            scroll = i;
        }
        while scroll < self.max_scroll() && self.scrolled(scroll).row_of(i).is_none() {
            scroll += 1;
        }
        scroll
    }

    /// The row `row` rows below the header's; a cue row is none.
    pub fn hit(self, row: u16) -> Option<Hit> {
        let row = usize::from(row);
        let win = self.window();
        let list = self.list_rows();
        match row {
            0 => Some(Hit::Header),
            r if r <= list => {
                let at = r - 1;
                (at >= usize::from(win.cue_above) && at - usize::from(win.cue_above) < win.shown)
                    .then(|| Hit::Item(win.first + at - usize::from(win.cue_above)))
            }
            r if r == list + 1 && self.has_toggle() => Some(Hit::Toggle),
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
    /// These actions in order, each stopping the flow when it is refused: `plan-review` opens its Lead, then copies its page.
    Steps(Vec<Action>),
    /// The logs popup, at this Event; at the bottom with none.
    Logs(Option<i64>),
    /// The merge popup of this pull request (`pr-ready`).
    Merge(Pull),
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

/// Where a plain click on `item` goes (commands.md §13.2, "Clicks on an Inbox item"): every kind acts. `engine-down`
/// starts the engine; `job-stuck` opens the Lead of its Plan and the Worker of the failed Job's latest Attempt beside it; `pr-ready` opens the merge popup of its pull request (`None` for a subject that is not
/// `pr:<owner>/<repo>#<n>`); `plan-review` opens the Lead of its Plan and copies the review page's `url` (or says there is
/// none yet); the `url` is copied for `pr-failing`, `pr-changes` and `review-requested` (their OSC 8 link stays for the
/// terminal's modifier-click). `None` only for an item without what its jump needs.
pub fn jump(tree: &Tree, item: &Item) -> Option<Jump> {
    match item.kind {
        Kind::Dialog | Kind::Unread => item.session.as_ref().map(|s| Jump::Run(open_or_history(tree, s))),
        Kind::JobStuck => {
            let plan = item.plan.as_ref()?;
            let open = open_lead(tree, plan)?;
            match &item.job {
                // the Worker of the Job's latest Attempt, which the Runner reads from the store; a Job with none opens the Lead only
                Some(job) => Some(Jump::Steps(vec![open, Action::OpenWorker { plan: plan.clone(), job: job.clone() }])),
                None => Some(Jump::Run(open)),
            }
        }
        Kind::EngineError => Some(Jump::Logs(item.event)),
        Kind::EngineDown => Some(Jump::Run(Action::EnsureWatch)),
        Kind::Question => item.context.as_ref().map(|c| Jump::Run(Action::OpenManager(c.clone()))),
        Kind::PlanReview => {
            let open = open_lead(tree, item.plan.as_ref()?)?;
            let page = item.url.as_ref().map_or_else(|| Action::Note(NO_REVIEW_PAGE.into()), |url| Action::CopyUrl(url.clone()));
            Some(Jump::Steps(vec![open, page]))
        }
        Kind::PrReady => Pull::from_subject(&item.subject).map(Jump::Merge),
        Kind::PrFailing | Kind::PrChanges | Kind::ReviewRequested => {
            item.url.as_ref().map(|url| Jump::Run(Action::CopyUrl(url.clone())))
        }
    }
}

/// The note of a `plan-review` click when the Plan has no port file, so no page to copy.
const NO_REVIEW_PAGE: &str = "no review page yet";

/// `session open` of the Lead of Plan `plan` (`session history` if archived); none for a Plan the tree does not have.
fn open_lead(tree: &Tree, plan: &PlanId) -> Option<Action> {
    let plan = tree.contexts.iter().flat_map(|c| &c.plans).find(|p| &p.head.id == plan)?;
    Some(open_or_history(tree, &plan.head.lead_session))
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
/// is on (its row is inverted across its width). `shape` is the section's, as capped and scrolled.
pub fn draw(f: &mut Frame, area: Rect, items: &[Item], shape: Shape, cursor: Option<usize>, now: OffsetDateTime, frame: usize) {
    let win = shape.window();
    let list = usize::from(shape.height()) - 1 - usize::from(shape.has_toggle());
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
                let text = if shape.expanded { "− less".to_owned() } else { format!("+{} more", items.len() - SHOWN) };
                f.render_widget(Paragraph::new(Span::styled(text, palette::fg(Meaning::Nothing))), row);
            }
            None if win.cue_above && dy == 1 => {
                f.render_widget(Paragraph::new(dim(format!(" ▲ {} more", win.first))), row);
            }
            None if win.below > 0 && usize::from(dy) == list => {
                f.render_widget(Paragraph::new(dim(format!(" ▼ {} more", win.below))), row);
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
    use cpd_core::ids::{ContextId, JobKey, PlanId};
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
    fn expanded_the_section_is_capped_but_never_below_the_collapsed_one_and_its_items_scroll_between_cues() {
        let shape = |count, cap, scroll| Shape::new(count, true).capped(cap).scrolled(scroll);
        // 50 items in a 40-row column: 20 rows, the header, 18 list rows, the toggle
        assert_eq!(shape(50, 20, 0).height(), 20);
        assert_eq!(shape(5, 20, 0).height(), 7, "a short list is not padded to the cap");
        assert_eq!(shape(50, 2, 0).height(), 5, "never less than the collapsed section");
        assert_eq!(Shape::new(50, false).capped(20).height(), 5);
        let rows = |s: Shape| (0..s.height() + 1).map(|r| s.hit(r)).collect::<Vec<_>>();
        let top = rows(shape(50, 8, 0));
        // the header, 6 list rows (5 items and the ▼ cue), the toggle
        assert_eq!(top, [Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2)), Some(Hit::Item(3)), Some(Hit::Item(4)), None, Some(Hit::Toggle), None]);
        let middle = rows(shape(50, 8, 10));
        assert_eq!(middle, [Some(Hit::Header), None, Some(Hit::Item(10)), Some(Hit::Item(11)), Some(Hit::Item(12)), Some(Hit::Item(13)), None, Some(Hit::Toggle), None]);
        // the offset clamps: the last item reaches the bottom, under the ▲ cue
        let bottom = rows(shape(50, 8, 999));
        assert_eq!(bottom, [Some(Hit::Header), None, Some(Hit::Item(45)), Some(Hit::Item(46)), Some(Hit::Item(47)), Some(Hit::Item(48)), Some(Hit::Item(49)), Some(Hit::Toggle), None]);
        assert_eq!(shape(50, 8, 0).max_scroll(), 45);
        assert_eq!(Shape::new(50, false).max_scroll(), 0);
        assert_eq!(shape(5, 20, 3).max_scroll(), 0, "nothing to scroll when all fit");
    }

    #[test]
    fn an_items_row_is_none_while_it_is_hidden_and_scroll_to_brings_it_in() {
        let shape = Shape::new(50, true).capped(8);
        assert_eq!((shape.row_of(0), shape.row_of(4), shape.row_of(5)), (Some(1), Some(5), None));
        assert_eq!(shape.scrolled(10).row_of(10), Some(2), "under the ▲ cue");
        assert_eq!(shape.scroll_to(4), 0, "already in view");
        let down = shape.scroll_to(20);
        assert_eq!((down, shape.scrolled(down).row_of(20)), (17, Some(5)), "the last row above the ▼ cue");
        assert_eq!(shape.scrolled(30).scroll_to(29), 29, "above the window: it becomes the first");
        assert_eq!(shape.scroll_to(49), 45);
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
        assert_eq!(jump(&tree, &with(Kind::JobStuck, &plan)), open("l-ship"), "no Job: the Lead of the item's Plan only");
        let job = |item: &mut Item| {
            plan(item);
            item.job = Some(JobKey::parse("c").unwrap());
        };
        assert_eq!(
            jump(&tree, &with(Kind::JobStuck, &job)),
            Some(Jump::Steps(vec![Action::Open(sid("l-ship")), Action::OpenWorker { plan: PlanId::parse("ship").unwrap(), job: JobKey::parse("c").unwrap() }])),
            "the Lead of the item's Plan, then the Worker of the Job's latest Attempt"
        );
        assert_eq!(jump(&tree, &with(Kind::EngineError, &event)), Some(Jump::Logs(Some(7))));
        assert_eq!(jump(&tree, &with(Kind::EngineDown, &|_| {})), Some(Jump::Run(Action::EnsureWatch)), "engine-down starts the engine");
        assert_eq!(jump(&tree, &with(Kind::Question, &context)), Some(Jump::Run(Action::OpenManager(ContextId::parse("alpha").unwrap()))));
        let url = |item: &mut Item| item.url = Some("https://example.com".into());
        let owned = |item: &mut Item| {
            plan(item);
            url(item);
        };
        let copy = Some(Jump::Run(Action::CopyUrl("https://example.com".into())));
        let steps = |action| Some(Jump::Steps(vec![Action::Open(sid("l-ship")), action]));
        assert_eq!(
            jump(&tree, &with(Kind::PlanReview, &owned)),
            steps(Action::CopyUrl("https://example.com".into())),
            "the Lead of the review's Plan, then the page's url"
        );
        assert_eq!(
            jump(&tree, &with(Kind::PlanReview, &plan)),
            steps(Action::Note("no review page yet".into())),
            "no port file: the Lead, and the note says so"
        );
        let merge = |subject: &str| Pull::from_subject(subject).map(Jump::Merge);
        let ready = |subject: &'static str| move |item: &mut Item| item.subject = subject.to_owned();
        assert!(merge("pr:acme/api#3").is_some());
        assert_eq!(jump(&tree, &with(Kind::PrReady, &ready("pr:acme/api#3"))), merge("pr:acme/api#3"), "a pull request no Plan owns");
        let owned_ready = |item: &mut Item| {
            plan(item);
            ready("pr:acme/api#3")(item);
        };
        assert_eq!(jump(&tree, &with(Kind::PrReady, &owned_ready)), merge("pr:acme/api#3"), "a Plan's pull request");
        assert_eq!(jump(&tree, &with(Kind::PrReady, &|i| i.url = Some("https://example.com".into()))), None, "subject:1 is no pull request");
        for subject in ["acme/api#3", "pr:api#3", "pr:acme/api", "pr:acme/api#x", "pr:acme/api#0", "session:l-ship", "pr:"] {
            assert_eq!(jump(&tree, &with(Kind::PrReady, &ready(subject))), None, "{subject:?} gives no merge popup");
        }
        for kind in [Kind::PrFailing, Kind::PrChanges] {
            assert_eq!(jump(&tree, &with(kind, &owned)), copy, "{kind:?} copies its url, a Plan's or not");
            assert_eq!(jump(&tree, &with(kind, &url)), copy);
        }
        assert_eq!(jump(&tree, &with(Kind::ReviewRequested, &url)), copy);
        assert_eq!(jump(&tree, &with(Kind::ReviewRequested, &owned)), copy, "a review request is never a Plan's");
        assert_eq!(jump(&tree, &with(Kind::PrFailing, &|_| {})), None, "a link without a url has nothing to copy");
        assert_eq!(jump(&tree, &with(Kind::PlanReview, &|_| {})), None);
        // an item without what its jump needs jumps nowhere
        assert_eq!(jump(&tree, &with(Kind::Dialog, &|_| {})), None);
        assert_eq!(jump(&tree, &with(Kind::JobStuck, &|i| i.plan = Some(PlanId::parse("nope").unwrap()))), None);
        assert_eq!(jump(&tree, &with(Kind::JobStuck, &|i| i.job = Some(JobKey::parse("c").unwrap()))), None, "a Job with no Plan");
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
        terminal.draw(|f| draw(f, f.area(), &[linked], Shape::new(1, false), None, at("2026-01-01T00:03:30.000Z"), 0)).unwrap();
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
        terminal.draw(|f| draw(f, f.area(), &[dialog, no_url], Shape::new(2, false), None, at("2026-01-01T00:03:30.000Z"), 0)).unwrap();
        assert!(!printed(&sink.0.borrow()).contains("\x1b]8;;"));
    }
}
