//! The tree itself (the spike's `context.rs`, on the read model): Contexts,
//! their Plans, and a Plan's side forks as a `tui-tree-widget` tree.
//!
//! The widget draws one text per row (guide, dot, name, state). It has no
//! right-aligned column, so each Plan's five Job counters are drawn as a
//! right-aligned `Line` into the row's `Rect` after the tree. The `⏵` of the
//! Session in the chat slot, and the `│` guide of a fork under a Plan that is
//! not the last, are drawn over the indent of their rows for the same reason
//! (the widget owns the indent). The cursor is inverted over its row's whole
//! width, only while the pane has keyboard focus.
//!
//! Rows (identifiers are paths): a Context `[context]`, a Plan `[context, plan]`
//! (its row shows its Lead and is the Lead's: selecting it opens the Lead),
//! a side fork of the Plan's Lead `[context, plan, session]`. Unlike the
//! spike's, Context rows are selectable too: `n` needs a Context to make the
//! Plan in, and a Context without Plans has no other row.
//!
//! Everything here is pure except `draw`, which only writes to the frame's buffer.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState};

use super::palette::{self, Meaning};
use crate::plan::Phase;
use crate::session::State;
use crate::tui::read::{ContextNode, PlanNode, SessionNode, Tree};

/// Tree identifiers: a Context's id, a Plan's id, a Session's id.
pub type Id = String;

/// One tree level is indented by two columns (`tui-tree-widget`'s own indentation).
const INDENT: usize = 2;

/// The mark of the Session in the chat slot, in column 0 of its row.
pub const ACTIVE_MARK: &str = "⏵";

/// What a row of the tree stands for.
#[derive(Clone, Copy, Debug)]
pub enum Row<'a> {
    Context(&'a ContextNode),
    /// A Plan, with its Lead.
    Plan(&'a ContextNode, &'a PlanNode),
    /// A side fork of the Plan's Lead.
    Fork(&'a ContextNode, &'a PlanNode, &'a SessionNode),
}

/// The row at `path`, `None` when the tree has none.
pub fn row<'a>(tree: &'a Tree, path: &[Id]) -> Option<Row<'a>> {
    let context = tree.contexts.iter().find(|c| Some(c.id.as_str()) == path.first().map(String::as_str))?;
    let Some(plan) = path.get(1) else { return Some(Row::Context(context)) };
    let plan = context.plans.iter().find(|p| p.head.id.as_str() == plan)?;
    let Some(fork) = path.get(2) else { return Some(Row::Plan(context, plan)) };
    let fork = forks(plan).find(|s| s.id.as_str() == fork)?;
    Some(Row::Fork(context, plan, fork))
}

impl<'a> Row<'a> {
    pub fn context(&self) -> &'a ContextNode {
        match *self {
            Row::Context(c) | Row::Plan(c, _) | Row::Fork(c, _, _) => c,
        }
    }

    /// The Session the row opens, parks, forks and renames: a Plan's Lead, or the fork; none for a Context.
    pub fn session(&self) -> Option<&'a SessionNode> {
        match *self {
            Row::Context(_) => None,
            Row::Plan(_, p) => p.lead(),
            Row::Fork(_, _, s) => Some(s),
        }
    }
}

/// A Plan's side forks, oldest first (its Lead is the Plan's own row).
fn forks(plan: &PlanNode) -> impl Iterator<Item = &SessionNode> {
    plan.sessions.iter().filter(|s| !s.plan_lead)
}

/// A Session's pi name, else the first 8 characters of its id.
pub fn session_label(s: &SessionNode) -> String {
    let name = s.name.trim();
    if name.is_empty() { s.id.as_str().chars().take(8).collect() } else { name.to_owned() }
}

/// A Plan's row label: its Lead's name, else the Plan's id.
pub fn plan_label(p: &PlanNode) -> String {
    match p.lead().map(|s| s.name.trim()) {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => p.head.id.to_string(),
    }
}

/// The busiest phase among Plans: dispatch > plan > grounding > pr > done > none.
pub fn busiest(plans: &[PlanNode]) -> Option<Phase> {
    [Phase::Dispatch, Phase::Plan, Phase::Grounding, Phase::Pr, Phase::Done]
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
        spans.push(Span::styled(n.to_string(), palette::count_style(i, phase, n)));
    }
    Line::from(spans)
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

/// A Session's state as the row says it after the name: working, the label of a
/// blocked one (else `blocked`), stopped; an idle Session says nothing.
fn state_text(s: &SessionNode) -> Option<(String, Meaning)> {
    match s.state {
        State::Working => Some(("working".into(), Meaning::InProgress)),
        State::Blocked => Some((s.state_label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| "blocked".into()), Meaning::Review)),
        State::Idle => None,
        State::Stopped => Some(("stopped".into(), Meaning::Nothing)),
    }
}

/// What a Plan's or fork's row says.
struct RowText<'a> {
    /// The Session whose state the row shows, and whose chat the `⏵` marks.
    session: Option<&'a SessionNode>,
    name: String,
    /// 1 for a Plan, 2 for a fork.
    depth: usize,
    last: bool,
    needs_you: bool,
    phase: Phase,
    /// The width of the counters drawn at the right (0: none).
    counters_width: usize,
    /// The forks a collapsed Plan hides.
    hidden_forks: usize,
}

/// A Plan's or fork's row text: guide (`├ `, `└ ` for the last), dot, name, then what fits of the state and
/// the number of hidden forks. The name is cut so that, in a column `width` wide, it never reaches the
/// counters drawn at the right; the rest gets what is left.
fn row_text(r: RowText, width: u16) -> Line<'static> {
    let mut spans = vec![
        Span::styled(if r.last { "└ " } else { "├ " }, palette::fg(Meaning::Nothing)),
        palette::dot(r.needs_you, Some(r.phase)),
        " ".into(),
    ];
    let used = r.depth * INDENT + Line::from(spans.clone()).width() + if r.counters_width > 0 { r.counters_width + 1 } else { 0 };
    let budget = (width as usize).saturating_sub(used);
    let name = truncate(&r.name, budget);
    let mut left = budget.saturating_sub(name.chars().count());
    let active = r.session.is_some_and(SessionNode::is_active);
    spans.push(if active { Span::styled(name, Style::new().add_modifier(Modifier::BOLD)) } else { name.into() });
    let mut tail: Vec<(String, Meaning)> = r.session.and_then(state_text).into_iter().collect();
    if r.hidden_forks > 0 {
        tail.push((format!("+{}", r.hidden_forks), Meaning::Nothing));
    }
    for (text, meaning) in tail {
        let text = truncate(&format!(" {text}"), left);
        if text.chars().count() < 2 {
            break;
        }
        left -= text.chars().count();
        spans.push(Span::styled(text, palette::fg(meaning)));
    }
    Line::from(spans)
}

/// A Context's row text: dot (● when a Plan inside needs you, coloured by the busiest phase), id, and the number
/// of Plans inside when collapsed.
fn context_text(c: &ContextNode, collapsed: bool) -> Line<'static> {
    let mut spans = vec![
        palette::dot(c.needs_you(), busiest(&c.plans)),
        " ".into(),
        Span::styled(c.id.to_string(), Style::new().add_modifier(Modifier::BOLD)),
    ];
    if collapsed {
        spans.push(Span::styled(format!(" {}", c.plans.len()), palette::fg(Meaning::Nothing)));
    }
    Line::from(spans)
}

/// The tree's items for a column `width` wide. `opened` is the tree state's open set: a Context or a Plan
/// not in it is collapsed.
pub fn items(tree: &Tree, opened: &HashSet<Vec<Id>>, width: u16) -> Vec<TreeItem<'static, Id>> {
    tree.contexts
        .iter()
        .map(|c| {
            let context_path = vec![c.id.to_string()];
            let plans = c.plans.iter().enumerate().map(|(i, p)| {
                let phase = p.head.phase;
                let plan_last = i + 1 == c.plans.len();
                let plan_path = vec![c.id.to_string(), p.head.id.to_string()];
                let kids: Vec<&SessionNode> = forks(p).collect();
                let collapsed = !kids.is_empty() && !opened.contains(&plan_path);
                let text = row_text(
                    RowText {
                        session: p.lead(),
                        name: plan_label(p),
                        depth: 1,
                        last: plan_last,
                        needs_you: p.needs_you(),
                        phase,
                        counters_width: counters(p).width(),
                        hidden_forks: if collapsed { kids.len() } else { 0 },
                    },
                    width,
                );
                let fork_items: Vec<TreeItem<'static, Id>> = kids
                    .iter()
                    .enumerate()
                    .map(|(j, s)| {
                        let text = row_text(
                            RowText {
                                session: Some(s),
                                name: session_label(s),
                                depth: 2,
                                last: j + 1 == kids.len(),
                                needs_you: s.state == State::Blocked,
                                phase,
                                counters_width: 0,
                                hidden_forks: 0,
                            },
                            width,
                        );
                        TreeItem::new_leaf(s.id.to_string(), text)
                    })
                    .collect();
                TreeItem::new(p.head.id.to_string(), text, fork_items).expect("a Session is in one Plan once")
            });
            let plans: Vec<TreeItem<'static, Id>> = plans.collect();
            TreeItem::new(c.id.to_string(), context_text(c, !opened.contains(&context_path)), plans)
                .expect("Plan ids are unique")
        })
        .collect()
}

/// Draw the column into `area`: the tree, then the counters, the guides and the active mark of each visible
/// row, then the cursor row inverted when `cursor` says it is shown.
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
        match row(tree, &flat_row.identifier) {
            Some(Row::Plan(_, plan)) => {
                counters(plan).right_aligned().render(rect, buf);
                if plan.lead().is_some_and(SessionNode::is_active) {
                    Span::styled(ACTIVE_MARK, Style::new().add_modifier(Modifier::BOLD)).render(rect, buf);
                }
            }
            Some(Row::Fork(context, plan, fork)) => {
                if context.plans.last().is_some_and(|last| last.head.id != plan.head.id) {
                    Span::styled("│", palette::fg(Meaning::Nothing)).render(Rect::new(area.x + INDENT as u16, y, 1, 1), buf);
                }
                if fork.is_active() {
                    Span::styled(ACTIVE_MARK, Style::new().add_modifier(Modifier::BOLD)).render(rect, buf);
                }
            }
            _ => {}
        }
        if cursor && flat_row.identifier == state.selected() {
            buf.set_style(rect, Modifier::REVERSED);
        }
    }
}

/// Open each Context, and each Plan with forks, the first time it is seen (they start expanded; a collapse by
/// the User is kept across refreshes).
pub fn open_new(state: &mut TreeState<Id>, known: &mut HashSet<Vec<Id>>, tree: &Tree) {
    for c in &tree.contexts {
        let path = vec![c.id.to_string()];
        if known.insert(path.clone()) {
            state.open(path);
        }
        for p in c.plans.iter().filter(|p| forks(p).next().is_some()) {
            let path = vec![c.id.to_string(), p.head.id.to_string()];
            if known.insert(path.clone()) {
                state.open(path);
            }
        }
    }
}

/// The visible rows, in order.
fn visible(state: &TreeState<Id>, tree: &Tree) -> Vec<Vec<Id>> {
    state.flatten(&items(tree, state.opened(), 0)).into_iter().map(|f| f.identifier).collect()
}

/// After a refresh, keep the cursor on its row; when it is gone (a Plan or Session that went, a collapsed or
/// vanished Context) move it to the row now at the same position, else the last one; with no row visible,
/// clear it. `before` is the cursor's index among the visible rows before the refresh.
pub fn reselect(state: &mut TreeState<Id>, tree: &Tree, before: Option<usize>) {
    let visible = visible(state, tree);
    if visible.iter().any(|id| id.as_slice() == state.selected()) {
        return;
    }
    state.select(visible.get(before.unwrap_or(0).min(visible.len().saturating_sub(1))).cloned().unwrap_or_default());
}

/// The index of the cursor among the visible rows.
pub fn selected_index(state: &TreeState<Id>, tree: &Tree) -> Option<usize> {
    visible(state, tree).iter().position(|id| id.as_slice() == state.selected())
}

/// Move the cursor `delta` rows (clamped); with no cursor, down goes to the first, up to the last.
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

/// Collapse or expand the row at `path` when it is a Context, or a Plan with forks. Returns whether it was one.
pub fn toggle(state: &mut TreeState<Id>, tree: &Tree, path: &[Id]) -> bool {
    let foldable = match row(tree, path) {
        Some(Row::Context(_)) => true,
        Some(Row::Plan(_, plan)) => forks(plan).next().is_some(),
        _ => false,
    };
    if foldable {
        state.toggle(path.to_vec());
    }
    foldable
}
