//! The tree itself (the spike's `context.rs`, on the read model): Contexts,
//! their Plans, and the Sessions forked by `session fork` as a `tui-tree-widget` tree.
//!
//! The widget draws one text per row (guide, dot, name). It has no
//! right-aligned column, so each Plan's five Job counters are drawn as a
//! right-aligned `Line` into the row's `Rect` after the tree. The `⏵` of the
//! Session in the chat slot is drawn over the indent of its row for the same
//! reason (the widget owns the indent). Context rows are not selectable (a click only
//! collapses or expands them): the cursor moves between Sessions only, and is
//! inverted over its row's whole width, only while the pane has keyboard focus.
//!
//! Labels (commands.md §13.2): a Context row shows its title, a Plan row its Plan's title, a fork its
//! Session's name.
//!
//! **Forks** (B1, contract §6.8): a forked Session is a sibling of the Plans directly under its Context, after
//! them, never under a Plan or under its source.
//!
//! **Archived rows** (hs-d53, F1). Archived Contexts are behind one `…` row at the end of the live Context list;
//! the archived Plans, and the forks that are archived or of an archived Plan, of a live Context are behind one
//! `…` row at the end of that Context's live rows. The `…` row is a leaf that stays where it is; expanded, the
//! archived rows follow it at the depth they would have if live, with the normal guides, the whole row drawn in
//! the darker grey. Whether a `…` row is expanded is the tree state's open set, so view state only. The cursor
//! never rests on them.
//!
//! Rows (identifiers are paths; `…` is [`MORE`]):
//!
//! | Path | Row |
//! |---|---|
//! | `[context]`, `[context, plan]`, `[context, session]` | a live Context, a Plan (its row is its Lead's), a forked Session |
//! | `[…]` | the `…` row of the archived Contexts |
//! | `[context]`, `[context, plan]`, `[context, session]` | the same, for an archived Context, Plan or fork |
//! | `[context, …]` | the `…` row of the Context's archived Plans and forks |
//!
//! Everything here is pure except `draw`, which only writes to the frame's buffer.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState};

use crate::palette::{self, Meaning};
use cpd_core::plan::Phase;
use crate::read::{ContextNode, Dot, JobStatus, PlanNode, SessionNode, Tree};

/// Tree identifiers: a Context's id, a Plan's id, a Session's id, or [`MORE`].
pub type Id = String;

/// One tree level is indented by two columns (`tui-tree-widget`'s own indentation).
const INDENT: usize = 2;

/// The glyph of a folded Context row, and of an open one: the first cell of its text.
const FOLDED: &str = "▸";
const OPEN: &str = "▾";

/// The column of a Context row's fold glyph in the tree's area: every Context row is at the top level (the archived
/// ones after the `…` row too), so it is the first cell. The click that folds is exactly here.
pub(crate) const FOLD_COLUMN: u16 = 0;

/// The mark of the Session in the chat slot, in column 0 of its row.
pub const ACTIVE_MARK: &str = "⏵";

/// The identifier of a `…` row. No id has this shape.
pub const MORE: &str = "…";

/// What a row of the tree stands for.
#[derive(Clone, Copy, Debug)]
pub enum Row<'a> {
    Context(&'a ContextNode),
    /// A Plan of the Context, with its Lead.
    Plan(&'a ContextNode, &'a PlanNode),
    /// A Session forked by `session fork`, with the Plan it belongs to.
    Fork(&'a ContextNode, &'a PlanNode, &'a SessionNode),
    /// A `…` row: the archived Contexts at the end of the list, or the archived Plans and forks at the end of a Context.
    More,
}

/// What a Context shows directly, and what its `…` row holds.
struct Kids<'a> {
    /// The Plans listed under the Context: its live Plans, or every Plan when the Context is archived.
    direct: Vec<&'a PlanNode>,
    /// The forks listed under the Context after its Plans (B1): those of `direct` that are not archived, or
    /// every fork when the Context is archived.
    forks: Vec<(&'a PlanNode, &'a SessionNode)>,
    /// The archived Plans of a live Context.
    archived: Vec<&'a PlanNode>,
    /// The forks of a live Context that are archived or of an archived Plan.
    strays: Vec<(&'a PlanNode, &'a SessionNode)>,
}

impl Kids<'_> {
    /// The Context has a `…` row.
    fn has_group(&self) -> bool {
        !self.archived.is_empty() || !self.strays.is_empty()
    }
}

fn kids(c: &ContextNode) -> Kids<'_> {
    if c.is_archived() {
        return Kids { direct: c.plans.iter().collect(), forks: c.forks(), archived: Vec::new(), strays: Vec::new() };
    }
    let (archived, direct): (Vec<&PlanNode>, Vec<&PlanNode>) = c.plans.iter().partition(|p| p.head.is_archived());
    let (strays, forks): (Vec<_>, Vec<_>) = c.forks().into_iter().partition(|(p, s)| p.head.is_archived() || s.is_archived());
    Kids { direct, forks, archived, strays }
}

/// The row at `path`, `None` when the tree has none.
pub fn row<'a>(tree: &'a Tree, path: &[Id]) -> Option<Row<'a>> {
    let (context, path) = path.split_first()?;
    if context == MORE {
        return (path.is_empty() && tree.contexts.iter().any(ContextNode::is_archived)).then_some(Row::More);
    }
    let context = tree.contexts.iter().find(|c| c.id.as_str() == context)?;
    let Some((next, path)) = path.split_first() else { return Some(Row::Context(context)) };
    if !path.is_empty() {
        return None;
    }
    let k = kids(context);
    if next == MORE {
        return k.has_group().then_some(Row::More);
    }
    if let Some(plan) = k.direct.iter().chain(&k.archived).copied().find(|p| p.head.id.as_str() == next) {
        return Some(Row::Plan(context, plan));
    }
    k.forks.iter().chain(&k.strays).find(|(_, s)| s.id.as_str() == next).map(|(p, s)| Row::Fork(context, p, s))
}

impl<'a> Row<'a> {
    /// The Session the row opens and closes: a Plan's Lead, or the fork; none for a Context or a `…`.
    pub fn session(&self) -> Option<&'a SessionNode> {
        match *self {
            Row::Context(_) | Row::More => None,
            Row::Plan(_, p) => p.lead(),
            Row::Fork(_, _, s) => Some(s),
        }
    }

    /// The Plan of the row; none for a Context or a `…`.
    pub fn plan(&self) -> Option<&'a PlanNode> {
        match *self {
            Row::Context(_) | Row::More => None,
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
            Row::More => false,
        }
    }

    /// The cursor may rest on it: a live Plan or fork.
    fn selectable(&self) -> bool {
        matches!(self, Row::Plan(..) | Row::Fork(..)) && !self.archived()
    }

    /// The id the row shows in the tree: a Context's id, a Plan's id, a fork's `<plan id>·<first 4 characters of its
    /// Session id>` (the first 8 characters of its id when it has no Plan); none for a `…` row.
    pub fn shown_id(&self) -> Option<String> {
        match *self {
            Row::Context(c) => Some(c.id.to_string()),
            Row::Plan(_, p) => Some(p.head.id.to_string()),
            Row::Fork(_, _, s) => Some(fork_id(s)),
            Row::More => None,
        }
    }

    /// The label the row has in the prompts: a Context's title, a Plan's title, a Session's name. The tree itself
    /// shows ids (`shown_id`).
    pub fn label(&self) -> String {
        match *self {
            Row::Context(c) => c.title.clone(),
            Row::Plan(_, p) => p.head.title.clone(),
            Row::Fork(_, _, s) => session_label(s),
            Row::More => MORE.to_owned(),
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

/// `36⋅0⋅0⋅1⋅5`: each count in its status colour (nothing when zero; the drafts of phase `plan` always), `⋅` between.
pub fn counters(plan: &PlanNode) -> Line<'static> {
    let phase = plan.head.phase;
    let mut spans = Vec::new();
    for (i, n) in plan.head.counts.counters(phase).into_iter().enumerate() {
        if i > 0 {
            spans.push(palette::separator());
        }
        spans.push(Span::styled(n.to_string(), counter_style(i, phase, n)));
    }
    Line::from(spans)
}

/// Style of the `index`th counter of `JobCounts::counters`: its status colour, or nothing when the count is zero. The
/// first counter is the queued Jobs, in phase `plan` the drafts.
fn counter_style(index: usize, phase: Phase, n: usize) -> Style {
    let (status, merging) = match index {
        0 if phase == Phase::Plan => (JobStatus::Draft, false),
        0 => (JobStatus::Queued, false),
        1 => (JobStatus::Running, false),
        2 => (JobStatus::Running, true),
        3 => (JobStatus::Failed, false),
        _ => (JobStatus::Merged, false),
    };
    palette::count_style(status, merging, n)
}

/// `text` cut to `width` columns with a final `…`. Counted in chars (names are plain text).
pub fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else if width == 0 {
        String::new()
    } else {
        text.chars().take(width - 1).collect::<String>() + "…"
    }
}

/// What a Plan's or fork's row says (both are one level under their Context).
struct RowText<'a> {
    /// The Session of the row: its chat is marked with `⏵`, and its name is bold, when it is in the chat slot.
    session: Option<&'a SessionNode>,
    name: String,
    /// How deep the row is: 1 for a Plan or fork of a live Context.
    depth: usize,
    last: bool,
    /// The animation frame of the working dot (`palette::frame`).
    frame: usize,
    /// The width of the counters drawn at the right (0: none).
    counters_width: usize,
    /// Archived: its dot is grey and never turns, and its name is not bold (the colours are muted when drawn).
    muted: bool,
}

/// A Plan's or fork's row text (the reference's `session_text`): guide (`├ `, `└ ` for the last), dot, name. The name is cut so that, in a column `width` wide, it never reaches the counters drawn at
/// the right.
fn row_text(r: RowText, width: u16) -> Line<'static> {
    let dot = r.session.map_or(Dot::Stopped, |s| s.dot);
    let guide = if r.last { "└ " } else { "├ " };
    let mut spans = vec![Span::styled(guide, palette::fg(Meaning::Nothing)), palette::dot_span(dot, r.frame, r.muted), " ".into()];
    let used = r.depth * INDENT + Line::from(spans.clone()).width() + if r.counters_width > 0 { r.counters_width + 1 } else { 0 };
    let name = truncate(&r.name, (width as usize).saturating_sub(used));
    let active = r.session.is_some_and(SessionNode::is_active) && !r.muted;
    spans.push(if active { Span::styled(name, Style::new().add_modifier(Modifier::BOLD)) } else { name.into() });
    Line::from(spans)
}

/// A Context's row text: its fold glyph (`▸` collapsed, `▾` open), its dot (the most urgent of its live Sessions, grey
/// when the Context is archived), id, and the number of Plans listed inside when collapsed.
fn context_text(c: &ContextNode, collapsed: bool, frame: usize) -> Line<'static> {
    let k = kids(c);
    let mut spans = vec![
        (if collapsed { FOLDED } else { OPEN }).into(),
        " ".into(),
        palette::dot_span(c.dot, frame, c.is_archived()),
        " ".into(),
        Span::styled(c.id.to_string(), Style::new().add_modifier(Modifier::BOLD)),
    ];
    if collapsed {
        spans.push(Span::styled(format!(" {}", k.direct.len()), palette::fg(Meaning::Nothing)));
    }
    Line::from(spans)
}

/// The text of a `…` row: `…` in the list of Contexts, below a Context the guide first (`├ ` when its archived
/// rows follow, i.e. expanded, else `└ `).
fn more_text(in_context: bool, expanded: bool) -> Line<'static> {
    let text = match (in_context, expanded) {
        (false, _) => MORE,
        (true, true) => "├ …",
        (true, false) => "└ …",
    };
    Line::from(Span::styled(text, palette::fg(Meaning::Nothing)))
}

/// A Plan's item, one level under its Context; `last` when it is the last child of its Context.
fn plan_item(c: &ContextNode, p: &PlanNode, last: bool, width: u16, frame: usize) -> TreeItem<'static, Id> {
    let muted = c.is_archived() || p.head.is_archived();
    let text = row_text(
        RowText { session: p.lead(), name: p.head.id.to_string(), depth: 1, last, frame, counters_width: counters(p).width(), muted },
        width,
    );
    TreeItem::new_leaf(p.head.id.to_string(), text)
}

/// A Context's item; `opened` is the tree state's open set. Under it: its live Plans and forks, then, when it has
/// archived ones, its `…` row and, expanded, those.
fn context_item(c: &ContextNode, opened: &HashSet<Vec<Id>>, width: u16, frame: usize) -> TreeItem<'static, Id> {
    let path = vec![c.id.to_string()];
    let k = kids(c);
    let n = k.direct.len();
    let mut children: Vec<TreeItem<'static, Id>> =
        k.direct.iter().enumerate().map(|(i, p)| plan_item(c, p, i + 1 == n && k.forks.is_empty() && !k.has_group(), width, frame)).collect();
    for (j, (_, s)) in k.forks.iter().enumerate() {
        let text = row_text(
            RowText {
                session: Some(s),
                name: fork_id(s),
                depth: 1,
                last: j + 1 == k.forks.len() && !k.has_group(),
                frame,
                counters_width: 0,
                muted: c.is_archived(),
            },
            width,
        );
        children.push(TreeItem::new_leaf(s.id.to_string(), text));
    }
    if k.has_group() {
        let expanded = opened.contains(&[path.clone(), vec![MORE.to_owned()]].concat());
        children.push(TreeItem::new_leaf(MORE.to_owned(), more_text(true, expanded)));
        if expanded {
            let n = k.archived.len();
            children.extend(k.archived.iter().enumerate().map(|(i, p)| plan_item(c, p, i + 1 == n && k.strays.is_empty(), width, frame)));
            for (i, (_, s)) in k.strays.iter().enumerate() {
                let text = row_text(
                    RowText {
                        session: Some(s),
                        name: fork_id(s),
                        depth: 1,
                        last: i + 1 == k.strays.len(),
                        frame,
                        counters_width: 0,
                        muted: true,
                    },
                    width,
                );
                children.push(TreeItem::new_leaf(s.id.to_string(), text));
            }
        }
    }
    TreeItem::new(c.id.to_string(), context_text(c, !opened.contains(&path), frame), children).expect("Plan and Session ids are unique")
}

/// The tree's items for a column `width` wide. `opened` is the tree state's open set: a Context not in it is
/// collapsed, and so is a `…` row. `frame` is the animation frame of the working dots.
pub fn items(tree: &Tree, opened: &HashSet<Vec<Id>>, width: u16, frame: usize) -> Vec<TreeItem<'static, Id>> {
    let (archived, live): (Vec<&ContextNode>, Vec<&ContextNode>) = tree.contexts.iter().partition(|c| c.is_archived());
    let mut items: Vec<TreeItem<'static, Id>> = live.into_iter().map(|c| context_item(c, opened, width, frame)).collect();
    if !archived.is_empty() {
        items.push(TreeItem::new_leaf(MORE.to_owned(), more_text(false, false)));
        if opened.contains(&vec![MORE.to_owned()]) {
            items.extend(archived.into_iter().map(|c| context_item(c, opened, width, frame)));
        }
    }
    items
}

/// Draw the column into `area`: the tree, then the counters, the guides and the active mark of each visible
/// row, the archived rows muted, then the cursor's Session row inverted when
/// `cursor` says it is shown (a Context row never is). `frame` is the animation frame of the working dots.
pub fn draw(f: &mut Frame, area: Rect, tree: &Tree, state: &mut TreeState<Id>, cursor: bool, frame: usize) {
    if tree.contexts.is_empty() {
        f.render_widget(Paragraph::new(Line::from(Span::styled(" no Contexts", Style::new().add_modifier(Modifier::DIM)))), area);
        return;
    }
    let items = items(tree, state.opened(), area.width, frame);
    let widget = TreeWidget::new(&items).expect("Context ids are unique").node_closed_symbol("").node_open_symbol("").node_no_children_symbol("");
    f.render_stateful_widget(widget, area, state);
    let flat = state.flatten(&items);
    let buf = f.buffer_mut();
    for (y, flat_row) in (area.y..area.bottom()).zip(flat.iter().skip(state.get_offset())) {
        let rect = Rect::new(area.x, y, area.width, 1);
        let path = &flat_row.identifier;
        let Some(row) = row(tree, path) else { continue };
        let muted = row.archived() || matches!(row, Row::More);
        match row {
            Row::Plan(_, plan) => {
                counters(plan).right_aligned().render(rect, buf);
                if plan.lead().is_some_and(SessionNode::is_active) && !muted {
                    Span::styled(ACTIVE_MARK, Style::new().add_modifier(Modifier::BOLD)).render(rect, buf);
                }
            }
            Row::Fork(_, _, fork) => {
                if fork.is_active() && !muted {
                    Span::styled(ACTIVE_MARK, Style::new().add_modifier(Modifier::BOLD)).render(rect, buf);
                }
            }
            Row::Context(_) | Row::More => {}
        }
        if muted {
            buf.set_style(rect, palette::fg(Meaning::Nothing));
        }
        if cursor && row.selectable() && path.as_slice() == state.selected() {
            buf.set_style(rect, Modifier::REVERSED);
        }
    }
}

/// Open each Context the first time it is seen (Contexts start expanded; a collapse by the User is kept across
/// refreshes). The `…` rows are not opened: they start collapsed.
pub fn open_new(state: &mut TreeState<Id>, known: &mut HashSet<Vec<Id>>, tree: &Tree) {
    for c in &tree.contexts {
        let context = vec![c.id.to_string()];
        if known.insert(context.clone()) {
            state.open(context);
        }
    }
}

/// The visible Session rows (the only selectable ones: the live Plans' Leads and the live forks), in order.
fn visible(state: &TreeState<Id>, tree: &Tree) -> Vec<Vec<Id>> {
    state
        .flatten(&items(tree, state.opened(), 0, 0))
        .into_iter()
        .map(|f| f.identifier)
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

/// Move the cursor `delta` Sessions (clamped), skipping Context, archived and `…` rows; with no cursor, down goes
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

/// A click on a Context row only collapses or expands it; it selects nothing. Returns whether `path` was a Context.
pub fn click_context(state: &mut TreeState<Id>, tree: &Tree, path: &[Id]) -> bool {
    let is_context = matches!(row(tree, path), Some(Row::Context(_)));
    if is_context {
        state.toggle(path.to_vec());
    }
    is_context
}

/// A click on a `…` row expands or collapses its archived rows. Returns whether `path` was a `…` row.
pub fn click_more(state: &mut TreeState<Id>, tree: &Tree, path: &[Id]) -> bool {
    let is_more = matches!(row(tree, path), Some(Row::More));
    if is_more {
        state.toggle(path.to_vec());
    }
    is_more
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    use super::super::fixture::{WHEN, archived, context, plan, refresh_dots, sample, session, tree};
    use super::*;
    use cpd_core::session::State;
    use crate::palette::{IDLE, NEEDS_YOU, SPINNER, STOPPED, color};

    const WORKING: &str = SPINNER[0];
    use crate::read::{Dot, Place};

    fn path(parts: &[&str]) -> Vec<Id> {
        parts.iter().map(|p| (*p).to_owned()).collect()
    }

    /// A state with every Context and Plan open, except the Contexts `collapsed`, and the cursor on `selected`.
    fn state_for(tree: &Tree, collapsed: &[&str], selected: &[&str]) -> TreeState<Id> {
        let mut state = TreeState::default();
        open_new(&mut state, &mut HashSet::new(), tree);
        for c in collapsed {
            state.close(&path(&[c]));
        }
        state.select(path(selected));
        state
    }

    fn render(tree: &Tree, state: &mut TreeState<Id>, width: u16, height: u16, cursor: bool) -> Buffer {
        render_at(tree, state, width, height, cursor, 0)
    }

    fn render_at(tree: &Tree, state: &mut TreeState<Id>, width: u16, height: u16, cursor: bool, frame: usize) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, f.area(), tree, state, cursor, frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    fn lines(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height).map(|y| text(buf, y)).collect()
    }

    /// A row `width` wide: `left`, then `right` flush against the right edge.
    fn row_of(left: &str, right: &str, width: usize) -> String {
        format!("{left:<w$}{right}", w = width - right.chars().count())
    }

    fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
        buf[(x, y)].fg
    }

    fn reversed(buf: &Buffer, y: u16) -> Vec<bool> {
        (0..buf.area.width).map(|x| buf[(x, y)].modifier.contains(Modifier::REVERSED)).collect()
    }

    /// A Plan with an idle Lead named `name` and no forks.
    fn lone(id: &str, name: &str, phase: Phase, counts: [usize; 6]) -> PlanNode {
        plan(id, "cc", phase, counts, session(&format!("s-{id}"), name, State::Idle, None, Place::Parked), vec![])
    }

    /// The README's "Context column" example (`main:tui/README.md`): a Context with three Lead Sessions; the
    /// one in the chat slot needs you.
    fn readme() -> Tree {
        let mut rust = session("s-rust", "cpd rust lead", State::Blocked, Some("cpd: Worker question"), Place::Chat);
        rust.dot = Dot::Red;
        tree(vec![context(
            "cpd",
            vec![
                plan("v1", "cpd", Phase::Done, [0, 36, 0, 0, 1, 5], session("s-v1", "v1", State::Idle, None, Place::Parked), vec![]),
                plan("rust", "cpd", Phase::Dispatch, [0, 0, 1, 0, 0, 1], rust, vec![]),
                plan("ui", "cpd", Phase::Deploy, [0, 1, 0, 2, 0, 3], session("s-ui", "cpd-ui", State::Idle, None, Place::Parked), vec![]),
            ],
        )])
    }

    #[test]
    fn the_readme_context_column_example() {
        let t = readme();
        let buf = render(&t, &mut state_for(&t, &[], &[]), 31, 4, false);
        assert_eq!(
            lines(&buf),
            [format!("▾ {NEEDS_YOU} cpd"), format!("  ├ {IDLE} v1             36⋅0⋅0⋅1⋅5"), format!("⏵ ├ {NEEDS_YOU} rust            0⋅1⋅0⋅0⋅1"), format!("  └ {IDLE} ui              1⋅0⋅2⋅0⋅3")]
        );
        assert!(buf[(6, 2)].modifier.contains(Modifier::BOLD), "the name of the centre row is bold");
        assert!(buf[(0, 2)].modifier.contains(Modifier::BOLD), "so is its ⏵");
        assert!(buf[(4, 0)].modifier.contains(Modifier::BOLD), "and a Context's name");
        assert!(!buf[(0, 0)].modifier.contains(Modifier::BOLD), "but not its glyph");
        assert!(!buf[(6, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_tree_shows_contexts_plans_and_then_the_forks_as_their_siblings() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 10, false);
        assert_eq!(
            lines(&buf),
            [
                format!("▾ {NEEDS_YOU} alpha").to_owned(),
                row_of(&format!("⏵ ├ {WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44),
                row_of(&format!("  ├ {IDLE} sketch"), "4⋅0⋅0⋅0⋅0", 44),
                row_of(&format!("  ├ {STOPPED} old"), "0⋅0⋅0⋅1⋅36", 44),
                format!("  ├ {IDLE} ship·f-id").to_owned(),
                format!("  └ {NEEDS_YOU} ship·f-bl").to_owned(),
                format!("▾ {NEEDS_YOU} beta").to_owned(),
                row_of(&format!("  └ {NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44),
                format!("▾ {STOPPED} gamma").to_owned(),
                String::new(),
            ],
            "no state text after a name"
        );
    }

    #[test]
    fn a_context_row_starts_with_its_fold_glyph_and_no_other_row_has_one() {
        let tree = sample();
        let mut state = state_for(&tree, &["alpha"], &[]);
        let buf = render(&tree, &mut state, 44, 4, false);
        assert_eq!(lines(&buf).iter().map(|l| l.chars().next()).collect::<Vec<_>>(), [Some('▸'), Some('▾'), Some(' '), Some('▾')]);
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 10, false);
        let rows = lines(&buf);
        for (y, row) in rows.iter().enumerate() {
            let glyphs = row.chars().filter(|c| matches!(c, '▸' | '▾')).count();
            assert_eq!(glyphs, [0, 6, 8].contains(&y) as usize, "row {y}: {row}");
        }
        assert_eq!(buf[(FOLD_COLUMN, 0)].symbol(), "▾", "the click target is the cell the glyph is drawn in");
        assert_eq!(fg(&buf, FOLD_COLUMN, 0), fg(&buf, 43, 0), "the default foreground");
        // an archived Context, after the `…` row, is at the top level too
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &t, &["…"]);
        let rows = lines(&render(&t, &mut state, 44, 14, false));
        let y = rows.iter().position(|r| r.contains("zeta")).unwrap();
        let buf = render(&t, &mut state, 44, 14, false);
        assert_eq!(buf[(FOLD_COLUMN, y as u16)].symbol(), "▾", "{rows:?}");
        assert_eq!(fg(&buf, FOLD_COLUMN, y as u16), color(Meaning::Nothing), "dim in an archived Context");
    }

    #[test]
    fn an_empty_store_says_so() {
        let empty = tree(vec![]);
        let buf = render(&empty, &mut TreeState::default(), 30, 2, true);
        assert_eq!(lines(&buf), [" no Contexts", ""]);
    }

    #[test]
    fn a_collapsed_context_shows_the_number_of_plans_and_hides_them() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 4, false);
        assert_eq!(lines(&buf), [format!("▸ {NEEDS_YOU} alpha 3").to_owned(), format!("{NEEDS_YOU} beta").to_owned(), row_of(&format!("  └ {NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44), format!("{STOPPED} gamma").to_owned()]);
    }

    #[test]
    fn a_context_has_the_dot_of_any_session_inside_that_needs_you() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 1, false);
        assert_eq!(text(&buf, 0), format!("▸ {NEEDS_YOU} alpha 3"), "through its blocked fork");
        let calm = super::super::fixture::tree(vec![context("cc", vec![lone("pp", "s", Phase::Plan, [0; 6])])]);
        assert_eq!(text(&render(&calm, &mut state_for(&calm, &[], &[]), 20, 1, false), 0), format!("▾ {IDLE} cc"));
    }

    #[test]
    fn a_rows_dot_is_its_sessions_and_not_its_plans() {
        // an open Escalation is not the ●: the Lead's state is (commands.md §17)
        let mut p = lone("pp", "lead", Phase::Dispatch, [0; 6]);
        p.head.open_escalations = 1;
        let t = tree(vec![context("cc", vec![p])]);
        assert_eq!(lines(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false)), [format!("▾ {IDLE} cc").to_owned(), row_of(&format!("  └ {IDLE} pp"), "0⋅0⋅0⋅0⋅0", 30)]);
        let mut t = t;
        t.contexts[0].plans[0].sessions[0].dot = Dot::Red;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false), 1), row_of(&format!("  └ {NEEDS_YOU} pp"), "0⋅0⋅0⋅0⋅0", 30));
    }

    #[test]
    fn drafts_are_counted_and_dark_grey_in_phase_plan() {
        let t = tree(vec![context("cc", vec![lone("pp", "p", Phase::Plan, [4, 0, 0, 0, 0, 0])])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!(text(&buf, 1), row_of(&format!("  └ {IDLE} pp"), "4⋅0⋅0⋅0⋅0", 20));
        assert_eq!(fg(&buf, 11, 1), color(Meaning::Nothing));
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_the_counters_stay_right_aligned() {
        let long = lone("a-very-long-plan-id", "lead", Phase::Dispatch, [0, 36, 0, 0, 1, 5]);
        let t = tree(vec![context("cc", vec![long])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 2, false);
        assert_eq!(text(&buf, 1), row_of(&format!("  └ {IDLE} a-very-long-…"), "36⋅0⋅0⋅1⋅5", 30));
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "ab…");
        assert_eq!(truncate("abcd", 0), "");
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
            for dot in [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::White, Dot::Working, Dot::Idle, Dot::Stopped] {
                let mut p = with_dot("pp", dot);
                p.head.phase = phase;
                let t = tree(vec![context("cc", vec![p])]);
                let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
                assert_eq!(cell(&buf, 4, 1), drawn_dot(dot), "{dot:?} in {phase:?}");
                assert_eq!(cell(&buf, 2, 0), drawn_dot(dot), "its Context row too: {dot:?} in {phase:?}");
            }
        }
        assert_eq!(drawn_dot(Dot::Red).0, format!("{NEEDS_YOU}"));
        assert_eq!(drawn_dot(Dot::Idle), (format!("{IDLE}").to_owned(), Default::default()), "the terminal's foreground");
    }

    #[test]
    fn a_stopped_dot_is_dim() {
        let t = tree(vec![context("cc", vec![with_dot("pp", Dot::Stopped)])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!((buf[(4, 1)].symbol(), buf[(2, 0)].symbol()), (STOPPED, STOPPED));
        assert!(buf[(4, 1)].modifier.contains(Modifier::DIM) && buf[(2, 0)].modifier.contains(Modifier::DIM));
        let t = tree(vec![context("cc", vec![with_dot("pp", Dot::Idle)])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert!(!buf[(4, 1)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_working_dot_turns_with_the_frame_on_session_rows_and_their_context() {
        let t = tree(vec![context("cc", vec![with_dot("pp", Dot::Working)])]);
        let glyphs: Vec<(String, String)> = (0..9)
            .map(|frame| {
                let buf = render_at(&t, &mut state_for(&t, &[], &[]), 20, 2, false, frame);
                (buf[(2, 0)].symbol().to_owned(), buf[(4, 1)].symbol().to_owned())
            })
            .collect();
        let want: Vec<(String, String)> =
            (0..9).map(|f| (SPINNER[f % SPINNER.len()].to_owned(), SPINNER[f % SPINNER.len()].to_owned())).collect();
        assert_eq!(glyphs, want);
    }

    #[test]
    fn an_archived_working_row_is_grey_and_does_not_turn() {
        let mut p = with_dot("pp", Dot::Working);
        p.head.archived_at = Some(WHEN.into());
        let mut gone = context("zz", vec![with_dot("qq", Dot::Working)]);
        gone.archived_at = Some(WHEN.into());
        let t = tree(vec![context("cc", vec![p]), gone]);
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &t, &["cc", "…"]);
        expand(&mut state, &t, &["…"]);
        for frame in 0..SPINNER.len() {
            let buf = render_at(&t, &mut state, 24, 8, false, frame);
            let grey = (String::from(format!("{WORKING}")), color(Meaning::Nothing));
            assert_eq!(cell(&buf, 4, 2), grey, "frame {frame}: an archived Plan row, always the first frame {:?}", lines(&buf));
            assert_eq!(cell(&buf, 2, 4), grey, "frame {frame}: an archived Context row {:?}", lines(&buf));
            assert_eq!(cell(&buf, 4, 5), grey, "frame {frame}: its Plan row");
            assert_eq!(buf[(2, 0)].symbol(), SPINNER[frame], "while the live Context row turns");
        }
    }

    #[test]
    fn an_archived_row_keeps_its_glyph_in_grey() {
        let mut p = with_dot("pp", Dot::Red);
        p.head.archived_at = Some(WHEN.into());
        let t = tree(vec![context("cc", vec![p])]);
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &t, &["cc", "…"]);
        let buf = render(&t, &mut state, 24, 4, false);
        assert_eq!(cell(&buf, 4, 2), (format!("{NEEDS_YOU}").to_owned(), color(Meaning::Nothing)), "{:?}", lines(&buf));
    }

    #[test]
    fn a_context_row_shows_its_most_urgent_live_session_red_over_yellow() {
        let t = tree(vec![context("cc", vec![with_dot("aa", Dot::Yellow), with_dot("bb", Dot::Red), with_dot("dd", Dot::White)])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 4, false);
        assert_eq!(cell(&buf, 2, 0), drawn_dot(Dot::Red));
        let collapsed = render(&t, &mut state_for(&t, &["cc"], &[]), 20, 1, false);
        assert_eq!(cell(&collapsed, 2, 0), drawn_dot(Dot::Red), "collapsed too");
        assert_eq!((cell(&buf, 4, 1), cell(&buf, 4, 3)), (drawn_dot(Dot::Yellow), drawn_dot(Dot::White)), "while each Session row keeps its own");
        assert_ne!(drawn_dot(Dot::Red), drawn_dot(Dot::Yellow));
    }

    #[test]
    fn every_status_has_its_counter_colour_and_zeros_and_separators_are_grey() {
        let t = tree(vec![context("cc", vec![lone("aa", "x", Phase::Dispatch, [9, 1, 2, 3, 4, 5]), lone("bb", "y", Phase::Dispatch, [0; 6])])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 3, false);
        // counters "1⋅2⋅3⋅4⋅5" occupy the last 9 columns
        let at = |i: u16| 11 + 2 * i;
        let colours: Vec<Color> = (0..5).map(|i| fg(&buf, at(i), 1)).collect();
        assert_eq!(
            colours,
            [color(Meaning::NotStarted), color(Meaning::InProgress), color(Meaning::Review), color(Meaning::Failed), color(Meaning::Done)]
        );
        assert!((0..4).all(|i| fg(&buf, at(i) + 1, 1) == color(Meaning::Nothing)));
        assert!((0..5).all(|i| fg(&buf, at(i), 2) == color(Meaning::Nothing)));
    }

    #[test]
    fn the_guides_are_dark_grey() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 8, false);
        for (x, y, glyph) in [(2, 1, '├'), (2, 2, '├'), (2, 3, '├'), (2, 4, '├'), (2, 5, '└')] {
            assert_eq!((text(&buf, y).chars().nth(x), fg(&buf, x as u16, y)), (Some(glyph), color(Meaning::Nothing)), "({x}, {y})");
        }
    }

    #[test]
    fn a_fork_is_a_sibling_of_the_plans_and_the_last_row_closes_the_guide() {
        let t = tree(vec![context(
            "cc",
            vec![plan("pp", "cc", Phase::Plan, [0; 6], session("l", "lead", State::Idle, None, Place::None), vec![session("f", "fork", State::Idle, None, Place::None)])],
        )]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 3, false);
        assert_eq!(lines(&buf)[1..], [row_of(&format!("  ├ {IDLE} pp"), "0⋅0⋅0⋅0⋅0", 30), format!("  └ {IDLE} pp·f").to_owned()]);
    }

    #[test]
    fn the_selected_session_row_is_reversed_across_the_full_width_and_no_other_row() {
        let tree = sample();
        for (selected, y) in [(&["alpha", "ship"][..], 1), (&["alpha", "f-idle"], 4)] {
            let buf = render(&tree, &mut state_for(&tree, &[], selected), 44, 9, true);
            for row in 0..9 {
                assert_eq!(reversed(&buf, row), vec![row == y; 44], "{selected:?}, row {row}");
            }
        }
        // every element keeps its colour: the zero counter and the separator are still dark grey
        let buf = render(&tree, &mut state_for(&tree, &[], &["alpha", "ship"]), 44, 9, true);
        assert_eq!(fg(&buf, 41, 1), color(Meaning::Nothing), "the zero");
        assert_eq!(fg(&buf, 38, 1), color(Meaning::Nothing), "a separator");
        assert_eq!(fg(&buf, 37, 1), color(Meaning::InProgress), "the running Jobs");
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

    #[test]
    fn the_active_session_has_the_mark_in_column_0_and_is_bold_and_no_other_row() {
        let mut tree = sample();
        // the fork is in the chat slot, not the Lead
        tree.contexts[0].plans[0].sessions[0].place = Place::Parked;
        tree.contexts[0].plans[0].sessions[1].place = Place::Chat;
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 8, false);
        assert_eq!(text(&buf, 4), format!("⏵ ├ {IDLE} ship·f-id"));
        for y in [0, 1, 2, 3, 5, 6, 7] {
            assert!(!text(&buf, y).contains(ACTIVE_MARK), "row {y}");
        }
        assert!(buf[(6, 4)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(8, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_keyboard_cursor_skips_context_rows() {
        let tree = sample();
        let mut state = state_for(&tree, &[], &[]);
        let at = |state: &mut TreeState<Id>, d| {
            move_cursor(state, &tree, d);
            state.selected().to_vec()
        };
        assert_eq!(at(&mut state, 1), path(&["alpha", "ship"]), "down from nothing: the first Session");
        assert_eq!(at(&mut state, 1), path(&["alpha", "sketch"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "old"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "f-idle"]), "the forks come after the Plans");
        assert_eq!(at(&mut state, 1), path(&["alpha", "f-blocked"]));
        assert_eq!(at(&mut state, 1), path(&["beta", "rel"]), "over the Context row `beta`");
        assert_eq!(at(&mut state, 1), path(&["beta", "rel"]), "clamped at the end: `gamma` has no Session");
        assert_eq!(at(&mut state, -1), path(&["alpha", "f-blocked"]), "back over the Context row");
        let mut none = state_for(&tree, &[], &[]);
        move_cursor(&mut none, &tree, -1);
        assert_eq!(none.selected(), path(&["beta", "rel"]), "up from nothing: the last Session");
    }

    #[test]
    fn the_cursor_skips_the_sessions_of_a_collapsed_context() {
        let tree = sample();
        let mut state = state_for(&tree, &["alpha"], &[]);
        move_cursor(&mut state, &tree, 1);
        assert_eq!(state.selected(), path(&["beta", "rel"]));
    }

    #[test]
    fn a_click_on_a_context_toggles_it_and_selects_nothing() {
        let tree = sample();
        let mut state = state_for(&tree, &[], &["alpha", "ship"]);
        assert!(click_context(&mut state, &tree, &path(&["alpha"])));
        assert!(!state.opened().contains(&path(&["alpha"])));
        assert_eq!(state.selected(), path(&["alpha", "ship"]), "the cursor stays where it was");
        assert!(click_context(&mut state, &tree, &path(&["alpha"])));
        assert!(state.opened().contains(&path(&["alpha"])));
        let mut fresh = state_for(&tree, &[], &[]);
        click_context(&mut fresh, &tree, &path(&["beta"]));
        assert!(fresh.selected().is_empty());
        assert!(!click_context(&mut fresh, &tree, &path(&["alpha", "ship"])), "a Session is not a Context");
        assert!(!click_context(&mut fresh, &tree, &path(&["alpha", "f-idle"])), "nor is a fork");
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
        assert_eq!(state.opened().len(), 3, "the three Contexts");
        state.close(&path(&["alpha"]));
        open_new(&mut state, &mut known, &tree);
        assert_eq!(state.opened().len(), 2);
    }

    #[test]
    fn the_cursor_moves_to_the_session_now_at_its_place_when_its_session_goes() {
        let mut tree = sample();
        let mut state = state_for(&tree, &[], &["alpha", "sketch"]);
        let before = selected_index(&state, &tree);
        assert_eq!(before, Some(1));
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
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 6, false);
        assert_eq!(text(&buf, 0), format!("▾ {NEEDS_YOU} alpha"), "not its title `Payments`");
        assert_eq!(text(&buf, 1), row_of(&format!("⏵ ├ {WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44), "not its title `Pay by card`, nor its Lead's name");
        assert_eq!(text(&buf, 5), format!("  └ {NEEDS_YOU} ship·f-bl"), "the Plan id, `·`, the first 4 characters of the Session id; not its name");
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
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 3, false), 2), format!("  └ {IDLE} pp·0123"));
        t.contexts[0].plans[0].sessions[1].plan = None;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 3, false), 2), format!("  └ {IDLE} 01234567"));
        assert_eq!(row(&t, &path(&["cc", "0123456789abcdef"])).unwrap().shown_id().as_deref(), Some("01234567"));
    }

    #[test]
    fn shown_id_is_the_id_each_row_shows_and_a_more_row_has_none() {
        let t = archived();
        let shown = |parts: &[&str]| row(&t, &path(parts)).unwrap().shown_id();
        assert_eq!(shown(&["alpha"]).as_deref(), Some("alpha"), "a Context: its id");
        assert_eq!(shown(&["alpha", "ship"]).as_deref(), Some("ship"), "a Plan: its id");
        assert_eq!(shown(&["alpha", "f-idle"]).as_deref(), Some("ship·f-id"), "a fork: its Plan, `·`, 4 characters of its id");
        assert_eq!(shown(&["alpha", "old"]).as_deref(), Some("old"), "an archived Plan");
        assert_eq!(shown(&["alpha", "f-blocked"]).as_deref(), Some("ship·f-bl"), "an archived fork");
        assert_eq!(shown(&["zeta"]).as_deref(), Some("zeta"), "an archived Context");
        assert_eq!(shown(&["zeta", "f-zp"]).as_deref(), Some("zp·f-zp"), "a fork of an archived Context");
        assert_eq!(shown(&["alpha", "…"]), None);
        assert_eq!(shown(&["…"]), None);
    }

    // ---------- archived rows (hs-d53) ----------

    /// `state` with the group whose `…` row is at `head` expanded.
    fn expand(state: &mut TreeState<Id>, t: &Tree, head: &[&str]) {
        assert!(click_more(state, t, &path(head)));
    }

    #[test]
    fn archived_contexts_collapse_into_one_more_row_at_the_end_of_the_list() {
        let t = archived();
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 10, false);
        assert_eq!(
            lines(&buf),
            [
                format!("▾ {WORKING} alpha").to_owned(),
                row_of(&format!("⏵ ├ {WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44),
                row_of(&format!("  ├ {IDLE} sketch"), "4⋅0⋅0⋅0⋅0", 44),
                format!("  ├ {IDLE} ship·f-id").to_owned(),
                "  └ …".to_owned(),
                format!("▾ {NEEDS_YOU} beta").to_owned(),
                row_of(&format!("  └ {NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44),
                format!("▾ {STOPPED} gamma").to_owned(),
                "…".to_owned(),
                String::new(),
            ],
            "zeta is only in the `…` row at the end; `alpha`'s archived Plan and fork are in its own `…` row, and its blocked fork no longer makes it a needs-you dot: its dot is the working Lead's"
        );
        assert_eq!((fg(&buf, 0, 8), fg(&buf, 4, 4)), (color(Meaning::Nothing), color(Meaning::Nothing)), "muted");
    }

    #[test]
    fn a_context_with_nothing_archived_has_no_more_row_and_an_archived_fork_of_a_live_plan_makes_one() {
        let mut t = sample();
        assert!(!lines(&render(&t, &mut state_for(&t, &[], &[]), 44, 10, false)).iter().any(|l| l.contains('…')));
        let mut fork = session("f-rel", "rel fork", State::Idle, None, Place::None);
        fork.plan = Some(t.contexts[1].plans[0].head.id.clone());
        fork.archived_at = Some(WHEN.into());
        t.contexts[1].plans[0].sessions.push(fork);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 10, false);
        assert_eq!(lines(&buf)[6..9], [format!("▾ {NEEDS_YOU} beta"), row_of(&format!("  ├ {NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44), "  └ …".to_owned()]);
        let mut t = sample();
        t.contexts[1].plans[0].head.archived_at = Some(WHEN.into());
        let mut fork = session("f-rel", "rel fork", State::Idle, None, Place::None);
        fork.plan = Some(t.contexts[1].plans[0].head.id.clone());
        t.contexts[1].plans[0].sessions.push(fork);
        for s in &mut t.contexts[1].plans[0].sessions {
            s.archived_at = Some(WHEN.into()); // archiving a Plan archives its Sessions
        }
        refresh_dots(&mut t);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 10, false);
        assert_eq!(lines(&buf)[6..8], [format!("▾ {STOPPED} beta"), "  └ …".to_owned()], "a fork of an archived Plan is in the `…` row too");
    }

    #[test]
    fn a_click_on_a_more_row_shows_the_archived_rows_at_their_live_depth_with_normal_dark_grey_guides_and_a_click_on_it_again_collapses_them() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &t, &["alpha", "…"]);
        let buf = render(&t, &mut state, 44, 12, false);
        assert_eq!(
            lines(&buf)[..9],
            [
                format!("▾ {WORKING} alpha").to_owned(),
                row_of(&format!("⏵ ├ {WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44),
                row_of(&format!("  ├ {IDLE} sketch"), "4⋅0⋅0⋅0⋅0", 44),
                format!("  ├ {IDLE} ship·f-id").to_owned(),
                "  ├ …".to_owned(),
                row_of(&format!("  ├ {STOPPED} old"), "0⋅0⋅0⋅1⋅36", 44),
                format!("  └ {NEEDS_YOU} ship·f-bl").to_owned(),
                format!("▾ {NEEDS_YOU} beta").to_owned(),
                row_of(&format!("  └ {NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44),
            ],
            "no empty row; the archived Plan and fork are as deep as the live ones"
        );
        // the live rows keep their colours; the archived ones are all dark grey, guides, counters and dots included
        assert_eq!(cell(&buf, 4, 2), drawn_dot(Dot::Idle), "a live dot is not greyed");
        for (y, columns) in [(4, 2..5), (5, 2..44), (6, 2..24)] {
            for x in columns {
                if buf[(x, y)].symbol() != " " {
                    assert_eq!(fg(&buf, x, y), color(Meaning::Nothing), "({x}, {y}) {}", buf[(x, y)].symbol());
                }
            }
        }
        assert_eq!((buf[(2, 5)].symbol(), buf[(2, 6)].symbol()), ("├", "└"), "the normal guides");
        // a click on the `…` row collapses; a click on a guide or an archived row does not
        assert!(!click_more(&mut state, &t, &path(&["alpha", "old"])), "an archived row is no `…` row");
        assert!(!click_more(&mut state, &t, &path(&["alpha", "ship"])), "nor is a live one");
        assert!(!click_more(&mut state, &t, &path(&["alpha"])), "nor a Context");
        assert!(state.opened().contains(&path(&["alpha", "…"])));
        assert!(click_more(&mut state, &t, &path(&["alpha", "…"])));
        assert!(!state.opened().contains(&path(&["alpha", "…"])));
        assert_eq!(text(&render(&t, &mut state, 44, 10, false), 4), "  └ …");
        expand(&mut state, &t, &["alpha", "…"]);
        assert_eq!(text(&render(&t, &mut state, 44, 10, false), 4), "  ├ …");
    }

    #[test]
    fn expanded_archived_contexts_are_drawn_like_live_ones_in_dark_grey_and_the_more_row_collapses_them() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &t, &["…"]);
        let buf = render(&t, &mut state, 44, 14, false);
        let rows = lines(&buf);
        assert_eq!(
            rows[8..12],
            [
                "…".to_owned(),
                format!("▾ {STOPPED} zeta"),
                row_of(&format!("  ├ {STOPPED} zp"), "0⋅0⋅0⋅0⋅2", 44),
                format!("  └ {STOPPED} zp·f-zp"),
            ],
            "{rows:#?}"
        );
        for y in 9..12 {
            for x in 0..44 {
                if buf[(x, y)].symbol() != " " {
                    assert_eq!(fg(&buf, x, y), color(Meaning::Nothing), "({x}, {y})");
                }
            }
        }
        assert!(click_more(&mut state, &t, &path(&["…"])));
        assert_eq!(lines(&render(&t, &mut state, 44, 14, false))[8..10], ["…", ""]);
        // an archived Context folds like any other
        expand(&mut state, &t, &["…"]);
        assert!(click_context(&mut state, &t, &path(&["zeta"])));
        assert_eq!(text(&render(&t, &mut state, 44, 14, false), 9), format!("▸ {STOPPED} zeta 1"));
    }

    #[test]
    fn the_cursor_never_stops_on_an_archived_or_more_row() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &t, &["alpha", "…"]);
        expand(&mut state, &t, &["…"]);
        let mut seen = vec![];
        for _ in 0..8 {
            move_cursor(&mut state, &t, 1);
            seen.push(state.selected().join("/"));
        }
        assert_eq!(seen, ["alpha/ship", "alpha/sketch", "alpha/f-idle", "beta/rel", "beta/rel", "beta/rel", "beta/rel", "beta/rel"]);
        for _ in 0..8 {
            move_cursor(&mut state, &t, -1);
        }
        assert_eq!(state.selected(), path(&["alpha", "ship"]));
        let mut at_end = state_for(&t, &[], &["gamma"]);
        move_cursor(&mut at_end, &t, -1);
        assert_eq!(at_end.selected(), path(&["beta", "rel"]), "up from nothing: the last live Session");
        // a cursor on a Session that gets archived moves to a live one
        let mut state = state_for(&sample(), &[], &["alpha", "sketch"]);
        let before = selected_index(&state, &sample());
        let mut t = sample();
        t.contexts[0].plans[1].head.archived_at = Some(WHEN.into());
        reselect(&mut state, &t, before);
        assert_eq!(state.selected(), path(&["alpha", "old"]));
    }

    #[test]
    fn archived_rows_resolve_from_their_paths_and_are_archived() {
        let t = archived();
        let archived_of = |parts: &[&str]| row(&t, &path(parts)).map(|r| r.archived());
        assert_eq!(archived_of(&["alpha"]), Some(false));
        assert_eq!(archived_of(&["alpha", "f-idle"]), Some(false));
        assert_eq!(archived_of(&["alpha", "…"]), Some(false), "a `…` row is no object");
        assert!(matches!(row(&t, &path(&["…"])), Some(Row::More)));
        assert_eq!(archived_of(&["alpha", "old"]), Some(true));
        assert_eq!(archived_of(&["alpha", "f-blocked"]), Some(true));
        assert_eq!(archived_of(&["zeta"]), Some(true));
        assert_eq!(archived_of(&["zeta", "f-zp"]), Some(true), "a fork of an archived Context is its sibling too");
        let fork = row(&t, &path(&["alpha", "f-blocked"])).unwrap();
        assert_eq!((fork.session().unwrap().id.as_str(), fork.plan().unwrap().head.id.as_str()), ("f-blocked", "ship"));
        assert_eq!(row(&t, &path(&["zeta"])).unwrap().label(), "Zeta");
        for missing in [
            &["…", "zeta"][..],
            &["…", "alpha"],
            &["alpha", "ship", "f-blocked"],
            &["alpha", "…", "ship"],
            &["alpha", "…", "old"],
            &["zeta", "zp", "f-zp"],
            &["beta", "…"],
            &["zeta", "…"],
            &["…", "…"],
            &["nope"],
            &[],
        ] {
            assert!(row(&t, &path(missing)).is_none(), "{missing:?}");
        }
    }

    #[test]
    fn only_the_live_plans_are_counted_in_a_collapsed_context_and_archived_ones_do_not_colour_its_dot() {
        let mut t = archived();
        t.contexts[0].plans[2].head.phase = Phase::Done;
        let buf = render(&t, &mut state_for(&t, &["alpha"], &[]), 44, 3, false);
        assert_eq!(text(&buf, 0), format!("▸ {WORKING} alpha 2"), "ship and sketch; the archived Plan is behind `…`, and the archived blocked fork does not make it a needs-you dot");
    }
}
