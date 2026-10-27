//! The Inbox section at the bottom of the Deploy column (commands.md §13.2, §17; contract §2.2 "Inbox item"): a collapsible
//! widget (`crate::fold`) with its margins, one empty row above and one below (hs-d476): the grey pill title
//! `╭─Inbox────…────<n>` (`crate::section`, inside the panel's padding), up to [`SHOWN`] item rows `│ <glyph> <label>` (each with
//! its tier's glyph and colour, hs-d483) with the age right-aligned, and the fold widget's closing row (`crate::fold`): with more items
//! the muted `╰󰬦 expand` that expands the list inline and becomes `╰󰬬 collapse`, with 3 or fewer the terminator `╰󰬪` (U+F0B2A; hs-d574), with none no
//! row. Only the toggle row toggles. Whether it is expanded is view state only. Expanded, it is capped at the rows the
//! column's split gives it (the PRs have two thirds, hs-d307; [`super::draw`]) and its items scroll inside the cap, with right-aligned `▲ <n> more` / `▼ <n> more` cue rows
//! (`dialog::scroll::cue`, [`Shape`]).
//!
//! A click on an item row jumps to its cause ([`jump`]): a command, the merge popup of
//! its pull request, or the copy of its `url`. The label of a link kind with a `url` is a `Link` widget (`crate::link`): a click on it opens the page
//! instead. Everything here is pure but [`draw`], which only writes to the frame.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::tree::actions::Action;
use crate::box_widget::{BoxWidget, Walls, geometry, spacing};
use crate::cells::{self, truncate};
use crate::fold;
use crate::merge::Pull;
use crate::palette;
use crate::phi;
use crate::read::{Tree, age_text};
use crate::section;
use crate::dialog::scroll::{Cue, cue};
use crate::dialog::scroll::WHEEL_ROWS;
use super::{Window, max_scroll, window};
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Item, Kind};
use crate::link::{Link, Links};

/// How many item rows the section shows before the `╰󰬦 expand` row.
pub const SHOWN: usize = 3;

/// The columns before the label: the item's tier glyph and a space.
const LEAD_WIDTH: usize = 2;

/// The rows above the items: the empty row (hs-d346) and the title.
const TOP: usize = 2;

/// The rows below the closing row: the empty row (hs-d476).
const BOTTOM: usize = 1;

/// The most rows the Inbox section may take in a column of `column` rows (commands.md §13.2, hs-d307), for a tree of
/// `tree` rows and `count` items. When both fit, each has its rows. Otherwise the tree is guaranteed two thirds of the
/// column and the Inbox the third that is left (rounded down, but never below the collapsed section, so that its
/// header, three items and `+<n> more` show in a column that is not tiny); a section that needs less than its share gives
/// the rest to the other.
pub(crate) fn inbox_share(tree: usize, count: usize, expanded: bool, column: u16) -> u16 {
    let column = usize::from(column);
    let wants = Shape::new(count, expanded).height().into();
    if tree + wants <= column {
        return u16::try_from(wants).unwrap_or(u16::MAX);
    }
    let share = (column / 3).max(Shape::new(count, false).height().into());
    let rows = if wants <= share {
        wants
    } else if tree + share <= column {
        column - tree
    } else {
        share
    };
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// Whether the merge of the `pr-ready` `item`'s pull request runs: `merging` is the pull requests (`<owner>/<repo>#<n>`) with a live
/// merge marker (hs-d495).
pub(crate) fn merges(merging: &std::collections::BTreeSet<String>, item: &Item) -> bool {
    item.kind == Kind::PrReady && Pull::from_subject(&item.subject).is_some_and(|pull| merging.contains(&pull.to_string()))
}

/// What a row of the section is, by its offset from the section's first row (the empty one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Header,
    /// The item at this index of the snapshot's Inbox.
    Item(usize),
    /// `╰󰬦 expand` or `╰󰬬 collapse`.
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

    /// The rows the section wants: the empty row, the title, the items, the closing row (the toggle, the terminator `╰󰬪`, or
    /// with no item the up-to-date row, hs-d487; hs-d574) and the empty row below; expanded, at most the cap, but not less than
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

    /// The scroll offset after a wheel notch down or up over the section: [`WHEEL_ROWS`] items a notch, clamped.
    pub fn wheeled(self, down: bool) -> usize {
        let max = self.max_scroll();
        let scroll = self.scroll.min(max);
        if down { (scroll + WHEEL_ROWS).min(max) } else { scroll.saturating_sub(WHEEL_ROWS) }
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

    /// The row `row` rows below the section's first; the empty rows, a cue row and the terminator are none (hs-d574).
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

/// The age of an item opened at `opened_at` (§1.3's RFC 3339), at `now`; `?` when the stored time does not parse.
fn age_of(opened_at: &str, now: OffsetDateTime) -> String {
    OffsetDateTime::parse(opened_at, &Rfc3339).map_or_else(|_| "?".to_owned(), |t| age_text((now - t).whole_seconds()))
}

/// What a click on an item does besides the link's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jump {
    /// Run this command and focus the chat slot (a click on a Session row).
    Run(Action),
    /// These actions in order, each stopping the flow when it is refused: `plan-review` opens its Lead, then copies its page.
    Steps(Vec<Action>),
    /// The merge popup of this pull request (`pr-ready`).
    Merge(Pull),
    /// The checklist dialog of a `curate-retire` or `curate-docs` item of this Context (hs-d549, hs-d551).
    Curate(Kind, cpd_core::ids::ContextId),
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

/// The checklist of `kind` for `context` from the reply of `cpd context curation` (commands.md §13.2): the pending
/// `retire` or `docs` entries; `None` when none is pending (or the reply has none of the shape).
pub fn checklist(kind: Kind, context: &cpd_core::ids::ContextId, reply: &serde_json::Value) -> Option<Checklist> {
    let (key, title) = match kind {
        Kind::CurateRetire => ("retire", format!("Retire records of {context}")),
        Kind::CurateDocs => ("docs", format!("Documentation candidates of {context}")),
        _ => return None,
    };
    let text = |e: &serde_json::Value, k: &str| e[k].as_str().map(str::to_owned);
    let rows = reply[key]
        .as_array()?
        .iter()
        .map(|e| {
            let detail = if key == "retire" {
                format!("{} {}", text(e, "reason")?, text(e, "target")?)
            } else {
                e["paths"].as_array()?.iter().filter_map(|p| p.as_str()).collect::<Vec<_>>().join(", ")
            };
            Some(Row { id: text(e, "record")?, text: text(e, "text")?, detail })
        })
        .collect::<Option<Vec<_>>>()?;
    let total = reply[format!("{key}_total")].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(rows.len());
    let title = if total > rows.len() { format!("{title} ({} of {total})", rows.len()) } else { title };
    (!rows.is_empty()).then_some(Checklist { title, rows })
}

/// The most entries one checklist shows (hs-d569 CD4).
const CURATION_LIMIT: usize = 10;

/// The `cpd` arguments of the curation request: at most [`CURATION_LIMIT`] entries of each list.
pub fn curation_argv(context: &cpd_core::ids::ContextId) -> Vec<String> {
    vec!["context".into(), "curation".into(), format!("--context={context}"), format!("--limit={CURATION_LIMIT}")]
}

/// The `cpd` arguments an approved checklist runs (commands.md §13.2); a flag with no ids is left out.
pub fn approved(kind: Kind, context: &cpd_core::ids::ContextId, ticked: &[String], unticked: &[String]) -> Vec<String> {
    let (verb, yes, no) = if kind == Kind::CurateDocs { ("docs", "send", "dismiss") } else { ("retire", "approve", "reject") };
    let mut argv = vec!["context".to_owned(), verb.to_owned(), format!("--context={context}")];
    for (flag, ids) in [(yes, ticked), (no, unticked)] {
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
    if archived { Action::History(id.clone()) } else { Action::Open(id.clone()) }
}

/// Where a plain click on `item` goes (commands.md §13.2, "Clicks on an Inbox item"): every kind acts, but a row of the old `engine-error` or `job-stuck` kind (hs-d314, hs-d318) or the terminator `╰󰬪` (hs-d574). `engine-down`
/// starts the engine; `pr-ready` opens the merge popup of its pull request (`None` for a subject that is not
/// `pr:<owner>/<repo>#<n>`); `plan-review` opens the Lead of its Plan and copies the review page's `url` (when there is
/// one); `review-waiting` opens its Session, then copies the `url` likewise; the `url` is copied for `review-requested` (a click on the label of a link kind opens its page instead, see [`Link`]). `None` only for an item without what its jump needs.
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
        Kind::CurateRetire | Kind::CurateDocs => item.context.as_ref().map(|c| Jump::Curate(item.kind, c.clone())),
        // one background `session resume`: the pane does not open; a refusal is the failure dialog
        Kind::LoopGuard => item.session.as_ref().map(|s| Jump::Run(Action::SessionResume(s.clone()))),
        Kind::PrReady => Pull::from_subject(&item.subject).map(Jump::Merge),
        Kind::ReviewRequested => {
            item.url.as_ref().map(|url| Jump::Run(Action::CopyUrl(url.clone())))
        }
    }
}

/// What a left click on `item` runs, as [`jump`] says but for what is already so: an action whose Session already is in the
/// chat slot is dropped, and so is the merge popup of a pull request whose merge runs (`merging`, hs-d495): the click, like
/// the glyph's spinner, is its marker's.
pub fn click(tree: &Tree, item: &Item, merging: &std::collections::BTreeSet<String>) -> Option<Jump> {
    let in_focus = |action: &Action| {
        let Action::Open(id) = action else { return false };
        tree.contexts.iter().flat_map(|c| &c.plans).flat_map(|p| &p.sessions).any(|s| &s.id == id && s.is_active())
    };
    match jump(tree, item)? {
        Jump::Run(action) if in_focus(&action) => None,
        Jump::Steps(steps) => {
            let steps: Vec<Action> = steps.into_iter().filter(|a| !in_focus(a)).collect();
            (!steps.is_empty()).then_some(Jump::Steps(steps))
        }
        Jump::Merge(pull) if merging.contains(&pull.to_string()) => None,
        jump => Some(jump),
    }
}

/// An entry of an item's right-click menu (commands.md §13.2): the labels are the contract's, exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Claim,
    Snooze15,
    Snooze1h,
    SnoozeTomorrow,
    Dismiss,
}

impl Choice {
    /// The entries of the menu of a row of `kind`, in order: `Claim` first on `review-requested` rows only.
    pub fn entries(kind: Kind) -> Vec<Choice> {
        let rest = [Choice::Snooze15, Choice::Snooze1h, Choice::SnoozeTomorrow, Choice::Dismiss];
        let claim = (kind == Kind::ReviewRequested).then_some(Choice::Claim);
        claim.into_iter().chain(rest).collect()
    }

    /// The words of the command of the item `id`: `inbox claim <id>`, `inbox snooze <id> <15m|1h|tomorrow>` (§21.3) or `inbox dismiss <id>` (§21.4).
    pub fn argv(self, id: i64) -> Vec<String> {
        let id = id.to_string();
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
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
            Choice::Claim => "Claim",
            Choice::Snooze15 => "Snooze 15 min",
            Choice::Snooze1h => "Snooze 1 h",
            Choice::SnoozeTomorrow => "Snooze until tomorrow 09:00",
            Choice::Dismiss => "Dismiss",
        }
    }
}

/// `session open` of the Lead of Plan `plan` (`session history` if archived); none for a Plan the tree does not have.
fn open_lead(tree: &Tree, plan: &PlanId) -> Option<Action> {
    let plan = tree.contexts.iter().flat_map(|c| &c.plans).find(|p| &p.head.id == plan)?;
    Some(open_or_history(tree, &plan.head.lead_session))
}

/// The kinds whose label is a `Link` to the item's `url`.
fn is_link(kind: Kind) -> bool {
    matches!(kind, Kind::PlanReview | Kind::ReviewWaiting | Kind::PrReady | Kind::ReviewRequested)
}

/// An item's row, in the box's body: the item's tier glyph in its urgency's colour, a space, the label cut with
/// `…` so that it never reaches the age, and the age (`width` cells: the body's). Returns the line and the number of cells of the label.
/// A merge of the item's pull request runs (hs-d495): `spin` is the spinner's frame, and the glyph cell shows it.
/// The label of a link kind with a url is no text of the line: its cells are blank, for the [`Link`] drawn over them, which is as
/// wide as the label and the cue (cut to the room before the age); the number of cells is that width.
fn item_line(item: &Item, age: &str, width: usize, spin: Option<usize>) -> (Line<'static>, usize) {
    let room = width.saturating_sub(LEAD_WIDTH + 1 + cells::width(age));
    let linked = link_url(item).is_some();
    let label = if linked { String::new() } else { truncate(&item.label, room) };
    let label_width = if linked { (cells::width(&item.label) + 2).min(room) } else { cells::width(&label) };
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

/// The box's body: the rows of the list (items and cues), each a line, and the links of the labels of the link kinds.
struct Body<'a> {
    rows: Vec<Line<'static>>,
    /// (row, width of the label and the cue, the item)
    links: Vec<(u16, u16, &'a Item)>,
    drawn: &'a mut Links,
}

impl Widget for Body<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (y, line) in (area.y..area.bottom()).zip(self.rows) {
            line.render(Rect::new(area.x, y, area.width, 1), buf);
        }
        for (row, width, item) in self.links {
            let Some(url) = link_url(item) else { continue };
            if row < area.height {
                let text = Line::from(phi::spans(&item.label, palette::clickable_text()));
                self.drawn.draw(Link::new(url).text(text), Rect::new(area.x + LEAD_WIDTH as u16, area.y + row, width, 1), buf);
            }
        }
    }
}

/// Draws the section into `area`, the panel's full width: a `BoxWidget<Rail>` over `spacing::contain(area)` (its first row is the
/// empty one; rows that do not fit are cut): `items` are the
/// snapshot's visible open items, `now` the clock the ages are taken from, `cursor` the item the keyboard cursor
/// is on (its row is the blue block across its width). `shape` is the section's, as capped and scrolled. `spinning` says, of an
/// item whose merge runs, the frame of the spinner its glyph cell shows instead of its tier glyph (hs-d495).
pub fn draw(f: &mut Frame, area: Rect, items: &[Item], shape: Shape, cursor: Option<usize>, now: OffsetDateTime, spinning: &dyn Fn(&Item) -> Option<usize>) -> Links {
    // the section is drawn whole; rows below the area are cut
    let wall = Rect { height: shape.height(), ..spacing::contain(area) };
    let closing = if items.is_empty() {
        fold::done("up to date, drink some water")
    } else if shape.has_toggle() {
        fold::toggle(shape.expanded)
    } else {
        fold::plain()
    };
    let closing = fold::after_corner(closing, usize::from(wall.width).saturating_sub(1 + spacing::RIGHT_PAD as usize));
    let walls = Walls::Rail { closing };
    let inner = geometry(&walls, wall).inner;
    let width = usize::from(inner.width);
    let win = shape.window();
    let list = shape.list_rows();
    let mut rows = Vec::new();
    let mut links = Vec::new();
    let mut highlight = None;
    let mut drawn = Links::default();
    for at in 0..list {
        let row = at as u16;
        match shape.hit(row + TOP as u16) {
            Some(Hit::Item(i)) => {
                let item = &items[i];
                let (line, label_width) = item_line(item, &age_of(&item.opened_at, now), width, spinning(item));
                rows.push(line);
                if cursor == Some(i) {
                    highlight = Some(row..row + 1);
                }
                if link_url(item).is_some() {
                    links.push((row, label_width as u16, item));
                }
            }
            _ if win.cue_above && at == 0 => rows.push(cue(Cue::Above, win.first, width)),
            _ if win.below > 0 && at == list - 1 => rows.push(cue(Cue::Below, win.below, width)),
            _ => rows.push(Line::default()),
        }
    }
    let title = section::rail_title("Inbox", Some(items.len()), usize::from(wall.width).saturating_sub(1));
    let mut widget = BoxWidget::new(walls, palette::muted_text(), Body { rows, links, drawn: &mut drawn }).title(title);
    if let Some(rows) = highlight {
        widget = widget.highlight(rows);
    }
    let shown = area.height.min(wall.height);
    if shown == wall.height {
        f.render_widget(widget, wall);
        return drawn;
    }
    let mut whole = Buffer::empty(wall);
    widget.render(wall, &mut whole);
    let buf = f.buffer_mut();
    for y in wall.y..wall.y + shown {
        for x in wall.x..wall.right() {
            buf[(x, y)] = whole[(x, y)].clone();
        }
    }
    drawn
}

/// The layout test's sample (`layout_tests`): the Inbox section drawn by [`draw`] at the top of a `width` x `height` pane
/// (the column's pane: the tree's container); the section is capped at `height` rows. Placed is the Rail's wall rect,
/// `spacing::contain(pane)` as wide and as tall as the section.
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::TreeCase) -> crate::layout_tests::Sample {
    use crate::box_widget::spacing;
    use crate::layout_tests::{Kind as Walls, Placed, Sample, TreeCase};
    use cpd_core::inbox::model::Urgency;
    use ratatui::backend::TestBackend;
    let labels = ["f-blocked: Worker question", "acme/api#12 checks failed", "ship: Job c is stuck", "engine: store busy", "ship lead has a reply", "beta: Plan review", "gamma: nothing", "delta: Job d failed"];
    let ages = ["2026-01-02T23:59:18.000Z", "2026-01-02T23:55:00.000Z", "2026-01-02T21:00:00.000Z", "2026-01-01T00:00:00.000Z", "2026-01-02T23:50:00.000Z", "2026-01-02T22:00:00.000Z", "2026-01-02T20:00:00.000Z", "2026-01-02T10:00:00.000Z"];
    let (count, expanded, scroll) = match case {
        TreeCase::InboxNone => (0, false, 0),
        TreeCase::Inbox3 => (3, false, 0),
        TreeCase::Inbox5Folded => (5, false, 0),
        TreeCase::Inbox5Expanded => (5, true, 0),
        TreeCase::InboxCues => (8, true, 2),
        TreeCase::ContextFold => panic!("the Inbox samples an Inbox case"),
    };
    let items: Vec<Item> = (0..count)
        .map(|i| item(i as i64 + 1, Kind::Unread, Urgency::White, labels[i], ages[i]))
        .collect();
    let shape = Shape::new(count, expanded).capped(height).scrolled(scroll);
    let now = OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap();
    let pane = Rect::new(0, 0, width, height);
    let mut terminal = ratatui::Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            draw(f, Rect { height: shape.height().min(height), ..pane }, &items, shape, None, now, &|_| None);
        })
        .unwrap();
    let wall = Rect { height: shape.height().min(height), ..spacing::contain(pane) };
    let boxes = vec![Placed { name: "inbox", kind: Walls::Rail, rect: wall, exempt: Vec::new() }];
    Sample { buf: terminal.backend().buffer().clone(), container: pane, boxes }
}

/// An open item for the tests: `kind` and `urgency`, its `label`, opened at `opened_at`; the rest is none.
#[cfg(test)]
pub(crate) fn item(id: i64, kind: Kind, urgency: cpd_core::inbox::model::Urgency, label: &str, opened_at: &str) -> Item {
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
    use ratatui::Terminal;

    use crate::tree::fixture::{archived, sample};
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

    // ---- review-waiting (hs-d496): the click opens the item's Session then copies its url ----

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

    // ---- plan-review with an optional url (hs-d502) ----

    #[test]
    fn a_plan_review_with_no_url_opens_the_lead_and_copies_nothing_hs_d502() {
        let tree = sample();
        let mut review = item(1, Kind::PlanReview, Urgency::Magenta, "ship plan review", OPENED);
        review.plan = Some(PlanId::parse("ship").unwrap());
        assert_eq!(jump(&tree, &review), Some(Jump::Run(Action::Open(sid("l-ship")))));
        review.url = Some("http://review.example:20000/".into());
        assert_eq!(
            jump(&tree, &review),
            Some(Jump::Steps(vec![Action::Open(sid("l-ship")), Action::CopyUrl("http://review.example:20000/".into())]))
        );
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
        terminal.draw(|f| {
            draw(f, f.area(), &[], Shape::new(0, false), None, at("2026-01-01T00:00:00.000Z"), &|_| None);
        }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| (0..41).map(|x| buffer[(x, y)].symbol()).collect::<String>().trim_end().to_owned();
        let title = format!(" ╭─\u{E0B6}Inbox\u{E0B4}{}\u{E0B6}0\u{E0B4}", "─".repeat(39 - 2 - 2 - "Inbox".len() - 3));
        let close = format!(" ╰{} up to date, drink some water", crate::fold::TERMINATOR);
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

    fn curate_click(kind: Kind, label: &str, reply: serde_json::Value, title: &str, rows: [(&str, &str, &str); 2], command: [&str; 5]) {
        let alpha = ContextId::parse("alpha").unwrap();
        assert_eq!(curation_argv(&alpha), ["context", "curation", "--context=alpha", "--limit=10"].map(String::from).to_vec(), "asks for at most ten entries");
        let tree = sample();
        let alpha = ContextId::parse("alpha").unwrap();
        let mut it = item(5, kind, Urgency::Magenta, label, OPENED);
        assert_eq!(jump(&tree, &it), None, "no Context: nowhere to go");
        it.context = Some(alpha.clone());
        assert_eq!(jump(&tree, &it), Some(Jump::Curate(kind, alpha.clone())));
        let list = checklist(kind, &alpha, &reply).expect("something is pending");
        assert_eq!(list.title, title);
        let want: Vec<Row> = rows.iter().map(|(id, text, detail)| Row { id: (*id).into(), text: (*text).into(), detail: (*detail).into() }).collect();
        assert_eq!(list.rows, want);
        let ticked = vec!["r-1".to_owned()];
        let unticked = vec!["r-2".to_owned()];
        assert_eq!(approved(kind, &alpha, &ticked, &unticked), command.map(String::from).to_vec());
        // a flag with no ids is left out
        let only = approved(kind, &alpha, &ticked, &[]);
        assert_eq!(only.len(), 4, "{only:?}");
    }

    #[test]
    fn a_curate_retire_click_opens_the_checklist_and_runs_retire_with_the_ticks() {
        curate_click(
            Kind::CurateRetire,
            "alpha: 2 to retire",
            serde_json::json!({"ok": true, "context": "alpha", "docs": [], "retire": [
                {"record": "r-1", "kind": "decision", "text": "use sqlite is the full text", "reason": "duplicate", "target": "r-0"},
                {"record": "r-2", "kind": "note", "text": "old plan is the full text", "reason": "superseded", "target": "r-9"}]}),
            "Retire records of alpha",
            [("r-1", "use sqlite is the full text", "duplicate r-0"), ("r-2", "old plan is the full text", "superseded r-9")],
            ["context", "retire", "--context=alpha", "--approve=r-1", "--reject=r-2"],
        );
    }

    #[test]
    fn a_curate_docs_click_opens_the_checklist_and_runs_docs_with_the_ticks() {
        curate_click(
            Kind::CurateDocs,
            "alpha: 2 doc candidates",
            serde_json::json!({"ok": true, "context": "alpha", "retire": [], "docs": [
                {"record": "r-1", "kind": "decision", "text": "use sqlite is the full text", "paths": ["docs/a.md", "README.md"]},
                {"record": "r-2", "kind": "note", "text": "old plan is the full text", "paths": ["docs/b.md"]}]}),
            "Documentation candidates of alpha",
            [("r-1", "use sqlite is the full text", "docs/a.md, README.md"), ("r-2", "old plan is the full text", "docs/b.md")],
            ["context", "docs", "--context=alpha", "--send=r-1", "--dismiss=r-2"],
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
        assert_eq!(checklist(Kind::CurateRetire, &alpha, &retire).unwrap().title, "Retire records of alpha (10 of 56)");
        let docs = serde_json::json!({"ok": true, "context": "alpha", "retire": [], "docs": entries(10, "docs"), "docs_total": 11, "retire_total": 0});
        assert_eq!(checklist(Kind::CurateDocs, &alpha, &docs).unwrap().title, "Documentation candidates of alpha (10 of 11)");
        let all = serde_json::json!({"ok": true, "context": "alpha", "docs": [], "retire": entries(3, "retire"), "retire_total": 3, "docs_total": 0});
        assert_eq!(checklist(Kind::CurateRetire, &alpha, &all).unwrap().title, "Retire records of alpha", "no suffix when all are shown");
    }

    #[test]
    fn a_curate_click_asks_cpd_for_at_most_ten_entries() {
        let alpha = ContextId::parse("alpha").unwrap();
        assert_eq!(curation_argv(&alpha), ["context", "curation", "--context=alpha", "--limit=10"].map(String::from).to_vec());
    }

    #[test]
    fn a_loop_guard_item_click_runs_session_resume_in_the_background() {
        let tree = sample();
        let mut guard = item(1, Kind::LoopGuard, Urgency::Red, "m-alpha: loop guard tripped", OPENED);
        assert_eq!(jump(&tree, &guard), None, "no Session: nowhere to go");
        guard.session = Some(sid("m-alpha"));
        // one command, no dialog and no `session open`: the Session's pane does not open
        let resume = Action::SessionResume(sid("m-alpha"));
        assert_eq!(jump(&tree, &guard), Some(Jump::Run(resume.clone())));
        assert_eq!(resume.argv(), ["session", "resume", "m-alpha"].map(String::from).to_vec());
        // an archived Session is resumed too: it is never opened
        guard.session = Some(sid("f-blocked"));
        assert_eq!(jump(&archived(), &guard), Some(Jump::Run(Action::SessionResume(sid("f-blocked")))));
    }

    /// The glyph span of the row `item_line` draws for `item`.
    fn glyph_of(item: &Item) -> (String, Option<ratatui::style::Color>) {
        let (line, _) = item_line(item, "3m", 41, None);
        let glyphs = ["\u{F06F1}", "\u{F0653}", "\u{F116E}", "\u{F173A}", "\u{F0362}", "\u{F069A}", "\u{F0366}"];
        let span = line.spans.iter().find(|s| glyphs.contains(&s.content.as_ref()));
        match span {
            Some(s) => (s.content.to_string(), s.style.fg),
            None => panic!("no glyph in {line:?}"),
        }
    }

    #[test]
    fn a_curate_item_draws_its_own_glyph_in_the_cyan_colour_and_another_cyan_item_the_generic_one() {
        let cyan = Some(palette::color_clickable(Meaning::Notice));
        let row = |kind| glyph_of(&item(1, kind, Urgency::Cyan, "alpha: 2", OPENED));
        assert_eq!(row(Kind::CurateDocs), ("\u{F0653}".to_owned(), cyan), "a documentation candidate");
        assert_eq!(row(Kind::CurateRetire), ("\u{F116E}".to_owned(), cyan), "a record to retire");
        assert_eq!(row(Kind::Unread), ("\u{F06F1}".to_owned(), cyan), "the generic system item is the cyan tier's");
        // the other tiers: magenta is back to U+F173A
        let tier = |u| glyph_of(&item(1, Kind::PlanReview, u, "x", OPENED)).0;
        assert_eq!(
            [tier(Urgency::Red), tier(Urgency::Yellow), tier(Urgency::Magenta), tier(Urgency::White)],
            ["\u{F0362}", "\u{F069A}", "\u{F173A}", "\u{F0366}"]
        );
    }
}
