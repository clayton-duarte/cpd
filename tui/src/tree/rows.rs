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
//! **Archived rows** (hs-d53). Archived Contexts collapse into one `…` row at the end of the Context list;
//! the archived Plans, and the forks that are archived or of an archived Plan, of a live Context into one `…`
//! row at the end of that Context's subtree. Expanded, the `…` row becomes a dotted line (`┊`) and its children hang under it,
//! one level deeper, drawn muted with dotted guides (`┆`) and the dotted line in the column of the head. Whether
//! a group is expanded is the tree state's open set, so view state only. The cursor never rests on them.
//!
//! Rows (identifiers are paths; `…` is [`MORE`]):
//!
//! | Path | Row |
//! |---|---|
//! | `[context]`, `[context, plan]`, `[context, session]` | a live Context, a Plan (its row is its Lead's), a forked Session |
//! | `[…]` | the `…` row of the archived Contexts |
//! | `[…, context]`, `[…, context, plan]`, `[…, context, session]` | the same, in an archived Context |
//! | `[context, …]` | the `…` row of the Context's archived Plans and forks |
//! | `[context, …, plan]` | an archived Plan |
//! | `[context, …, fork:<session>]` | an archived fork, or a fork of an archived Plan |
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
use crate::read::{ContextNode, JobStatus, PlanNode, SessionNode, Tree};

/// Tree identifiers: a Context's id, a Plan's id, a Session's id, or [`MORE`].
pub type Id = String;

/// One tree level is indented by two columns (`tui-tree-widget`'s own indentation).
const INDENT: usize = 2;

/// The mark of the Session in the chat slot, in column 0 of its row.
pub const ACTIVE_MARK: &str = "⏵";

/// The identifier of a `…` row. No id has this shape.
pub const MORE: &str = "…";

/// The prefix of the identifier of a fork in a Context's `…` row, beside the archived Plans (no Plan id has a
/// `:`).
const STRAY: &str = "fork:";

/// The dotted line of an expanded group.
const DOTTED: &str = "┊";

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
    let (archived, path) = match path.split_first() {
        Some((first, rest)) if first == MORE => (true, rest),
        _ => (false, path),
    };
    let Some((context, path)) = path.split_first() else { return archived.then_some(Row::More) };
    let context = tree.contexts.iter().find(|c| c.id.as_str() == context && c.is_archived() == archived)?;
    let Some((next, path)) = path.split_first() else { return Some(Row::Context(context)) };
    let k = kids(context);
    if next == MORE {
        if archived || !k.has_group() {
            return None;
        }
        let plan = |id: &String| k.archived.iter().copied().find(|p| p.head.id.as_str() == id);
        return match path {
            [] => Some(Row::More),
            [x] => match x.strip_prefix(STRAY) {
                Some(session) => k.strays.iter().find(|(_, s)| s.id.as_str() == session).map(|(p, s)| Row::Fork(context, p, s)),
                None => plan(x).map(|p| Row::Plan(context, p)),
            },
            _ => None,
        };
    }
    if !path.is_empty() {
        return None;
    }
    if let Some(plan) = k.direct.iter().copied().find(|p| p.head.id.as_str() == next) {
        return Some(Row::Plan(context, plan));
    }
    k.forks.iter().find(|(_, s)| s.id.as_str() == next).map(|(p, s)| Row::Fork(context, p, s))
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

    /// The label the row shows, as the prompts word it: a Context's title, a Plan's title, a Session's name.
    pub fn label(&self) -> String {
        match *self {
            Row::Context(c) => c.title.clone(),
            Row::Plan(_, p) => p.head.title.clone(),
            Row::Fork(_, _, s) => session_label(s),
            Row::More => MORE.to_owned(),
        }
    }
}

/// A Session's pi name, else the first 8 characters of its id.
pub fn session_label(s: &SessionNode) -> String {
    let name = s.name.trim();
    if name.is_empty() { s.id.as_str().chars().take(8).collect() } else { name.to_owned() }
}

/// The busiest phase among Plans: dispatch > plan > grounding > deploy > done > none.
pub fn busiest<'a>(plans: impl IntoIterator<Item = &'a PlanNode>) -> Option<Phase> {
    let plans: Vec<&PlanNode> = plans.into_iter().collect();
    [Phase::Dispatch, Phase::Plan, Phase::Grounding, Phase::Deploy, Phase::Done]
        .into_iter()
        .find(|p| plans.iter().any(|n| n.head.phase == *p))
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
    phase: Phase,
    /// The width of the counters drawn at the right (0: none).
    counters_width: usize,
    /// Archived: the guide is dotted (the colours are muted when drawn).
    muted: bool,
}

/// A Plan's or fork's row text (the reference's `session_text`): guide (`├ `, `└ ` for the last; `┆ ` when
/// muted), dot, name. The name is cut so that, in a column `width` wide, it never reaches the counters drawn at
/// the right.
fn row_text(r: RowText, width: u16) -> Line<'static> {
    let needs_you = !r.muted && r.session.is_some_and(|s| s.needs_you);
    let guide = match (r.muted, r.last) {
        (true, _) => "┆ ",
        (false, true) => "└ ",
        (false, false) => "├ ",
    };
    let mut spans = vec![Span::styled(guide, palette::fg(Meaning::Nothing)), palette::dot(needs_you, Some(r.phase)), " ".into()];
    let used = r.depth * INDENT + Line::from(spans.clone()).width() + if r.counters_width > 0 { r.counters_width + 1 } else { 0 };
    let name = truncate(&r.name, (width as usize).saturating_sub(used));
    let active = r.session.is_some_and(SessionNode::is_active) && !r.muted;
    spans.push(if active { Span::styled(name, Style::new().add_modifier(Modifier::BOLD)) } else { name.into() });
    Line::from(spans)
}

/// A Context's row text: dot (● when a live Session inside needs you, coloured by the busiest phase), title, and
/// the number of Plans listed inside when collapsed.
fn context_text(c: &ContextNode, collapsed: bool) -> Line<'static> {
    let k = kids(c);
    let needs_you = k.direct.iter().flat_map(|p| &p.sessions).any(|s| s.needs_you && !s.is_archived());
    let mut spans = vec![
        palette::dot(needs_you, busiest(k.direct.iter().copied())),
        " ".into(),
        Span::styled(c.title.clone(), Style::new().add_modifier(Modifier::BOLD)),
    ];
    if collapsed {
        spans.push(Span::styled(format!(" {}", k.direct.len()), palette::fg(Meaning::Nothing)));
    }
    Line::from(spans)
}

/// The text of a `…` row at `depth` levels: `…` (`└ …` below a Context), or the dotted line when expanded.
fn more_text(depth: usize, expanded: bool) -> Line<'static> {
    let text = match (expanded, depth) {
        (true, _) => DOTTED,
        (false, 0) => MORE,
        (false, _) => "└ …",
    };
    Line::from(Span::styled(text, palette::fg(Meaning::Nothing)))
}

/// A Plan's item at `depth` levels; `last` when it is the last child of its parent.
fn plan_item(c: &ContextNode, p: &PlanNode, depth: usize, last: bool, width: u16) -> TreeItem<'static, Id> {
    let muted = c.is_archived() || p.head.is_archived();
    let text = row_text(
        RowText { session: p.lead(), name: p.head.title.clone(), depth, last, phase: p.head.phase, counters_width: counters(p).width(), muted },
        width,
    );
    TreeItem::new_leaf(p.head.id.to_string(), text)
}

/// A Context's item at `depth` levels (0, or 1 under the `…` row of the archived Contexts); `opened` is the
/// tree state's open set.
fn context_item(c: &ContextNode, depth: usize, opened: &HashSet<Vec<Id>>, width: u16) -> TreeItem<'static, Id> {
    let mut path = if depth == 0 { Vec::new() } else { vec![MORE.to_owned()] };
    path.push(c.id.to_string());
    let k = kids(c);
    let n = k.direct.len();
    let mut children: Vec<TreeItem<'static, Id>> = k
        .direct
        .iter()
        .enumerate()
        .map(|(i, p)| plan_item(c, p, depth + 1, i + 1 == n && k.forks.is_empty() && !k.has_group(), width))
        .collect();
    for (j, (p, s)) in k.forks.iter().enumerate() {
        let text = row_text(
            RowText {
                session: Some(s),
                name: session_label(s),
                depth: depth + 1,
                last: j + 1 == k.forks.len() && !k.has_group(),
                phase: p.head.phase,
                counters_width: 0,
                muted: c.is_archived(),
            },
            width,
        );
        children.push(TreeItem::new_leaf(s.id.to_string(), text));
    }
    if k.has_group() {
        let n = k.archived.len();
        let mut group: Vec<TreeItem<'static, Id>> =
            k.archived.iter().enumerate().map(|(i, p)| plan_item(c, p, depth + 2, i + 1 == n && k.strays.is_empty(), width)).collect();
        for (i, (p, s)) in k.strays.iter().enumerate() {
            let text = row_text(
                RowText {
                    session: Some(s),
                    name: format!("{} ({})", session_label(s), p.head.title),
                    depth: depth + 2,
                    last: i + 1 == k.strays.len(),
                    phase: p.head.phase,
                    counters_width: 0,
                    muted: true,
                },
                width,
            );
            group.push(TreeItem::new_leaf(format!("{STRAY}{}", s.id), text));
        }
        let expanded = opened.contains(&[path.clone(), vec![MORE.to_owned()]].concat());
        children.push(TreeItem::new(MORE.to_owned(), more_text(depth + 1, expanded), group).expect("Plan and Session ids are unique"));
    }
    TreeItem::new(c.id.to_string(), context_text(c, !opened.contains(&path)), children).expect("Plan and Session ids are unique")
}

/// The tree's items for a column `width` wide. `opened` is the tree state's open set: a Context not in it is
/// collapsed, and so is a `…` row.
pub fn items(tree: &Tree, opened: &HashSet<Vec<Id>>, width: u16) -> Vec<TreeItem<'static, Id>> {
    let (archived, live): (Vec<&ContextNode>, Vec<&ContextNode>) = tree.contexts.iter().partition(|c| c.is_archived());
    let mut items: Vec<TreeItem<'static, Id>> = live.into_iter().map(|c| context_item(c, 0, opened, width)).collect();
    if !archived.is_empty() {
        let expanded = opened.contains(&vec![MORE.to_owned()]);
        let group = archived.into_iter().map(|c| context_item(c, 1, opened, width)).collect();
        items.push(TreeItem::new(MORE.to_owned(), more_text(0, expanded), group).expect("Context ids are unique"));
    }
    items
}

/// The column of the dotted line of the group a row at `path` is in, with the length of the path of the group's `…`
/// row; none for a row outside every group.
fn group_of(path: &[Id]) -> Option<(usize, usize)> {
    match path {
        [m, ..] if m == MORE => Some((0, 1)),
        [_, m, ..] if m == MORE => Some((INDENT, 2)),
        _ => None,
    }
}

/// Draw the column into `area`: the tree, then the counters, the guides and the active mark of each visible
/// row, the archived rows muted with the dotted line of their group, then the cursor's Session row inverted when
/// `cursor` says it is shown (a Context row never is).
pub fn draw(f: &mut Frame, area: Rect, tree: &Tree, state: &mut TreeState<Id>, cursor: bool) {
    if tree.contexts.is_empty() {
        f.render_widget(Paragraph::new(Line::from(Span::styled(" no Contexts", Style::new().add_modifier(Modifier::DIM)))), area);
        return;
    }
    let items = items(tree, state.opened(), area.width);
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
        if let Some((column, head)) = group_of(path) {
            if path.len() > head {
                Span::styled(DOTTED, palette::fg(Meaning::Nothing)).render(Rect::new(area.x + column as u16, y, 1, 1), buf);
            }
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
        let context = if c.is_archived() { vec![MORE.to_owned(), c.id.to_string()] } else { vec![c.id.to_string()] };
        if known.insert(context.clone()) {
            state.open(context);
        }
    }
}

/// The visible Session rows (the only selectable ones: the live Plans' Leads and the live forks), in order.
fn visible(state: &TreeState<Id>, tree: &Tree) -> Vec<Vec<Id>> {
    state
        .flatten(&items(tree, state.opened(), 0))
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

/// A click at `column` of the row at `path` on an archive group: on its `…` row it expands, on the expanded
/// group's dotted line (its head row, or the column of the line in the rows below) it collapses. Returns whether
/// the click was on a group.
pub fn click_group(state: &mut TreeState<Id>, path: &[Id], column: u16) -> bool {
    let Some((guide, head)) = group_of(path) else { return false };
    if path.len() == head || usize::from(column) == guide {
        state.toggle(path[..head].to_vec());
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    use super::super::fixture::{WHEN, archived, context, plan, sample, session, tree};
    use super::*;
    use cpd_core::session::State;
    use crate::palette::color;
    use crate::read::Place;

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
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, f.area(), tree, state, cursor)).unwrap();
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
        rust.needs_you = true;
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
            ["● Cpd", "  ├ ○ V1             36⋅0⋅0⋅1⋅5", "⏵ ├ ● Rust            0⋅1⋅0⋅0⋅1", "  └ ○ Ui              1⋅0⋅2⋅0⋅3"]
        );
        assert!(buf[(6, 2)].modifier.contains(Modifier::BOLD), "the name of the centre row is bold");
        assert!(buf[(0, 2)].modifier.contains(Modifier::BOLD), "so is its ⏵");
        assert!(buf[(2, 0)].modifier.contains(Modifier::BOLD), "and a Context's name");
        assert!(!buf[(6, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_tree_shows_contexts_plans_and_then_the_forks_as_their_siblings() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 10, false);
        assert_eq!(
            lines(&buf),
            [
                "● Alpha".to_owned(),
                row_of("⏵ ├ ○ Ship", "2⋅1⋅1⋅0⋅5", 44),
                row_of("  ├ ○ Sketch", "4⋅0⋅0⋅0⋅0", 44),
                row_of("  ├ ○ Old", "0⋅0⋅0⋅1⋅36", 44),
                "  ├ ○ ship fork".to_owned(),
                "  └ ● ship review".to_owned(),
                "● Beta".to_owned(),
                row_of("  └ ● Rel", "0⋅0⋅0⋅0⋅3", 44),
                "○ Gamma".to_owned(),
                String::new(),
            ],
            "no state text after a name"
        );
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
        assert_eq!(lines(&buf), ["● Alpha 3".to_owned(), "● Beta".to_owned(), row_of("  └ ● Rel", "0⋅0⋅0⋅0⋅3", 44), "○ Gamma".to_owned()]);
    }

    #[test]
    fn a_context_has_the_dot_of_any_session_inside_that_needs_you() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 1, false);
        assert_eq!(text(&buf, 0), "● Alpha 3", "through its blocked fork");
        let calm = super::super::fixture::tree(vec![context("cc", vec![lone("pp", "s", Phase::Plan, [0; 6])])]);
        assert_eq!(text(&render(&calm, &mut state_for(&calm, &[], &[]), 20, 1, false), 0), "○ Cc");
    }

    #[test]
    fn a_rows_dot_is_its_sessions_and_not_its_plans() {
        // an open Escalation is not the ●: the Lead's state is (commands.md §17)
        let mut p = lone("pp", "lead", Phase::Dispatch, [0; 6]);
        p.head.open_escalations = 1;
        let t = tree(vec![context("cc", vec![p])]);
        assert_eq!(lines(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false)), ["○ Cc".to_owned(), row_of("  └ ○ Pp", "0⋅0⋅0⋅0⋅0", 30)]);
        let mut t = t;
        t.contexts[0].plans[0].sessions[0].needs_you = true;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false), 1), row_of("  └ ● Pp", "0⋅0⋅0⋅0⋅0", 30));
    }

    #[test]
    fn drafts_are_counted_and_dark_grey_in_phase_plan() {
        let t = tree(vec![context("cc", vec![lone("pp", "p", Phase::Plan, [4, 0, 0, 0, 0, 0])])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!(text(&buf, 1), row_of("  └ ○ Pp", "4⋅0⋅0⋅0⋅0", 20));
        assert_eq!(fg(&buf, 11, 1), color(Meaning::Nothing));
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_the_counters_stay_right_aligned() {
        let mut long = lone("pp", "lead", Phase::Dispatch, [0, 36, 0, 0, 1, 5]);
        long.head.title = "a very long plan title indeed".into();
        let t = tree(vec![context("cc", vec![long])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 2, false);
        assert_eq!(text(&buf, 1), row_of("  └ ○ a very long …", "36⋅0⋅0⋅1⋅5", 30));
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "ab…");
        assert_eq!(truncate("abcd", 0), "");
    }

    #[test]
    fn every_phase_has_its_dot_colour() {
        for (phase, meaning) in [
            (Phase::Grounding, Meaning::Grounding),
            (Phase::Plan, Meaning::Planning),
            (Phase::Dispatch, Meaning::InProgress),
            (Phase::Deploy, Meaning::Review),
            (Phase::Done, Meaning::Done),
        ] {
            let t = tree(vec![context("cc", vec![lone("pp", "x", phase, [0; 6])])]);
            let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
            assert_eq!((fg(&buf, 0, 0), fg(&buf, 4, 1)), (color(meaning), color(meaning)), "{phase:?}");
        }
    }

    #[test]
    fn the_context_dot_is_the_busiest_phase_inside() {
        let plans = |phases: &[Phase]| -> Vec<PlanNode> {
            phases.iter().enumerate().map(|(i, ph)| lone(&format!("pp{i}"), "s", *ph, [0; 6])).collect()
        };
        use Phase::*;
        assert_eq!(busiest(&plans(&[Done, Deploy, Grounding, Plan, Dispatch])), Some(Dispatch));
        assert_eq!(busiest(&plans(&[Done, Deploy, Grounding, Plan])), Some(Plan));
        assert_eq!(busiest(&plans(&[Done, Deploy, Grounding])), Some(Grounding));
        assert_eq!(busiest(&plans(&[Done, Deploy])), Some(Deploy));
        assert_eq!(busiest(&plans(&[Done])), Some(Done));
        assert_eq!(busiest(&[]), None);
        let t = tree(vec![context("cc", plans(&[Done, Plan]))]);
        assert_eq!(fg(&render(&t, &mut state_for(&t, &[], &[]), 20, 1, false), 0, 0), color(Meaning::Planning));
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
        assert_eq!(lines(&buf)[1..], [row_of("  ├ ○ Pp", "0⋅0⋅0⋅0⋅0", 30), "  └ ○ fork".to_owned()]);
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
        assert_eq!(text(&buf, 4), "⏵ ├ ○ ship fork");
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
    fn a_context_row_shows_its_title_a_plan_row_its_plans_title_and_a_fork_its_name() {
        let mut tree = sample();
        tree.contexts[0].title = "Payments".into();
        tree.contexts[0].plans[0].head.title = "Pay by card".into();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 5, false);
        assert_eq!(text(&buf, 0), "● Payments", "not its id `alpha`");
        assert_eq!(text(&buf, 1), row_of("⏵ ├ ○ Pay by card", "2⋅1⋅1⋅0⋅5", 44), "not its Lead's name `ship lead`");
        assert_eq!(text(&buf, 4), "  ├ ○ ship fork", "a fork is its Session's name");
        assert_eq!(row(&tree, &path(&["alpha"])).unwrap().label(), "Payments");
        assert_eq!(row(&tree, &path(&["alpha", "ship"])).unwrap().label(), "Pay by card");
        assert_eq!(row(&tree, &path(&["alpha", "f-idle"])).unwrap().label(), "ship fork");
        let mut s = session("0123456789abcdef", "", State::Idle, None, Place::None);
        assert_eq!(session_label(&s), "01234567", "a blank name: the short id");
        s.name = " named ".into();
        assert_eq!(session_label(&s), "named");
    }

    // ---------- archived rows (hs-d53) ----------

    /// `state` with the group whose `…` row is at `head` expanded.
    fn expand(state: &mut TreeState<Id>, head: &[&str]) {
        assert!(click_group(state, &path(head), 0));
    }

    #[test]
    fn archived_contexts_collapse_into_one_more_row_at_the_end_of_the_list() {
        let t = archived();
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 10, false);
        assert_eq!(
            lines(&buf),
            [
                "○ Alpha".to_owned(),
                row_of("⏵ ├ ○ Ship", "2⋅1⋅1⋅0⋅5", 44),
                row_of("  ├ ○ Sketch", "4⋅0⋅0⋅0⋅0", 44),
                "  ├ ○ ship fork".to_owned(),
                "  └ …".to_owned(),
                "● Beta".to_owned(),
                row_of("  └ ● Rel", "0⋅0⋅0⋅0⋅3", 44),
                "○ Gamma".to_owned(),
                "…".to_owned(),
                String::new(),
            ],
            "zeta is only in the `…` row at the end; `alpha`'s archived Plan and fork are in its own `…` row, and its blocked fork no longer makes it ●"
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
        assert_eq!(lines(&buf)[6..9], ["● Beta", row_of("  ├ ● Rel", "0⋅0⋅0⋅0⋅3", 44).as_str(), "  └ …"]);
        let mut t = sample();
        t.contexts[1].plans[0].head.archived_at = Some(WHEN.into());
        let mut fork = session("f-rel", "rel fork", State::Idle, None, Place::None);
        fork.plan = Some(t.contexts[1].plans[0].head.id.clone());
        t.contexts[1].plans[0].sessions.push(fork);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 10, false);
        assert_eq!(lines(&buf)[6..8], ["○ Beta", "  └ …"], "a fork of an archived Plan is in the `…` row too");
    }

    #[test]
    fn a_click_on_a_more_row_shows_the_archived_rows_muted_with_dotted_guides_and_a_click_on_the_dotted_line_collapses_them() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &["alpha", "…"]);
        let buf = render(&t, &mut state, 44, 12, false);
        assert_eq!(
            lines(&buf)[..9],
            [
                "○ Alpha".to_owned(),
                row_of("⏵ ├ ○ Ship", "2⋅1⋅1⋅0⋅5", 44),
                row_of("  ├ ○ Sketch", "4⋅0⋅0⋅0⋅0", 44),
                "  ├ ○ ship fork".to_owned(),
                "  ┊".to_owned(),
                row_of("  ┊ ┆ ○ Old", "0⋅0⋅0⋅1⋅36", 44),
                "  ┊ ┆ ○ ship review (Ship)".to_owned(),
                "● Beta".to_owned(),
                row_of("  └ ● Rel", "0⋅0⋅0⋅0⋅3", 44),
            ]
        );
        // the live rows keep their colours; the archived ones are all dark grey, counters and dots included
        assert_eq!(fg(&buf, 4, 2), color(Meaning::Planning), "a live dot");
        for (y, columns) in [(4, 2..3), (5, 2..44), (6, 2..24)] {
            for x in columns {
                if buf[(x, y)].symbol() != " " {
                    assert_eq!(fg(&buf, x, y), color(Meaning::Nothing), "({x}, {y}) {}", buf[(x, y)].symbol());
                }
            }
        }
        // a click on the dotted column collapses; elsewhere on an archived row nothing happens
        assert!(!click_group(&mut state, &path(&["alpha", "…", "old"]), 7), "not on the dotted line");
        assert!(state.opened().contains(&path(&["alpha", "…"])));
        assert!(click_group(&mut state, &path(&["alpha", "…", "old"]), 2));
        assert!(!state.opened().contains(&path(&["alpha", "…"])));
        assert_eq!(text(&render(&t, &mut state, 44, 10, false), 4), "  └ …");
        expand(&mut state, &["alpha", "…"]);
        assert!(click_group(&mut state, &path(&["alpha", "…"]), 30), "the dotted line row collapses anywhere");
        assert!(!state.opened().contains(&path(&["alpha", "…"])));
        assert!(!click_group(&mut state, &path(&["alpha", "ship"]), 2), "a live row is in no group");
    }

    #[test]
    fn expanded_archived_contexts_are_muted_with_the_dotted_line_in_column_0() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &["…"]);
        let buf = render(&t, &mut state, 44, 14, false);
        let rows = lines(&buf);
        assert_eq!(
            rows[8..13],
            ["┊", "┊ ○ Zeta", row_of("┊   ┆ ○ Zp", "0⋅0⋅0⋅0⋅2", 44).as_str(), "┊   ┆ ○ zp fork", ""],
            "{rows:#?}"
        );
        for y in 8..12 {
            for x in 0..44 {
                if buf[(x, y)].symbol() != " " {
                    assert_eq!(fg(&buf, x, y), color(Meaning::Nothing), "({x}, {y})");
                }
            }
        }
        assert!(click_group(&mut state, &path(&["…", "zeta"]), 0));
        assert_eq!(text(&render(&t, &mut state, 44, 14, false), 8), "…");
        // an archived Context folds like any other
        expand(&mut state, &["…"]);
        assert!(click_context(&mut state, &t, &path(&["…", "zeta"])));
        assert_eq!(text(&render(&t, &mut state, 44, 14, false), 9), "┊ ○ Zeta 1");
    }

    #[test]
    fn the_cursor_never_stops_on_an_archived_or_more_row() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        expand(&mut state, &["alpha", "…"]);
        expand(&mut state, &["…"]);
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
        assert_eq!(archived_of(&["alpha", "…", "old"]), Some(true));
        assert_eq!(archived_of(&["alpha", "…", "fork:f-blocked"]), Some(true));
        assert_eq!(archived_of(&["…", "zeta"]), Some(true));
        assert_eq!(archived_of(&["…", "zeta", "f-zp"]), Some(true), "a fork of an archived Context is its sibling too");
        let fork = row(&t, &path(&["alpha", "…", "fork:f-blocked"])).unwrap();
        assert_eq!((fork.session().unwrap().id.as_str(), fork.plan().unwrap().head.id.as_str()), ("f-blocked", "ship"));
        assert_eq!(row(&t, &path(&["…", "zeta"])).unwrap().label(), "Zeta");
        for missing in [
            &["zeta"][..],
            &["…", "alpha"],
            &["alpha", "old"],
            &["alpha", "ship", "f-blocked"],
            &["alpha", "f-blocked"],
            &["alpha", "…", "ship"],
            &["alpha", "…", "f-blocked"],
            &["…", "zeta", "zp", "f-zp"],
            &["beta", "…"],
            &["…", "zeta", "…"],
            &["…", "…"],
        ] {
            assert!(row(&t, &path(missing)).is_none(), "{missing:?}");
        }
    }

    #[test]
    fn only_the_live_plans_are_counted_in_a_collapsed_context_and_archived_ones_do_not_colour_its_dot() {
        let mut t = archived();
        t.contexts[0].plans[2].head.phase = Phase::Done;
        let buf = render(&t, &mut state_for(&t, &["alpha"], &[]), 44, 3, false);
        assert_eq!(text(&buf, 0), "○ Alpha 2", "ship and sketch; the archived Plan is behind `…`");
    }
}
