//! The tree itself (the spike's `context.rs`, on the read model): Contexts,
//! their Plans, and the Sessions forked by `session fork` as a `tui-tree-widget` tree.
//!
//! The widget draws one text per row (guide, dot, name). It has no
//! right-aligned column, so each Plan's five Job counters are drawn as a
//! right-aligned `Line` into the row's `Rect` after the tree. The `󰐊` of the
//! Session in the chat slot is drawn over the indent of its row for the same
//! reason (the widget owns the indent). Context rows are not selectable: the cursor moves between Sessions
//! only, and is inverted over its row's whole width, only while the pane has keyboard focus.
//!
//! **Folds** (decisions hs-d337, hs-d339, hs-d344, hs-d349; `crate::fold`, the one widget). A Context is a collapsible
//! block: the title row `╭` + the Context's dot + its id, each content row `├─` + the row's dot + its name, straight
//! against the branch (the widget indents a child by two columns; `draw` takes it back), and **one** closing row. The
//! toggle shows and hides **only the archived rows**: the live rows always show. Folded, the closing row is the muted
//! `╰󰁋 expand`; expanded, the archived rows (muted) and `╰󰁣 collapse`; a Context with no archived rows closes with a
//! plain `╰─` and has no toggle. Only the closing row folds ([`Row::Toggle`], [`click_toggle`]); a click on the title
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
//! | `[╰─ context]` | the closing row of that Context (`╰󰁋 expand`, `╰󰁣 collapse` or `╰─`); its toggle |
//!
//! Everything here is pure except `draw`, which only writes to the frame's buffer.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::buffer::Cell;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState};

use crate::fold;
use crate::palette::{self, Meaning};
use cpd_core::plan::Phase;
use crate::read::{ContextNode, Dot, JobStatus, PlanNode, SessionNode, Tree};

/// Tree identifiers: a Context's id, a Plan's id, a Session's id.
pub type Id = String;

/// One tree level is indented by two columns (`tui-tree-widget`'s own indentation, which [`draw`] takes back).
const INDENT: usize = 2;

/// The mark of the Session in the chat slot, in column 0 of its row.
pub const ACTIVE_MARK: &str = "\u{F040A}"; // md-play

/// What a closing row's identifier starts with: the identifier of the fold it toggles, last element, behind this.
const TOGGLE: &str = "╰─ ";

/// The columns a content row's branch takes (`fold::TREE_BRANCH`).
const BRANCH_WIDTH: usize = 2;

/// The identifier of the toggle row of the Context whose own identifier is `id`; it is a sibling of it, so the rest of
/// the path is the same.
fn toggle_id(id: &str) -> Id {
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
    /// The closing row of a Context (`╰󰁋 expand`, `╰󰁣 collapse` or a plain `╰─`): the only row that folds its archived rows.
    Toggle,
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
}

impl Kids<'_> {
    /// The Context has rows its toggle shows and hides.
    fn has_archived(&self) -> bool {
        !self.archived.is_empty() || !self.strays.is_empty()
    }
}

fn kids(c: &ContextNode) -> Kids<'_> {
    if c.is_archived() {
        return Kids { direct: Vec::new(), forks: Vec::new(), archived: c.plans.iter().collect(), strays: c.forks() };
    }
    let (archived, direct): (Vec<&PlanNode>, Vec<&PlanNode>) = c.plans.iter().partition(|p| p.head.is_archived());
    let (strays, forks): (Vec<_>, Vec<_>) = c.forks().into_iter().partition(|(p, s)| p.head.is_archived() || s.is_archived());
    Kids { direct, forks, archived, strays }
}

/// The row at `path`, `None` when the tree has none.
pub fn row<'a>(tree: &'a Tree, path: &[Id]) -> Option<Row<'a>> {
    if let Some(owner) = toggle_owner(path) {
        return matches!(row(tree, &owner), Some(Row::Context(_))).then_some(Row::Toggle);
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
    k.forks.iter().chain(&k.strays).find(|(_, s)| s.id.as_str() == next).map(|(p, s)| Row::Fork(context, p, s))
}

impl<'a> Row<'a> {
    /// The Session the row opens and closes: a Plan's Lead, or the fork; none for a Context or a toggle.
    pub fn session(&self) -> Option<&'a SessionNode> {
        match *self {
            Row::Context(_) | Row::Toggle => None,
            Row::Plan(_, p) => p.lead(),
            Row::Fork(_, _, s) => Some(s),
        }
    }

    /// The Plan of the row; none for a Context or a toggle.
    pub fn plan(&self) -> Option<&'a PlanNode> {
        match *self {
            Row::Context(_) | Row::Toggle => None,
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
            Row::Toggle => false,
        }
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
            Row::Toggle => None,
        }
    }

    /// The label the row has in the prompts: a Context's title, a Plan's title, a Session's name. The tree itself
    /// shows ids (`shown_id`).
    pub fn label(&self) -> String {
        match *self {
            Row::Context(c) => c.title.clone(),
            Row::Plan(_, p) => p.head.title.clone(),
            Row::Fork(_, _, s) => session_label(s),
            Row::Toggle => String::new(),
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

/// The display width of `text` in terminal columns (wide characters take two, combining marks none).
pub fn width(text: &str) -> usize {
    Span::raw(text).width()
}

/// `text` cut to `width` columns with a final `…`. Counted in display columns, not chars (names are free text).
pub fn truncate(text: &str, width: usize) -> String {
    if self::width(text) <= width {
        return text.to_string();
    }
    let Some(room) = width.checked_sub(1) else { return String::new() };
    let (mut cut, mut used) = (String::new(), 0);
    for c in text.chars() {
        used += self::width(c.encode_utf8(&mut [0; 4]));
        if used > room {
            break;
        }
        cut.push(c);
    }
    cut + "…"
}

/// What a Plan's or fork's row says (both are one level under their Context).
struct RowText<'a> {
    /// The Session of the row: its chat is marked with `󰐊`, and its name is bold, when it is in the chat slot.
    session: Option<&'a SessionNode>,
    name: String,
    /// How deep the row is: 1 for a Plan or fork of a live Context.
    depth: usize,
    /// The animation frame of the working dot (`palette::frame`).
    frame: usize,
    /// The width of the counters drawn at the right (0: none).
    counters_width: usize,
    /// Archived: its dot is grey and never turns, and its name is not bold (the colours are muted when drawn).
    muted: bool,
}

/// A Plan's or fork's row text (the reference's `session_text`), without its branch (the fold's): dot, name. The
/// name is cut so that, in a column `width` wide, it never reaches the counters drawn at the right.
fn row_text(r: RowText, width: u16) -> Line<'static> {
    let dot = r.session.map_or(Dot::Stopped, |s| s.dot);
    let mut spans = vec![palette::dot_span(dot, r.frame, r.muted), " ".into()];
    let used = r.depth * INDENT + BRANCH_WIDTH + Line::from(spans.clone()).width() + if r.counters_width > 0 { r.counters_width + 1 } else { 0 };
    let name = truncate(&r.name, (width as usize).saturating_sub(used));
    let active = r.session.is_some_and(SessionNode::is_active) && !r.muted;
    spans.push(if active { Span::styled(name, Style::new().add_modifier(Modifier::BOLD)) } else { name.into() });
    Line::from(spans)
}

/// A Context's title (after the `╭`): its dot (the most urgent of its live Sessions, grey when the Context is
/// archived) and id. The fold makes it bold.
fn context_text(c: &ContextNode, frame: usize) -> Line<'static> {
    Line::from(vec![palette::dot_span(c.dot, frame, c.is_archived()), " ".into(), c.id.to_string().into()])
}

/// A Plan's row text, one level under its Context.
fn plan_row(c: &ContextNode, p: &PlanNode, width: u16, frame: usize) -> (Id, Line<'static>) {
    let muted = c.is_archived() || p.head.is_archived();
    let text = row_text(
        RowText { session: p.lead(), name: p.head.id.to_string(), depth: 1, frame, counters_width: counters(p).width(), muted },
        width,
    );
    (p.head.id.to_string(), text)
}

/// A fork's row text, one level under its Context.
fn fork_row(s: &SessionNode, width: u16, frame: usize, muted: bool) -> (Id, Line<'static>) {
    let text = row_text(RowText { session: Some(s), name: fork_id(s), depth: 1, frame, counters_width: 0, muted }, width);
    (s.id.to_string(), text)
}

/// A Context's items: its item (the title row and, as the children, its live Plans and forks, then, when expanded, its
/// archived ones) and, right after it, its closing row. `opened` is the tree state's open set: the Context is expanded
/// when it holds the closing row's identifier. The closing row is a sibling of the title row, not a child, so that it
/// stays under the live rows (the tree widget draws a child only under an open item) and is drawn once.
fn context_items(c: &ContextNode, opened: &HashSet<Vec<Id>>, width: u16, frame: usize) -> Vec<TreeItem<'static, Id>> {
    let id = c.id.to_string();
    let close = toggle_id(&id);
    let k = kids(c);
    let expanded = opened.contains(&vec![close.clone()]);
    let mut rows: Vec<(Id, Line<'static>)> = k.direct.iter().map(|p| plan_row(c, p, width, frame)).collect();
    rows.extend(k.forks.iter().map(|(_, s)| fork_row(s, width, frame, false)));
    if expanded {
        rows.extend(k.archived.iter().map(|p| plan_row(c, p, width, frame)));
        rows.extend(k.strays.iter().map(|(_, s)| fork_row(s, width, frame, true)));
    }
    let children = rows.into_iter().map(|(id, line)| TreeItem::new_leaf(id, fold::content(fold::TREE_BRANCH, line))).collect();
    let closing = if k.has_archived() { fold::toggle(expanded) } else { fold::plain() };
    vec![
        TreeItem::new(id, fold::title(context_text(c, frame)), children).expect("Plan and Session ids are unique"),
        TreeItem::new_leaf(close, closing),
    ]
}

/// The tree's items for a column `width` wide: the live Contexts, then the archived ones. `opened` is the tree state's
/// open set: a Context not in it is folded. `frame` is the animation frame of the working dots.
pub fn items(tree: &Tree, opened: &HashSet<Vec<Id>>, width: u16, frame: usize) -> Vec<TreeItem<'static, Id>> {
    let (archived, live): (Vec<&ContextNode>, Vec<&ContextNode>) = tree.contexts.iter().partition(|c| c.is_archived());
    live.into_iter().chain(archived).flat_map(|c| context_items(c, opened, width, frame)).collect()
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
        // the widget indents a child by two columns per level; the fold's branch is straight against the edge
        let shift = u16::try_from(path.len().saturating_sub(1) * INDENT).unwrap_or(0).min(rect.width);
        for x in rect.x..rect.right() {
            let cell = if x + shift < rect.right() { buf[(x + shift, y)].clone() } else { Cell::EMPTY };
            buf[(x, y)] = cell;
        }
        let muted = row.archived();
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
            Row::Context(_) | Row::Toggle => {}
        }
        if muted {
            buf.set_style(rect, palette::fg(Meaning::Nothing));
        }
        if cursor && row.selectable() && path.as_slice() == state.selected() {
            buf.set_style(rect, Modifier::REVERSED);
        }
    }
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

/// A click on a closing row (`╰󰁋 expand`, `╰󰁣 collapse`) shows or hides its Context's archived rows; it selects nothing.
/// Returns whether `path` was a closing row: the only row that folds. A plain `╰─` (nothing to fold) is one, and folds
/// nothing.
pub fn click_toggle(state: &mut TreeState<Id>, tree: &Tree, path: &[Id]) -> bool {
    let Some(Row::Context(c)) = toggle_owner(path).and_then(|owner| row(tree, &owner)) else { return false };
    if kids(c).has_archived() {
        state.toggle(path.to_vec());
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_glyphs_are_one_cell_wide() {
        for glyph in [ACTIVE_MARK] {
            assert_eq!(ratatui::text::Span::raw(glyph).width(), 1, "{glyph:?}");
        }
    }

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    use super::super::fixture::{WHEN, archived, context, plan, sample, session, tree, managed};
    use super::*;
    use cpd_core::session::State;
    use crate::palette::{IDLE, NEEDS_YOU, SPINNER, STOPPED, color};

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

    /// The closing toggle `╰󰁋 expand` of a Context with archived rows, as the fold widget draws it.
    const EXPAND: &str = "╰\u{F004B} expand";
    /// The closing toggle `╰󰁣 collapse`.
    const COLLAPSE: &str = "╰\u{F0063} collapse";
    /// The closing row of a Context with nothing to fold.
    const PLAIN: &str = "╰─";

    /// A Plan with an idle Lead named `name` and no forks.
    fn lone(id: &str, name: &str, phase: Phase, counts: [usize; 6]) -> PlanNode {
        plan(id, "cc", phase, counts, session(&format!("s-{id}"), name, State::Idle, None, Place::Parked), vec![])
    }

    /// The README's "Context column" example (`main:tui/README.md`): a Context with three Lead Sessions; the
    /// one in the chat slot needs you.
    fn readme() -> Tree {
        let mut rust = session("s-rust", "cpd rust lead", State::Blocked, Some("cpd: Worker question"), Place::Chat);
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
        let buf = render(&t, &mut state_for(&t, &[], &[]), 31, 5, false);
        assert_eq!(
            lines(&buf),
            [
                format!("╭{NEEDS_YOU} cpd"),
                row_of(&format!("├─{IDLE} v1"), "36⋅0⋅0⋅1⋅5", 31),
                row_of(&format!("{ACTIVE_MARK}─{NEEDS_YOU} rust"), "0⋅1⋅0⋅0⋅1", 31),
                row_of(&format!("├─{IDLE} ui"), "1⋅0⋅2⋅0⋅3", 31),
                PLAIN.to_owned(),
            ]
        );
        assert!(buf[(4, 2)].modifier.contains(Modifier::BOLD), "the name of the centre row is bold");
        assert!(buf[(0, 2)].modifier.contains(Modifier::BOLD), "so is its {ACTIVE_MARK}");
        assert!(buf[(2, 0)].modifier.contains(Modifier::BOLD), "and a Context's title");
        assert!(!buf[(4, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_tree_shows_contexts_plans_and_then_the_forks_as_their_siblings() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 12, false);
        assert_eq!(
            lines(&buf),
            [
                format!("╭{NEEDS_YOU} alpha").to_owned(),
                row_of(&format!("{ACTIVE_MARK}─{WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44),
                row_of(&format!("├─{IDLE} sketch"), "4⋅0⋅0⋅0⋅0", 44),
                row_of(&format!("├─{STOPPED} old"), "0⋅0⋅0⋅1⋅36", 44),
                format!("├─{IDLE} ship·f-id").to_owned(),
                format!("├─{NEEDS_YOU} ship·f-bl").to_owned(),
                PLAIN.to_owned(),
                format!("╭{NEEDS_YOU} beta").to_owned(),
                row_of(&format!("├─{NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44),
                PLAIN.to_owned(),
                format!("╭{STOPPED} gamma").to_owned(),
                PLAIN.to_owned(),
            ],
            "no state text after a name; a plain `╰─` closes each Context with nothing archived, even one with no row inside"
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
        for y in [1, 2, 3, 4, 5] {
            assert_eq!((buf[(1, y)].symbol(), fg(&buf, 1, y)), ("─", color(Meaning::Nothing)), "a muted branch at row {y}");
        }
        for y in [2, 3, 4, 5] {
            assert_eq!((buf[(0, y)].symbol(), fg(&buf, 0, y)), ("├", color(Meaning::Nothing)), "a muted branch at row {y}");
        }
        for y in [6, 9, 11] {
            for x in 0..12 {
                if buf[(x, y)].symbol() != " " {
                    assert_eq!(fg(&buf, x, y), color(Meaning::Nothing), "the closing row is muted: ({x}, {y})");
                    assert!(!buf[(x, y)].modifier.contains(Modifier::BOLD));
                }
            }
        }
        assert_ne!(fg(&buf, 4, 2), color(Meaning::Nothing), "a content row is not muted");
        assert_ne!(fg(&buf, 2, 2), color(Meaning::Nothing));
        // a Context with nothing archived has a closing row that folds nothing
        let before = state.opened().clone();
        assert!(click_toggle(&mut state, &tree, &toggle_of(&["alpha"])), "it is a closing row");
        assert_eq!(state.opened(), &before, "but there is nothing to fold");
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
        let folded = lines(&render(&tree, &mut state, 44, 6, false));
        assert_eq!(folded[..5], [format!("╭{NEEDS_YOU} alpha"), row_of(&format!("{ACTIVE_MARK}─{WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44), row_of(&format!("├─{IDLE} sketch"), "4⋅0⋅0⋅0⋅0", 44), format!("├─{IDLE} ship·f-id"), EXPAND.to_owned()]);
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
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 9, false);
        assert_eq!(
            lines(&buf),
            [
                format!("╭{NEEDS_YOU} alpha").to_owned(),
                row_of(&format!("{ACTIVE_MARK}─{WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44),
                row_of(&format!("├─{IDLE} sketch"), "4⋅0⋅0⋅0⋅0", 44),
                format!("├─{IDLE} ship·f-id").to_owned(),
                EXPAND.to_owned(),
                format!("╭{NEEDS_YOU} beta").to_owned(),
                row_of(&format!("├─{NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44),
                PLAIN.to_owned(),
                format!("╭{STOPPED} gamma").to_owned(),
            ]
        );
    }

    #[test]
    fn a_context_has_the_dot_of_its_manager_and_not_of_any_session_inside() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 1, false);
        assert_eq!(text(&buf, 0), format!("╭{NEEDS_YOU} alpha"), "its Manager's, not its working Lead's");
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
        assert_eq!(lines(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false)), [format!("╭{IDLE} cc").to_owned(), row_of(&format!("├─{IDLE} pp"), "0⋅0⋅0⋅0⋅0", 30)]);
        let mut t = t;
        t.contexts[0].plans[0].sessions[0].dot = Dot::Red;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false), 1), row_of(&format!("├─{NEEDS_YOU} pp"), "0⋅0⋅0⋅0⋅0", 30));
    }

    #[test]
    fn drafts_are_counted_and_dark_grey_in_phase_plan() {
        let t = tree(vec![context("cc", vec![lone("pp", "p", Phase::Plan, [4, 0, 0, 0, 0, 0])])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!(text(&buf, 1), row_of(&format!("├─{IDLE} pp"), "4⋅0⋅0⋅0⋅0", 20));
        assert_eq!(fg(&buf, 11, 1), color(Meaning::Nothing));
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_the_counters_stay_right_aligned() {
        let long = lone("a-very-long-plan-id", "lead", Phase::Dispatch, [0, 36, 0, 0, 1, 5]);
        let t = tree(vec![context("cc", vec![long])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 2, false);
        assert_eq!(text(&buf, 1), row_of(&format!("├─{IDLE} a-very-long-…"), "36⋅0⋅0⋅1⋅5", 30));
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
            for dot in [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::White, Dot::Working, Dot::Idle, Dot::Stopped] {
                let mut p = with_dot("pp", dot);
                p.head.phase = phase;
                let t = tree(vec![managed(context("cc", vec![p]), dot)]);
                let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
                assert_eq!(cell(&buf, 2, 1), drawn_dot(dot), "{dot:?} in {phase:?}");
                assert_eq!(cell(&buf, 1, 0), drawn_dot(dot), "its Context row too: {dot:?} in {phase:?}");
            }
        }
        assert_eq!(drawn_dot(Dot::Red).0, format!("{NEEDS_YOU}"));
        assert_eq!(drawn_dot(Dot::Idle), (format!("{IDLE}").to_owned(), Default::default()), "the terminal's foreground");
    }

    #[test]
    fn a_stopped_dot_is_dim() {
        let t = tree(vec![managed(context("cc", vec![with_dot("pp", Dot::Stopped)]), Dot::Stopped)]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!((buf[(2, 1)].symbol(), buf[(1, 0)].symbol()), (STOPPED, STOPPED));
        assert!(buf[(2, 1)].modifier.contains(Modifier::DIM) && buf[(1, 0)].modifier.contains(Modifier::DIM));
        let t = tree(vec![context("cc", vec![with_dot("pp", Dot::Idle)])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert!(!buf[(2, 1)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_working_dot_turns_with_the_frame_on_session_rows_and_their_context() {
        let t = tree(vec![managed(context("cc", vec![with_dot("pp", Dot::Working)]), Dot::Working)]);
        let glyphs: Vec<(String, String)> = (0..9)
            .map(|frame| {
                let buf = render_at(&t, &mut state_for(&t, &[], &[]), 20, 2, false, frame);
                (buf[(1, 0)].symbol().to_owned(), buf[(2, 1)].symbol().to_owned())
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
        let mut gone = managed(context("zz", vec![with_dot("qq", Dot::Working)]), Dot::Working);
        gone.archived_at = Some(WHEN.into());
        let t = tree(vec![managed(context("cc", vec![p]), Dot::Working), gone]);
        let mut state = state_for(&t, &[], &[]);
        unfold(&mut state, &t, "zz");
        for frame in 0..SPINNER.len() {
            let buf = render_at(&t, &mut state, 24, 12, false, frame);
            let grey = (String::from(format!("{WORKING}")), color(Meaning::Nothing));
            assert_eq!(cell(&buf, 2, 1), grey, "frame {frame}: an archived Plan row, always the first frame {:?}", lines(&buf));
            assert_eq!(cell(&buf, 1, 3), grey, "frame {frame}: an archived Context row {:?}", lines(&buf));
            assert_eq!(cell(&buf, 2, 4), grey, "frame {frame}: its Plan row");
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
        let t = tree(vec![managed(context("cc", vec![with_dot("aa", Dot::Yellow), with_dot("bb", Dot::Red), with_dot("dd", Dot::White)]), Dot::Working)]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 4, false);
        assert_eq!(cell(&buf, 1, 0), drawn_dot(Dot::Working));
        let collapsed = render(&t, &mut state_for(&t, &["cc"], &[]), 20, 1, false);
        assert_eq!(cell(&collapsed, 1, 0), drawn_dot(Dot::Working), "collapsed too");
        assert_eq!((cell(&buf, 2, 1), cell(&buf, 2, 2), cell(&buf, 2, 3)), (drawn_dot(Dot::Yellow), drawn_dot(Dot::Red), drawn_dot(Dot::White)), "while each Session row keeps its own");
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
        for (x, y, glyph) in [(1, 1, '─'), (0, 2, '├'), (0, 3, '├'), (0, 4, '├'), (0, 5, '├'), (1, 5, '─'), (0, 6, '╰'), (1, 6, '─')] {
            assert_eq!((text(&buf, y).chars().nth(x), fg(&buf, x as u16, y)), (Some(glyph), color(Meaning::Nothing)), "({x}, {y})");
        }
    }

    #[test]
    fn a_fork_is_a_sibling_of_the_plans_and_the_closing_row_closes_the_guide() {
        let t = tree(vec![context(
            "cc",
            vec![plan("pp", "cc", Phase::Plan, [0; 6], session("l", "lead", State::Idle, None, Place::None), vec![session("f", "fork", State::Idle, None, Place::None)])],
        )]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 4, false);
        assert_eq!(lines(&buf)[1..], [row_of(&format!("├─{IDLE} pp"), "0⋅0⋅0⋅0⋅0", 30), format!("├─{IDLE} pp·f").to_owned(), PLAIN.to_owned()]);
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
        assert_eq!(text(&buf, 4), format!("{ACTIVE_MARK}─{IDLE} ship·f-id"));
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
    fn the_cursor_still_reaches_the_live_sessions_of_a_folded_context() {
        let tree = archived();
        let mut state = state_for(&tree, &["alpha"], &[]);
        move_cursor(&mut state, &tree, 1);
        assert_eq!(state.selected(), path(&["alpha", "ship"]));
        for _ in 0..3 {
            move_cursor(&mut state, &tree, 1);
        }
        assert_eq!(state.selected(), path(&["beta", "rel"]), "ship, sketch, the live fork, then the next Context's");
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
        assert_eq!(text(&buf, 0), format!("╭{NEEDS_YOU} alpha"), "not its title `Payments`");
        assert_eq!(text(&buf, 1), row_of(&format!("{ACTIVE_MARK}─{WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44), "not its title `Pay by card`, nor its Lead's name");
        assert_eq!(text(&buf, 5), format!("├─{NEEDS_YOU} ship·f-bl"), "the Plan id, `·`, the first 4 characters of the Session id; not its name");
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
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 3, false), 2), format!("├─{IDLE} pp·0123"));
        t.contexts[0].plans[0].sessions[1].plan = None;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 3, false), 2), format!("├─{IDLE} 01234567"));
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
        let buf = render(&t, &mut state_for(&t, &[], &[]), 44, 14, false);
        assert_eq!(
            lines(&buf)[..12],
            [
                format!("╭{NEEDS_YOU} alpha").to_owned(),
                row_of(&format!("{ACTIVE_MARK}─{WORKING} ship"), "2⋅1⋅1⋅0⋅5", 44),
                row_of(&format!("├─{IDLE} sketch"), "4⋅0⋅0⋅0⋅0", 44),
                format!("├─{IDLE} ship·f-id").to_owned(),
                row_of(&format!("├─{STOPPED} old"), "0⋅0⋅0⋅1⋅36", 44),
                format!("├─{NEEDS_YOU} ship·f-bl").to_owned(),
                COLLAPSE.to_owned(),
                format!("╭{NEEDS_YOU} beta").to_owned(),
                row_of(&format!("├─{NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44),
                PLAIN.to_owned(),
                format!("╭{STOPPED} gamma").to_owned(),
                PLAIN.to_owned(),
            ],
            "the archived Plan and fork follow the live rows, no empty row and no `󰇘`; one closing row per Context"
        );
        let all = lines(&buf).join("\n");
        assert!(!all.contains('\u{F01D8}'), "no `󰇘` row anywhere");
        assert_eq!(lines(&buf).iter().filter(|l| l.starts_with('╰')).count(), 4, "one closing row per Context, never two: {all}");
        // the live rows keep their colours; the archived ones are all dark grey, branch, counters and dots included
        assert_eq!(cell(&buf, 2, 2), drawn_dot(Dot::Idle), "a live dot is not greyed");
        for (y, columns) in [(4, 0..44), (5, 0..24)] {
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
            lines(&buf)[7..11],
            [format!("╭{NEEDS_YOU} beta"), row_of(&format!("├─{NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44), format!("├─{IDLE} rel·f-re"), COLLAPSE.to_owned()]
        );
        assert_eq!(fg(&buf, 3, 9), color(Meaning::Nothing), "the archived fork is muted");
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
            lines(&buf)[7..11],
            [format!("╭{NEEDS_YOU} beta"), row_of(&format!("├─{NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅3", 44), format!("├─{IDLE} rel·f-re"), COLLAPSE.to_owned()],
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
        let rows = lines(&render(&t, &mut state, 44, 7, false));
        assert_eq!(rows[3..6], [format!("├─{IDLE} ship·f-id"), EXPAND.to_owned(), format!("╭{NEEDS_YOU} beta")], "{rows:?}");
        unfold(&mut state, &t, "alpha");
        assert_eq!(text(&render(&t, &mut state, 44, 10, false), 6), COLLAPSE);
    }

    #[test]
    fn archived_contexts_follow_the_live_ones_muted_start_folded_and_unfold_at_their_own_toggle_row() {
        let t = archived();
        let mut state = state_for(&t, &[], &[]);
        assert!(!state.opened().contains(&toggle_of(&["zeta"])), "an archived Context starts folded");
        let buf = render(&t, &mut state, 44, 14, false);
        let rows = lines(&buf);
        assert_eq!(rows[10..], [format!("╭{STOPPED} gamma"), PLAIN.to_owned(), format!("╭{STOPPED} zeta"), EXPAND.to_owned()], "{rows:#?}");
        unfold(&mut state, &t, "zeta");
        let buf = render(&t, &mut state, 44, 16, false);
        let rows = lines(&buf);
        assert_eq!(
            rows[12..],
            [
                format!("╭{STOPPED} zeta"),
                row_of(&format!("├─{STOPPED} zp"), "0⋅0⋅0⋅0⋅2", 44),
                format!("├─{STOPPED} zp·f-zp"),
                COLLAPSE.to_owned()
            ],
            "{rows:#?}"
        );
        for y in 12..16 {
            for x in 0..44 {
                if buf[(x, y)].symbol() != " " {
                    assert_eq!(fg(&buf, x, y), color(Meaning::Nothing), "({x}, {y})");
                }
            }
        }
        assert!(click_toggle(&mut state, &t, &toggle_of(&["zeta"])));
        assert_eq!(lines(&render(&t, &mut state, 44, 16, false))[12..14], [format!("╭{STOPPED} zeta"), EXPAND.to_owned()]);
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

    fn archived_of_path(t: &Tree, p: &[Id]) -> Option<bool> {
        row(t, p).map(|r| r.archived())
    }

    #[test]
    fn archived_rows_resolve_from_their_paths_and_are_archived() {
        let t = archived();
        let archived_of = |parts: &[&str]| row(&t, &path(parts)).map(|r| r.archived());
        assert_eq!(archived_of(&["alpha"]), Some(false));
        assert_eq!(archived_of(&["alpha", "f-idle"]), Some(false));
        for fold in [&["alpha"][..], &["beta"], &["zeta"]] {
            assert!(matches!(row(&t, &toggle_of(fold)), Some(Row::Toggle)), "the toggle row of {fold:?}");
        }
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
        let buf = render(&t, &mut state_for(&t, &["alpha"], &[]), 44, 3, false);
        assert_eq!(text(&buf, 0), format!("╭{NEEDS_YOU} alpha"), "the dot is the Manager's");
    }
}
