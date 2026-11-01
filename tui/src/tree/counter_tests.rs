//! The four Job counters of the Context tree (hs-d636, commands.md §13.2, contract §6.1.1): a live Lead's row draws
//! `planned⋅active⋅failed⋅done` in the roles' colours, only the nonzero roles; on an asleep Lead's row every counter
//! (and separator) is colour 8.

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
use cpd_core::ids::PlanId;
use cpd_core::inbox::model::{Kind, Urgency};
use cpd_core::plan::Phase;
use cpd_core::session::State;

const WIDTH: u16 = 20;

fn lone(id: &str, phase: Phase, counts: [usize; 6]) -> PlanNode {
    plan(
        id,
        "cc",
        phase,
        counts,
        session(&format!("s-{id}"), "x", State::Idle, None, Place::Parked),
        vec![],
    )
}

fn render(t: &Tree, height: u16) -> Buffer {
    let mut state: TreeState<String> = TreeState::default();
    open_new(&mut state, &mut HashSet::new(), t);
    state.select(vec![]);
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, height + 40)).unwrap();
    terminal
        .draw(|f| {
            draw(
                f,
                f.area(),
                t,
                &mut state,
                false,
                0,
                false,
                &mut Scroll::default(),
            )
        })
        .unwrap();
    let full = terminal.backend().buffer();
    let mut buf = Buffer::empty(Rect::new(0, 0, WIDTH, height));
    for y in 0..height {
        for x in 0..WIDTH {
            buf[(x, y)] = full[(x, y + 1)].clone(); // row 0 is the ▲ line
        }
    }
    buf
}

fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
    buf[(x, y)].fg
}

/// The drawn counter string of row `y`, read from column 9 to the edge, trimmed of surrounding spaces.
fn counters_text(buf: &Buffer, y: u16) -> String {
    let text: String = (9..WIDTH).map(|x| buf[(x, y)].symbol()).collect();
    text.trim().to_owned()
}

/// The fg colour of each drawn counter digit cell of row `y` (spaces and `⋅` skipped), in order.
fn counter_colours(buf: &Buffer, y: u16) -> Vec<Color> {
    (9..WIDTH)
        .filter_map(|x| {
            let cell = &buf[(x, y)];
            (!matches!(cell.symbol(), " " | "⋅")).then_some(cell.fg)
        })
        .collect()
}

/// The fg colour of each `⋅` of row `y`, in order.
fn separator_colours(buf: &Buffer, y: u16) -> Vec<Color> {
    (9..WIDTH)
        .filter_map(|x| (buf[(x, y)].symbol() == "⋅").then_some(buf[(x, y)].fg))
        .collect()
}

#[test]
fn the_planned_role_is_the_drafts_plus_queued() {
    let t = tree(vec![context(
        "cc",
        vec![
            lone("aa", Phase::Dispatch, [3, 7, 0, 0, 0, 0]),
            lone("bb", Phase::Dispatch, [3, 0, 0, 0, 0, 0]),
            lone("dd", Phase::Dispatch, [0, 7, 0, 0, 0, 0]),
        ],
    )]);
    let buf = render(&t, 4);
    assert_eq!(
        counters_text(&buf, 1),
        "10",
        "3 drafts and 7 queued show 10"
    );
    assert_eq!(counters_text(&buf, 2), "3");
    assert_eq!(
        counter_colours(&buf, 2),
        [C13],
        "a planned role with a draft is magenta 13"
    );
    assert_eq!(counters_text(&buf, 3), "7");
    assert_eq!(
        counter_colours(&buf, 3),
        [C15],
        "a planned role with no draft is the queued colour 15"
    );
}

#[test]
fn the_active_role_is_running_plus_merging() {
    let t = tree(vec![context(
        "cc",
        vec![
            lone("aa", Phase::Dispatch, [0, 0, 2, 1, 0, 0]),
            lone("bb", Phase::Dispatch, [0, 0, 2, 0, 0, 0]),
        ],
    )]);
    let buf = render(&t, 3);
    assert_eq!(
        counters_text(&buf, 1),
        "3",
        "2 running and 1 merging show 3"
    );
    assert_eq!(
        counter_colours(&buf, 1),
        [C11],
        "an active role with a landing Job is yellow 11"
    );
    assert_eq!(counters_text(&buf, 2), "2");
    assert_eq!(
        counter_colours(&buf, 2),
        [C14],
        "an active role with no landing Job is cyan 14"
    );
}

#[test]
fn the_failed_and_done_roles_are_red_and_green() {
    let t = tree(vec![context(
        "cc",
        vec![
            lone("aa", Phase::Dispatch, [0, 0, 0, 0, 4, 0]),
            lone("bb", Phase::Dispatch, [0, 0, 0, 0, 0, 5]),
        ],
    )]);
    let buf = render(&t, 3);
    assert_eq!(counters_text(&buf, 1), "4");
    assert_eq!(counter_colours(&buf, 1), [C9], "failed is red 9");
    assert_eq!(counters_text(&buf, 2), "5");
    assert_eq!(counter_colours(&buf, 2), [C10], "done is green 10");
}

#[test]
fn the_four_roles_draw_in_order_with_separators_only_between_them() {
    let t = tree(vec![context(
        "cc",
        vec![
            lone("aa", Phase::Dispatch, [1, 0, 1, 0, 1, 1]),
            lone("bb", Phase::Dispatch, [0, 2, 1, 1, 3, 4]),
            lone("dd", Phase::Dispatch, [0, 0, 0, 0, 0, 5]),
        ],
    )]);
    let buf = render(&t, 4);
    assert_eq!(
        counters_text(&buf, 1),
        "1⋅1⋅1⋅1",
        "planned⋅active⋅failed⋅done"
    );
    assert_eq!(counter_colours(&buf, 1), [C13, C14, C9, C10]);
    assert_eq!(separator_colours(&buf, 1), [C8, C8, C8]);
    assert_eq!(counters_text(&buf, 2), "2⋅2⋅3⋅4");
    assert_eq!(counter_colours(&buf, 2), [C15, C11, C9, C10]);
    assert_eq!(counters_text(&buf, 3), "5");
    assert_eq!(counter_colours(&buf, 3), [C10]);
    assert_eq!(
        separator_colours(&buf, 3),
        [],
        "only one role: no separator"
    );
}

#[test]
fn a_plan_with_no_nonzero_role_draws_no_counters() {
    let t = tree(vec![context(
        "cc",
        vec![lone("aa", Phase::Dispatch, [0, 0, 0, 0, 0, 0])],
    )]);
    let buf = render(&t, 2);
    assert_eq!(counters_text(&buf, 1), "", "every role zero: nothing drawn");
    assert_eq!(counter_colours(&buf, 1), []);
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
    assert_eq!(counters_text(&buf, 1), "1⋅5⋅4⋅5");
    assert_eq!(counter_colours(&buf, 1), [C8; 4], "row 1: every counter");
    assert_eq!(separator_colours(&buf, 1), [C8; 3], "row 1: the separators");
    assert_eq!(
        (fg(&buf, 0, 1), fg(&buf, 1, 1)),
        (C8, C8),
        "row 1: the branch"
    );
    assert_eq!(
        (fg(&buf, 2, 1), fg(&buf, 4, 1)),
        (C7, C7),
        "row 1: the dot and the name"
    );
    assert_eq!(
        counter_colours(&buf, 2),
        [],
        "row 2: an asleep all-zero Plan draws no counters"
    );
    assert_eq!(separator_colours(&buf, 2), [], "row 2: no separators");
    assert_eq!(
        counter_colours(&buf, 3),
        [C15, C11, C9, C10],
        "a live row keeps its status colours"
    );
}

#[test]
fn the_failed_counter_is_red_whether_or_not_the_plan_has_a_job_stuck_item() {
    let plans = || {
        vec![context(
            "cc",
            vec![
                lone("aa", Phase::Dispatch, [0, 1, 0, 2, 2, 5]),
                lone("bb", Phase::Dispatch, [0, 1, 0, 2, 2, 5]),
            ],
        )]
    };
    let calm = tree(plans());
    let mut stuck = tree(plans());
    stuck.inbox = vec![cpd_core::inbox::model::Item {
        plan: Some(PlanId::parse("aa").unwrap()),
        ..crate::widgets::inbox::item(1, Kind::JobStuck, Urgency::Red, "x", WHEN)
    }];
    for (label, t) in [("no item", &calm), ("job-stuck for aa", &stuck)] {
        let buf = render(t, 3);
        for y in [1, 2] {
            assert_eq!(counters_text(&buf, y), "1⋅2⋅2⋅5", "{label}: row {y}");
            assert_eq!(
                counter_colours(&buf, y)[2],
                C9,
                "{label}: row {y} failed counter"
            );
        }
    }
}
