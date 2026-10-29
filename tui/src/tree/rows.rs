//! The tree itself (the spike's `context.rs`, on the read model): Contexts,
//! their Plans, and the Sessions forked by `session fork` as a `tui-tree-widget` tree.
//!
//! The widget draws one text per row (guide, dot, name). It has no
//! right-aligned column, so each Plan's five Job counters are drawn as a
//! right-aligned `Line` into the row's `Rect` after the tree, ending at the pane's inner edge (pro-d186). The cost of a Plan
//! or a Context is drawn beside its name or id, one space after it, in colour 8. Context rows are not selectable: the cursor moves between
//! Sessions only, and is inverted over its row's whole width, only while the pane has keyboard focus.
//!
//! **Selected** (hs-d351): the Context or Plan whose Session is in the chat slot (a Context row when its Manager is)
//! draws its name in the palette's focus blue, the muted colour of an archived row included; the rest of its row keeps
//! its colours.
//!
//! **Folds** (decisions hs-d337, hs-d339, hs-d344, hs-d349; `crate::fold`, the one widget). A Context is a collapsible
//! block: the title row `╭` + the Context's dot + its id, each content row `├─` + the row's dot + its name, straight
//! against the branch (no widget indent), and **one** closing row. The
//! toggle shows and hides **only the archived rows**: the live rows always show. Folded, the closing row is the muted
//! `╰󰬦 expand`; expanded, the archived rows (muted) and `╰󰬬 collapse`; a Context with no archived rows closes with the
//! terminator ` ╰󰬪` (U+F0B2A; hs-d574) and has no toggle. Only the closing row folds ([`Row::Toggle`], [`click_toggle`]); a click on the title
//! row keeps the row's own action. A Context's closing row is drawn as the Context's next sibling.
//!
//! Labels (commands.md §13.2): a Context row shows its id, a Plan row its Plan's id, a fork its Session's id.
//!
//! **Forks** (B1, contract §6.8): a forked Session is a sibling of the Plans directly under its Context, after
//! them, never under a Plan or under its source.
//!
//! **Archived rows** (hs-d339, hs-d344). An expanded Context's block lists its archived Plans, and the forks that are
//! archived or of an archived Plan, after the live rows, muted, with the same `├─` rows. There is no `…` row and no
//! separate toggle. Archived Contexts follow the live ones, each its own block, with every row archived; they start
//! folded. The widget's open set holds every Context (its live rows always show); a Context is **expanded** when the open
//! set holds its closing row's identifier, so view state only. The cursor never rests on archived rows.
//!
//! Rows (identifiers are paths):
//!
//! | Path | Row |
//! |---|---|
//! | `[context]`, `[context, plan]`, `[context, session]` | a Context, a Plan (its row is its Lead's), a forked Session; archived or not |
//! | `[╰─ context]` | the closing row of that Context (`╰󰬦 expand`, `╰󰬬 collapse`, or the terminator `╰󰬪`; hs-d574); its toggle |
//!
//! Everything here is pure except `draw`, which only writes to the frame's buffer.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use tui_tree_widget::TreeState;

use crate::fold;
use crate::palette;
use crate::dialog::scroll::{Cue, cue};
use crate::widgets::Window;
use crate::read::{ContextNode, Dot, JobCounts, OrderKey, Place, PlanNode, SessionNode, Tree};
use super::actions::{Action, Subject};

/// Tree identifiers: a Context's id, a Plan's id, a Session's id.
pub type Id = String;

/// One tree level is two columns, as the width of a row counts it (the frames keep it, A11).
const INDENT: usize = 2;

/// What a closing row's identifier starts with: the identifier of the fold it toggles, last element, behind this.
const TOGGLE: &str = "╰─ ";

/// What the identifier of the empty row after the Context whose own identifier is `id` starts with (hs-d476).
const SPACER: &str = "  ";

/// The synthetic path id of the tree root's archive row (hs-d698, hs-d707). Context ids are lowercase `^[a-z]`, so it
/// can never collide with a Context.
pub(super) const ARCHIVE: &str = "Archive";

/// The columns a Plan row keeps for itself before its counters may be drawn: the branch, the glyph, its space and one cell of the name.
const MIN_LEFT: usize = BRANCH_WIDTH + 3;

/// The columns a content row's branch takes (`fold::TREE_BRANCH`).
const BRANCH_WIDTH: usize = 2;

/// The identifier of the toggle row of the Context whose own identifier is `id`; it is a sibling of it, so the rest of
/// the path is the same.
pub(super) fn toggle_id(id: &str) -> Id {
    format!("{TOGGLE}{id}")
}

/// The fold at `path`, when `path` is a toggle row's identifier.
fn toggle_owner(path: &[Id]) -> Option<Vec<Id>> {
    let (last, parents) = path.split_last()?;
    Some([parents, &[last.strip_prefix(TOGGLE)?.to_owned()]].concat())
}

/// What a row of the tree stands for.
#[derive(Clone, Copy, Debug)]
pub enum Row<'a> {
    Context(&'a ContextNode),
    /// A Plan of the Context, with its Lead.
    Plan(&'a ContextNode, &'a PlanNode),
    /// A Session forked by `session fork`, with the Plan it belongs to.
    Fork(&'a ContextNode, &'a PlanNode, &'a SessionNode),
    /// An archived Manager Session of the Context, drawn in its archived section (commands.md §17, hs-d630, hs-d633).
    Manager(&'a ContextNode, &'a SessionNode),
    /// The closing row of a Context (`╰󰬦 expand`, `╰󰬬 collapse`, or the terminator `╰󰬪` of a Context with no row; hs-d574): the only row that folds its archived rows.
    Toggle,
    /// The closing row `╰󰬪 new` of a live Context with no row at all (hs-d688): a left click opens its New Plan popup.
    NewPlan(&'a ContextNode),
    /// The tree root's single archive row (hs-d698, hs-d707): `󰬦 archive` folded, `󰬬 archive` expanded; the whole
    /// row toggles the archived Contexts. No Session, Plan or Context, never selectable, no menu.
    Archive,
}

/// What a Context lists: the live rows, then the archived ones (the rows its toggle shows and hides).
struct Kids<'a> {
    /// The live Plans listed under the Context (none when the Context is archived).
    direct: Vec<&'a PlanNode>,
    /// The forks listed under the Context after its Plans (B1): those of `direct` that are not archived.
    forks: Vec<(&'a PlanNode, &'a SessionNode)>,
    /// The archived Plans, listed after the live rows (every Plan when the Context is archived).
    archived: Vec<&'a PlanNode>,
    /// The forks that are archived or of an archived Plan, listed last (every fork when the Context is archived).
    strays: Vec<(&'a PlanNode, &'a SessionNode)>,
    /// The archived Manager Sessions, listed after the strays (hs-d630, hs-d633).
    managers: Vec<&'a SessionNode>,
}

impl Kids<'_> {
    /// The Context has rows its toggle shows and hides.
    fn has_archived(&self) -> bool {
        !self.archived.is_empty() || !self.strays.is_empty() || !self.managers.is_empty()
    }
}

fn kids(c: &ContextNode) -> Kids<'_> {
    let managers = c.archived_managers.iter().collect();
    if c.is_archived() {
        return Kids { direct: Vec::new(), forks: Vec::new(), archived: c.plans.iter().collect(), strays: c.forks(), managers };
    }
    let (archived, direct): (Vec<&PlanNode>, Vec<&PlanNode>) = c.plans.iter().partition(|p| p.head.is_archived());
    let (strays, forks): (Vec<_>, Vec<_>) = c.forks().into_iter().partition(|(p, s)| p.head.is_archived() || s.is_archived());
    Kids { direct, forks, archived, strays, managers }
}

/// The row at `path`, `None` when the tree has none.
pub fn row<'a>(tree: &'a Tree, path: &[Id]) -> Option<Row<'a>> {
    if let Some(owner) = toggle_owner(path) {
        let Some(Row::Context(c)) = row(tree, &owner) else { return None };
        // a live Context with no archived row closes with the clickable new row; every other closing row is a toggle
        return if !c.is_archived() && !kids(c).has_archived() { Some(Row::NewPlan(c)) } else { Some(Row::Toggle) };
    }
    if path == [ARCHIVE] {
        return Some(Row::Archive);
    }
    let (context, path) = path.split_first()?;
    let context = tree.contexts.iter().find(|c| c.id.as_str() == context)?;
    let Some((next, path)) = path.split_first() else { return Some(Row::Context(context)) };
    if !path.is_empty() {
        return None;
    }
    let k = kids(context);
    if let Some(plan) = k.direct.iter().chain(&k.archived).copied().find(|p| p.head.id.as_str() == next) {
        return Some(Row::Plan(context, plan));
    }
    if let Some(s) = k.managers.iter().copied().find(|s| s.id.as_str() == next) {
        return Some(Row::Manager(context, s));
    }
    k.forks.iter().chain(&k.strays).find(|(_, s)| s.id.as_str() == next).map(|(p, s)| Row::Fork(context, p, s))
}

impl<'a> Row<'a> {
    /// The Session the row opens and closes: a Plan's Lead, or the fork; none for a Context or a toggle.
    pub fn session(&self) -> Option<&'a SessionNode> {
        match *self {
            Row::Context(_) | Row::Toggle | Row::NewPlan(_) | Row::Archive => None,
            Row::Plan(_, p) => p.lead(),
            Row::Fork(_, _, s) | Row::Manager(_, s) => Some(s),
        }
    }

    /// The Plan of the row; none for a Context or a toggle.
    pub fn plan(&self) -> Option<&'a PlanNode> {
        match *self {
            Row::Context(_) | Row::Toggle | Row::NewPlan(_) | Row::Manager(..) | Row::Archive => None,
            Row::Plan(_, p) | Row::Fork(_, p, _) => Some(p),
        }
    }

    /// The row is of an archived object, or inside one (its Plan or Context): drawn muted, never selected, never
    /// acted on but by Unarchive.
    pub fn archived(&self) -> bool {
        match *self {
            Row::Context(c) => c.is_archived(),
            Row::Plan(c, p) => c.is_archived() || p.head.is_archived(),
            Row::Fork(c, p, s) => c.is_archived() || p.head.is_archived() || s.is_archived(),
            Row::Manager(c, s) => c.is_archived() || s.is_archived(),
            Row::Toggle | Row::NewPlan(_) | Row::Archive => false,
        }
    }

    /// The row is drawn muted: archived, or the row of an asleep Session (contract §2.2 "Asleep"). An asleep row is live:
    /// the cursor rests on it and Enter resumes it.
    pub fn muted(&self) -> bool {
        self.archived()
            || match *self {
                Row::Context(c) => c.manager_asleep,
                Row::Plan(_, p) => p.lead().is_some_and(SessionNode::is_asleep),
                Row::Fork(_, _, s) => s.is_asleep(),
                Row::Manager(..) => true,
                Row::Toggle | Row::NewPlan(_) | Row::Archive => false,
            }
    }

    /// The row is of a Plan or a Context that `plan delete` or `context delete` may delete: archived, or asleep (a Plan whose
    /// Lead is asleep; a Context whose every Plan is asleep or archived), contract §2.2 "Deleted". The command says whether
    /// the rest holds (nothing running, no open PR); the menu offers Delete on this much.
    pub fn deletable(&self) -> bool {
        let quiet = |p: &PlanNode| p.head.is_archived() || p.lead().is_some_and(SessionNode::is_asleep);
        match *self {
            Row::Context(c) => c.is_archived() || (!c.plans.is_empty() && c.plans.iter().all(quiet)),
            Row::Plan(c, p) => c.is_archived() || quiet(p),
            Row::Fork(..) | Row::Manager(..) | Row::Toggle | Row::NewPlan(_) | Row::Archive => false,
        }
    }

    /// What a click on an archived Session, Plan or fork row does first: `unarchive` of the outermost archived object of
    /// the row (its Context, else its Plan, else the Session), and the Open of its Session. None when the row is not archived.
    pub fn resume(&self) -> Option<(Subject, Action)> {
        if let Row::Manager(c, s) = *self {
            return Some((Subject::Context(c.id.clone()), Action::Open(s.id.clone())));
        }
        let (context, plan, session) = match *self {
            Row::Plan(c, p) => (c, p, p.lead()?),
            Row::Fork(c, p, s) => (c, p, s),
            Row::Context(_) | Row::Manager(..) | Row::Toggle | Row::NewPlan(_) | Row::Archive => return None,
        };
        let subject = if context.is_archived() {
            Subject::Context(context.id.clone())
        } else if plan.head.is_archived() {
            Subject::Plan(plan.head.id.clone())
        } else if session.is_archived() {
            Subject::Session(session.id.clone())
        } else {
            return None;
        };
        Some((subject, Action::Open(session.id.clone())))
    }

    /// The cursor may rest on it: a live Plan or fork.
    fn selectable(&self) -> bool {
        matches!(self, Row::Plan(..) | Row::Fork(..)) && !self.archived()
    }

    /// The id the row shows in the tree: a Context's id, a Plan's id, a fork's `<plan id>·<first 4 characters of its
    /// Session id>` (the first 8 characters of its id when it has no Plan); none for a toggle row.
    pub fn shown_id(&self) -> Option<String> {
        match *self {
            Row::Context(c) => Some(c.id.to_string()),
            Row::Plan(_, p) => Some(p.head.id.to_string()),
            Row::Fork(_, _, s) => Some(fork_id(s)),
            Row::Manager(_, s) => Some(s.id.as_str().chars().take(8).collect()),
            Row::Toggle | Row::NewPlan(_) | Row::Archive => None,
        }
    }

    /// The label the row has in the prompts: a Context's title, a Plan's title, a Session's name. The tree itself
    /// shows ids (`shown_id`).
    pub fn label(&self) -> String {
        match *self {
            Row::Context(c) => c.title.clone(),
            Row::Plan(_, p) => p.head.title.clone(),
            Row::Fork(_, _, s) | Row::Manager(_, s) => session_label(s),
            Row::Toggle | Row::NewPlan(_) | Row::Archive => String::new(),
        }
    }
}

/// What the tree shows for a fork: `<plan id>·<first 4 characters of its Session id>`, else (no Plan) the first 8
/// characters of its Session id.
fn fork_id(s: &SessionNode) -> String {
    match &s.plan {
        Some(plan) => format!("{plan}·{}", s.id.as_str().chars().take(4).collect::<String>()),
        None => s.id.as_str().chars().take(8).collect(),
    }
}

/// A Session's pi name, else the first 8 characters of its id.
pub fn session_label(s: &SessionNode) -> String {
    let name = s.name.trim();
    if name.is_empty() { s.id.as_str().chars().take(8).collect() } else { name.to_owned() }
}

/// `10⋅3⋅1⋅5`: each nonzero role in its status colour, `⋅` between two drawn roles only. The failed count is always red.
pub fn counters(plan: &PlanNode) -> Line<'static> {
    let counts = plan.head.counts;
    let mut spans = Vec::new();
    for (i, n) in counts.counters().into_iter().enumerate() {
        if n == 0 {
            continue;
        }
        if !spans.is_empty() {
            spans.push(palette::separator());
        }
        spans.push(Span::styled(n.to_string(), counter_style(&counts, i, n)));
    }
    Line::from(spans)
}

/// Style of the `index`th counter of `JobCounts::counters`: its role's status colour (colour 8 when zero).
fn counter_style(counts: &JobCounts, index: usize, n: usize) -> Style {
    let (status, merging) = counts.counter_role(index);
    palette::count_style(status, merging, n)
}

// `width` and `truncate` live in `crate::cells`, the one place text is measured in display columns; the re-export
// keeps this module's callers (`tree/view.rs`) and tests reading `rows::width` / `truncate`.
pub(crate) use crate::cells::{truncate, width};

/// A cost drawn beside a row's name or id: `format_cost`, colour 8.
fn cost_span(usd: f64) -> Span<'static> {
    Span::styled(cpd_core::metrics::format_cost(usd), palette::muted_text())
}

/// What a Plan row draws at the right in a row `width` wide: its counters, right-aligned to the row's last cell, when
/// they leave `MIN_LEFT`; else nothing. The cost is not here: it is drawn beside the name ([`plan_cost`]). Both the
/// name's cut and `draw` read it.
fn plan_right(p: &PlanNode, width: usize) -> Line<'static> {
    let counters = counters(p);
    if !counters.spans.is_empty() && counters.width() + MIN_LEFT <= width { counters } else { Line::default() }
}

/// The cost a Plan row draws one space after its name (pro-d186): when its counters (if any) are drawn and the name
/// keeps a cell after the reserve of the counters, one space and the cost. The cost is dropped first, then the counters.
fn plan_cost(p: &PlanNode, width: usize) -> Option<Span<'static>> {
    let cost = cost_span(p.cost_usd?);
    let c = counters(p).width();
    let counters_drawn = plan_right(p, width).width() > 0;
    if c > 0 && !counters_drawn {
        return None;
    }
    let reserve = 1 + cost.width() + if c > 0 { c + 1 } else { 0 };
    (width >= PLAN_ROW_FIXED + reserve + 1).then_some(cost)
}

/// The cells of a Plan row that the name never takes: the branch, the dot and its space, and the one level of indent
/// that `row_text` counts.
const PLAN_ROW_FIXED: usize = 6;

/// The cost a Context row draws one space after its id (pro-d186), when the id keeps a cell: `3 + 1 + k + 1 <= width`.
fn context_cost(c: &ContextNode, width: usize) -> Option<Span<'static>> {
    let cost = cost_span(c.cost_usd?);
    (cost.width() + MIN_LEFT <= width).then_some(cost)
}

/// What a Plan's or fork's row says (both are one level under their Context).
struct RowText<'a> {
    /// The Session of the row: its name is blue when it is in the chat slot (selected).
    session: Option<&'a SessionNode>,
    name: String,
    /// How deep the row is: 1 for a Plan or fork of a live Context.
    depth: usize,
    /// The animation frame of the working dot (`palette::frame`).
    frame: usize,
    /// The width of the counters drawn at the right (0: none).
    counters_width: usize,
    /// The cost drawn one space after the name (a Plan row whose cost fits).
    cost: Option<Span<'static>>,
    /// Archived: its dot is grey and never turns (the colours are muted when drawn, but the selected name stays blue).
    muted: bool,
}

/// A Plan's or fork's row text (the reference's `session_text`), without its branch (the fold's): dot, name. The
/// name is cut so that, in a column `width` wide, it never reaches its cost or the counters drawn at the right; the cost
/// follows the cut name, one space after it.
fn row_text(r: RowText, width: u16) -> Line<'static> {
    let dot = r.session.map_or(Dot::Stopped, |s| s.dot);
    let mut spans = vec![palette::dot_span(dot, r.frame, r.muted), " ".into()];
    let used = r.depth * INDENT + BRANCH_WIDTH + Line::from(spans.clone()).width() + if r.counters_width > 0 { r.counters_width + 1 } else { 0 }
        + r.cost.as_ref().map_or(0, |c| 1 + c.width());
    let name = truncate(&r.name, (width as usize).saturating_sub(used));
    spans.push(selectable_name(name, r.session.is_some_and(SessionNode::is_active)));
    if let Some(cost) = r.cost {
        spans.extend([" ".into(), cost]);
    }
    Line::from(spans)
}

/// A row's name: blue when `selected` (hs-d351), else clickable text, colour 15 (the row is a hit target).
fn selectable_name(name: String, selected: bool) -> Span<'static> {
    Span::styled(name, if selected { palette::selected_name() } else { palette::clickable_text() })
}

/// A Context's title (after the `╭`): its dot (the most urgent of its live Sessions, grey when the Context is
/// archived) and id, blue when its Manager is in the chat slot, cut so that its cost fits. The fold makes it bold; the
/// cost is not part of it ([`context_cost`], added after the fold's styling so that it stays colour 8, never bold).
fn context_text(c: &ContextNode, width: u16, frame: usize) -> Line<'static> {
    let selected = c.manager_place == Some(Place::Chat);
    let dot = palette::dot_span(c.dot, frame, c.is_archived() || c.manager_asleep);
    let id = match context_cost(c, usize::from(width)) {
        None => c.id.to_string(),
        // the `╭`, the dot and its space come first; a cell stays blank before the cost
        Some(cost) => truncate(c.id.as_str(), usize::from(width).saturating_sub(1 + dot.width() + 1 + cost.width() + 1)),
    };
    Line::from(vec![dot, " ".into(), selectable_name(id, selected)])
}

/// A Plan's row text, one level under its Context.
fn plan_row(c: &ContextNode, p: &PlanNode, width: u16, frame: usize) -> (Id, Line<'static>) {
    let muted = c.is_archived() || p.head.is_archived() || p.lead().is_some_and(SessionNode::is_asleep);
    // what is reserved is what `draw` draws; counters too narrow to be drawn keep today's reserve
    let counters_width = counters(p).width(); // drawn or not; the width is the same in any colour
    let cost = plan_cost(p, usize::from(width));
    let text = row_text(RowText { session: p.lead(), name: p.head.id.to_string(), depth: 1, frame, counters_width, cost, muted }, width);
    (p.head.id.to_string(), text)
}

/// A fork's row text, one level under its Context.
fn fork_row(s: &SessionNode, width: u16, frame: usize, muted: bool) -> (Id, Line<'static>) {
    let text = row_text(RowText { session: Some(s), name: fork_id(s), depth: 1, frame, counters_width: 0, cost: None, muted }, width);
    (s.id.to_string(), text)
}

/// An archived Manager's row text, one level under its Context: always muted, named by `session_label`.
fn manager_row(s: &SessionNode, width: u16, frame: usize) -> (Id, Line<'static>) {
    let text = row_text(RowText { session: Some(s), name: session_label(s), depth: 1, frame, counters_width: 0, cost: None, muted: true }, width);
    (s.id.to_string(), text)
}

/// A visible row of the tree: its identifier (a path) and what it says.
pub struct FlatRow {
    pub path: Vec<Id>,
    pub line: Line<'static>,
}

/// What [`draw`] drew: the rectangle of the rows (the cue rows are not in it) and, one per row of it, the path drawn.
#[derive(Debug, Default)]
pub struct Hits {
    pub rows: Rect,
    pub paths: Vec<Vec<Id>>,
}

/// A Context's visible rows: its title row and, when its path is in `opened`, its live Plans and forks, then, when
/// expanded, its archived ones, and, right after, its closing row. `opened` is the tree state's open set: the Context is
/// expanded when it holds the closing row's identifier. The closing row is a sibling of the title row, not a child, so
/// that it stays under the live rows.
///
/// The drawn order's key (hs-d686, pro-d48): the row's dot-urgency tier first, then its creation, newest first
/// ([`Dot::order_key`]). The sorts are stable, so a tie keeps the read model's order.
///
/// A Plan row's key: its Lead's dot (a missing Lead has no Inbox item, tier 4), then the Plan's `created_at`.
pub(crate) fn plan_key(p: &PlanNode) -> OrderKey {
    p.lead().map_or(Dot::Stopped, |l| l.dot).order_key(&p.head.created_at)
}

/// A Context with nothing to fold (no archived row) has no closing row when it has rows: its last row's branch is the
/// `╰─` instead of `├─` (hs-d477). With no row at all it ends with the terminator ` ╰󰬪` (U+F0B2A; hs-d574).
fn context_items(c: &ContextNode, opened: &HashSet<Vec<Id>>, width: u16, frame: usize) -> Vec<FlatRow> {
    let id = c.id.to_string();
    let close = toggle_id(&id);
    let k = kids(c);
    let expanded = opened.contains(&vec![close.clone()]);
    // The live Plans and forks are ONE list, stable-sorted by tier then creation, newest first (hs-d686, pro-d48): a tie keeps the
    // read model's order (the Plans before the forks).
    let mut live: Vec<(OrderKey, Id, Line<'static>)> = Vec::new();
    for p in &k.direct {
        let (id, line) = plan_row(c, p, width, frame);
        live.push((plan_key(p), id, line));
    }
    for (_, s) in &k.forks {
        let (id, line) = fork_row(s, width, frame, s.is_asleep());
        live.push((s.dot.order_key(&s.created_at), id, line));
    }
    live.sort_by(|a, b| a.0.cmp(&b.0));
    let mut rows: Vec<(Id, Line<'static>)> = live.into_iter().map(|(_, id, line)| (id, line)).collect();
    if expanded {
        rows.extend(k.archived.iter().map(|p| plan_row(c, p, width, frame)));
        rows.extend(k.strays.iter().map(|(_, s)| fork_row(s, width, frame, true)));
        rows.extend(k.managers.iter().map(|s| manager_row(s, width, frame)));
    }
    let empty = rows.is_empty();
    let last = rows.len().saturating_sub(1);
    let closes_in_a_row = !k.has_archived() && !empty;
    let mut title = fold::title(context_text(c, width, frame));
    if let Some(cost) = context_cost(c, usize::from(width)) {
        title.spans.extend([" ".into(), cost]);
    }
    let mut flat = vec![FlatRow { path: vec![id.clone()], line: title }];
    if opened.contains(&vec![id.clone()]) {
        flat.extend(rows.into_iter().enumerate().map(|(i, (child, line))| {
            let branch = if closes_in_a_row && i == last { fold::LAST_BRANCH } else { fold::TREE_BRANCH };
            FlatRow { path: vec![id.clone(), child], line: fold::content(branch, line) }
        }));
    }
    if !closes_in_a_row {
        let closing = if k.has_archived() {
            fold::toggle(expanded)
        } else if !c.is_archived() && empty {
            fold::new_plan()
        } else {
            fold::plain()
        };
        flat.push(FlatRow { path: vec![close], line: closing });
    }
    flat
}

/// The empty row between two Contexts: one, not two (the margins of two blocks collapse into one; hs-d476).
fn spacer(c: &ContextNode) -> FlatRow {
    FlatRow { path: vec![format!("{SPACER}{}", c.id)], line: Line::default() }
}

/// The tree root's archive row: `󰬦 archive` folded, `󰬬 archive` expanded, colour 8 on every cell (hs-d698, hs-d707).
fn archive_item(opened: &HashSet<Vec<Id>>) -> FlatRow {
    let path = vec![ARCHIVE.to_owned()];
    let glyph = if opened.contains(&path) { fold::UP } else { fold::DOWN };
    FlatRow { path, line: Line::from(Span::styled(format!("{glyph} archive"), palette::muted_text())) }
}

/// The tree's visible rows for a column `width` wide: the live Contexts, then the archive row, then the archived
/// Contexts when the archive path is open. `opened` is the tree state's open set: a Context not in it shows its title
/// only. `frame` is the animation frame of the working dots.
pub fn items(tree: &Tree, opened: &HashSet<Vec<Id>>, width: u16, frame: usize) -> Vec<FlatRow> {
    let (archived, mut live): (Vec<&ContextNode>, Vec<&ContextNode>) = tree.contexts.iter().partition(|c| c.is_archived());
    // The live Contexts by tier then creation, newest first (hs-d686, pro-d48); the archived ones keep their read-model order after them.
    live.sort_by(|a, b| a.dot.order_key(&a.created_at).cmp(&b.dot.order_key(&b.created_at)));
    let mut rows: Vec<FlatRow> = Vec::new();
    let last_live = live.len().saturating_sub(1);
    for (i, c) in live.iter().enumerate() {
        rows.extend(context_items(c, opened, width, frame));
        if i != last_live || !archived.is_empty() {
            rows.push(spacer(c));
        }
    }
    if !archived.is_empty() {
        rows.push(archive_item(opened));
        if opened.contains(&vec![ARCHIVE.to_owned()]) {
            let last = archived.len().saturating_sub(1);
            for (i, c) in archived.iter().enumerate() {
                rows.extend(context_items(c, opened, width, frame));
                if i != last {
                    rows.push(spacer(c));
                }
            }
        }
    }
    rows
}

/// The tree's scroll (commands.md §13.2, hs-d307): the first row drawn, which the wheel and the cursor move. View state
/// only. The rows scroll in their area with a dim, right-aligned `▲ <n> more` / `▼ <n> more` cue row (`dialog::scroll::cue`, hs-d348) where some are hidden (the Deploy
/// column's PR list and the Inbox section scroll with the same code, one row a row).
#[derive(Debug, Default)]
pub struct Scroll {
    offset: usize,
    /// The largest offset, as the last frame had it.
    max: usize,
    /// The cursor's path, the number of rows and the rows of the area when the cursor was last brought into view: the
    /// cursor is followed when one of them changed (it moved, a fold or a snapshot changed the rows, the section
    /// below grew), never because of the wheel.
    followed: Option<(Vec<Id>, usize, u16)>,
    /// What the last [`draw`] drew: the click geometry (`draw` is called as a statement by the tests, so it hands the
    /// hits back through the state it already takes).
    hits: Hits,
}

impl Scroll {
    /// The rows the last frame drew, with the path of each.
    pub fn hits(&self) -> &Hits {
        &self.hits
    }

    /// A wheel notch: `rows` down or up, clamped to the rows as the last frame had them.
    pub fn wheel(&mut self, down: bool, rows: usize) {
        let offset = self.offset.min(self.max);
        self.offset = if down { (offset + rows).min(self.max) } else { offset.saturating_sub(rows) };
    }
}

/// The number of rows the tree draws: one for every visible row, or the one `no Contexts` row.
pub fn row_count(state: &TreeState<Id>, tree: &Tree) -> usize {
    if tree.contexts.is_empty() { 1 } else { items(tree, state.opened(), 0, 0).len() }
}

/// The scroll offset nearest `scroll` at which the row `i` of `n` is drawn in `rows` list rows.
fn scroll_to(i: usize, n: usize, rows: usize, scroll: usize) -> usize {
    let max = Window::max_scroll(n, rows);
    scroll.min(max).min(i).max((i + 1).saturating_sub(rows).min(max))
}

/// The list's rows inside the pane's `area` (the ▲ line, the list, the ▼ line): row 0 is the ▲ line, the last row the ▼
/// line; with fewer than 3 rows there is no ▼ line and the list takes the rows after row 0.
pub fn list_area(area: Rect) -> Rect {
    let top = area.height.min(1);
    let below = u16::from(area.height >= 3);
    Rect::new(area.x, area.y + top, area.width, area.height - top - below)
}

/// Draw the column into `area`, the pane without its footer: row 0 is the ▲ line, the list is [`list_area`], the last row
/// is the ▼ line (both reserved, blank when nothing is hidden, never a click target). The tree from the row `scroll` says
/// (the cursor's Session is brought into view when it moved), then the counters and the guides of each visible
/// row, the archived rows muted, then the cursor's Session row as the blue block when
/// `cursor` says it is shown (a Context row never is). `frame` is the animation frame of the working dots.
pub fn draw(f: &mut Frame, area: Rect, tree: &Tree, state: &mut TreeState<Id>, cursor: bool, frame: usize, scroll: &mut Scroll) {
    if tree.contexts.is_empty() {
        scroll.max = 0;
        f.render_widget(Paragraph::new(Line::from(Span::styled(" no Contexts", palette::muted_text()))), list_area(area));
        scroll.hits = Hits::default();
        return;
    }
    let items = items(tree, state.opened(), area.width, frame);
    let flat = items;
    let list = list_area(area);
    let height = usize::from(list.height);
    scroll.max = Window::max_scroll(flat.len(), height);
    let key = (state.selected().to_vec(), flat.len(), list.height);
    if scroll.followed.as_ref() != Some(&key) {
        if let Some(i) = flat.iter().position(|r| r.path == key.0) {
            scroll.offset = scroll_to(i, flat.len(), height, scroll.offset);
        }
        scroll.followed = Some(key);
    }
    let win = Window::new(flat.len(), height, scroll.offset);
    scroll.offset = win.first;
    let rows = Rect { height: u16::try_from(win.shown).unwrap_or(u16::MAX), ..list };
    let drawn = &flat[win.first..win.first + win.shown];
    if win.cue_above && area.height > 0 {
        f.render_widget(Paragraph::new(cue(Cue::Above, win.first, usize::from(area.width))), Rect::new(area.x, area.y, area.width, 1));
    }
    if win.below > 0 && list.bottom() < area.bottom() {
        f.render_widget(Paragraph::new(cue(Cue::Below, win.below, usize::from(area.width))), Rect::new(area.x, area.bottom() - 1, area.width, 1));
    }
    let buf = f.buffer_mut();
    for (y, flat_row) in (rows.y..rows.bottom()).zip(drawn) {
        let rect = Rect::new(area.x, y, area.width, 1);
        let path = &flat_row.path;
        let Some(row) = row(tree, path) else { continue };
        flat_row.line.clone().render(rect, buf);
        // an asleep row's counters (separators too) are colour 8 whatever the count: the muted restyle skips them
        let mut counter_cols = 0..0;
        let width = rect.width;
        // the counters are dropped when they would not leave the branch, the glyph and a cell of the name
        if let Row::Plan(_, plan) = row {
            let right = plan_right(plan, usize::from(width));
            if right.width() > 0 {
                let start = rect.x + width.saturating_sub(u16::try_from(right.width()).unwrap_or(u16::MAX));
                counter_cols = start..rect.x + width;
                right.right_aligned().render(rect, buf);
            }
        }
        // the cost ends its row's text (one space after the name or id): colour 8 whatever the row, the restyle skips it
        let cost_width = match row {
            Row::Plan(_, plan) => plan_cost(plan, usize::from(width)),
            Row::Context(c) => context_cost(c, usize::from(width)),
            _ => None,
        }
        .map_or(0, |cost| u16::try_from(cost.width()).unwrap_or(u16::MAX));
        let text_end = rect.x + u16::try_from(flat_row.line.width()).unwrap_or(u16::MAX).min(width);
        let cost_cols = text_end.saturating_sub(cost_width)..text_end;
        if row.muted() {
            // archived: colour 8 on every mark; asleep: colour 7 on every mark, the fold's lines (the branch) and the counters
            // staying 8; either way except the selected name: selection beats the muted colour
            let archived = row.archived();
            let style = if archived { palette::archived_text() } else { palette::asleep_text() };
            for x in rect.x..rect.right() {
                let branch = x < rect.x + 3 && "╭├╰│─".contains(buf[(x, y)].symbol());
                if buf[(x, y)].fg != palette::selected_name().fg.unwrap_or_default() {
                    let kept = counter_cols.contains(&x) || cost_cols.contains(&x);
                    if archived || !branch && !kept {
                        buf[(x, y)].set_style(style);
                    } else if kept {
                        buf[(x, y)].set_style(palette::archived_text());
                    }
                }
            }
        }
        if cursor && row.selectable() && path.as_slice() == state.selected() {
            buf.set_style(rect, palette::selected());
        }
    }
    scroll.hits = Hits { rows, paths: drawn.iter().map(|r| r.path.clone()).collect() };
}

/// Open every Context the first time it is seen, so that its live rows show, and expand each live one (its archived rows
/// show; a fold by the User is kept across refreshes). The archived Contexts start folded.
pub fn open_new(state: &mut TreeState<Id>, known: &mut HashSet<Vec<Id>>, tree: &Tree) {
    for c in &tree.contexts {
        let context = vec![c.id.to_string()];
        if known.insert(context.clone()) {
            state.open(context);
            if !c.is_archived() {
                state.open(vec![toggle_id(c.id.as_str())]);
            }
        }
    }
}

/// The visible Session rows (the only selectable ones: the live Plans' Leads and the live forks), in order.
fn visible(state: &TreeState<Id>, tree: &Tree) -> Vec<Vec<Id>> {
    items(tree, state.opened(), 0, 0)
        .into_iter()
        .map(|f| f.path)
        .filter(|id| row(tree, id).is_some_and(|r| r.selectable()))
        .collect()
}

/// After a refresh, keep the cursor on its Session; when it is gone (a Plan or Session that went or was archived, a
/// collapsed or vanished Context) move it to the Session now at the same position, else the last one; with no
/// Session visible, clear it. `before` is the cursor's index among the visible Sessions before the refresh.
pub fn reselect(state: &mut TreeState<Id>, tree: &Tree, before: Option<usize>) {
    let visible = visible(state, tree);
    if visible.iter().any(|id| id.as_slice() == state.selected()) {
        return;
    }
    state.select(visible.get(before.unwrap_or(0).min(visible.len().saturating_sub(1))).cloned().unwrap_or_default());
}

/// The index of the cursor among the visible Sessions.
pub fn selected_index(state: &TreeState<Id>, tree: &Tree) -> Option<usize> {
    visible(state, tree).iter().position(|id| id.as_slice() == state.selected())
}

/// Move the cursor `delta` Sessions (clamped), skipping Context, archived and toggle rows; with no cursor, down goes
/// to the first, up to the last.
pub fn move_cursor(state: &mut TreeState<Id>, tree: &Tree, delta: isize) {
    let visible = visible(state, tree);
    let Some(last) = visible.len().checked_sub(1) else { return };
    let next = match selected_index(state, tree) {
        Some(i) => i.saturating_add_signed(delta).min(last),
        None if delta < 0 => last,
        None => 0,
    };
    state.select(visible[next].clone());
}

/// A click on a closing row (`╰󰬦 expand`, `╰󰬬 collapse`) shows or hides its Context's archived rows; it selects nothing.
/// Returns whether `path` actually toggled: the closing row of a Context with archived rows, and no other. The terminator
/// `╰󰬪` of a Context with no row (nothing to fold) and the live Context's `╰󰬪 new` row do not toggle, so a click falls
/// through to the caller.
pub fn click_toggle(state: &mut TreeState<Id>, tree: &Tree, path: &[Id]) -> bool {
    if path == [ARCHIVE] {
        if !tree.contexts.iter().any(ContextNode::is_archived) {
            return false;
        }
        state.toggle(path.to_vec());
        return true;
    }
    let Some(Row::Context(c)) = toggle_owner(path).and_then(|owner| row(tree, &owner)) else { return false };
    if kids(c).has_archived() {
        state.toggle(path.to_vec());
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    /// The dropped mark of the Session in the chat slot (md-play): only the tests that it is gone name it.
    const ACTIVE_MARK: &str = "\u{F040A}";

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Position;
    use ratatui::style::{Color, Modifier};

    use crate::buffer::copy_cells;
    use super::super::fixture::{WHEN, archived, context, plan, sample, session, tree, managed};
    use super::*;
    use cpd_core::plan::Phase;
    use cpd_core::session::State;
    use crate::palette::{IDLE, Meaning, NEEDS_YOU, SPINNER, STOPPED, color};
    use crate::test_text::{row as text, rows as lines};

    const WORKING: &str = SPINNER[0];
    use crate::read::{Dot, Place};

    fn path(parts: &[&str]) -> Vec<Id> {
        parts.iter().map(|p| (*p).to_owned()).collect()
    }

    /// The tree state `open_new` makes (every Context open, the live ones expanded), with the Contexts `collapsed` folded
    /// (their archived rows hidden), and the cursor on `selected`.
    fn state_for(tree: &Tree, collapsed: &[&str], selected: &[&str]) -> TreeState<Id> {
        let mut state = TreeState::default();
        open_new(&mut state, &mut HashSet::new(), tree);
        for c in collapsed {
            state.close(&toggle_of(&[c]));
        }
        state.select(path(selected));
        state
    }

    fn render(tree: &Tree, state: &mut TreeState<Id>, width: u16, height: u16, cursor: bool) -> Buffer {
        render_at(tree, state, width, height, cursor, 0)
    }

    /// The first `height` rows of the column drawn in a pane tall enough for every row: these tests are about the rows,
    /// not the scroll (`view::tests` test the scroll and its cues), so no cue row replaces a row they read; row 0, the ▲ line, is skipped.
    fn render_at(tree: &Tree, state: &mut TreeState<Id>, width: u16, height: u16, cursor: bool, frame: usize) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height + 40)).unwrap();
        terminal.draw(|f| draw(f, f.area(), tree, state, cursor, frame, &mut Scroll::default())).unwrap();
        let full = terminal.backend().buffer();
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        copy_cells(full, Rect::new(0, 1, width, height), &mut buf, Position::new(0, 0));
        buf
    }

    /// A row of an area `width` wide: `left`, then `right`, which ends at the row's last cell.
    fn row_of(left: &str, right: &str, width: usize) -> String {
        format!("{left:<w$}{right}", w = width - right.chars().count())
    }

    fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
        buf[(x, y)].fg
    }

    fn reversed(buf: &Buffer, y: u16) -> Vec<bool> {
        (0..buf.area.width).map(|x| crate::palette::check::is_selected(&buf[(x, y)])).collect()
    }

    /// The closing toggle `╰󰬦 expand` of a Context with archived rows, as the fold widget draws it.
    const EXPAND: &str = "╰\u{F0B26} expand";
    /// The closing toggle `╰󰬬 collapse`.
    const COLLAPSE: &str = "╰\u{F0B2C} collapse";
    /// The closing row of a Context with nothing to fold.
    const TERMINATOR: &str = "╰󰬪";
    /// The closing row of a LIVE Context with nothing to fold (hs-d688).
    const NEW_PLAN: &str = "╰󰬪 new";
    /// The tree root's archive row, folded and expanded (hs-d698, hs-d707).
    const ARCHIVE_DOWN: &str = "\u{F0B26} archive";
    const ARCHIVE_UP: &str = "\u{F0B2C} archive";

    /// A Plan with an idle Lead named `name` and no forks.
    fn lone(id: &str, name: &str, phase: Phase, counts: [usize; 6]) -> PlanNode {
        plan(id, "cc", phase, counts, session(&format!("s-{id}"), name, State::Idle, None, Place::Parked), vec![])
    }

    /// The README's "Context column" example (`main:tui/README.md`): a Context with three Lead Sessions; the
    /// one in the chat slot needs you.
    fn readme() -> Tree {
        let mut rust = session("s-rust", "cpd rust lead", State::Blocked, Some("φ: Worker question"), Place::Chat);
        rust.dot = Dot::Red;
        tree(vec![managed(context(
            "cpd",
            vec![
                plan("v1", "cpd", Phase::Done, [0, 36, 0, 0, 1, 5], session("s-v1", "v1", State::Idle, None, Place::Parked), vec![]),
                plan("rust", "cpd", Phase::Dispatch, [0, 0, 1, 0, 0, 1], rust, vec![]),
                plan("ui", "cpd", Phase::Deploy, [0, 1, 0, 2, 0, 3], session("s-ui", "cpd-ui", State::Idle, None, Place::Parked), vec![]),
            ],
        ), Dot::White)])
    }

    #[test]
    fn the_readme_context_column_example() {
        let t = readme();
        let buf = render(&t, &mut state_for(&t, &[], &[]), 31, 4, false);
        assert_eq!(
            lines(&buf),
            [
                format!("╭{NEEDS_YOU} cpd"),
                row_of(&format!("├─{NEEDS_YOU} rust"), "1⋅1", 31),
                row_of(&format!("├─{IDLE} v1"), "36⋅1⋅5", 31),
                row_of(&format!("╰─{IDLE} ui"), "1⋅2⋅3", 31),
            ]
        );
        assert_eq!(buf[(4, 1)].fg, palette::selected_name().fg.unwrap(), "the name of the needs-you row is blue");
        assert!(!buf[(4, 1)].modifier.contains(Modifier::BOLD), "not bold");
        assert!(buf[(2, 0)].modifier.contains(Modifier::BOLD), "and a Context's title");
        assert!(!buf[(4, 2)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_tree_shows_contexts_plans_and_then_the_forks_as_their_siblings() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 12, false);
        assert_eq!(
            lines(&buf),
            [
                format!("╭{NEEDS_YOU} beta").to_owned(),
                row_of(&format!("╰─{NEEDS_YOU} rel"), "3", 44),
                String::new(),
                format!("╭{NEEDS_YOU} alpha").to_owned(),
                format!("├─{NEEDS_YOU} ship·f-bl").to_owned(),
                row_of(&format!("├─{WORKING} ship"), "2⋅2⋅5", 44),
                row_of(&format!("├─{IDLE} sketch"), "4", 44),
                row_of(&format!("├─{STOPPED} old"), "1⋅36", 44),
                format!("╰─{IDLE} ship·f-id").to_owned(),
                String::new(),
                format!("╭{STOPPED} gamma").to_owned(),
                NEW_PLAN.to_owned(),
            ],
            "no state text after a name; the last row of a Context with nothing archived ends it with `╰─`, one empty row between two Contexts, and the clickable new row closes a live Context with no row inside"
        );
    }

    #[test]
    fn a_context_is_a_fold_a_bold_title_muted_branches_and_a_muted_closing_row_and_only_that_row_toggles() {
        let tree = sample();
        let mut state = state_for(&tree, &[], &[]);
        let buf = render(&tree, &mut state, 44, 12, false);
        assert_eq!((buf[(0, 0)].symbol(), fg(&buf, 0, 0), buf[(0, 0)].modifier.contains(Modifier::BOLD)), ("╭", color(Meaning::Nothing), false), "a muted `╭`");
        for x in 1..44 {
            if !buf[(x, 0)].symbol().trim().is_empty() {
                assert!(buf[(x, 0)].modifier.contains(Modifier::BOLD), "the title is bold: column {x}");
            }
        }
        for y in [1, 4, 5, 6, 7, 8] {
            assert_eq!((buf[(1, y)].symbol(), fg(&buf, 1, y)), ("─", color(Meaning::Nothing)), "a muted branch at row {y}");
        }
        for (y, branch) in [(1, "╰"), (4, "├"), (5, "├"), (6, "├"), (7, "├"), (8, "╰")] {
            assert_eq!((buf[(0, y)].symbol(), fg(&buf, 0, y)), (branch, color(Meaning::Nothing)), "a muted branch at row {y}");
        }
        // gamma, a live Context with no row, closes with `╰󰬪 new`: the corner and the glyph are muted, the word clickable
        assert_eq!(text(&buf, 11), NEW_PLAN);
        for x in [0, 1] {
            assert_eq!(fg(&buf, x, 11), color(Meaning::Nothing), "the corner and the glyph are muted: ({x}, 11)");
            assert!(!buf[(x, 11)].modifier.contains(Modifier::BOLD));
        }
        for x in 3..6 {
            assert_eq!(fg(&buf, x, 11), palette::clickable_text().fg.unwrap(), "the word `new` is clickable: ({x}, 11)");
        }
        assert_ne!(fg(&buf, 4, 5), color(Meaning::Nothing), "a content row is not muted");
        assert_ne!(fg(&buf, 2, 5), color(Meaning::Nothing));
        // a live Context with no row at all ends with the clickable new row, which folds nothing
        let before = state.opened().clone();
        assert!(!click_toggle(&mut state, &tree, &toggle_of(&["gamma"])), "the new row is not a toggle");
        assert_eq!(state.opened(), &before, "and there is nothing to fold");
        // only the closing row folds
        let tree = archived();
        let mut state = state_for(&tree, &[], &[]);
        for context in [["alpha"], ["beta"]] {
            assert!(!click_toggle(&mut state, &tree, &path(&context)), "the title row does not toggle");
        }
        assert!(!click_toggle(&mut state, &tree, &path(&["alpha", "ship"])), "nor a content row");
        assert!(click_toggle(&mut state, &tree, &toggle_of(&["alpha"])));
        assert!(!state.opened().contains(&toggle_of(&["alpha"])), "folded");
        assert!(state.opened().contains(&path(&["alpha"])), "the live rows stay open");
        let folded = lines(&render(&tree, &mut state, 44, 8, false));
        assert_eq!(folded[3..8], [format!("╭{NEEDS_YOU} alpha"), row_of(&format!("├─{WORKING} ship"), "2⋅2⋅5", 44), row_of(&format!("├─{IDLE} sketch"), "4", 44), format!("├─{IDLE} ship·f-id"), EXPAND.to_owned()]);
        assert!(!click_toggle(&mut state, &tree, &toggle_of(&["nope"])), "no Context, no closing row");
        assert!(click_toggle(&mut state, &tree, &toggle_of(&["alpha"])));
        assert!(state.opened().contains(&toggle_of(&["alpha"])), "the closing row unfolds it");
    }

    #[test]
    fn an_empty_store_says_so() {
        let empty = tree(vec![]);
        let buf = render(&empty, &mut TreeState::default(), 30, 2, true);
        assert_eq!(lines(&buf), [" no Contexts", ""]);
    }

    #[test]
    fn a_folded_context_shows_its_live_rows_and_hides_only_its_archived_ones() {
        let tree = archived();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 10, false);
        assert_eq!(
            lines(&buf),
            [
                format!("╭{NEEDS_YOU} beta").to_owned(),
                row_of(&format!("╰─{NEEDS_YOU} rel"), "3", 44),
                String::new(),
                format!("╭{NEEDS_YOU} alpha").to_owned(),
                row_of(&format!("├─{WORKING} ship"), "2⋅2⋅5", 44),
                row_of(&format!("├─{IDLE} sketch"), "4", 44),
                format!("├─{IDLE} ship·f-id").to_owned(),
                EXPAND.to_owned(),
                String::new(),
                format!("╭{STOPPED} gamma").to_owned(),
            ]
        );
    }

    #[test]
    fn a_context_has_the_dot_of_its_manager_and_not_of_any_session_inside() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 9, false);
        assert_eq!(text(&buf, 3), format!("╭{NEEDS_YOU} alpha"), "its Manager's, not its working Lead's, and no Plan count: a fold hides only archived rows");
        let calm = super::super::fixture::tree(vec![managed(context("cc", vec![lone("pp", "s", Phase::Plan, [0; 6])]), Dot::Idle)]);
        assert_eq!(text(&render(&calm, &mut state_for(&calm, &[], &[]), 20, 1, false), 0), format!("╭{IDLE} cc"));
        let blocked = super::super::fixture::tree(vec![managed(context("cc", vec![with_dot("pp", Dot::Red)]), Dot::Idle)]);
        assert_eq!(text(&render(&blocked, &mut state_for(&blocked, &[], &[]), 20, 1, false), 0), format!("╭{IDLE} cc"), "a blocked Lead does not show");
        let none = super::super::fixture::tree(vec![context("cc", vec![with_dot("pp", Dot::Red)])]);
        assert_eq!(text(&render(&none, &mut state_for(&none, &[], &[]), 20, 1, false), 0), format!("╭{STOPPED} cc"), "no Manager: stopped");
    }

    #[test]
    fn a_rows_dot_is_its_sessions_and_not_its_plans() {
        // an open Escalation is not the ●: the Lead's state is (commands.md §17)
        let mut p = lone("pp", "lead", Phase::Dispatch, [0; 6]);
        p.head.open_escalations = 1;
        let t = tree(vec![managed(context("cc", vec![p]), Dot::Idle)]);
        assert_eq!(lines(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false)), [format!("╭{IDLE} cc").to_owned(), format!("╰─{IDLE} pp")]);
        let mut t = t;
        t.contexts[0].plans[0].sessions[0].dot = Dot::Red;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false), 1), format!("╰─{NEEDS_YOU} pp"));
    }

    #[test]
    fn drafts_are_counted_and_dark_grey_in_phase_plan() {
        let t = tree(vec![context("cc", vec![lone("pp", "p", Phase::Plan, [4, 0, 0, 0, 0, 0])])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} pp"), "4", 20));
        assert_eq!(fg(&buf, 19, 1), palette::color_clickable(Meaning::Planning), "the drafts of phase `plan` are magenta (13)");
        assert_eq!(buf[(18, 1)].symbol(), " ", "no separator before a lone role");
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_the_counters_stay_right_aligned() {
        let long = lone("a-very-long-plan-id", "lead", Phase::Dispatch, [0, 36, 0, 0, 1, 5]);
        let t = tree(vec![context("cc", vec![long])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 2, false);
        assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} a-very-long-plan…"), "36⋅1⋅5", 30));
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "ab…");
        assert_eq!(truncate("abcd", 0), "");
    }

    /// tui-W8: a name is cut by its display width, not its `char` count (wide characters take two columns).
    #[test]
    fn a_name_is_cut_by_display_width_not_char_count() {
        assert_eq!(truncate("計画計画", 8), "計画計画");
        assert_eq!(truncate("計画計画", 7), "計画計…", "6 columns and the …");
        assert_eq!(truncate("計画計画", 6), "計画…", "a wide character that would reach the … is dropped");
        assert_eq!(truncate("計画", 1), "…");
        assert_eq!(truncate("計画", 0), "");
    }

    /// A Plan whose Lead has the dot `dot`.
    fn with_dot(id: &str, dot: Dot) -> PlanNode {
        let mut p = lone(id, "s", Phase::Dispatch, [0; 6]);
        p.sessions[0].dot = dot;
        p
    }

    /// The glyph and foreground of the dot `dot` at frame 0, as the palette draws it.
    fn drawn_dot(dot: Dot) -> (String, Color) {
        let span = palette::dot_span(dot, 0, false);
        (span.content.into_owned(), span.style.fg.unwrap_or_default())
    }

    fn cell(buf: &Buffer, x: u16, y: u16) -> (String, Color) {
        (buf[(x, y)].symbol().to_owned(), fg(buf, x, y))
    }

    #[test]
    fn every_dot_is_drawn_as_the_palette_draws_it_on_a_session_row_and_its_context_and_no_phase_colours_it() {
        for phase in [Phase::Grounding, Phase::Plan, Phase::Dispatch, Phase::Deploy, Phase::Done] {
            for dot in [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::Cyan, Dot::White, Dot::Working(None), Dot::Idle, Dot::Stopped] {
                let mut p = with_dot("pp", dot);
                p.head.phase = phase;
                let t = tree(vec![managed(context("cc", vec![p]), dot)]);
                let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
                assert_eq!(cell(&buf, 2, 1), drawn_dot(dot), "{dot:?} in {phase:?}");
                assert_eq!(cell(&buf, 1, 0), drawn_dot(dot), "its Context row too: {dot:?} in {phase:?}");
            }
        }
        assert_eq!(drawn_dot(Dot::Red).0, format!("{NEEDS_YOU}"));
        assert_eq!(drawn_dot(Dot::Idle), (format!("{IDLE}").to_owned(), palette::clickable_text().fg.unwrap()), "clickable text, 15");
    }

    #[test]
    fn a_stopped_dot_is_muted() {
        let t = tree(vec![managed(context("cc", vec![with_dot("pp", Dot::Stopped)]), Dot::Stopped)]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!((buf[(2, 1)].symbol(), buf[(1, 0)].symbol()), (STOPPED, STOPPED));
        assert!(crate::palette::check::is_muted(&buf[(2, 1)]) && crate::palette::check::is_muted(&buf[(1, 0)]));
        let t = tree(vec![context("cc", vec![with_dot("pp", Dot::Idle)])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert!(!crate::palette::check::is_muted(&buf[(2, 1)]));
    }


    #[test]
    fn a_working_dot_turns_with_the_frame_on_session_rows_and_their_context() {
        let t = tree(vec![managed(context("cc", vec![with_dot("pp", Dot::Working(None))]), Dot::Working(None))]);
        let glyphs: Vec<(String, String)> = (0..=SPINNER.len())
            .map(|frame| {
                let buf = render_at(&t, &mut state_for(&t, &[], &[]), 20, 2, false, frame);
                (buf[(1, 0)].symbol().to_owned(), buf[(2, 1)].symbol().to_owned())
            })
            .collect();
        let want: Vec<(String, String)> =
            (0..=SPINNER.len()).map(|f| (SPINNER[f % SPINNER.len()].to_owned(), SPINNER[f % SPINNER.len()].to_owned())).collect();
        assert_eq!(glyphs, want);
    }

    #[test]
    fn an_archived_working_row_is_grey_and_does_not_turn() {
        let mut p = with_dot("pp", Dot::Working(None));
        p.head.archived_at = Some(WHEN.into());
        let mut gone = managed(context("zz", vec![with_dot("qq", Dot::Working(None))]), Dot::Working(None));
        gone.archived_at = Some(WHEN.into());
        let t = tree(vec![managed(context("cc", vec![p]), Dot::Working(None)), gone]);
        let mut state = state_for(&t, &[], &[]);
        state.open(vec![ARCHIVE.to_owned()]);
        unfold(&mut state, &t, "zz");
        for frame in 0..SPINNER.len() {
            let buf = render_at(&t, &mut state, 24, 12, false, frame);
            let grey = (String::from("\u{E3D5}"), color(Meaning::Nothing));
            assert_eq!(cell(&buf, 2, 1), grey, "frame {frame}: an archived Plan row, always the hollow ring {:?}", lines(&buf));
            assert_eq!(cell(&buf, 1, 5), grey, "frame {frame}: an archived Context row {:?}", lines(&buf));
            assert_eq!(cell(&buf, 2, 6), grey, "frame {frame}: its Plan row");
            assert_eq!(buf[(1, 0)].symbol(), SPINNER[frame], "while the live Context row turns");
        }
    }

    #[test]
    fn an_archived_row_keeps_its_glyph_in_grey() {
        let mut p = with_dot("pp", Dot::Red);
        p.head.archived_at = Some(WHEN.into());
        let t = tree(vec![context("cc", vec![p])]);
        let mut state = state_for(&t, &[], &[]);
        let buf = render(&t, &mut state, 24, 4, false);
        assert_eq!(cell(&buf, 2, 1), (format!("{NEEDS_YOU}").to_owned(), color(Meaning::Nothing)), "{:?}", lines(&buf));
    }

    #[test]
    fn a_context_row_shows_its_managers_dot_whatever_the_sessions_inside() {
        let t = tree(vec![managed(context("cc", vec![with_dot("aa", Dot::Yellow), with_dot("bb", Dot::Red), with_dot("dd", Dot::White)]), Dot::Working(None))]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 4, false);
        assert_eq!(cell(&buf, 1, 0), drawn_dot(Dot::Working(None)));
        let collapsed = render(&t, &mut state_for(&t, &["cc"], &[]), 20, 1, false);
        assert_eq!(cell(&collapsed, 1, 0), drawn_dot(Dot::Working(None)), "collapsed too");
        assert_eq!((cell(&buf, 2, 1), cell(&buf, 2, 2), cell(&buf, 2, 3)), (drawn_dot(Dot::Red), drawn_dot(Dot::Yellow), drawn_dot(Dot::White)), "while each Session row keeps its own, red first by tier");
    }



    #[test]
    fn the_guides_are_dark_grey() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 9, false);
        for (x, y, glyph) in [(1, 1, '─'), (0, 4, '├'), (1, 4, '─'), (0, 5, '├'), (0, 6, '├'), (0, 7, '├'), (0, 8, '╰'), (1, 8, '─')] {
            assert_eq!((text(&buf, y).chars().nth(x), fg(&buf, x as u16, y)), (Some(glyph), color(Meaning::Nothing)), "({x}, {y})");
        }
    }

    #[test]
    fn a_fork_is_a_sibling_of_the_plans_and_the_last_row_closes_the_guide() {
        let t = tree(vec![context(
            "cc",
            vec![plan("pp", "cc", Phase::Plan, [0; 6], session("l", "lead", State::Idle, None, Place::None), vec![session("f", "fork", State::Idle, None, Place::None)])],
        )]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 4, false);
        assert_eq!(lines(&buf)[1..], [format!("├─{IDLE} pp"), format!("╰─{IDLE} pp·f").to_owned(), String::new()]);
    }

    #[test]
    fn the_selected_session_row_is_the_blue_block_across_the_full_width_and_no_other_row() {
        let tree = sample();
        for (selected, y) in [(&["alpha", "ship"][..], 5), (&["alpha", "f-idle"], 8)] {
            let buf = render(&tree, &mut state_for(&tree, &[], selected), 44, 9, true);
            for row in 0..9 {
                assert_eq!(reversed(&buf, row), vec![row == y; 44], "{selected:?}, row {row}");
            }
        }
        // no mark keeps a colour of its own: every cell of the row is the blue block
        let buf = render(&tree, &mut state_for(&tree, &[], &["alpha", "ship"]), 44, 9, true);
        assert!((0..44).all(|x| fg(&buf, x, 5) == palette::selected().fg.unwrap()), "the zero, a separator, the running Jobs: all the block's blue");
    }

    #[test]
    fn the_cursor_is_drawn_only_when_it_is_shown_and_with_no_selection_no_row_is_reversed() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &["alpha", "ship"]), 44, 9, false);
        assert!((0..9).all(|y| reversed(&buf, y) == vec![false; 44]));
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 9, true);
        assert!((0..9).all(|y| reversed(&buf, y) == vec![false; 44]));
    }

    #[test]
    fn a_context_row_is_never_inverted() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &["alpha"]), 44, 3, true);
        assert!((0..3).all(|y| reversed(&buf, y) == vec![false; 44]));
    }

    // ---- hs-d351: the selected row's name is blue, the chat-slot mark is dropped (commands.md §13.2 "Rows", §17 "Blue") ----

    /// The one colour for in focus and selected. Parsed, not named, so that no colour is spelled outside the palette
    /// (`no_colour_outside_the_palette`); `palette`'s blue is this one.
    fn blue() -> Color {
        "blue".parse().unwrap()
    }

    /// The row of `buf` that says `needle`.
    fn row_with(buf: &Buffer, needle: &str) -> u16 {
        (0..buf.area.height).find(|&y| text(buf, y).contains(needle)).unwrap_or_else(|| panic!("no row says {needle:?}: {:?}", lines(buf)))
    }

    /// Every cell of `buf` whose foreground is blue, a cursor's block apart.
    fn blue_cells(buf: &Buffer) -> Vec<(u16, u16)> {
        (0..buf.area.height).flat_map(|y| (0..buf.area.width).map(move |x| (x, y))).filter(|&(x, y)| fg(buf, x, y) == blue() && !crate::palette::check::is_selected(&buf[(x, y)])).collect()
    }

    /// The cells `x0..x0 + width` of row `y`, the width of `name`, with the name drawn there.
    fn name_cells(buf: &Buffer, y: u16, x0: u16, name: &str) -> Vec<(u16, u16)> {
        let at: String = (x0..x0 + name.chars().count() as u16).map(|x| buf[(x, y)].symbol()).collect();
        assert_eq!(at, name, "the name starts at column {x0} of {:?}", text(buf, y));
        (x0..x0 + name.chars().count() as u16).map(|x| (x, y)).collect()
    }

    #[test]
    fn the_selected_plan_row_draws_its_name_in_blue_and_no_chat_mark_anywhere() {
        let tree = sample(); // `ship`'s Lead is in the chat slot
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 8, false);
        assert_eq!(text(&buf, 5), row_of(&format!("├─{WORKING} ship"), "2⋅2⋅5", 44), "column 0 is the branch, not `{ACTIVE_MARK}`");
        assert!(!lines(&buf).iter().any(|l| l.contains(ACTIVE_MARK)), "{:?}", lines(&buf));
        let name = name_cells(&buf, 5, 4, "ship");
        assert_eq!(blue_cells(&buf), name, "the name, and nothing else on the screen");
        assert!(name.iter().all(|&(x, y)| !buf[(x, y)].modifier.contains(Modifier::BOLD)), "selected is blue, not bold");
    }

    #[test]
    fn the_rest_of_the_selected_row_keeps_its_colours() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 8, false);
        let grey = color(Meaning::Nothing);
        assert_eq!((fg(&buf, 4, 5), fg(&buf, 7, 5)), (blue(), blue()), "the name is what turns blue");
        assert_eq!(text(&buf, 5).chars().next(), Some('├'), "the branch, not a mark");
        assert_eq!((fg(&buf, 0, 5), fg(&buf, 1, 5)), (grey, grey), "the branch");
        assert_eq!(cell(&buf, 2, 5), drawn_dot(Dot::Working(None)), "the dot");
        // counters "2⋅2⋅5" end at the row's last cell
        let counters: Vec<Color> = [39, 41, 43].iter().map(|&x| fg(&buf, x, 5)).collect();
        assert_eq!(
            counters,
            [palette::color_clickable(Meaning::NotStarted), palette::color_clickable(Meaning::Review), palette::color_clickable(Meaning::Done)]
        );
        assert_eq!((fg(&buf, 40, 5), fg(&buf, 42, 5)), (grey, grey), "the separators");
    }

    #[test]
    fn a_selected_fork_row_draws_its_name_in_blue_and_nothing_else_is() {
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[0].place = Place::Parked;
        tree.contexts[0].plans[0].sessions[1].place = Place::Chat; // the fork `ship·f-id`
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 9, false);
        assert_eq!(text(&buf, 8), format!("╰─{IDLE} ship·f-id"));
        assert!(!lines(&buf).iter().any(|l| l.contains(ACTIVE_MARK)), "{:?}", lines(&buf));
        assert_eq!(blue_cells(&buf), name_cells(&buf, 8, 4, "ship·f-id"));
        assert_eq!(cell(&buf, 2, 8), drawn_dot(Dot::Idle), "the dot keeps its colour");
        assert_eq!((fg(&buf, 0, 8), fg(&buf, 1, 8)), (color(Meaning::Nothing), color(Meaning::Nothing)), "so does the branch");
    }

    #[test]
    fn no_row_has_a_blue_name_when_no_session_is_in_the_chat_slot_whatever_the_cursor() {
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[0].place = Place::Parked;
        for cursor in [false, true] {
            let buf = render(&tree, &mut state_for(&tree, &[], &["alpha", "sketch"]), 44, 8, cursor);
            assert_eq!(blue_cells(&buf), [], "cursor shown: {cursor}; the cursor is not the selection");
        }
    }

    #[test]
    fn the_cursor_on_another_row_does_not_move_the_blue() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &["alpha", "sketch"]), 44, 8, true);
        assert_eq!(blue_cells(&buf), name_cells(&buf, 5, 4, "ship"), "still the chat slot's row, not `sketch`");
        assert!(reversed(&buf, 6).iter().all(|&r| r), "while the cursor row is reversed");
    }

    #[test]
    fn a_selected_archived_plan_row_is_blue_where_the_archived_rows_are_muted() {
        let mut tree = archived();
        tree.contexts[0].plans[0].sessions[0].place = Place::Parked;
        tree.contexts[0].plans[2].sessions[0].place = Place::Chat; // the archived Plan `old`
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 10, false);
        let y = row_with(&buf, " old");
        assert_eq!(blue_cells(&buf), name_cells(&buf, y, 4, "old"), "its name, not muted; no other archived row: {:?}", lines(&buf));
        let grey = color(Meaning::Nothing);
        assert_eq!((fg(&buf, 0, y), fg(&buf, 1, y)), (grey, grey), "the branch stays muted");
        assert_eq!(cell(&buf, 2, y), (STOPPED.to_owned(), grey), "the dot stays muted");
        assert!(!text(&buf, y).contains(ACTIVE_MARK));
        assert!((35..44).all(|x| fg(&buf, x, y) == grey), "the counters stay muted");
        assert!(!buf[(4, y)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_selected_archived_fork_row_is_blue_where_the_archived_rows_are_muted() {
        let mut tree = archived();
        tree.contexts[0].plans[0].sessions[0].place = Place::Parked;
        tree.contexts[0].plans[0].sessions[2].place = Place::Chat; // the archived fork `ship·f-bl`
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 10, false);
        let y = row_with(&buf, "ship·f-bl");
        assert_eq!(blue_cells(&buf), name_cells(&buf, y, 4, "ship·f-bl"), "{:?}", lines(&buf));
        let grey = color(Meaning::Nothing);
        assert_eq!((fg(&buf, 0, y), fg(&buf, 1, y), fg(&buf, 2, y)), (grey, grey, grey), "branch and dot stay muted");
    }

    #[test]
    fn an_unselected_archived_row_stays_muted_all_through() {
        let mut tree = archived();
        tree.contexts[0].plans[0].sessions[0].place = Place::Parked;
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 10, false);
        assert_eq!(blue_cells(&buf), []);
        let y = row_with(&buf, " old");
        assert!((0..44).filter(|&x| buf[(x, y)].symbol() != " ").all(|x| fg(&buf, x, y) == color(Meaning::Nothing)));
    }

    #[test]
    fn the_keyboard_cursor_skips_context_rows() {
        let tree = sample();
        let mut state = state_for(&tree, &[], &[]);
        let at = |state: &mut TreeState<Id>, d| {
            move_cursor(state, &tree, d);
            state.selected().to_vec()
        };
        assert_eq!(at(&mut state, 1), path(&["beta", "rel"]), "down from nothing: the first Session");
        assert_eq!(at(&mut state, 1), path(&["alpha", "f-blocked"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "ship"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "sketch"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "old"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "f-idle"]), "the red fork leads its Context, then the Plans and the idle fork");
        assert_eq!(at(&mut state, 1), path(&["alpha", "f-idle"]), "clamped at the end: `gamma` has no Session");
        assert_eq!(at(&mut state, -1), path(&["alpha", "old"]), "back over the idle fork");
        let mut none = state_for(&tree, &[], &[]);
        move_cursor(&mut none, &tree, -1);
        assert_eq!(none.selected(), path(&["alpha", "f-idle"]), "up from nothing: the last Session");
    }

    #[test]
    fn the_cursor_still_reaches_the_live_sessions_of_a_folded_context() {
        let tree = archived();
        let mut state = state_for(&tree, &["alpha"], &[]);
        move_cursor(&mut state, &tree, 1);
        assert_eq!(state.selected(), path(&["beta", "rel"]), "the first live Session, in the Context before alpha");
        for _ in 0..3 {
            move_cursor(&mut state, &tree, 1);
        }
        assert_eq!(state.selected(), path(&["alpha", "f-idle"]), "rel, then ship, sketch and the live fork of the folded Context");
    }

    #[test]
    fn a_click_on_a_closing_row_folds_its_context_and_selects_nothing() {
        let tree = archived();
        let mut state = state_for(&tree, &[], &["alpha", "ship"]);
        assert!(click_toggle(&mut state, &tree, &toggle_of(&["alpha"])));
        assert_eq!(state.selected(), path(&["alpha", "ship"]), "the cursor stays where it was");
        let mut fresh = state_for(&tree, &[], &[]);
        click_toggle(&mut fresh, &tree, &toggle_of(&["beta"]));
        assert!(fresh.selected().is_empty());
        assert!(!click_toggle(&mut fresh, &tree, &path(&["alpha", "f-idle"])), "a fork is no toggle row");
    }

    #[test]
    fn rows_resolve_from_their_paths() {
        let tree = sample();
        assert!(matches!(row(&tree, &path(&["alpha"])), Some(Row::Context(c)) if c.id.as_str() == "alpha"));
        assert!(matches!(row(&tree, &path(&["alpha", "ship"])), Some(Row::Plan(c, p)) if p.head.id.as_str() == "ship" && c.id.as_str() == "alpha"));
        let fork = row(&tree, &path(&["alpha", "f-idle"])).unwrap();
        assert!(matches!(fork, Row::Fork(..)), "a fork is a row of its Context, not of its Plan");
        assert_eq!(fork.session().unwrap().id.as_str(), "f-idle");
        assert_eq!(fork.plan().unwrap().head.id.as_str(), "ship");
        assert_eq!(fork.label(), "ship fork");
        assert_eq!(row(&tree, &path(&["alpha", "ship"])).unwrap().session().unwrap().id.as_str(), "l-ship", "a Plan row is its Lead's");
        assert!(row(&tree, &path(&["gamma"])).unwrap().session().is_none());
        for missing in [&["nope"][..], &["alpha", "nope"], &["alpha", "l-ship"], &["alpha", "ship", "f-idle"], &["alpha", "ship", "nope"], &[]] {
            assert!(row(&tree, &path(missing)).is_none(), "{missing:?}");
        }
    }

    #[test]
    fn contexts_open_once_and_a_collapse_is_kept() {
        let tree = sample();
        let (mut state, mut known) = (TreeState::default(), HashSet::new());
        open_new(&mut state, &mut known, &tree);
        assert_eq!(state.opened().len(), 6, "the three Contexts, and the three live ones expanded");
        state.close(&toggle_of(&["alpha"]));
        open_new(&mut state, &mut known, &tree);
        assert_eq!(state.opened().len(), 5, "a fold is kept");
        let t = archived();
        let mut state = TreeState::default();
        open_new(&mut state, &mut HashSet::new(), &t);
        assert!(state.opened().contains(&path(&["zeta"])) && !state.opened().contains(&toggle_of(&["zeta"])), "an archived Context starts folded");
    }

    #[test]
    fn the_cursor_moves_to_the_session_now_at_its_place_when_its_session_goes() {
        let mut tree = sample();
        let mut state = state_for(&tree, &[], &["alpha", "sketch"]);
        let before = selected_index(&state, &tree);
        assert_eq!(before, Some(3));
        tree.contexts[0].plans.remove(1);
        reselect(&mut state, &tree, before);
        assert_eq!(state.selected(), path(&["alpha", "old"]));
        reselect(&mut state, &tree, Some(0));
        assert_eq!(state.selected(), path(&["alpha", "old"]), "a selection still there stays");
        tree.contexts.clear();
        reselect(&mut state, &tree, Some(3));
        assert!(state.selected().is_empty(), "nothing visible: no cursor");
    }

    #[test]
    fn a_context_row_shows_its_id_a_plan_row_its_plans_id_and_a_fork_its_plan_and_the_start_of_its_session_id() {
        let mut tree = sample();
        tree.contexts[0].title = "Payments".into();
        tree.contexts[0].plans[0].head.title = "Pay by card".into();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 9, false);
        assert_eq!(text(&buf, 3), format!("╭{NEEDS_YOU} alpha"), "not its title `Payments`");
        assert_eq!(text(&buf, 5), row_of(&format!("├─{WORKING} ship"), "2⋅2⋅5", 44), "not its title `Pay by card`, nor its Lead's name");
        assert_eq!(text(&buf, 4), format!("├─{NEEDS_YOU} ship·f-bl"), "the Plan id, `·`, the first 4 characters of the Session id; not its name");
        // the prompts word a row by its title or name: Rename… edits those, not the id
        assert_eq!(row(&tree, &path(&["alpha"])).unwrap().label(), "Payments");
        assert_eq!(row(&tree, &path(&["alpha", "ship"])).unwrap().label(), "Pay by card");
        assert_eq!(row(&tree, &path(&["alpha", "f-idle"])).unwrap().label(), "ship fork");
        let mut s = session("0123456789abcdef", "", State::Idle, None, Place::None);
        assert_eq!(session_label(&s), "01234567", "a blank name: the short id");
        s.name = " named ".into();
        assert_eq!(session_label(&s), "named");
    }

    #[test]
    fn a_fork_with_no_plan_shows_the_first_8_characters_of_its_session_id() {
        let t = tree(vec![context(
            "cc",
            vec![plan("pp", "cc", Phase::Plan, [0; 6], session("l", "lead", State::Idle, None, Place::None), vec![session("0123456789abcdef", "named", State::Idle, None, Place::None)])],
        )]);
        let mut t = t;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 3, false), 2), format!("╰─{IDLE} pp·0123"));
        t.contexts[0].plans[0].sessions[1].plan = None;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 3, false), 2), format!("╰─{IDLE} 01234567"));
        assert_eq!(row(&t, &path(&["cc", "0123456789abcdef"])).unwrap().shown_id().as_deref(), Some("01234567"));
    }

    #[test]
    fn shown_id_is_the_id_each_row_shows_and_a_toggle_row_has_none() {
        let t = archived();
        let shown = |parts: &[&str]| row(&t, &path(parts)).unwrap().shown_id();
        assert_eq!(shown(&["alpha"]).as_deref(), Some("alpha"), "a Context: its id");
        assert_eq!(shown(&["alpha", "ship"]).as_deref(), Some("ship"), "a Plan: its id");
        assert_eq!(shown(&["alpha", "f-idle"]).as_deref(), Some("ship·f-id"), "a fork: its Plan, `·`, 4 characters of its id");
        assert_eq!(shown(&["alpha", "old"]).as_deref(), Some("old"), "an archived Plan");
        assert_eq!(shown(&["alpha", "f-blocked"]).as_deref(), Some("ship·f-bl"), "an archived fork");
        assert_eq!(shown(&["zeta"]).as_deref(), Some("zeta"), "an archived Context");
        assert_eq!(shown(&["zeta", "f-zp"]).as_deref(), Some("zp·f-zp"), "a fork of an archived Context");
        assert_eq!(shown(&toggle_of(&["alpha"]).iter().map(String::as_str).collect::<Vec<_>>()), None, "nor has a toggle row");
    }

    // ---------- archived rows (hs-d339) ----------

    /// The identifier of the toggle row of the Context whose own row is at `head`.
    fn toggle_of(head: &[&str]) -> Vec<Id> {
        let (last, parents) = head.split_last().unwrap();
        let mut id = path(parents);
        id.push(toggle_id(last));
        id
    }

    /// `state` with the Context `id` unfolded, by a click on its toggle row.
    fn unfold(state: &mut TreeState<Id>, t: &Tree, id: &str) {
        assert!(click_toggle(state, t, &toggle_of(&[id])));
    }

    #[test]
    fn archived_plans_and_forks_are_inside_the_expanded_block_after_the_live_rows_muted_with_no_group_row_and_one_toggle() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        state.open(vec![ARCHIVE.to_owned()]);
        let buf = render(&t, &mut state, 44, 20, false);
        assert_eq!(
            lines(&buf)[..13],
            [
                format!("╭{NEEDS_YOU} beta").to_owned(),
                row_of(&format!("╰─{NEEDS_YOU} rel"), "3", 44),
                String::new(),
                format!("╭{NEEDS_YOU} alpha").to_owned(),
                row_of(&format!("├─{WORKING} ship"), "2⋅2⋅5", 44),
                row_of(&format!("├─{IDLE} sketch"), "4", 44),
                format!("├─{IDLE} ship·f-id").to_owned(),
                row_of(&format!("├─{STOPPED} old"), "1⋅36", 44),
                format!("├─{NEEDS_YOU} ship·f-bl").to_owned(),
                COLLAPSE.to_owned(),
                String::new(),
                format!("╭{STOPPED} gamma").to_owned(),
                NEW_PLAN.to_owned(),
            ],
            "the archived Plan and fork follow the live rows, no `󰇘`; one closing row per Context (or its last row's `╰─`), one empty row between two Contexts"
        );
        let all = lines(&buf).join("\n");
        assert!(!all.contains('\u{F01D8}'), "no `󰇘` row anywhere");
        assert_eq!(lines(&buf).iter().filter(|l| l.starts_with('╰')).count(), 4, "one closing row (or last row) per Context, never two: {all}");
        // the live rows keep their colours; the archived ones are all dark grey, branch, counters and dots included
        assert_eq!(cell(&buf, 2, 5), drawn_dot(Dot::Idle), "a live dot is not greyed");
        for (y, columns) in [(7, 0..44), (8, 0..24)] {
            for x in columns {
                if buf[(x, y)].symbol() != " " {
                    assert_eq!(fg(&buf, x, y), color(Meaning::Nothing), "({x}, {y}) {}", buf[(x, y)].symbol());
                }
            }
        }
    }

    #[test]
    fn an_archived_fork_of_a_live_plan_and_the_rows_of_an_archived_plan_are_listed_last_muted_inside_the_block() {
        let mut t = sample();
        let mut fork = session("f-rel", "rel fork", State::Idle, None, Place::None);
        fork.plan = Some(t.contexts[1].plans[0].head.id.clone());
        fork.archived_at = Some(WHEN.into());
        t.contexts[1].plans[0].sessions.push(fork);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 14, false);
        assert_eq!(
            lines(&buf)[0..4],
            [format!("╭{NEEDS_YOU} beta"), row_of(&format!("├─{NEEDS_YOU} rel"), "3", 44), format!("├─{IDLE} rel·f-re"), COLLAPSE.to_owned()]
        );
        assert_eq!(fg(&buf, 3, 2), color(Meaning::Nothing), "the archived fork is muted");
        let mut t = sample();
        t.contexts[1].plans[0].head.archived_at = Some(WHEN.into());
        let mut fork = session("f-rel", "rel fork", State::Idle, None, Place::None);
        fork.plan = Some(t.contexts[1].plans[0].head.id.clone());
        t.contexts[1].plans[0].sessions.push(fork);
        for s in &mut t.contexts[1].plans[0].sessions {
            s.archived_at = Some(WHEN.into()); // archiving a Plan archives its Sessions
        }
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 14, false);
        assert_eq!(
            lines(&buf)[0..4],
            [format!("╭{NEEDS_YOU} beta"), row_of(&format!("├─{NEEDS_YOU} rel"), "3", 44), format!("├─{IDLE} rel·f-re"), COLLAPSE.to_owned()],
            "an archived Plan and the fork of an archived Plan are inside the block too"
        );
    }

    #[test]
    fn folded_a_context_hides_its_archived_rows_only_and_its_closing_row_unfolds_them_again() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        assert!(!click_toggle(&mut state, &t, &path(&["alpha"])), "the title row does not toggle");
        assert!(!click_toggle(&mut state, &t, &path(&["alpha", "old"])), "an archived row is no closing row");
        assert!(!click_toggle(&mut state, &t, &path(&["alpha", "ship"])), "nor is a live one");
        assert!(click_toggle(&mut state, &t, &toggle_of(&["alpha"])));
        let rows = lines(&render(&t, &mut state, 44, 10, false));
        assert_eq!(rows[6..10], [format!("├─{IDLE} ship·f-id"), EXPAND.to_owned(), String::new(), format!("╭{STOPPED} gamma")], "{rows:?}");
        unfold(&mut state, &t, "alpha");
        assert_eq!(text(&render(&t, &mut state, 44, 10, false), 9), COLLAPSE);
    }

    #[test]
    fn archived_contexts_follow_the_live_ones_muted_start_folded_and_unfold_at_their_own_toggle_row() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        assert!(!state.opened().contains(&toggle_of(&["zeta"])), "an archived Context starts folded");
        state.open(vec![ARCHIVE.to_owned()]);
        let buf = render(&t, &mut state, 44, 22, false);
        let rows = lines(&buf);
        assert_eq!(
            rows[11..17],
            [
                format!("╭{STOPPED} gamma"),
                NEW_PLAN.to_owned(),
                String::new(),
                ARCHIVE_UP.to_owned(),
                format!("╭{STOPPED} zeta"),
                EXPAND.to_owned()
            ],
            "{rows:#?}"
        );
        unfold(&mut state, &t, "zeta");
        let buf = render(&t, &mut state, 44, 24, false);
        let rows = lines(&buf);
        assert_eq!(
            rows[15..19],
            [
                format!("╭{STOPPED} zeta"),
                row_of(&format!("├─{STOPPED} zp"), "2", 44),
                format!("├─{STOPPED} zp·f-zp"),
                COLLAPSE.to_owned()
            ],
            "{rows:#?}"
        );
        for y in 15..19 {
            for x in 0..44 {
                if buf[(x, y)].symbol() != " " {
                    // the closing row's arrow and word are the fold toggle's colour 7; everything else is muted
                    let toggle = text(&buf, y).contains("expand") || text(&buf, y).contains("collapse");
                    let want = if toggle && buf[(x, y)].symbol() != "╰" { palette::toggle_text().fg.unwrap() } else { color(Meaning::Nothing) };
                    assert_eq!(fg(&buf, x, y), want, "({x}, {y})");
                }
            }
        }
        assert!(click_toggle(&mut state, &t, &toggle_of(&["zeta"])));
        assert_eq!(lines(&render(&t, &mut state, 44, 24, false))[15..17], [format!("╭{STOPPED} zeta"), EXPAND.to_owned()]);
    }

    #[test]
    fn the_cursor_never_stops_on_an_archived_row() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        unfold(&mut state, &t, "zeta");
        let mut seen = vec![];
        for _ in 0..8 {
            move_cursor(&mut state, &t, 1);
            seen.push(state.selected().join("/"));
        }
        assert_eq!(seen, ["beta/rel", "alpha/ship", "alpha/sketch", "alpha/f-idle", "alpha/f-idle", "alpha/f-idle", "alpha/f-idle", "alpha/f-idle"]);
        for _ in 0..8 {
            move_cursor(&mut state, &t, -1);
        }
        assert_eq!(state.selected(), path(&["beta", "rel"]));
        let mut at_end = state_for(&t, &[], &["gamma"]);
        move_cursor(&mut at_end, &t, -1);
        assert_eq!(at_end.selected(), path(&["alpha", "f-idle"]), "up from nothing: the last live Session");
        // a cursor on a Session that gets archived moves to a live one
        let mut state = state_for(&sample(), &[], &["alpha", "sketch"]);
        let before = selected_index(&state, &sample());
        let mut t = sample();
        t.contexts[0].plans[1].head.archived_at = Some(WHEN.into());
        reselect(&mut state, &t, before);
        assert_eq!(state.selected(), path(&["alpha", "old"]));
    }

    fn archived_of_path(t: &Tree, p: &[Id]) -> Option<bool> {
        row(t, p).map(|r| r.archived())
    }

    #[test]
    fn archived_rows_resolve_from_their_paths_and_are_archived() {
        let t = archived();
        let archived_of = |parts: &[&str]| row(&t, &path(parts)).map(|r| r.archived());
        assert_eq!(archived_of(&["alpha"]), Some(false));
        assert_eq!(archived_of(&["alpha", "f-idle"]), Some(false));
        for fold in [&["alpha"][..], &["zeta"]] {
            assert!(matches!(row(&t, &toggle_of(fold)), Some(Row::Toggle)), "the toggle row of {fold:?}");
        }
        assert!(matches!(row(&t, &toggle_of(&["beta"])), Some(Row::NewPlan(_))), "beta is live with nothing archived: the clickable new row");
        for none in [&["beta", "rel"][..], &["nope"], &["alpha", "ship"], &["alpha", "old"]] {
            assert!(row(&t, &toggle_of(none)).is_none(), "{none:?} has no toggle row");
        }
        assert_eq!(archived_of_path(&t, &toggle_of(&["zeta"])), Some(false), "a toggle row is no object");
        assert_eq!(archived_of(&["alpha", "old"]), Some(true));
        assert_eq!(archived_of(&["alpha", "f-blocked"]), Some(true));
        assert_eq!(archived_of(&["zeta"]), Some(true));
        assert_eq!(archived_of(&["zeta", "f-zp"]), Some(true), "a fork of an archived Context is its sibling too");
        let fork = row(&t, &path(&["alpha", "f-blocked"])).unwrap();
        assert_eq!((fork.session().unwrap().id.as_str(), fork.plan().unwrap().head.id.as_str()), ("f-blocked", "ship"));
        assert_eq!(row(&t, &path(&["zeta"])).unwrap().label(), "Zeta");
        for missing in [
            &["alpha", "ship", "f-blocked"][..],
            &["zeta", "zp", "f-zp"],
            &["nope"],
            &[],
        ] {
            assert!(row(&t, &path(missing)).is_none(), "{missing:?}");
        }
    }

    #[test]
    fn a_folded_contexts_title_has_its_managers_dot_and_no_count() {
        let t = archived();
        let buf = render(&t, &mut state_for(&t, &["alpha"], &[]), 44, 6, false);
        assert_eq!(text(&buf, 3), format!("╭{NEEDS_YOU} alpha"), "the dot is the Manager's");
    }
}
