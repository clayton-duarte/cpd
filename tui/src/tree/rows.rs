//! The tree itself (the spike's `context.rs`, on the read model): Contexts,
//! their Plans, and a Plan's side forks as a `tui-tree-widget` tree.
//!
//! The widget draws one text per row (guide, dot, name). It has no
//! right-aligned column, so each Plan's five Job counters are drawn as a
//! right-aligned `Line` into the row's `Rect` after the tree. The `⏵` of the
//! Session in the chat slot, and the `│` guide of a fork under a Plan that is
//! not the last, are drawn over the indent of their rows for the same reason
//! (the widget owns the indent). Context rows are not selectable (a click only
//! collapses or expands them): the cursor moves between Sessions only, and is
//! inverted over its row's whole width, only while the pane has keyboard focus.
//!
//! Rows (identifiers are paths): a Context `[context]`, a Plan `[context, plan]`
//! (its row shows its Lead and is the Lead's: selecting it opens the Lead),
//! a side fork of the Plan's Lead `[context, plan, session]`.
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
    Plan(&'a PlanNode),
    /// A side fork of the Plan's Lead.
    Fork(&'a ContextNode, &'a PlanNode, &'a SessionNode),
}

/// The row at `path`, `None` when the tree has none.
pub fn row<'a>(tree: &'a Tree, path: &[Id]) -> Option<Row<'a>> {
    let context = tree.contexts.iter().find(|c| Some(c.id.as_str()) == path.first().map(String::as_str))?;
    let Some(plan) = path.get(1) else { return Some(Row::Context(context)) };
    let plan = context.plans.iter().find(|p| p.head.id.as_str() == plan)?;
    let Some(fork) = path.get(2) else { return Some(Row::Plan(plan)) };
    let fork = forks(plan).find(|s| s.id.as_str() == fork)?;
    Some(Row::Fork(context, plan, fork))
}

impl<'a> Row<'a> {
    /// The Session the row opens and closes: a Plan's Lead, or the fork; none for a Context.
    pub fn session(&self) -> Option<&'a SessionNode> {
        match *self {
            Row::Context(_) => None,
            Row::Plan(p) => p.lead(),
            Row::Fork(_, _, s) => Some(s),
        }
    }

    /// The Plan of the row; none for a Context.
    pub fn plan(&self) -> Option<&'a PlanNode> {
        match *self {
            Row::Context(_) => None,
            Row::Plan(p) | Row::Fork(_, p, _) => Some(p),
        }
    }

    /// The name the row shows, as the prompts word it.
    pub fn label(&self) -> String {
        match *self {
            Row::Context(c) => c.id.to_string(),
            Row::Plan(p) => plan_label(p),
            Row::Fork(_, _, s) => session_label(s),
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

/// What a Plan's or fork's row says.
struct RowText<'a> {
    /// The Session of the row: its chat is marked with `⏵`, and its name is bold, when it is in the chat slot.
    session: Option<&'a SessionNode>,
    name: String,
    /// 1 for a Plan, 2 for a fork.
    depth: usize,
    last: bool,
    phase: Phase,
    /// The width of the counters drawn at the right (0: none).
    counters_width: usize,
}

/// A Plan's or fork's row text (the reference's `session_text`): guide (`├ `, `└ ` for the last), dot, name. The
/// name is cut so that, in a column `width` wide, it never reaches the counters drawn at the right.
fn row_text(r: RowText, width: u16) -> Line<'static> {
    let needs_you = r.session.is_some_and(|s| s.needs_you);
    let mut spans = vec![
        Span::styled(if r.last { "└ " } else { "├ " }, palette::fg(Meaning::Nothing)),
        palette::dot(needs_you, Some(r.phase)),
        " ".into(),
    ];
    let used = r.depth * INDENT + Line::from(spans.clone()).width() + if r.counters_width > 0 { r.counters_width + 1 } else { 0 };
    let name = truncate(&r.name, (width as usize).saturating_sub(used));
    let active = r.session.is_some_and(SessionNode::is_active);
    spans.push(if active { Span::styled(name, Style::new().add_modifier(Modifier::BOLD)) } else { name.into() });
    Line::from(spans)
}

/// Whether a Session of the Context needs you (the ● of its row).
fn context_needs_you(c: &ContextNode) -> bool {
    c.plans.iter().flat_map(|p| &p.sessions).any(|s| s.needs_you)
}

/// A Context's row text: dot (● when a Session inside needs you, coloured by the busiest phase), id, and the number
/// of Plans inside when collapsed.
fn context_text(c: &ContextNode, collapsed: bool) -> Line<'static> {
    let mut spans = vec![
        palette::dot(context_needs_you(c), busiest(&c.plans)),
        " ".into(),
        Span::styled(c.id.to_string(), Style::new().add_modifier(Modifier::BOLD)),
    ];
    if collapsed {
        spans.push(Span::styled(format!(" {}", c.plans.len()), palette::fg(Meaning::Nothing)));
    }
    Line::from(spans)
}

/// The tree's items for a column `width` wide. `opened` is the tree state's open set: a Context not in it is
/// collapsed (a Plan with forks is always open: [`open_new`]).
pub fn items(tree: &Tree, opened: &HashSet<Vec<Id>>, width: u16) -> Vec<TreeItem<'static, Id>> {
    tree.contexts
        .iter()
        .map(|c| {
            let context_path = vec![c.id.to_string()];
            let plans = c.plans.iter().enumerate().map(|(i, p)| {
                let phase = p.head.phase;
                let plan_last = i + 1 == c.plans.len();
                let kids: Vec<&SessionNode> = forks(p).collect();
                let text = row_text(
                    RowText {
                        session: p.lead(),
                        name: plan_label(p),
                        depth: 1,
                        last: plan_last,
                        phase,
                        counters_width: counters(p).width(),
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
                                phase,
                                counters_width: 0,
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
/// row, then the cursor's Session row inverted when `cursor` says it is shown (a Context row never is).
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
            Some(Row::Plan(plan)) => {
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
            _ => continue,
        }
        if cursor && flat_row.identifier == state.selected() {
            buf.set_style(rect, Modifier::REVERSED);
        }
    }
}

/// Open each Context the first time it is seen (Contexts start expanded; a collapse by the User is kept across
/// refreshes), and every Plan with forks (nothing collapses a Plan: its forks are Lead Sessions, always listed).
pub fn open_new(state: &mut TreeState<Id>, known: &mut HashSet<Vec<Id>>, tree: &Tree) {
    for c in &tree.contexts {
        let path = vec![c.id.to_string()];
        if known.insert(path.clone()) {
            state.open(path);
        }
        for p in c.plans.iter().filter(|p| forks(p).next().is_some()) {
            state.open(vec![c.id.to_string(), p.head.id.to_string()]);
        }
    }
}

/// The visible Session rows (the only selectable ones: a Plan's Lead and the forks), in order.
fn visible(state: &TreeState<Id>, tree: &Tree) -> Vec<Vec<Id>> {
    state.flatten(&items(tree, state.opened(), 0)).into_iter().map(|f| f.identifier).filter(|id| id.len() >= 2).collect()
}

/// After a refresh, keep the cursor on its Session; when it is gone (a Plan or Session that went, a collapsed or
/// vanished Context) move it to the Session now at the same position, else the last one; with no Session
/// visible, clear it. `before` is the cursor's index among the visible Sessions before the refresh.
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

/// Move the cursor `delta` Sessions (clamped), skipping Context rows; with no cursor, down goes to the first, up
/// to the last.
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
pub fn click_context(state: &mut TreeState<Id>, path: &[Id]) -> bool {
    let is_context = path.len() == 1;
    if is_context {
        state.toggle(path.to_vec());
    }
    is_context
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    use super::super::fixture::{context, plan, sample, session, tree};
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
                plan("ui", "cpd", Phase::Pr, [0, 1, 0, 2, 0, 3], session("s-ui", "cpd-ui", State::Idle, None, Place::Parked), vec![]),
            ],
        )])
    }

    #[test]
    fn the_readme_context_column_example() {
        let t = readme();
        let buf = render(&t, &mut state_for(&t, &[], &[]), 31, 4, false);
        assert_eq!(
            lines(&buf),
            ["● cpd", "  ├ ○ v1             36⋅0⋅0⋅1⋅5", "⏵ ├ ● cpd rust lead   0⋅1⋅0⋅0⋅1", "  └ ○ cpd-ui          1⋅0⋅2⋅0⋅3"]
        );
        assert!(buf[(6, 2)].modifier.contains(Modifier::BOLD), "the name of the centre row is bold");
        assert!(buf[(0, 2)].modifier.contains(Modifier::BOLD), "so is its ⏵");
        assert!(buf[(2, 0)].modifier.contains(Modifier::BOLD), "and a Context's name");
        assert!(!buf[(6, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_tree_shows_contexts_plans_and_forks() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &[], &[]), 44, 10, false);
        assert_eq!(
            lines(&buf),
            [
                "● alpha".to_owned(),
                row_of("⏵ ├ ○ ship lead", "2⋅1⋅1⋅0⋅5", 44),
                "  │ ├ ○ ship fork".to_owned(),
                "  │ └ ● ship review".to_owned(),
                row_of("  ├ ○ sketch", "4⋅0⋅0⋅0⋅0", 44),
                row_of("  └ ○ old", "0⋅0⋅0⋅1⋅36", 44),
                "● beta".to_owned(),
                row_of("  └ ● rel lead", "0⋅0⋅0⋅0⋅3", 44),
                "○ gamma".to_owned(),
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
        assert_eq!(lines(&buf), ["● alpha 3".to_owned(), "● beta".to_owned(), row_of("  └ ● rel lead", "0⋅0⋅0⋅0⋅3", 44), "○ gamma".to_owned()]);
    }

    #[test]
    fn a_context_has_the_dot_of_any_session_inside_that_needs_you() {
        let tree = sample();
        let buf = render(&tree, &mut state_for(&tree, &["alpha"], &[]), 44, 1, false);
        assert_eq!(text(&buf, 0), "● alpha 3", "through its blocked fork");
        let calm = super::super::fixture::tree(vec![context("cc", vec![lone("pp", "s", Phase::Plan, [0; 6])])]);
        assert_eq!(text(&render(&calm, &mut state_for(&calm, &[], &[]), 20, 1, false), 0), "○ cc");
    }

    #[test]
    fn a_rows_dot_is_its_sessions_and_not_its_plans() {
        // an open Escalation is not the ●: the Lead's state is (commands.md §17)
        let mut p = lone("pp", "lead", Phase::Dispatch, [0; 6]);
        p.head.open_escalations = 1;
        let t = tree(vec![context("cc", vec![p])]);
        assert_eq!(lines(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false)), ["○ cc".to_owned(), row_of("  └ ○ lead", "0⋅0⋅0⋅0⋅0", 30)]);
        let mut t = t;
        t.contexts[0].plans[0].sessions[0].needs_you = true;
        assert_eq!(text(&render(&t, &mut state_for(&t, &[], &[]), 30, 2, false), 1), row_of("  └ ● lead", "0⋅0⋅0⋅0⋅0", 30));
    }

    #[test]
    fn drafts_are_counted_and_dark_grey_in_phase_plan() {
        let t = tree(vec![context("cc", vec![lone("pp", "p", Phase::Plan, [4, 0, 0, 0, 0, 0])])]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 20, 2, false);
        assert_eq!(text(&buf, 1), row_of("  └ ○ p", "4⋅0⋅0⋅0⋅0", 20));
        assert_eq!(fg(&buf, 11, 1), color(Meaning::Nothing));
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_the_counters_stay_right_aligned() {
        let t = tree(vec![context("cc", vec![lone("pp", "a very long session name indeed", Phase::Dispatch, [0, 36, 0, 0, 1, 5])])]);
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
            (Phase::Pr, Meaning::Review),
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
        assert_eq!(busiest(&plans(&[Done, Pr, Grounding, Plan, Dispatch])), Some(Dispatch));
        assert_eq!(busiest(&plans(&[Done, Pr, Grounding, Plan])), Some(Plan));
        assert_eq!(busiest(&plans(&[Done, Pr, Grounding])), Some(Grounding));
        assert_eq!(busiest(&plans(&[Done, Pr])), Some(Pr));
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
        for (x, y, glyph) in [(2, 1, '├'), (2, 2, '│'), (4, 2, '├'), (4, 3, '└'), (2, 5, '└')] {
            assert_eq!((text(&buf, y).chars().nth(x), fg(&buf, x as u16, y)), (Some(glyph), color(Meaning::Nothing)), "({x}, {y})");
        }
    }

    #[test]
    fn the_last_plans_forks_have_no_guide_to_the_left() {
        let t = tree(vec![context(
            "cc",
            vec![plan("pp", "cc", Phase::Plan, [0; 6], session("l", "lead", State::Idle, None, Place::None), vec![session("f", "fork", State::Idle, None, Place::None)])],
        )]);
        let buf = render(&t, &mut state_for(&t, &[], &[]), 30, 3, false);
        assert_eq!(lines(&buf)[2], "    └ ○ fork");
    }

    #[test]
    fn the_selected_session_row_is_reversed_across_the_full_width_and_no_other_row() {
        let tree = sample();
        for (selected, y) in [(&["alpha", "ship"][..], 1), (&["alpha", "ship", "f-idle"], 2)] {
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
        assert_eq!(text(&buf, 2), "⏵ │ ├ ○ ship fork");
        for y in [0, 1, 3, 4, 5, 6, 7] {
            assert!(!text(&buf, y).contains(ACTIVE_MARK), "row {y}");
        }
        assert!(buf[(8, 2)].modifier.contains(Modifier::BOLD));
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
        assert_eq!(at(&mut state, 1), path(&["alpha", "ship", "f-idle"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "ship", "f-blocked"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "sketch"]));
        assert_eq!(at(&mut state, 1), path(&["alpha", "old"]));
        assert_eq!(at(&mut state, 1), path(&["beta", "rel"]), "over the Context row `beta`");
        assert_eq!(at(&mut state, 1), path(&["beta", "rel"]), "clamped at the end: `gamma` has no Session");
        assert_eq!(at(&mut state, -1), path(&["alpha", "old"]), "back over the Context row");
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
        assert!(click_context(&mut state, &path(&["alpha"])));
        assert!(!state.opened().contains(&path(&["alpha"])));
        assert_eq!(state.selected(), path(&["alpha", "ship"]), "the cursor stays where it was");
        assert!(click_context(&mut state, &path(&["alpha"])));
        assert!(state.opened().contains(&path(&["alpha"])));
        let mut fresh = state_for(&tree, &[], &[]);
        click_context(&mut fresh, &path(&["beta"]));
        assert!(fresh.selected().is_empty());
        assert!(!click_context(&mut fresh, &path(&["alpha", "ship"])), "a Session is not a Context");
        assert!(fresh.opened().contains(&path(&["alpha", "ship"])), "and a Plan never folds");
    }

    #[test]
    fn rows_resolve_from_their_paths() {
        let tree = sample();
        assert!(matches!(row(&tree, &path(&["alpha"])), Some(Row::Context(c)) if c.id.as_str() == "alpha"));
        assert!(matches!(row(&tree, &path(&["alpha", "ship"])), Some(Row::Plan(p)) if p.head.id.as_str() == "ship"));
        let fork = row(&tree, &path(&["alpha", "ship", "f-idle"])).unwrap();
        assert_eq!(fork.session().unwrap().id.as_str(), "f-idle");
        assert_eq!(fork.plan().unwrap().head.id.as_str(), "ship");
        assert_eq!(fork.label(), "ship fork");
        assert_eq!(row(&tree, &path(&["alpha", "ship"])).unwrap().session().unwrap().id.as_str(), "l-ship", "a Plan row is its Lead's");
        assert!(row(&tree, &path(&["gamma"])).unwrap().session().is_none());
        for missing in [&["nope"][..], &["alpha", "nope"], &["alpha", "ship", "l-ship"], &["alpha", "ship", "nope"], &[]] {
            assert!(row(&tree, &path(missing)).is_none(), "{missing:?}");
        }
    }

    #[test]
    fn contexts_open_once_and_a_collapse_is_kept() {
        let tree = sample();
        let (mut state, mut known) = (TreeState::default(), HashSet::new());
        open_new(&mut state, &mut known, &tree);
        assert_eq!(state.opened().len(), 4, "three Contexts and the Plan with forks");
        state.close(&path(&["alpha"]));
        open_new(&mut state, &mut known, &tree);
        assert_eq!(state.opened().len(), 3);
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
    fn a_plan_is_called_by_its_leads_name_else_its_id_and_a_session_by_its_name_else_its_short_id() {
        let tree = sample();
        assert_eq!(plan_label(&tree.contexts[0].plans[0]), "ship lead");
        assert_eq!(plan_label(&tree.contexts[0].plans[2]), "old", "a blank name");
        let mut s = session("0123456789abcdef", "", State::Idle, None, Place::None);
        assert_eq!(session_label(&s), "01234567");
        s.name = " named ".into();
        assert_eq!(session_label(&s), "named");
    }
}
