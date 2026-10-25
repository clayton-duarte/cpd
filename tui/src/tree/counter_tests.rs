//! The Job counters of the Context tree (hs-d580, commands.md §13.2, contract §6.1.1): on a live Lead's row colours 15, 14, 11, 9, 10
//! (8 at zero), the failed one always red; on an asleep Lead's row every counter is 8.

use std::collections::HashSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tui_tree_widget::TreeState;

use super::fixture::{WHEN, context, plan, session, tree};
use super::rows::{Scroll, draw, open_new};
use crate::palette::numbered::{C7, C8, C9, C10, C11, C13, C14, C15};
use crate::read::{Place, PlanNode, Tree};
use cpd_core::inbox::model::{Kind, Urgency};
use cpd_core::ids::PlanId;
use cpd_core::plan::Phase;
use cpd_core::session::State;

const WIDTH: u16 = 20;

fn lone(id: &str, phase: Phase, counts: [usize; 6]) -> PlanNode {
    plan(id, "cc", phase, counts, session(&format!("s-{id}"), "x", State::Idle, None, Place::Parked), vec![])
}

fn render(t: &Tree, height: u16) -> Buffer {
    let mut state: TreeState<String> = TreeState::default();
    open_new(&mut state, &mut HashSet::new(), t);
    state.select(vec![]);
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, height + 40)).unwrap();
    terminal.draw(|f| draw(f, f.area(), t, &mut state, false, 0, &mut Scroll::default())).unwrap();
    let full = terminal.backend().buffer();
    let mut buf = Buffer::empty(Rect::new(0, 0, WIDTH, height));
    for y in 0..height {
        for x in 0..WIDTH {
            buf[(x, y)] = full[(x, y)].clone();
        }
    }
    buf
}

fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
    buf[(x, y)].fg
}

/// The text of row `y` from column 9: counters `a⋅b⋅c⋅d⋅e` (single digits) end 2 columns before the edge.
fn counters_text(buf: &Buffer, y: u16) -> String {
    (9..WIDTH - 2).map(|x| buf[(x, y)].symbol()).collect()
}

/// The colours of the five counters of row `y`.
fn counter_colours(buf: &Buffer, y: u16) -> Vec<Color> {
    (0..5).map(|i| fg(buf, 9 + 2 * i, y)).collect()
}

fn separator_colours(buf: &Buffer, y: u16) -> Vec<Color> {
    (0..4).map(|i| fg(buf, 10 + 2 * i, y)).collect()
}

#[test]
fn a_live_rows_counters_are_in_their_status_colours() {
    let t = tree(vec![context("cc", vec![lone("aa", Phase::Dispatch, [0, 1, 2, 3, 4, 5]), lone("bb", Phase::Plan, [2, 0, 0, 0, 0, 0])])]);
    let buf = render(&t, 3);
    assert_eq!(counters_text(&buf, 1), "1⋅2⋅3⋅4⋅5");
    assert_eq!(counter_colours(&buf, 1), [C15, C14, C11, C9, C10]);
    assert_eq!(separator_colours(&buf, 1), [C8; 4]);
    assert_eq!(counters_text(&buf, 2), "2⋅0⋅0⋅0⋅0");
    assert_eq!(counter_colours(&buf, 2), [C13, C8, C8, C8, C8], "the drafts of phase `plan` keep 13, the zeros are 8");
}

#[test]
fn a_live_rows_zero_counters_are_colour_8() {
    let t = tree(vec![context("cc", vec![lone("aa", Phase::Dispatch, [0, 0, 2, 0, 1, 0]), lone("bb", Phase::Dispatch, [0, 1, 0, 0, 0, 5])])]);
    let buf = render(&t, 3);
    assert_eq!(counters_text(&buf, 1), "0⋅2⋅0⋅1⋅0");
    assert_eq!(counter_colours(&buf, 1), [C8, C14, C8, C9, C8]);
    assert_eq!(counters_text(&buf, 2), "1⋅0⋅0⋅0⋅5");
    assert_eq!(counter_colours(&buf, 2), [C15, C8, C8, C8, C10]);
}

#[test]
fn the_failed_counter_is_red_whether_or_not_the_plan_has_a_job_stuck_item() {
    let plans = || vec![context("cc", vec![lone("aa", Phase::Dispatch, [0, 1, 0, 2, 2, 5]), lone("bb", Phase::Dispatch, [0, 1, 0, 2, 2, 5])])];
    let calm = tree(plans());
    let mut stuck = tree(plans());
    stuck.inbox = vec![cpd_core::inbox::model::Item { plan: Some(PlanId::parse("aa").unwrap()), ..super::inbox::item(1, Kind::JobStuck, Urgency::Red, "x", WHEN) }];
    for (label, t) in [("no item", &calm), ("job-stuck for aa", &stuck)] {
        let buf = render(t, 3);
        for y in [1, 2] {
            assert_eq!(fg(&buf, 9 + 2 * 3, y), C9, "{label}: row {y} failed counter");
        }
    }
}

#[test]
fn an_asleep_rows_counters_are_all_colour_8_and_its_dot_and_name_stay_colour_7() {
    let mut t = tree(vec![context(
        "cc",
        vec![
            lone("aa", Phase::Dispatch, [0, 1, 2, 3, 4, 5]),
            lone("bb", Phase::Dispatch, [0; 6]),
            lone("dd", Phase::Dispatch, [0, 1, 2, 3, 4, 5]),
        ],
    )]);
    t.contexts[0].plans[0].sessions[0].asleep_at = Some(WHEN.to_owned());
    t.contexts[0].plans[1].sessions[0].asleep_at = Some(WHEN.to_owned());
    let buf = render(&t, 4);
    for y in [1, 2] {
        assert_eq!(counter_colours(&buf, y), [C8; 5], "row {y}: every counter");
        assert_eq!(separator_colours(&buf, y), [C8; 4], "row {y}: the separators");
        assert_eq!((fg(&buf, 0, y), fg(&buf, 1, y)), (C8, C8), "row {y}: the branch");
        assert_eq!((fg(&buf, 2, y), fg(&buf, 4, y)), (C7, C7), "row {y}: the dot and the name");
    }
    assert_eq!(counter_colours(&buf, 3), [C15, C14, C11, C9, C10], "a live row keeps its status colours");
}

const PHASES: [Phase; 5] = [Phase::Grounding, Phase::Plan, Phase::Dispatch, Phase::Deploy, Phase::Done];

#[test]
fn drafts_replace_queued_as_the_first_counter_in_every_phase() {
    for phase in PHASES {
        let t = tree(vec![context("cc", vec![lone("aa", phase, [3, 1, 2, 0, 0, 5])])]);
        let buf = render(&t, 2);
        assert_eq!(counters_text(&buf, 1), "3⋅2⋅0⋅0⋅5", "phase {phase:?}");
    }
}

#[test]
fn the_draft_counter_is_magenta_in_every_phase() {
    for phase in PHASES {
        let t = tree(vec![context("cc", vec![lone("aa", phase, [3, 1, 2, 0, 0, 5])])]);
        let buf = render(&t, 2);
        assert_eq!(counter_colours(&buf, 1), [C13, C14, C8, C8, C10], "phase {phase:?}");
    }
}
