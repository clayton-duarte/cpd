//! The Context view (left column): Contexts and their Lead sessions as a `tui-tree-widget` tree.
//!
//! The widget draws one text per row (guide, dot, name; for a Context: dot, name, and the number of Plans when
//! collapsed). It has no right-aligned column, so each session's five Job counters are drawn as a right-aligned `Line`
//! into the row's `Rect` after the tree (ratatui has no widget for a second aligned column in a tree row).
//! The `⏵` of the active session is drawn over the indent of its row, for the same reason (the widget owns the indent).
//! Context rows are not selectable (a click only collapses or expands them): the cursor moves between sessions only,
//! and is inverted over its row's whole width, only while the pane has keyboard focus.
//! Everything here is pure except `draw`, which only writes to the frame's buffer.

use std::collections::HashSet;

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::palette::{self, Meaning};

/// Tree identifiers: a Context's name, a session's id. A session's path is `[context, id]`.
pub type Id = String;

/// One tree level is indented by two columns (`tui-tree-widget`'s own indentation).
const INDENT: usize = 2;

/// The attached Workflow of a session, as the column shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workflow {
    pub phase: String,
    /// Jobs per status, in the fixed order: queued (drafts in phase `plan`), running, merging, failed, merged.
    pub counts: [usize; 5],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub label: String,
    /// The session whose chat is in the centre slot.
    pub active: bool,
    pub needs_you: bool,
    /// `None`: no Workflow attached (or not fetched yet): no counters, a dark-grey dot.
    pub workflow: Option<Workflow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextRow {
    pub name: String,
    pub sessions: Vec<SessionRow>,
}

/// A session needs the User when its Lead waits on them (`@asking`), or when it is idle and finished a turn the User
/// has not seen (`@settled_at` after `@seen_at`; never seen counts as seen at 0). Sessions that publish neither: no.
pub fn needs_you(asking: bool, state: &str, settled_at: Option<u64>, seen_at: Option<u64>) -> bool {
    asking || (state == "idle" && settled_at.is_some_and(|settled| settled > seen_at.unwrap_or(0)))
}

/// The busiest phase among sessions: dispatch > plan > context > pr > done > none.
pub fn busiest(sessions: &[SessionRow]) -> Option<&'static str> {
    ["dispatch", "plan", "context", "pr", "done"].into_iter().find(|p| sessions.iter().any(|s| s.workflow.as_ref().is_some_and(|w| w.phase == *p)))
}

/// The Job status each counter counts, in order; the first is the queued Jobs, in phase `plan` the drafts.
/// The one source of the counters' order: `counters`, `count_jobs` and the legend's label all follow it.
pub fn counter_statuses(phase: &str) -> [&'static str; 5] {
    [if phase == "plan" { "draft" } else { "queued" }, "running", "in_review", "failed", "closed"]
}

/// The five counters from a Workflow's phase and its Jobs' statuses. Cancelled Jobs are not counted;
/// the first counter is the queued Jobs, in phase `plan` the drafts.
pub fn count_jobs<'a>(phase: &str, statuses: impl IntoIterator<Item = &'a str>) -> [usize; 5] {
    let mut counts = [0; 5];
    for status in statuses {
        if let Some(i) = counter_statuses(phase).iter().position(|s| *s == status) {
            counts[i] += 1;
        }
    }
    counts
}

/// `36⋅0⋅0⋅1⋅5`: each count in its status colour (nothing when zero; the drafts of phase `plan` always), `⋅` between.
pub fn counters(w: &Workflow) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (n, status)) in w.counts.iter().zip(counter_statuses(&w.phase)).enumerate() {
        if i > 0 {
            spans.push(palette::separator());
        }
        spans.push(Span::styled(n.to_string(), palette::count_style(status, *n)));
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

/// A session's row text: guide (`├ `, `└ ` for the last), dot, name. The name is cut so that, in a column `width`
/// wide, it never reaches the counters drawn at the right.
fn session_text(s: &SessionRow, last: bool, width: u16) -> Line<'static> {
    let mut prefix = vec![Span::styled(if last { "└ " } else { "├ " }, palette::fg(Meaning::Nothing)), palette::dot(s.needs_you, s.workflow.as_ref().map(|w| w.phase.as_str())), " ".into()];
    let used = INDENT + Line::from(prefix.clone()).width() + s.workflow.as_ref().map_or(0, |w| counters(w).width() + 1);
    let name = truncate(&s.label, (width as usize).saturating_sub(used));
    prefix.push(if s.active { Span::styled(name, Style::new().add_modifier(Modifier::BOLD)) } else { name.into() });
    Line::from(prefix)
}

/// A Context's row text: dot (● when a session inside needs you, coloured by the busiest phase), name, and the number
/// of Plans inside when collapsed.
fn context_text(c: &ContextRow, collapsed: bool) -> Line<'static> {
    let mut spans = vec![palette::dot(c.sessions.iter().any(|s| s.needs_you), busiest(&c.sessions)), " ".into(), Span::styled(c.name.clone(), Style::new().add_modifier(Modifier::BOLD))];
    if collapsed {
        spans.push(Span::styled(format!(" {}", c.sessions.len()), palette::fg(Meaning::Nothing)));
    }
    Line::from(spans)
}

/// The tree's items for a column `width` wide. `opened` is the tree state's open set: a Context not in it is collapsed.
/// A second pane publishing the same session id is listed once.
pub fn items(rows: &[ContextRow], opened: &HashSet<Vec<Id>>, width: u16) -> Vec<TreeItem<'static, Id>> {
    rows.iter()
        .map(|c| {
            let mut seen = HashSet::new();
            let sessions: Vec<&SessionRow> = c.sessions.iter().filter(|s| seen.insert(&s.id)).collect();
            let kids = sessions.iter().enumerate().map(|(i, s)| TreeItem::new_leaf(s.id.clone(), session_text(s, i + 1 == sessions.len(), width))).collect();
            let collapsed = !opened.contains(&vec![c.name.clone()]);
            TreeItem::new(c.name.clone(), context_text(c, collapsed), kids).expect("session ids are unique within a Context")
        })
        .collect()
}

/// The mark of the active session in column 0 of its row (the column of a Context's dot).
pub const ACTIVE_MARK: &str = "⏵";

/// Draw the column into `area`: the tree, then the counters and the active mark of each visible session row, then the
/// cursor row inverted when the pane has `focused` keyboard focus.
pub fn draw(f: &mut Frame, area: Rect, rows: &[ContextRow], state: &mut TreeState<Id>, focused: bool) {
    if rows.is_empty() {
        f.render_widget(Paragraph::new(Line::from(Span::styled(" no Lead sessions in the cockpit", Style::new().add_modifier(Modifier::DIM)))), area);
        return;
    }
    let items = items(rows, state.opened(), area.width);
    let tree = Tree::new(&items).expect("Context names are unique").node_closed_symbol("").node_open_symbol("").node_no_children_symbol("");
    f.render_stateful_widget(tree, area, state);
    let flat = state.flatten(&items);
    let buf = f.buffer_mut();
    for (y, row) in (area.y..area.bottom()).zip(flat.iter().skip(state.get_offset())) {
        let rect = Rect::new(area.x, y, area.width, 1);
        if let [_, id] = row.identifier.as_slice() {
            let session = rows.iter().flat_map(|c| &c.sessions).find(|s| &s.id == id);
            if let Some(w) = session.and_then(|s| s.workflow.as_ref()) {
                counters(w).right_aligned().render(rect, buf);
            }
            if session.is_some_and(|s| s.active) {
                Span::styled(ACTIVE_MARK, Style::new().add_modifier(Modifier::BOLD)).render(rect, buf);
            }
            if focused && row.identifier == state.selected() {
                buf.set_style(rect, Modifier::REVERSED);
            }
        }
    }
}

/// Open each Context the first time it is seen (Contexts start expanded; a collapse by the User is kept across refreshes).
pub fn open_new(state: &mut TreeState<Id>, known: &mut HashSet<Id>, rows: &[ContextRow]) {
    for c in rows {
        if known.insert(c.name.clone()) {
            state.open(vec![c.name.clone()]);
        }
    }
}

/// The visible session rows (the only selectable ones), in order.
fn visible_sessions(state: &TreeState<Id>, rows: &[ContextRow]) -> Vec<Vec<Id>> {
    state.flatten(&items(rows, state.opened(), 0)).into_iter().map(|f| f.identifier).filter(|id| id.len() == 2).collect()
}

/// After a refresh, keep the cursor on its session; when it is gone (closed session, collapsed or vanished Context)
/// move it to the session now at the same position, else the last one; with no session visible, clear it.
/// `before` is the cursor's index among the visible sessions before the refresh.
pub fn reselect(state: &mut TreeState<Id>, rows: &[ContextRow], before: Option<usize>) {
    let visible = visible_sessions(state, rows);
    if visible.iter().any(|id| id.as_slice() == state.selected()) {
        return;
    }
    state.select(visible.get(before.unwrap_or(0).min(visible.len().saturating_sub(1))).cloned().unwrap_or_default());
}

/// The index of the cursor among the visible sessions.
pub fn selected_index(state: &TreeState<Id>, rows: &[ContextRow]) -> Option<usize> {
    visible_sessions(state, rows).iter().position(|id| id.as_slice() == state.selected())
}

/// Move the cursor `delta` sessions (clamped), skipping Context rows; with no cursor, down goes to the first, up to the last.
pub fn move_cursor(state: &mut TreeState<Id>, rows: &[ContextRow], delta: isize) {
    let visible = visible_sessions(state, rows);
    let Some(last) = visible.len().checked_sub(1) else { return };
    let next = match selected_index(state, rows) {
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
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Color};
    use crate::palette::color;

    fn session(id: &str, label: &str, phase: Option<&str>, counts: [usize; 5]) -> SessionRow {
        SessionRow { id: id.into(), label: label.into(), active: false, needs_you: false, workflow: phase.map(|p| Workflow { phase: p.into(), counts }) }
    }

    fn sample() -> Vec<ContextRow> {
        vec![
            ContextRow {
                name: "cpd".into(),
                sessions: vec![
                    session("a", "v1", Some("done"), [36, 0, 0, 1, 5]),
                    SessionRow { needs_you: true, ..session("b", "cpd rust lead", Some("dispatch"), [0, 1, 0, 0, 1]) },
                    session("c", "cpd-ui", Some("pr"), [1, 0, 2, 0, 3]),
                ],
            },
            ContextRow { name: "other".into(), sessions: vec![session("d", "scratch", None, [0; 5])] },
        ]
    }

    fn state_for(rows: &[ContextRow], collapsed: &[&str], selected: &[&str]) -> TreeState<Id> {
        let mut state = TreeState::default();
        for c in rows.iter().filter(|c| !collapsed.contains(&c.name.as_str())) {
            state.open(vec![c.name.clone()]);
        }
        state.select(selected.iter().map(|s| s.to_string()).collect());
        state
    }

    fn render(rows: &[ContextRow], state: &mut TreeState<Id>, width: u16, height: u16) -> Buffer {
        render_focus(rows, state, width, height, true)
    }

    fn render_focus(rows: &[ContextRow], state: &mut TreeState<Id>, width: u16, height: u16, focused: bool) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, f.area(), rows, state, focused)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    /// A row `width` wide: `left`, then `right` flush against the right edge.
    fn row(left: &str, right: &str, width: usize) -> String {
        format!("{left:<w$}{right}", w = width - right.chars().count())
    }

    fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
        buf[(x, y)].fg
    }

    fn reversed(buf: &Buffer, y: u16) -> Vec<bool> {
        (0..buf.area.width).map(|x| buf[(x, y)].modifier.contains(Modifier::REVERSED)).collect()
    }

    #[test]
    fn an_expanded_context_shows_its_dot_and_name_and_its_sessions() {
        let rows = sample();
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 38, 6);
        assert_eq!(text(&buf, 0), "● cpd");
        assert_eq!(text(&buf, 1), row("  ├ ○ v1", "36⋅0⋅0⋅1⋅5", 38));
        assert_eq!(text(&buf, 2), row("  ├ ● cpd rust lead", "0⋅1⋅0⋅0⋅1", 38));
        assert_eq!(text(&buf, 3), row("  └ ○ cpd-ui", "1⋅0⋅2⋅0⋅3", 38));
        assert_eq!(text(&buf, 4), "○ other");
        assert_eq!(text(&buf, 5), "  └ ○ scratch");
    }

    #[test]
    fn a_collapsed_context_shows_the_number_of_plans_and_hides_them() {
        let rows = sample();
        let buf = render(&rows, &mut state_for(&rows, &["cpd"], &[]), 38, 4);
        assert_eq!(text(&buf, 0), "● cpd 3");
        assert_eq!(text(&buf, 1), "○ other");
        assert_eq!(text(&buf, 2), "  └ ○ scratch");
        assert_eq!(text(&buf, 3), "");
    }

    #[test]
    fn a_context_has_the_dot_of_any_session_inside_that_needs_you() {
        let rows = sample();
        assert_eq!(text(&render(&rows, &mut state_for(&rows, &["cpd"], &[]), 38, 1), 0), "● cpd 3");
        assert_eq!(text(&render(&rows, &mut state_for(&rows, &[], &[]), 38, 1), 0), "● cpd");
    }

    #[test]
    fn a_session_without_a_workflow_has_no_counters() {
        let rows = sample();
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 38, 6);
        assert_eq!(text(&buf, 5), "  └ ○ scratch");
        assert_eq!(fg(&buf, 4, 5), color(Meaning::Nothing));
    }

    #[test]
    fn drafts_are_counted_and_dark_grey_in_phase_plan() {
        let rows = vec![ContextRow { name: "c".into(), sessions: vec![session("a", "p", Some("plan"), [4, 0, 0, 0, 0])] }];
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 20, 2);
        assert_eq!(text(&buf, 1), row("  └ ○ p", "4⋅0⋅0⋅0⋅0", 20));
        assert_eq!(fg(&buf, 14, 1), color(Meaning::Nothing));
        assert_eq!(counter_statuses("dispatch"), ["queued", "running", "in_review", "failed", "closed"]);
        assert_eq!(counter_statuses("plan")[0], "draft");
        assert_eq!(count_jobs("plan", ["draft", "draft", "queued", "cancelled", "running"]), [2, 1, 0, 0, 0]);
        assert_eq!(count_jobs("dispatch", ["draft", "queued", "queued", "cancelled", "in_review", "closed", "closed", "failed"]), [2, 0, 1, 1, 2]);
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_the_counters_stay_right_aligned() {
        let rows = vec![ContextRow { name: "c".into(), sessions: vec![session("a", "a very long session name indeed", Some("dispatch"), [36, 0, 0, 1, 5])] }];
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 30, 2);
        assert_eq!(text(&buf, 1), row("  └ ○ a very long …", "36⋅0⋅0⋅1⋅5", 30));
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "ab…");
        assert_eq!(truncate("abcd", 0), "");
    }

    #[test]
    fn every_phase_has_its_dot_colour() {
        for (phase, meaning) in [("context", Meaning::Grounding), ("plan", Meaning::Planning), ("dispatch", Meaning::InProgress), ("pr", Meaning::Review), ("done", Meaning::Done)] {
            let rows = vec![ContextRow { name: "c".into(), sessions: vec![session("a", "x", Some(phase), [0; 5])] }];
            let buf = render(&rows, &mut state_for(&rows, &[], &[]), 20, 2);
            assert_eq!((fg(&buf, 0, 0), fg(&buf, 4, 1)), (color(meaning), color(meaning)), "{phase}");
        }
    }

    #[test]
    fn the_context_dot_is_the_busiest_phase_inside() {
        let phases = |ps: &[&str]| ps.iter().map(|p| session(p, p, Some(p), [0; 5])).collect::<Vec<_>>();
        assert_eq!(busiest(&phases(&["done", "pr", "context", "plan", "dispatch"])), Some("dispatch"));
        assert_eq!(busiest(&phases(&["done", "pr", "context", "plan"])), Some("plan"));
        assert_eq!(busiest(&phases(&["done", "pr", "context"])), Some("context"));
        assert_eq!(busiest(&phases(&["done", "pr"])), Some("pr"));
        assert_eq!(busiest(&phases(&["done"])), Some("done"));
        assert_eq!(busiest(&[session("a", "a", None, [0; 5])]), None);
        let rows = vec![ContextRow { name: "c".into(), sessions: phases(&["done", "plan"]) }];
        assert_eq!(fg(&render(&rows, &mut state_for(&rows, &[], &[]), 20, 1), 0, 0), color(Meaning::Planning));
    }

    #[test]
    fn every_status_has_its_counter_colour_and_zeros_and_separators_are_grey() {
        let rows = vec![ContextRow { name: "c".into(), sessions: vec![session("a", "x", Some("dispatch"), [1, 2, 3, 4, 5]), session("b", "y", Some("dispatch"), [0; 5])] }];
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 20, 3);
        // counters "1⋅2⋅3⋅4⋅5" occupy the last 9 columns
        let at = |i: u16| 11 + 2 * i;
        let colours: Vec<Color> = (0..5).map(|i| fg(&buf, at(i), 1)).collect();
        assert_eq!(colours, [color(Meaning::NotStarted), color(Meaning::InProgress), color(Meaning::Review), color(Meaning::Failed), color(Meaning::Done)]);
        assert!((0..4).all(|i| fg(&buf, at(i) + 1, 1) == color(Meaning::Nothing)));
        assert!((0..5).all(|i| fg(&buf, at(i), 2) == color(Meaning::Nothing)));
    }

    #[test]
    fn the_guides_are_dark_grey() {
        let rows = sample();
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 38, 4);
        assert_eq!((text(&buf, 1).chars().nth(2), fg(&buf, 2, 1)), (Some('├'), color(Meaning::Nothing)));
        assert_eq!((text(&buf, 3).chars().nth(2), fg(&buf, 2, 3)), (Some('└'), color(Meaning::Nothing)));
    }

    #[test]
    fn the_selected_session_row_is_reversed_across_the_full_width_and_no_other_row() {
        let rows = sample();
        let mut state = state_for(&rows, &[], &["cpd", "b"]);
        let buf = render(&rows, &mut state, 38, 6);
        for y in 0..6 {
            assert_eq!(reversed(&buf, y), vec![y == 2; 38], "row {y}");
        }
        // every element keeps its colour: the counters' zero and the separator are still dark grey (now as backgrounds)
        assert_eq!(fg(&buf, 29, 2), color(Meaning::Nothing));
        assert_eq!(fg(&buf, 4, 2), color(Meaning::InProgress));
    }

    #[test]
    fn the_cursor_is_drawn_only_while_the_pane_is_focused() {
        let rows = sample();
        let mut state = state_for(&rows, &[], &["cpd", "b"]);
        let buf = render_focus(&rows, &mut state, 38, 6, false);
        assert!((0..6).all(|y| reversed(&buf, y) == vec![false; 38]), "no inverted row without focus");
        let buf = render_focus(&rows, &mut state, 38, 6, true);
        assert_eq!(reversed(&buf, 2), vec![true; 38]);
    }

    #[test]
    fn a_context_row_is_never_inverted() {
        let rows = sample();
        let buf = render(&rows, &mut state_for(&rows, &["cpd"], &["cpd"]), 38, 3);
        assert!((0..3).all(|y| reversed(&buf, y) == vec![false; 38]));
    }

    #[test]
    fn the_keyboard_cursor_skips_context_rows() {
        let rows = sample();
        let mut state = state_for(&rows, &[], &[]);
        let mut path = |state: &mut TreeState<Id>, d| {
            move_cursor(state, &rows, d);
            state.selected().to_vec()
        };
        assert_eq!(path(&mut state, 1), ["cpd", "a"], "down from nothing: the first session");
        assert_eq!(path(&mut state, 1), ["cpd", "b"]);
        assert_eq!(path(&mut state, 1), ["cpd", "c"]);
        assert_eq!(path(&mut state, 1), ["other", "d"], "over the Context row `other`");
        assert_eq!(path(&mut state, 1), ["other", "d"], "clamped at the end");
        assert_eq!(path(&mut state, -1), ["cpd", "c"], "back over the Context row");
        let mut none = state_for(&rows, &[], &[]);
        move_cursor(&mut none, &rows, -1);
        assert_eq!(none.selected(), ["other", "d"], "up from nothing: the last session");
    }

    #[test]
    fn the_cursor_skips_the_sessions_of_a_collapsed_context() {
        let rows = sample();
        let mut state = state_for(&rows, &["cpd"], &[]);
        move_cursor(&mut state, &rows, 1);
        assert_eq!(state.selected(), ["other", "d"]);
    }

    #[test]
    fn a_click_on_a_context_toggles_it_and_selects_nothing() {
        let rows = sample();
        let mut state = state_for(&rows, &[], &["cpd", "b"]);
        assert!(click_context(&mut state, &["cpd".to_string()]));
        assert!(!state.opened().contains(&vec!["cpd".to_string()]));
        assert_eq!(state.selected(), ["cpd", "b"], "the cursor stays where it was");
        assert!(click_context(&mut state, &["cpd".to_string()]));
        assert!(state.opened().contains(&vec!["cpd".to_string()]));
        let mut fresh = state_for(&rows, &[], &[]);
        click_context(&mut fresh, &["other".to_string()]);
        assert!(fresh.selected().is_empty());
        assert!(!click_context(&mut fresh, &["cpd".to_string(), "a".to_string()]), "a session is not a Context");
    }

    #[test]
    fn the_active_session_has_the_mark_in_column_0_and_no_other_row() {
        let mut rows = sample();
        rows[0].sessions[2].active = true;
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 38, 6);
        assert_eq!(text(&buf, 3), row("⏵ └ ○ cpd-ui", "1⋅0⋅2⋅0⋅3", 38));
        for y in [0, 1, 2, 4, 5] {
            assert!(!text(&buf, y).contains(ACTIVE_MARK), "row {y}");
        }
    }

    #[test]
    fn with_no_selection_no_row_is_reversed() {
        let rows = sample();
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 38, 6);
        assert!((0..6).all(|y| reversed(&buf, y) == vec![false; 38]));
    }

    #[test]
    fn needs_you_rule() {
        assert!(needs_you(true, "working", None, None), "asking");
        assert!(needs_you(false, "idle", Some(20), Some(10)), "idle with a turn finished after it was last seen");
        assert!(needs_you(false, "idle", Some(20), None), "never seen");
        assert!(!needs_you(false, "idle", Some(10), Some(10)), "seen");
        assert!(!needs_you(false, "idle", Some(10), Some(20)), "seen later");
        assert!(!needs_you(false, "working", Some(20), Some(10)), "working is not waiting");
        assert!(!needs_you(false, "idle", None, None), "no options published");
        assert!(!needs_you(false, "", None, None));
    }

    #[test]
    fn the_active_session_is_bold() {
        let mut rows = sample();
        rows[0].sessions[0].active = true;
        let buf = render(&rows, &mut state_for(&rows, &[], &[]), 38, 3);
        assert!(buf[(6, 1)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(6, 2)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn contexts_open_once_and_a_collapse_is_kept() {
        let rows = sample();
        let (mut state, mut known) = (TreeState::default(), HashSet::new());
        open_new(&mut state, &mut known, &rows);
        assert_eq!(state.opened().len(), 2);
        state.close(&["cpd".to_string()]);
        open_new(&mut state, &mut known, &rows);
        assert_eq!(state.opened().len(), 1);
    }

    #[test]
    fn the_selection_moves_to_the_next_row_when_its_session_goes() {
        let mut rows = sample();
        let mut state = state_for(&rows, &[], &["cpd", "b"]);
        let before = selected_index(&state, &rows);
        assert_eq!(before, Some(1));
        rows[0].sessions.remove(1);
        reselect(&mut state, &rows, before);
        assert_eq!(state.selected(), ["cpd", "c"]);
        reselect(&mut state, &rows, Some(2));
        assert_eq!(state.selected(), ["cpd", "c"], "a selection still there stays");
    }

    #[test]
    fn a_duplicate_session_id_is_listed_once() {
        let mut rows = sample();
        rows[0].sessions.push(session("a", "dup", None, [0; 5]));
        let state = state_for(&rows, &[], &[]);
        assert_eq!(state.flatten(&items(&rows, state.opened(), 38)).len(), 6);
    }
}
