//! The Inbox section at the bottom of the right column (commands.md §13.2, §17; contract §2.2 "Inbox item"): a flat list, no
//! rail, no fold. Its rows, top to bottom: a blank row, the header (`crate::section::flush_title`, `───<Inbox>───…<n>─`, from
//! column 0), the ▲ line, one row or two per item (Row 1 `<glyph> <label> … <age>`, Row 2 the muted subtitle, only when the label has one; each with its tier's glyph and colour, hs-d483; one blank
//! column on each side), and the ▼ line; with no item the list is the one muted row `up to date, drink some water`. Both cue
//! lines are always reserved (blank when nothing is hidden); with more items than the column's rows give it, the items scroll
//! with the wheel and the cue lines read `▲ <n> more` / `▼ <n> more` (`dialog::scroll::cue`, [`Shape`]).
//!
//! A click on an item row jumps to its cause ([`jump`]): a command or the copy of its `url` (a `pr-ready` row has none: its menu has Merge). The label of a link kind with a `url` is a `Link` widget (`crate::link`): a click on it opens the page
//! instead. Everything here is pure but [`draw`], which only writes to the frame.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::Window;
use crate::cells::{self, truncate};
use crate::dialog::scroll::WHEEL_ROWS;
use crate::dialog::scroll::{Cue, cue};
use crate::link::{Link, Links};
pub(crate) use crate::merge::Pull;
use crate::palette;
use crate::phi;
use crate::read::{Tree, age_text};
use crate::section;
use crate::tree::actions::Action;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Item, Kind};
use cpd_core::inbox::pr::Status;

/// The columns before the label: the item's tier glyph and a space.
const LEAD_WIDTH: usize = 2;

/// The rows above the list: the blank row, the header and the ▲ line.
const TOP: usize = 3;

/// The rows around the list: the three above and the ▼ line.
const AROUND: usize = TOP + 1;

/// Whether the merge of the `pr-ready` `item`'s pull request runs: `merging` is the pull requests (`<owner>/<repo>#<n>`) with a live
/// merge marker (hs-d495).
pub(crate) fn merges(merging: &std::collections::BTreeSet<String>, item: &Item) -> bool {
    item.kind == Kind::PrReady
        && Pull::from_subject(&item.subject).is_some_and(|pull| merging.contains(&pull.to_string()))
}

/// What a row of the section is, by its offset from the section's first row (the blank one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Header,
    /// The item at this index of the snapshot's Inbox.
    Item(usize),
}

/// The rows of an item: 1, plus 1 when the text after its label's first `\n` is non-empty (pro-d177). The one place the rule lives.
pub(crate) fn item_height(item: &Item) -> usize {
    1 + usize::from(!rows_of(&item.label).1.is_empty())
}

/// The rows of the section for its items: four rows and the list, which is capped with the section at `cap` rows and then
/// scrolls from the item `scroll`, [`item_height`] rows an item ([`Window::weighted`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shape {
    heights: Vec<usize>,
    cap: u16,
    scroll: usize,
}

impl Shape {
    /// The section as tall as its `items` want (no cap), at the top.
    pub fn new(items: &[Item]) -> Shape {
        Shape {
            heights: items.iter().map(item_height).collect(),
            cap: u16::MAX,
            scroll: 0,
        }
    }

    /// The same, at most `cap` rows tall.
    pub fn capped(self, cap: u16) -> Shape {
        Shape { cap, ..self }
    }

    /// The same, scrolled to the item `scroll` (clamped on use).
    pub fn scrolled(self, scroll: usize) -> Shape {
        Shape { scroll, ..self }
    }

    /// The rows the section wants: the list (the items' heights, one row at least: the up-to-date row) and four more, at most the cap.
    pub fn height(&self) -> u16 {
        let height =
            (self.heights.iter().sum::<usize>().max(1) + AROUND).min(usize::from(self.cap));
        u16::try_from(height).unwrap_or(u16::MAX)
    }

    /// The rows of the list, between the two cue lines.
    fn list_rows(&self) -> usize {
        usize::from(self.height()).saturating_sub(AROUND)
    }

    /// The items drawn and the cues: the items from the offset on that fit the list; a spare row stays blank, no item splits.
    fn window(&self) -> Window {
        Window::weighted(&self.heights, self.list_rows(), self.scroll)
    }

    /// The largest scroll offset: the first from which every remaining item fits. 0 when all fit.
    pub fn max_scroll(&self) -> usize {
        Window::max_scroll_weighted(&self.heights, self.list_rows())
    }

    /// The scroll offset after a wheel notch down or up over the section: [`WHEEL_ROWS`] items a notch, clamped.
    pub fn wheeled(&self, down: bool) -> usize {
        let max = self.max_scroll();
        let scroll = self.scroll.min(max);
        if down {
            (scroll + WHEEL_ROWS).min(max)
        } else {
            scroll.saturating_sub(WHEEL_ROWS)
        }
    }

    /// The row `row` rows below the section's first: the header, or either row of an item; the blank row, the spare blank row, the
    /// cue lines and the up-to-date row are none.
    pub fn hit(&self, row: u16) -> Option<Hit> {
        let row = usize::from(row);
        if row == 1 {
            return Some(Hit::Header);
        }
        let win = self.window();
        let mut top = TOP;
        for at in win.first..win.first + win.shown {
            let next = top + self.heights[at];
            if (top..next).contains(&row) {
                return Some(Hit::Item(at));
            }
            top = next;
        }
        None
    }
}

/// The age of an item opened at `opened_at` (§1.3's RFC 3339), at `now`; `?` when the stored time does not parse.
fn age_of(opened_at: &str, now: OffsetDateTime) -> String {
    OffsetDateTime::parse(opened_at, &Rfc3339)
        .map_or_else(|_| "?".to_owned(), |t| age_text((now - t).whole_seconds()))
}

/// What a click on an item does besides the link's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jump {
    /// Run this command and focus the chat slot (a click on a Session row).
    Run(Action),
    /// The checklist dialog of a `curate-retire` item of this Context (hs-d549, hs-d551).
    Curate(cpd_core::ids::ContextId),
    /// The read-only `text` dialog of the announced ids of a `models-changed` item: its provider and `changes.at`.
    Models(String, String),
}

/// The body of the `models-changed` dialog (commands.md §13.2): the announced ids of the change `at` of `provider`, the new
/// ones under `New (<N>)` and the gone ones under `Gone (<M>)`, one id per line, a part with no ids omitted; when the file
/// no longer has that change, it says so.
pub fn models_body(
    home: &cpd_core::home::CpdHome,
    provider: &str,
    at: &str,
) -> Result<String, cpd_core::error::CpdError> {
    use cpd_core::inbox::models::announced;
    use cpd_core::models::Read;
    let file = cpd_core::models::read_all(home)?
        .into_iter()
        .find_map(|r| match r {
            Read::Good(f) if f.provider == provider => Some(f),
            _ => None,
        });
    let Some(changes) = file.and_then(|f| f.changes).filter(|c| c.at == at) else {
        return Ok("changed again; the item closes at the next tick".to_owned());
    };
    let (new, gone) = announced(provider, &changes);
    let part = |name: &str, ids: &[String]| {
        (!ids.is_empty()).then(|| format!("{name} ({})\n{}", ids.len(), ids.join("\n")))
    };
    Ok([part("New", &new), part("Gone", &gone)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n"))
}

/// One row of a curation checklist: the record, its full text and the detail after the dash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub text: String,
    pub detail: String,
}

/// The checklist dialog a curate item opens: its title and rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checklist {
    pub title: String,
    pub rows: Vec<Row>,
}

/// The checklist for `context` from the reply of `cpd context curation` (commands.md §13.2): the pending
/// `retire` entries; `None` when none is pending (or the reply has none of the shape).
pub fn checklist(
    context: &cpd_core::ids::ContextId,
    reply: &serde_json::Value,
) -> Option<Checklist> {
    let (key, title) = ("retire", format!("Retire records of {context}"));
    let text = |e: &serde_json::Value, k: &str| e[k].as_str().map(str::to_owned);
    let rows = reply[key]
        .as_array()?
        .iter()
        .map(|e| {
            let detail = format!("{} {}", text(e, "reason")?, text(e, "target")?);
            Some(Row {
                id: text(e, "record")?,
                text: text(e, "text")?,
                detail,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let total = reply[format!("{key}_total")]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(rows.len());
    let title = if total > rows.len() {
        format!("{title} ({} of {total})", rows.len())
    } else {
        title
    };
    (!rows.is_empty()).then_some(Checklist { title, rows })
}

/// The most entries one checklist shows (hs-d569 CD4).
const CURATION_LIMIT: usize = 10;

/// The `cpd` arguments of the curation request: at most [`CURATION_LIMIT`] entries of each list.
pub fn curation_argv(context: &cpd_core::ids::ContextId) -> Vec<String> {
    vec![
        "context".into(),
        "curation".into(),
        format!("--context={context}"),
        format!("--limit={CURATION_LIMIT}"),
    ]
}

/// The `cpd` arguments an approved checklist runs (commands.md §13.2); a flag with no ids is left out.
pub fn approved(
    context: &cpd_core::ids::ContextId,
    ticked: &[String],
    unticked: &[String],
) -> Vec<String> {
    let mut argv = vec![
        "context".to_owned(),
        "retire".to_owned(),
        format!("--context={context}"),
    ];
    for (flag, ids) in [("approve", ticked), ("reject", unticked)] {
        if !ids.is_empty() {
            argv.push(format!("--{flag}={}", ids.join(",")));
        }
    }
    argv
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
    if archived {
        Action::History(id.clone())
    } else {
        Action::Open(id.clone())
    }
}

/// Where a plain click on `item` goes (commands.md §13.2, "Clicks on an Inbox item"): every kind acts, but a row of the old `engine-error` or `job-stuck` kind (hs-d314, hs-d318) or the terminator `╰󰬪` (hs-d574). `engine-down`
/// starts the engine; `pr-ready` has none (the label is a link; Merge is in its menu); `plan-review` opens the Lead of its Plan; the `url` is copied for `review-requested` (a click on the label of a link kind opens its page instead, see [`Link`]). `None` only for an item without what its jump needs.
pub fn jump(tree: &Tree, item: &Item) -> Option<Jump> {
    match item.kind {
        Kind::Dialog | Kind::Unread | Kind::LoopGuard => item
            .session
            .as_ref()
            .map(|s| Jump::Run(open_or_history(tree, s))),
        // no click: the engine's errors, the stuck Jobs and Workflow check/review state reach no Inbox (hs-d314, hs-d318, hs-d470); only a row an older engine opened remains
        // `curate-docs` is no longer opened (pro-d327): a history row has no click
        Kind::EngineError
        | Kind::JobStuck
        | Kind::PrFailing
        | Kind::PrChanges
        | Kind::CurateDocs => None,
        Kind::EngineDown => Some(Jump::Run(Action::EnsureWatch)),
        Kind::Question => item
            .context
            .as_ref()
            .map(|c| Jump::Run(Action::OpenManager(c.clone()))),
        Kind::PlanReview => {
            // a plan-review has no url (hs-d739): the Lead of its Plan opens
            open_lead(tree, item.plan.as_ref()?).map(Jump::Run)
        }
        Kind::CurateRetire => item.context.as_ref().map(|c| Jump::Curate(c.clone())),
        Kind::ModelsChanged => cpd_core::inbox::models::parse_subject(&item.subject)
            .map(|(p, at)| Jump::Models(p.to_owned(), at.to_owned())),
        // a deploy item has no jump; its label is a Link to the run's page (pro-d100)
        Kind::Deploy => None,
        // hs-d731: the Lead of the Plan opens, as for a `plan-review` with no url
        Kind::DraftsParked => open_lead(tree, item.plan.as_ref()?).map(Jump::Run),
        // the label is a Link to the pull request; a click on the rest of the row opens the owning Plan's Lead (pro-d130)
        Kind::PrReady | Kind::PrClosed => open_lead(tree, item.plan.as_ref()?).map(Jump::Run),
        Kind::ReviewRequested => item
            .url
            .as_ref()
            .map(|url| Jump::Run(Action::CopyUrl(url.clone()))),
    }
}

/// What a left click on `item` runs, as [`jump`] says but for what is already so: an action whose Session already is in the
/// chat slot is dropped.
pub fn click(tree: &Tree, item: &Item) -> Option<Jump> {
    let in_focus = |action: &Action| {
        let Action::Open(id) = action else {
            return false;
        };
        tree.contexts
            .iter()
            .flat_map(|c| &c.plans)
            .flat_map(|p| &p.sessions)
            .any(|s| &s.id == id && s.is_active())
    };
    match jump(tree, item)? {
        Jump::Run(action) if in_focus(&action) => None,
        jump => Some(jump),
    }
}

/// An entry of an item's right-click menu (commands.md §13.2): the labels are the contract's, exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Approve,
    Claim,
    Snooze15,
    Snooze1h,
    SnoozeTomorrow,
    Dismiss,
}

impl Choice {
    /// The entries of the menu of a row of `kind`, in order: `Approve` first on `deploy` rows only, `Claim` first on `review-requested` rows only.
    pub fn entries(kind: Kind) -> Vec<Choice> {
        let rest = [
            Choice::Snooze15,
            Choice::Snooze1h,
            Choice::SnoozeTomorrow,
            Choice::Dismiss,
        ];
        let first = match kind {
            Kind::Deploy => Some(Choice::Approve),
            Kind::ReviewRequested => Some(Choice::Claim),
            _ => None,
        };
        first.into_iter().chain(rest).collect()
    }

    /// The words of the command of the item `id`: `inbox approve <id>`, `inbox claim <id>`, `inbox snooze <id> <15m|1h|tomorrow>` (§21.3) or `inbox dismiss <id>` (§21.4).
    pub fn argv(self, id: i64) -> Vec<String> {
        let id = id.to_string();
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
            Choice::Approve => words(&["inbox", "approve", &id]),
            Choice::Claim => words(&["inbox", "claim", &id]),
            Choice::Snooze15 => words(&["inbox", "snooze", &id, "15m"]),
            Choice::Snooze1h => words(&["inbox", "snooze", &id, "1h"]),
            Choice::SnoozeTomorrow => words(&["inbox", "snooze", &id, "tomorrow"]),
            Choice::Dismiss => words(&["inbox", "dismiss", &id]),
        }
    }
}

impl crate::menu::Entry for Choice {
    fn label(self) -> &'static str {
        match self {
            Choice::Approve => "Approve",
            Choice::Claim => "Claim",
            Choice::Snooze15 => "Snooze 15 min",
            Choice::Snooze1h => "Snooze 1 h",
            Choice::SnoozeTomorrow => "Snooze until tomorrow 09:00",
            Choice::Dismiss => "Dismiss",
        }
    }
}

/// An entry of an item's menu as drawn: Merge or Post (a `pr-ready` item by its label's status, pro-d130), Go to Lead, Copy Link, or a [`Choice`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Merge,
    /// A merge of the pull request runs: the `Merge` entry with the spinner, not selectable.
    Merging,
    Post,
    GoToLead,
    CopyLink,
    Choice(Choice),
}

impl Entry {
    /// The entries of the menu of `item`, in order. A `pr-ready` item: `Merge` (`Merging` when `merging`) when its status is
    /// `ready to merge`, `Post` when `ready for user review` and it has a Plan, `Go to Lead` with a Plan, `Copy Link` with a url; then the
    /// [`Choice`]s (every other kind has only those).
    pub fn entries(item: &Item, merging: bool) -> Vec<Entry> {
        let mut entries = Vec::new();
        if item.kind == Kind::PrClosed {
            if item.plan.is_some() {
                entries.push(Entry::GoToLead);
            }
            if item.url.is_some() {
                entries.push(Entry::CopyLink);
            }
        }
        if item.kind == Kind::PrReady {
            match Status::of_item(item) {
                Some(Status::ReadyToMerge) => entries.push(if merging {
                    Entry::Merging
                } else {
                    Entry::Merge
                }),
                Some(Status::ReadyForReview) if item.plan.is_some() => entries.push(Entry::Post),
                _ => {}
            }
            if item.plan.is_some() {
                entries.push(Entry::GoToLead);
            }
            if item.url.is_some() {
                entries.push(Entry::CopyLink);
            }
        }
        entries.extend(Choice::entries(item.kind).into_iter().map(Entry::Choice));
        entries
    }
}

impl crate::menu::Entry for Entry {
    fn label(self) -> &'static str {
        match self {
            Entry::Merge | Entry::Merging => "Merge",
            Entry::Post => "Post",
            Entry::GoToLead => "Go to Lead",
            Entry::CopyLink => "Copy Link",
            Entry::Choice(choice) => crate::menu::Entry::label(choice),
        }
    }

    fn busy(self) -> bool {
        self == Entry::Merging
    }
}

/// `session open` of the Lead of Plan `plan` (`session history` if archived); none for a Plan the tree does not have.
fn open_lead(tree: &Tree, plan: &PlanId) -> Option<Action> {
    let plan = tree
        .contexts
        .iter()
        .flat_map(|c| &c.plans)
        .find(|p| &p.head.id == plan)?;
    Some(open_or_history(tree, &plan.head.lead_session))
}

/// The kinds whose label is a `Link` to the item's `url`.
fn is_link(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::PlanReview | Kind::PrReady | Kind::PrClosed | Kind::ReviewRequested | Kind::Deploy
    )
}

/// A label's two rows: the text before its one `\n`, and the subtitle after it (empty for a one-row label).
fn rows_of(label: &str) -> (&str, &str) {
    label.split_once('\n').unwrap_or((label, ""))
}

/// An item's Row 1, in the box's body: the item's tier glyph in its urgency's colour, a space, the label cut with
/// `…` so that it never reaches the age, and the age (`width` cells: the body's). Returns the line and the number of cells of the label.
/// A merge of the item's pull request runs (hs-d495): `spin` is the spinner's frame, and the glyph cell shows it.
/// The label of a link kind with a url is no text of the line: its cells are blank, for the [`Link`] drawn over them, which is as
/// wide as the label and the cue (cut to the room before the age); the number of cells is that width.
fn item_line(item: &Item, age: &str, width: usize, spin: Option<usize>) -> (Line<'static>, usize) {
    let room = width.saturating_sub(LEAD_WIDTH + 1 + cells::width(age));
    let head = rows_of(&item.label).0;
    let linked = link_url(item).is_some();
    let label = if linked {
        String::new()
    } else {
        truncate(head, room)
    };
    let label_width = if linked {
        (cells::width(head) + 2).min(room)
    } else {
        cells::width(&label)
    };
    let gap = width.saturating_sub(LEAD_WIDTH + label_width + cells::width(age));
    let mut glyph = palette::item_span(item.kind, item.urgency);
    if let Some(frame) = spin {
        glyph.content = palette::SPINNER[frame % palette::SPINNER.len()].into();
        glyph.style = palette::fg_clickable(palette::Meaning::Review); // a merge runs: yellow, whatever the tier
    }
    let mut spans = vec![glyph, " ".into()];
    if linked {
        spans.push(" ".repeat(label_width).into());
    } else {
        spans.extend(phi::spans(&label, palette::clickable_text()));
    }
    spans.push(" ".repeat(gap).into());
    spans.push(Span::styled(age.to_owned(), palette::muted_text()));
    let line = Line::from(spans);
    (line, label_width)
}

/// The url of the link of `item`: its `url`, for a link kind.
fn link_url(item: &Item) -> Option<&str> {
    item.url.as_deref().filter(|_| is_link(item.kind))
}

/// Draws the section into `area`, the pane's full width (its first row is the blank one; rows that do not fit are cut from the
/// bottom): `items` are the snapshot's visible open items, `now` the clock the ages are taken from, `shape` is the section's, as
/// capped and scrolled. `spinning` says, of an item whose merge runs, the frame of the spinner its glyph cell shows instead of its
/// tier glyph (hs-d495).
pub fn draw(
    f: &mut Frame,
    area: Rect,
    items: &[Item],
    shape: &Shape,
    now: OffsetDateTime,
    spinning: &dyn Fn(&Item) -> Option<usize>,
) -> Links {
    let mut drawn = Links::default();
    let width = usize::from(area.width);
    let inner = width.saturating_sub(2);
    let win = shape.window();
    let list = shape.list_rows();
    let buf = f.buffer_mut();
    // a line `row` rows below the section's first, `skip` columns in: rows below the area are cut
    let put = |buf: &mut Buffer, row: usize, skip: u16, line: Line<'static>| {
        let y = area
            .y
            .saturating_add(u16::try_from(row).unwrap_or(u16::MAX));
        if y < area.bottom() && skip < area.width {
            line.render(Rect::new(area.x + skip, y, area.width - skip, 1), buf);
        }
    };
    put(
        buf,
        1,
        0,
        section::flush_title("Inbox", Some(items.len()), width),
    );
    if win.cue_above {
        put(buf, 2, 1, cue(Cue::Above, win.first, inner));
    }
    if items.is_empty() {
        put(
            buf,
            TOP,
            1,
            Line::styled("up to date, drink some water", palette::muted_text()),
        );
    }
    let mut row = TOP;
    for item in &items[win.first..win.first + win.shown] {
        let (head, sub) = rows_of(&item.label);
        let (line, label_width) =
            item_line(item, &age_of(&item.opened_at, now), inner, spinning(item));
        put(buf, row, 1, line);
        if !sub.is_empty() {
            let mut spans = vec![" ".repeat(LEAD_WIDTH).into()];
            spans.extend(phi::spans(
                &truncate(sub, inner.saturating_sub(LEAD_WIDTH)),
                palette::muted_text(),
            ));
            put(buf, row + 1, 1, Line::from(spans));
        }
        let y = area
            .y
            .saturating_add(u16::try_from(row).unwrap_or(u16::MAX));
        row += item_height(item);
        if let Some(url) = link_url(item).filter(|_| y < area.bottom()) {
            let text = Line::from(phi::spans(head, palette::clickable_text()));
            drawn.draw(
                Link::new(url).text(text),
                Rect::new(area.x + 1 + LEAD_WIDTH as u16, y, label_width as u16, 1),
                buf,
            );
        }
    }
    if win.below > 0 {
        put(buf, TOP + list, 1, cue(Cue::Below, win.below, inner));
    }
    drawn
}

/// An open item for the tests: `kind` and `urgency`, its `label`, opened at `opened_at`; the rest is none.
#[cfg(test)]
pub(crate) fn item(
    id: i64,
    kind: Kind,
    urgency: cpd_core::inbox::model::Urgency,
    label: &str,
    opened_at: &str,
) -> Item {
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
        claimed_at: None,
    }
}

#[cfg(test)]
mod tests {
    use crate::palette::Meaning;

    use super::*;
    use crate::tree::fixture::{archived, sample};
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
    fn an_age_is_taken_from_opened_at_and_an_unreadable_time_is_a_question_mark() {
        assert_eq!(age_of(OPENED, at("2026-01-01T00:00:42.900Z")), "42s");
        assert_eq!(age_of(OPENED, at("2026-01-03T05:00:00.000Z")), "2d");
        assert_eq!(age_of("yesterday", at("2026-01-03T05:00:00.000Z")), "?");
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
        assert_eq!(
            jump(&tree, &with(Kind::JobStuck, &job)),
            None,
            "job-stuck: no click, a stuck Job goes to the Manager (hs-d318)"
        );
        assert_eq!(
            jump(&tree, &with(Kind::JobStuck, &plan)),
            None,
            "not even with no Job: the Lead of the item's Plan stays closed"
        );
        assert_eq!(
            jump(&tree, &with(Kind::EngineError, &|_| {})),
            None,
            "engine-error: no click (hs-d314)"
        );
        assert_eq!(
            jump(&tree, &with(Kind::EngineDown, &|_| {})),
            Some(Jump::Run(Action::EnsureWatch)),
            "engine-down starts the engine"
        );
        assert_eq!(
            jump(&tree, &with(Kind::Question, &context)),
            Some(Jump::Run(Action::OpenManager(
                ContextId::parse("alpha").unwrap()
            )))
        );
        let url = |item: &mut Item| item.url = Some("https://example.com".into());
        let owned = |item: &mut Item| {
            plan(item);
            url(item);
        };
        let copy = Some(Jump::Run(Action::CopyUrl("https://example.com".into())));
        assert_eq!(
            jump(&tree, &with(Kind::PlanReview, &owned)),
            Some(Jump::Run(Action::Open(sid("l-ship")))),
            "the Lead of the review's Plan, and nothing is copied (a plan-review has no url, hs-d739)"
        );
        assert_eq!(
            jump(&tree, &with(Kind::PlanReview, &plan)),
            Some(Jump::Run(Action::Open(sid("l-ship")))),
            "no port file: the Lead opens and nothing else happens"
        );
        let ready =
            |subject: &'static str| move |item: &mut Item| item.subject = subject.to_owned();
        for subject in ["pr:acme/api#3", "acme/api#3", "pr:", "session:l-ship"] {
            assert_eq!(
                jump(&tree, &with(Kind::PrReady, &ready(subject))),
                None,
                "{subject:?}: an unowned row: a click on the rest of it does nothing (pro-d130)"
            );
        }
        assert_eq!(
            jump(&tree, &with(Kind::PrReady, &owned)),
            Some(Jump::Run(Action::Open(sid("l-ship")))),
            "a Plan's pull request: a click outside the link opens its Lead (pro-d130)"
        );
        for kind in [Kind::PrFailing, Kind::PrChanges] {
            assert_eq!(
                jump(&tree, &with(kind, &owned)),
                None,
                "{kind:?} (an old row, hs-d470): no click, a Plan's or not"
            );
            assert_eq!(
                jump(&tree, &with(kind, &url)),
                None,
                "{kind:?} (an old row, hs-d470): no url copy"
            );
        }
        assert_eq!(jump(&tree, &with(Kind::ReviewRequested, &url)), copy);
        assert_eq!(
            jump(&tree, &with(Kind::ReviewRequested, &owned)),
            copy,
            "a review request is never a Plan's"
        );
        assert_eq!(
            jump(&tree, &with(Kind::ReviewRequested, &|_| {})),
            None,
            "a link without a url has nothing to copy"
        );
        assert_eq!(jump(&tree, &with(Kind::PlanReview, &|_| {})), None);
        // an item without what its jump needs jumps nowhere
        assert_eq!(jump(&tree, &with(Kind::Dialog, &|_| {})), None);
        assert_eq!(jump(&tree, &with(Kind::Question, &|_| {})), None);
        // an archived Session shows its history, as a click on its tree row
        assert_eq!(
            jump(
                &archived(),
                &with(Kind::Unread, &|i| i.session = Some(sid("f-blocked")))
            ),
            Some(Jump::Run(Action::History(sid("f-blocked"))))
        );
    }

    // ---- plan-review with an optional url (hs-d502) ----

    #[test]
    fn a_plan_review_opens_the_lead_and_copies_nothing() {
        let tree = sample();
        let mut review = item(
            1,
            Kind::PlanReview,
            Urgency::Magenta,
            "ship plan review",
            OPENED,
        );
        review.plan = Some(PlanId::parse("ship").unwrap());
        assert_eq!(
            jump(&tree, &review),
            Some(Jump::Run(Action::Open(sid("l-ship"))))
        );
    }

    fn curate_click(
        label: &str,
        reply: serde_json::Value,
        title: &str,
        rows: [(&str, &str, &str); 2],
        command: [&str; 5],
    ) {
        let alpha = ContextId::parse("alpha").unwrap();
        assert_eq!(
            curation_argv(&alpha),
            ["context", "curation", "--context=alpha", "--limit=10"]
                .map(String::from)
                .to_vec(),
            "asks for at most ten entries"
        );
        let tree = sample();
        let alpha = ContextId::parse("alpha").unwrap();
        let mut it = item(5, Kind::CurateRetire, Urgency::Magenta, label, OPENED);
        assert_eq!(jump(&tree, &it), None, "no Context: nowhere to go");
        it.context = Some(alpha.clone());
        assert_eq!(jump(&tree, &it), Some(Jump::Curate(alpha.clone())));
        let list = checklist(&alpha, &reply).expect("something is pending");
        assert_eq!(list.title, title);
        let want: Vec<Row> = rows
            .iter()
            .map(|(id, text, detail)| Row {
                id: (*id).into(),
                text: (*text).into(),
                detail: (*detail).into(),
            })
            .collect();
        assert_eq!(list.rows, want);
        let ticked = vec!["r-1".to_owned()];
        let unticked = vec!["r-2".to_owned()];
        assert_eq!(
            approved(&alpha, &ticked, &unticked),
            command.map(String::from).to_vec()
        );
        // a flag with no ids is left out
        let only = approved(&alpha, &ticked, &[]);
        assert_eq!(only.len(), 4, "{only:?}");
    }

    #[test]
    fn a_curate_retire_click_opens_the_checklist_and_runs_retire_with_the_ticks() {
        curate_click(
            "alpha: 2 to retire",
            serde_json::json!({"ok": true, "context": "alpha", "docs": [], "retire": [
                {"record": "r-1", "kind": "decision", "text": "use sqlite is the full text", "reason": "duplicate", "target": "r-0"},
                {"record": "r-2", "kind": "note", "text": "old plan is the full text", "reason": "superseded", "target": "r-9"}]}),
            "Retire records of alpha",
            [
                ("r-1", "use sqlite is the full text", "duplicate r-0"),
                ("r-2", "old plan is the full text", "superseded r-9"),
            ],
            [
                "context",
                "retire",
                "--context=alpha",
                "--approve=r-1",
                "--reject=r-2",
            ],
        );
    }

    #[test]
    fn a_checklist_title_says_how_many_of_the_pending_entries_it_shows() {
        let alpha = ContextId::parse("alpha").unwrap();
        let entries = |n: usize, key: &str| -> Vec<serde_json::Value> {
            (0..n)
                .map(|i| match key {
                    "retire" => serde_json::json!({"record": format!("r-{i}"), "kind": "note", "text": "t", "reason": "duplicate", "target": "r-x"}),
                    _ => serde_json::json!({"record": format!("r-{i}"), "kind": "note", "text": "t", "paths": ["docs/a.md"]}),
                })
                .collect()
        };
        let retire = serde_json::json!({"ok": true, "context": "alpha", "docs": [], "retire": entries(10, "retire"), "retire_total": 56, "docs_total": 0});
        assert_eq!(
            checklist(&alpha, &retire).unwrap().title,
            "Retire records of alpha (10 of 56)"
        );
        let all = serde_json::json!({"ok": true, "context": "alpha", "docs": [], "retire": entries(3, "retire"), "retire_total": 3, "docs_total": 0});
        assert_eq!(
            checklist(&alpha, &all).unwrap().title,
            "Retire records of alpha",
            "no suffix when all are shown"
        );
    }

    #[test]
    fn a_curate_click_asks_cpd_for_at_most_ten_entries() {
        let alpha = ContextId::parse("alpha").unwrap();
        assert_eq!(
            curation_argv(&alpha),
            ["context", "curation", "--context=alpha", "--limit=10"]
                .map(String::from)
                .to_vec()
        );
    }

    /// The glyph span of the row `item_line` draws for `item`.
    fn glyph_of(item: &Item) -> (String, Option<ratatui::style::Color>) {
        let (line, _) = item_line(item, "3m", 41, None);
        let glyphs = [
            "\u{F06F1}",
            "\u{F0653}",
            "\u{F116E}",
            "\u{F173A}",
            "\u{F0362}",
            "\u{F069A}",
            "\u{F0366}",
        ];
        let span = line
            .spans
            .iter()
            .find(|s| glyphs.contains(&s.content.as_ref()));
        match span {
            Some(s) => (s.content.to_string(), s.style.fg),
            None => panic!("no glyph in {line:?}"),
        }
    }

    #[test]
    fn a_curate_item_draws_its_own_glyph_in_the_cyan_colour_and_another_cyan_item_the_generic_one()
    {
        let cyan = Some(palette::color_clickable(Meaning::Notice));
        let row = |kind| glyph_of(&item(1, kind, Urgency::Cyan, "alpha: 2", OPENED));
        assert_eq!(
            row(Kind::CurateRetire),
            ("\u{F116E}".to_owned(), cyan),
            "a record to retire"
        );
        assert_eq!(
            row(Kind::Unread),
            ("\u{F06F1}".to_owned(), cyan),
            "the generic system item is the cyan tier's"
        );
        // the other tiers: magenta is back to U+F173A
        let tier = |u| glyph_of(&item(1, Kind::PlanReview, u, "x", OPENED)).0;
        assert_eq!(
            [
                tier(Urgency::Red),
                tier(Urgency::Yellow),
                tier(Urgency::Magenta),
                tier(Urgency::White)
            ],
            ["\u{F0362}", "\u{F069A}", "\u{F173A}", "\u{F0366}"]
        );
    }
}
