//! The Inbox section at the bottom of the tree column (commands.md §13.2, §17; contract §2.2 "Inbox item"): a collapsible
//! widget (`crate::fold`) with its margins, one empty row above and one below (hs-d476): the grey pill title
//! `╭─Inbox────…────<n>` (`crate::section`, inside the panel's padding), up to [`SHOWN`] item rows `│  <glyph> <label>` (each with
//! its tier's glyph and colour, hs-d483) with the age right-aligned, and the fold widget's closing row (`crate::fold`): with more items
//! the muted `╰󰁋 expand` that expands the list inline and becomes `╰󰁣 collapse`, with fewer a plain `╰─`, with none no
//! row. Only the toggle row toggles. Whether it is expanded is view state only. Expanded, it is capped at the rows the
//! column's split gives it (the tree has two thirds, hs-d307; `View::draw_at`) and its items scroll inside the cap, with right-aligned `▲ <n> more` / `▼ <n> more` cue rows
//! (`dialog::scroll::cue`, [`Shape`]).
//!
//! A click on an item row jumps to its cause ([`jump`]): a command, the merge popup of
//! its pull request, or the copy of its `url`. The label of a link kind is also an OSC 8 link to the `url` (`crate::link`, which the terminal opens with its
//! modifier-click). Everything here is pure but [`draw`], which only writes to the frame.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::actions::Action;
use super::rows::truncate;
use crate::fold;
use crate::link::link;
use crate::merge::Pull;
use crate::palette::{self, Meaning};
use crate::read::Tree;
use crate::section;
use crate::dialog::scroll::{Cue, cue};
use crate::widgets::{Window, max_scroll, window};
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Item, Kind};

/// How many item rows the section shows before the `╰󰁋 expand` row.
pub const SHOWN: usize = 3;

/// The columns before the label: the fold's `│`, two spaces, the item's tier glyph and a space.
const LEAD_WIDTH: usize = 5;

/// The rows above the items: the empty row (hs-d346) and the title.
const TOP: usize = 2;

/// The rows below the closing row: the empty row (hs-d476).
const BOTTOM: usize = 1;

/// What a row of the section is, by its offset from the section's first row (the empty one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Header,
    /// The item at this index of the snapshot's Inbox.
    Item(usize),
    /// `╰󰁋 expand` or `╰󰁣 collapse`.
    Toggle,
}

/// The rows of the section for `count` visible items. Expanded, its height is capped at `cap` rows (never below the
/// collapsed section's) and the items scroll in it from the item `scroll`, with a `▲ <n> more` and a `▼ <n> more` cue row
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

    /// The rows the section wants: the empty row, the title, the items, the closing row (the toggle, a plain `╰─`, or
    /// with no item the up-to-date row, hs-d487) and the empty row below; expanded, at most the cap, but not less than
    /// collapsed.
    pub fn height(self) -> u16 {
        let close = 1;
        let collapsed = TOP + self.count.min(SHOWN) + close + BOTTOM;
        let full = TOP + self.count + close + BOTTOM;
        let height = if self.expanded { full.min(usize::from(self.cap)).max(collapsed) } else { collapsed };
        u16::try_from(height).unwrap_or(u16::MAX)
    }

    /// The rows between the title and the closing row: the items and their cues.
    fn list_rows(self) -> usize {
        usize::from(self.height()) - TOP - 1 - BOTTOM
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

    /// The row, below the section's first, of the item `i`, when it is drawn.
    pub fn row_of(self, i: usize) -> Option<u16> {
        let win = self.window();
        (win.first..win.first + win.shown).contains(&i).then(|| (TOP + usize::from(win.cue_above) + i - win.first) as u16)
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

    /// The row `row` rows below the section's first; the empty rows, a cue row and a plain closing row are none.
    pub fn hit(self, row: u16) -> Option<Hit> {
        let row = usize::from(row);
        let win = self.window();
        let list = self.list_rows();
        match row {
            1 => Some(Hit::Header),
            r if (TOP..TOP + list).contains(&r) => {
                let at = r - TOP;
                (at >= usize::from(win.cue_above) && at - usize::from(win.cue_above) < win.shown)
                    .then(|| Hit::Item(win.first + at - usize::from(win.cue_above)))
            }
            r if r == TOP + list && self.has_toggle() => Some(Hit::Toggle),
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

/// Where a plain click on `item` goes (commands.md §13.2, "Clicks on an Inbox item"): every kind acts, but a row of the old `engine-error` or `job-stuck` kind (hs-d314, hs-d318). `engine-down`
/// starts the engine; `pr-ready` opens the merge popup of its pull request (`None` for a subject that is not
/// `pr:<owner>/<repo>#<n>`); `plan-review` opens the Lead of its Plan and copies the review page's `url` (when there is
/// one); `review-waiting` opens its Session, then copies the `url` likewise; the `url` is copied for `review-requested` (their OSC 8 link stays for the
/// terminal's modifier-click). `None` only for an item without what its jump needs.
pub fn jump(tree: &Tree, item: &Item) -> Option<Jump> {
    match item.kind {
        Kind::Dialog | Kind::Unread => item.session.as_ref().map(|s| Jump::Run(open_or_history(tree, s))),
        // no click: the engine's errors, the stuck Jobs and PR check/review state reach no Inbox (hs-d314, hs-d318, hs-d470); only a row an older engine opened remains
        Kind::EngineError | Kind::JobStuck | Kind::PrFailing | Kind::PrChanges => None,
        Kind::EngineDown => Some(Jump::Run(Action::EnsureWatch)),
        Kind::Question => item.context.as_ref().map(|c| Jump::Run(Action::OpenManager(c.clone()))),
        Kind::PlanReview => {
            let open = open_lead(tree, item.plan.as_ref()?)?;
            // with no port file yet there is no page to copy: the Lead opens, and nothing else happens
            Some(match item.url.as_ref() {
                Some(url) => Jump::Steps(vec![open, Action::CopyUrl(url.clone())]),
                None => Jump::Run(open),
            })
        }
        // hs-d496: the Session that opened the plannotator session, then the review page's url (when known)
        Kind::ReviewWaiting => {
            let open = open_or_history(tree, item.session.as_ref()?);
            Some(match item.url.as_ref() {
                Some(url) => Jump::Steps(vec![open, Action::CopyUrl(url.clone())]),
                None => Jump::Run(open),
            })
        }
        Kind::PrReady => Pull::from_subject(&item.subject).map(Jump::Merge),
        Kind::ReviewRequested => {
            item.url.as_ref().map(|url| Jump::Run(Action::CopyUrl(url.clone())))
        }
    }
}

/// `session open` of the Lead of Plan `plan` (`session history` if archived); none for a Plan the tree does not have.
fn open_lead(tree: &Tree, plan: &PlanId) -> Option<Action> {
    let plan = tree.contexts.iter().flat_map(|c| &c.plans).find(|p| &p.head.id == plan)?;
    Some(open_or_history(tree, &plan.head.lead_session))
}

/// The kinds whose label is an OSC 8 link to the item's `url`.
fn is_link(kind: Kind) -> bool {
    matches!(kind, Kind::PlanReview | Kind::ReviewWaiting | Kind::PrReady | Kind::ReviewRequested)
}

/// An item's row: the fold's muted `│`, two spaces, the item's tier glyph in its urgency's colour, a space, the label cut with
/// `…` so that it never reaches the age, and the age. Returns the line and the number of cells of the label.
/// A merge of the item's pull request runs (hs-d495): `spin` is the spinner's frame, and the glyph cell shows it.
fn item_line(item: &Item, age: &str, width: usize, spin: Option<usize>) -> (Line<'static>, usize) {
    let room = width.saturating_sub(LEAD_WIDTH + 1 + age.chars().count());
    let label = truncate(&item.label, room);
    let label_width = label.chars().count();
    let gap = width.saturating_sub(LEAD_WIDTH + label_width + age.chars().count());
    let mut glyph = palette::tier_span(item.urgency);
    if let Some(frame) = spin {
        glyph.content = palette::SPINNER[frame % palette::SPINNER.len()].into();
    }
    let line = fold::content(fold::BRANCH, Line::from(vec![
        "  ".into(),
        glyph,
        " ".into(),
        label.into(),
        " ".repeat(gap).into(),
        Span::styled(age.to_owned(), palette::fg(Meaning::Nothing)),
    ]));
    (line, label_width)
}

/// A scroll cue row: the fold's muted `│` on the left, the cue right-aligned after it.
fn cue_line(which: Cue, n: usize, width: usize) -> Line<'static> {
    fold::content(fold::BRANCH, cue(which, n, width.saturating_sub(1)))
}

/// Draws the section into `area`, the panel's full width (its first row is the empty one; rows that do not fit are cut;
/// the title keeps the padding of one column on each side, the rows are drawn inside it): `items` are the
/// snapshot's visible open items, `now` the clock the ages are taken from, `cursor` the item the keyboard cursor
/// is on (its row is inverted across its width). `shape` is the section's, as capped and scrolled. `spinning` says, of an
/// item whose merge runs, the frame of the spinner its glyph cell shows instead of its tier glyph (hs-d495).
pub fn draw(f: &mut Frame, area: Rect, items: &[Item], shape: Shape, cursor: Option<usize>, now: OffsetDateTime, spinning: &dyn Fn(&Item) -> Option<usize>) {
    let win = shape.window();
    let list = shape.list_rows();
    for dy in 0..area.height.min(shape.height()) {
        let full = Rect::new(area.x, area.y + dy, area.width, 1);
        let row = full.inner(Margin::new(1, 0));
        let width = usize::from(row.width);
        let at = usize::from(dy);
        match shape.hit(dy) {
            Some(Hit::Header) => {
                f.render_widget(Paragraph::new(section::collapsible_title("Inbox", Some(items.len()), usize::from(full.width))), full);
            }
            Some(Hit::Item(i)) => {
                let item = &items[i];
                let (line, label_width) = item_line(item, &age_of(&item.opened_at, now), width, spinning(item));
                f.render_widget(Paragraph::new(line), row);
                if cursor == Some(i) {
                    f.buffer_mut().set_style(row, Modifier::REVERSED);
                }
                if let (true, Some(url)) = (is_link(item.kind), &item.url) {
                    let label = Rect::new(row.x + LEAD_WIDTH as u16, row.y, label_width as u16, 1);
                    link(f.buffer_mut(), label, url);
                }
            }
            Some(Hit::Toggle) => {
                f.render_widget(Paragraph::new(fold::toggle(shape.expanded)), row);
            }
            None if win.cue_above && at == TOP => {
                f.render_widget(Paragraph::new(cue_line(Cue::Above, win.first, width)), row);
            }
            None if win.below > 0 && at == TOP + list - 1 => {
                f.render_widget(Paragraph::new(cue_line(Cue::Below, win.below, width)), row);
            }
            None if at == TOP + list => {
                let close = if items.is_empty() { fold::done("up to date, drink some water") } else { fold::plain() };
                f.render_widget(Paragraph::new(close), row);
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
    fn expanded_the_section_is_capped_but_never_below_the_collapsed_one_and_its_items_scroll_between_cues() {
        let shape = |count, cap, scroll| Shape::new(count, true).capped(cap).scrolled(scroll);
        // 50 items in a 40-row column: 20 rows, the empty row, the title, 16 list rows, the toggle, the empty row
        assert_eq!(shape(50, 20, 0).height(), 20);
        assert_eq!(shape(5, 20, 0).height(), 9, "a short list is not padded to the cap");
        assert_eq!(shape(50, 2, 0).height(), 7, "never less than the collapsed section");
        assert_eq!(Shape::new(50, false).capped(20).height(), 7);
        let rows = |s: Shape| (0..s.height() + 1).map(|r| s.hit(r)).collect::<Vec<_>>();
        let top = rows(shape(50, 9, 0));
        // the empty row, the title, 5 list rows (4 items and the ▼ cue), the toggle, the empty row
        assert_eq!(top, [None, Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2)), Some(Hit::Item(3)), None, Some(Hit::Toggle), None, None]);
        let middle = rows(shape(50, 9, 10));
        assert_eq!(middle, [None, Some(Hit::Header), None, Some(Hit::Item(10)), Some(Hit::Item(11)), Some(Hit::Item(12)), None, Some(Hit::Toggle), None, None]);
        // the offset clamps: the last item reaches the bottom, under the ▲ cue
        let bottom = rows(shape(50, 9, 999));
        assert_eq!(bottom, [None, Some(Hit::Header), None, Some(Hit::Item(46)), Some(Hit::Item(47)), Some(Hit::Item(48)), Some(Hit::Item(49)), Some(Hit::Toggle), None, None]);
        assert_eq!(shape(50, 9, 0).max_scroll(), 46);
        assert_eq!(Shape::new(50, false).max_scroll(), 0);
        assert_eq!(shape(5, 20, 3).max_scroll(), 0, "nothing to scroll when all fit");
    }

    #[test]
    fn an_items_row_is_none_while_it_is_hidden_and_scroll_to_brings_it_in() {
        let shape = Shape::new(50, true).capped(9);
        assert_eq!((shape.row_of(0), shape.row_of(3), shape.row_of(4)), (Some(2), Some(5), None));
        assert_eq!(shape.scrolled(10).row_of(10), Some(3), "under the ▲ cue");
        assert_eq!(shape.scroll_to(3), 0, "already in view");
        let down = shape.scroll_to(20);
        assert_eq!((down, shape.scrolled(down).row_of(20)), (18, Some(5)), "the last row above the ▼ cue");
        assert_eq!(shape.scrolled(30).scroll_to(29), 29, "above the window: it becomes the first");
        assert_eq!(shape.scroll_to(49), 46);
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
        let open = |id: &str| Some(Jump::Run(Action::Open(sid(id))));
        assert_eq!(jump(&tree, &with(Kind::Dialog, &session)), open("m-alpha"));
        assert_eq!(jump(&tree, &with(Kind::Unread, &session)), open("m-alpha"));
        let job = |item: &mut Item| {
            plan(item);
            item.job = Some(JobKey::parse("c").unwrap());
        };
        assert_eq!(jump(&tree, &with(Kind::JobStuck, &job)), None, "job-stuck: no click, a stuck Job goes to the Manager (hs-d318)");
        assert_eq!(jump(&tree, &with(Kind::JobStuck, &plan)), None, "not even with no Job: the Lead of the item's Plan stays closed");
        assert_eq!(jump(&tree, &with(Kind::EngineError, &|_| {})), None, "engine-error: no click (hs-d314)");
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
            Some(Jump::Run(Action::Open(sid("l-ship")))),
            "no port file: the Lead opens and nothing else happens"
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
            assert_eq!(jump(&tree, &with(kind, &owned)), None, "{kind:?} (an old row, hs-d470): no click, a Plan's or not");
            assert_eq!(jump(&tree, &with(kind, &url)), None, "{kind:?} (an old row, hs-d470): no url copy");
        }
        assert_eq!(jump(&tree, &with(Kind::ReviewRequested, &url)), copy);
        assert_eq!(jump(&tree, &with(Kind::ReviewRequested, &owned)), copy, "a review request is never a Plan's");
        assert_eq!(jump(&tree, &with(Kind::ReviewRequested, &|_| {})), None, "a link without a url has nothing to copy");
        assert_eq!(jump(&tree, &with(Kind::PlanReview, &|_| {})), None);
        // an item without what its jump needs jumps nowhere
        assert_eq!(jump(&tree, &with(Kind::Dialog, &|_| {})), None);
        assert_eq!(jump(&tree, &with(Kind::Question, &|_| {})), None);
        // an archived Session shows its history, as a click on its tree row
        assert_eq!(
            jump(&archived(), &with(Kind::Unread, &|i| i.session = Some(sid("f-blocked")))),
            Some(Jump::Run(Action::History(sid("f-blocked"))))
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
    fn a_link_kinds_label_is_written_as_an_osc_8_link_and_its_lead_and_age_are_not() {
        let sink = Sink::default();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(sink.clone()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 41, 5)) },
        )
        .unwrap();
        let url = "https://github.com/acme/api/pull/12";
        let mut linked = item(1, Kind::ReviewRequested, Urgency::Red, "api#12 failing", OPENED);
        linked.url = Some(url.into());
        terminal.draw(|f| draw(f, f.area(), &[linked], Shape::new(1, false), None, at("2026-01-01T00:03:30.000Z"), &|_| None)).unwrap();
        let bytes = sink.0.borrow().clone();
        let pieces: String = ["ap", "i#", "12", " f", "ai", "li", "ng"]
            .iter()
            .map(|p| format!("\x1b]8;;{url}\x07{p}\x1b]8;;\x07"))
            .collect();
        let text = printed(&bytes);
        assert!(text.contains(&format!("│{}{pieces}3m", palette::TIER_RED)), "the branch, the icon (the spaces around it are blank cells, not printed), the label in pieces of two cells, the age: {text:?}");
        assert_eq!(text.matches("\x1b]8;;https").count(), 7, "one link per piece, none around the lead (branch and icon) or the age");
        // an item with no url, or of a kind that jumps, has no link
        let sink = Sink::default();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(sink.clone()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 41, 6)) },
        )
        .unwrap();
        let mut dialog = item(2, Kind::Dialog, Urgency::Red, "dialog", OPENED);
        dialog.url = Some(url.into());
        let no_url = item(3, Kind::PlanReview, Urgency::Yellow, "review", OPENED);
        terminal.draw(|f| draw(f, f.area(), &[dialog, no_url], Shape::new(2, false), None, at("2026-01-01T00:03:30.000Z"), &|_| None)).unwrap();
        assert!(!printed(&sink.0.borrow()).contains("\x1b]8;;"));
    }

    // ---- review-waiting (hs-d496): the click opens the item's Session then copies its url; the label is an OSC 8 link ----

    #[test]
    fn a_review_waiting_click_opens_its_session_then_copies_its_url() {
        let tree = sample();
        let mut waiting = item(1, Kind::ReviewWaiting, Urgency::Magenta, "pn:m-alpha/pn-0a1b2c", OPENED);
        waiting.session = Some(sid("m-alpha"));
        waiting.url = Some("http://review.example:20000/".into());
        assert_eq!(
            jump(&tree, &waiting),
            Some(Jump::Steps(vec![Action::Open(sid("m-alpha")), Action::CopyUrl("http://review.example:20000/".into())])),
            "the Session first, then the page's url"
        );
        waiting.url = None;
        assert_eq!(jump(&tree, &waiting), Some(Jump::Run(Action::Open(sid("m-alpha")))), "no url: the Session opens and nothing else happens");
        waiting.session = None;
        assert_eq!(jump(&tree, &waiting), None, "no Session: nowhere to go");
        let mut archived_session = item(2, Kind::ReviewWaiting, Urgency::Magenta, "pn:f-blocked/pn-0a1b2c", OPENED);
        archived_session.session = Some(sid("f-blocked"));
        assert_eq!(jump(&archived(), &archived_session), Some(Jump::Run(Action::History(sid("f-blocked")))), "an archived Session shows its history");
    }

    #[test]
    fn a_review_waiting_label_is_written_as_an_osc_8_link() {
        let sink = Sink::default();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(sink.clone()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 41, 5)) },
        )
        .unwrap();
        let url = "http://review.example:20000/";
        let mut waiting = item(1, Kind::ReviewWaiting, Urgency::Magenta, "ab", OPENED);
        waiting.url = Some(url.into());
        terminal.draw(|f| draw(f, f.area(), &[waiting], Shape::new(1, false), None, at("2026-01-01T00:03:30.000Z"), &|_| None)).unwrap();
        let text = printed(&sink.0.borrow());
        assert!(text.contains(&format!("\x1b]8;;{url}\x07ab\x1b]8;;\x07")), "the label is a link to the review page: {text:?}");
    }

    // ---- spacing v2 (hs-d476): the section is a collapsible widget with an empty row above and one below; red until `Shape` has the row below ----

    #[test]
    fn the_section_is_an_empty_row_the_title_the_items_the_closing_row_and_an_empty_row() {
        let rows = |count, expanded| {
            let shape = Shape::new(count, expanded);
            (0..shape.height()).map(|r| shape.hit(r)).collect::<Vec<_>>()
        };
        assert_eq!(rows(0, false), [None, Some(Hit::Header), None, None], "no item: the up-to-date closing row (no hit, hs-d487), then the empty row");
        assert_eq!(rows(2, false), [None, Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), None, None], "a plain closing row, then the empty row");
        assert_eq!(rows(3, true), rows(3, false), "three items have nothing to expand");
        assert_eq!(rows(5, false), [None, Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2)), Some(Hit::Toggle), None]);
        assert_eq!(
            rows(5, true),
            [None, Some(Hit::Header), Some(Hit::Item(0)), Some(Hit::Item(1)), Some(Hit::Item(2)), Some(Hit::Item(3)), Some(Hit::Item(4)), Some(Hit::Toggle), None]
        );
        assert_eq!((Shape::new(0, false).height(), Shape::new(3, false).height(), Shape::new(5, false).height(), Shape::new(5, true).height()), (4, 7, 7, 9));
    }

    #[test]
    fn capped_the_section_is_never_below_the_folded_one_empty_rows_included() {
        assert_eq!(Shape::new(50, true).capped(20).height(), 20);
        assert_eq!(Shape::new(50, true).capped(2).height(), 7, "the empty row, the title, three items, the closing row, the empty row");
        assert_eq!(Shape::new(50, false).capped(20).height(), 7);
        let bottom = Shape::new(50, true).capped(20);
        assert_eq!(bottom.hit(18), Some(Hit::Toggle), "the closing row is the last but one");
        assert_eq!(bottom.hit(19), None, "and the last is the empty row");
    }

    // ---- the empty Inbox (hs-d487): the title with count 0, then one muted closing row, no toggle ----

    #[test]
    fn with_no_item_the_section_is_the_title_with_count_0_then_the_one_closing_row_and_it_is_no_click() {
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(41, 6)).unwrap();
        terminal.draw(|f| draw(f, f.area(), &[], Shape::new(0, false), None, at("2026-01-01T00:00:00.000Z"), &|_| None)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| (0..41).map(|x| buffer[(x, y)].symbol()).collect::<String>().trim_end().to_owned();
        let title = format!(" ╭─\u{E0B6}Inbox\u{E0B4}{}\u{E0B6}0\u{E0B4}", "─".repeat(39 - 2 - 2 - "Inbox".len() - 3));
        let close = " ╰\u{F005A} up to date, drink some water";
        let rows: Vec<String> = (0..6).map(row).collect();
        assert_eq!(rows[..3], ["".to_owned(), title, close.to_owned()], "the empty row, the title with count 0, the closing row: {rows:?}");
        assert!(rows[3..].iter().all(String::is_empty), "no other row: {rows:?}");
        let muted = palette::fg(Meaning::Nothing).fg.unwrap();
        for x in 1..close.chars().count() as u16 {
            assert_eq!(buffer[(x, 2)].fg, muted, "cell {x} of the closing row is muted");
        }
        for y in 0..4 {
            assert_eq!(Shape::new(0, false).hit(y), if y == 1 { Some(Hit::Header) } else { None }, "row {y}: only the title is a hit, the closing row is no toggle");
        }
    }
}
