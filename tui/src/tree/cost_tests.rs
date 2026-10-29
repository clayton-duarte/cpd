//! Cost counters on the tree's Context and Plan rows (pro-d179, commands.md §13.2 "Rows", F5): a Context row draws its
//! `cost_usd` alone, a Plan row draws `<counters> <cost>`, both as `format_cost` text in colour 8, ending `RIGHT_PAD`
//! columns before the row's edge. Fork, Manager and Worker rows draw none.

use std::collections::HashSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use tui_tree_widget::TreeState;

use super::fixture::{WHEN, context, plan, session, tree};
use super::rows::{Scroll, draw, open_new};
use crate::box_widget::spacing::RIGHT_PAD;
use crate::palette::numbered::{C7, C8, C10, C11, C15};
use crate::palette::{IDLE, SPINNER};
use crate::read::{Place, PlanNode, Tree};
use crate::test_text::row as text;
use cpd_core::metrics::format_cost;
use cpd_core::plan::Phase;
use cpd_core::session::State;

const WORKING: &str = SPINNER[0];

fn render(t: &Tree, width: u16, height: u16) -> Buffer {
    let mut state: TreeState<String> = TreeState::default();
    open_new(&mut state, &mut HashSet::new(), t);
    state.select(vec![]);
    let mut terminal = Terminal::new(TestBackend::new(width, height + 40)).unwrap();
    terminal.draw(|f| draw(f, f.area(), t, &mut state, false, 0, &mut Scroll::default())).unwrap();
    let full = terminal.backend().buffer();
    let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, width, height));
    for y in 0..height {
        for x in 0..width {
            buf[(x, y)] = full[(x, y + 1)].clone(); // row 0 is the ▲ line
        }
    }
    buf
}

/// A row of an area `width` wide: `left`, then `right`, which ends `RIGHT_PAD` columns before the right edge.
fn row_of(left: &str, right: &str, width: usize) -> String {
    format!("{left:<w$}{right}", w = width - usize::from(RIGHT_PAD) - right.chars().count())
}

fn fg(buf: &Buffer, x: u16, y: u16) -> Color {
    buf[(x, y)].fg
}

/// A Plan `ship` with its Lead idle; `counts` are `[draft, queued, running, merging, failed, merged]`.
fn ship(counts: [usize; 6], cost: Option<f64>) -> PlanNode {
    let mut p = plan("ship", "alpha", Phase::Dispatch, counts, session("l-ship", "ship lead", State::Idle, None, Place::Parked), vec![]);
    p.cost_usd = cost;
    p
}

/// `alpha` with the one Plan `p`: rows are 0 the Context, 1 the Plan, 2 the closing row.
fn with_plan(p: PlanNode) -> Tree {
    tree(vec![context("alpha", vec![p])])
}

const NONE: [usize; 6] = [0; 6];
const ONE: [usize; 6] = [0, 1, 0, 0, 0, 0];
const TWO_TWO_FIVE: [usize; 6] = [0, 2, 1, 1, 0, 5];

#[test]
fn a_plan_row_draws_its_counters_one_space_and_its_cost() {
    let mut p = plan("ship", "alpha", Phase::Dispatch, TWO_TWO_FIVE, session("l-ship", "ship lead", State::Working, None, Place::Parked), vec![]);
    p.cost_usd = Some(0.12);
    let buf = render(&with_plan(p), 44, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{WORKING} ship"), "2⋅2⋅5 $0.12", 44));
    let mut big = ship(ONE, Some(1234.0));
    big.cost_usd = Some(1234.0);
    let buf = render(&with_plan(big), 30, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship"), "1 $1.2k", 30));
}

#[test]
fn a_plan_row_with_no_counter_draws_its_cost_alone() {
    let buf = render(&with_plan(ship(NONE, Some(0.004))), 30, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship"), "<1¢", 30));
    let buf = render(&with_plan(ship(NONE, Some(0.12))), 30, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship"), "$0.12", 30));
}

#[test]
fn a_plan_row_with_no_cost_is_what_it_was() {
    let t = tree(vec![context("alpha", vec![ship(TWO_TWO_FIVE, None), {
        let mut p = plan("rel", "alpha", Phase::Dispatch, NONE, session("l-rel", "rel", State::Idle, None, Place::Parked), vec![]);
        p.cost_usd = Some(0.12);
        p
    }])]);
    let buf = render(&t, 44, 4);
    assert_eq!(text(&buf, 1), row_of(&format!("├─{IDLE} ship"), "2⋅2⋅5", 44), "no cost: the counters alone, no space after");
    assert_eq!(text(&buf, 2), row_of(&format!("╰─{IDLE} rel"), "$0.12", 44), "its neighbour with a cost draws it");
}

#[test]
fn a_context_row_draws_its_cost_at_the_right() {
    let mut c = context("alpha", vec![]);
    c.cost_usd = Some(12.3);
    let buf = render(&tree(vec![c]), 30, 2);
    assert_eq!(text(&buf, 0), row_of(&format!("╭{IDLE} alpha"), "$12.30", 30));
}

#[test]
fn a_context_row_with_no_cost_is_what_it_was() {
    let mut priced = context("beta", vec![]);
    priced.cost_usd = Some(12.3);
    let t = tree(vec![context("alpha", vec![]), priced]);
    let buf = render(&t, 30, 4);
    let rows: Vec<String> = (0..4).map(|y| text(&buf, y)).collect();
    let alpha = rows.iter().find(|r| r.contains("alpha")).unwrap();
    assert_eq!(alpha, &format!("╭{IDLE} alpha"), "no cost: the title alone");
    let beta = rows.iter().find(|r| r.contains("beta")).unwrap();
    assert_eq!(beta, &row_of(&format!("╭{IDLE} beta"), "$12.30", 30), "a priced Context draws it: {rows:?}");
}

#[test]
fn a_cost_of_zero_draws_dollar_zero() {
    let buf = render(&with_plan(ship(NONE, Some(0.0))), 30, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship"), "$0.00", 30));
    let mut c = context("alpha", vec![]);
    c.cost_usd = Some(0.0);
    let buf = render(&tree(vec![c]), 30, 2);
    assert_eq!(text(&buf, 0), row_of(&format!("╭{IDLE} alpha"), "$0.00", 30));
}

#[test]
fn the_cost_text_is_the_shared_formatters() {
    for usd in [0.0, 0.004, 0.0099, 0.01, 0.12, 4.5, 999.99, 1000.0, 1234.0, 15_400.0] {
        let buf = render(&with_plan(ship(NONE, Some(usd))), 30, 3);
        assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship"), &format_cost(usd), 30), "{usd}");
    }
}

#[test]
fn the_cost_is_colour_8_and_the_counters_keep_their_colours() {
    let mut p = ship(TWO_TWO_FIVE, Some(0.12));
    p.cost_usd = Some(0.12);
    let mut c = context("alpha", vec![p]);
    c.cost_usd = Some(12.3);
    let buf = render(&tree(vec![c]), 44, 3);
    // Plan row: "2⋅2⋅5 $0.12" ends at column 42: counters at 31, 33, 35, the space at 36, the cost 37..42
    assert_eq!([31, 33, 35].map(|x| fg(&buf, x, 1)), [C15, C11, C10], "the counters keep their colours");
    assert_eq!((37..42).map(|x| fg(&buf, x, 1)).collect::<Vec<_>>(), [C8; 5], "the cost is muted");
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship"), "2⋅2⋅5 $0.12", 44));
    // Context row: "$12.30" ends at column 42, from 36; not bold
    assert_eq!((36..42).map(|x| fg(&buf, x, 0)).collect::<Vec<_>>(), [C8; 6], "the Context's cost is muted");
    assert!((36..42).all(|x| !buf[(x, 0)].modifier.contains(Modifier::BOLD)), "and not bold");
    assert!(buf[(2, 0)].modifier.contains(Modifier::BOLD), "the title is");
    assert_eq!(text(&buf, 0), row_of(&format!("╭{IDLE} alpha"), "$12.30", 44));
}

#[test]
fn a_long_plan_name_is_cut_before_its_counters_and_cost() {
    let mut p = plan(
        "abcdefghijklmnopqrstuvwx",
        "alpha",
        Phase::Dispatch,
        TWO_TWO_FIVE,
        session("l-long", "lead", State::Idle, None, Place::Parked),
        vec![],
    );
    p.cost_usd = Some(0.12);
    let buf = render(&with_plan(p), 30, 3);
    let line = text(&buf, 1);
    assert!(line.ends_with("2⋅2⋅5 $0.12"), "{line}");
    assert!(line.contains('…'), "the name is cut: {line}");
    // the right text is 11 wide and ends at column 28: it starts at 17; the name ends before, one blank cell between
    assert_eq!(buf[(16, 1)].symbol(), " ", "a blank between the name and the counters: {line}");
    assert!(line.chars().take(16).any(|c| c == '…'), "{line}");
}

#[test]
fn a_long_context_id_is_cut_before_its_cost() {
    let id = "a".repeat(30);
    let mut c = context(&id, vec![]);
    c.cost_usd = Some(12.3);
    let buf = render(&tree(vec![c]), 30, 2);
    let line = text(&buf, 0);
    assert!(line.ends_with("$12.30"), "{line}");
    // the cost is 6 wide and ends at column 28: it starts at 22; the id's `…` ends before, one blank cell between
    assert_eq!(buf[(21, 0)].symbol(), " ", "a blank between the id and the cost: {line}");
    assert_eq!(buf[(20, 0)].symbol(), "…", "the id is cut with …: {line}");
}

#[test]
fn a_narrow_plan_row_drops_the_cost_and_keeps_the_counters() {
    // counters "1" (1 wide) + space + "$0.12" (5) = 7; with MIN_LEFT = 5 they fit when width >= 12, a pane of 14 (RIGHT_PAD 2)
    let fits = 7 + 5 + usize::from(RIGHT_PAD);
    let t = with_plan(ship(ONE, Some(0.12)));
    let buf = render(&t, u16::try_from(fits).unwrap(), 3);
    assert!(text(&buf, 1).ends_with("1 $0.12"), "{}", text(&buf, 1));
    let buf = render(&t, u16::try_from(fits - 1).unwrap(), 3);
    let line = text(&buf, 1);
    assert!(!line.contains('$'), "the cost is dropped first: {line}");
    assert!(line.ends_with(" 1"), "the counters stay: {line}");
    // the counters alone fit when width >= 6, a pane of 8; one column below draws neither
    let alone = 1 + 5 + usize::from(RIGHT_PAD);
    let buf = render(&t, u16::try_from(alone).unwrap(), 3);
    assert!(text(&buf, 1).ends_with(" 1"), "{}", text(&buf, 1));
    assert!(!text(&buf, 1).contains('$'));
    let buf = render(&t, u16::try_from(alone - 1).unwrap(), 3);
    let line = text(&buf, 1);
    assert!(!line.contains('$') && !line.contains('1'), "neither: {line}");
    // and, to be red until it is drawn, the full pair at its threshold is not a hand-edited expectation
    assert!(text(&render(&t, 44, 3), 1).ends_with("1 $0.12"));
}

#[test]
fn a_narrow_row_drops_a_lone_cost_that_leaves_no_room() {
    // "$0.12" (5) + MIN_LEFT (5) <= width: a pane of 12 draws it, 11 does not
    let t = with_plan(ship(NONE, Some(0.12)));
    let line = text(&render(&t, 12, 3), 1);
    assert!(line.ends_with("$0.12"), "{line}");
    let line = text(&render(&t, 11, 3), 1);
    assert!(!line.contains('$'), "{line}");
    let mut c = context("alpha", vec![]);
    c.cost_usd = Some(0.12);
    assert!(text(&render(&tree(vec![c.clone()]), 12, 2), 0).ends_with("$0.12"));
    assert!(!text(&render(&tree(vec![c]), 11, 2), 0).contains('$'));
}

#[test]
fn an_archived_row_keeps_its_cost_in_colour_8() {
    let mut p = ship(ONE, Some(0.12));
    p.head.archived_at = Some(WHEN.into());
    p.sessions[0].archived_at = Some(WHEN.into());
    let buf = render(&with_plan(p), 44, 6);
    let y = (0..6).find(|&y| text(&buf, y).contains("ship")).expect("the archived Plan's row shows");
    let line = text(&buf, y);
    assert!(line.ends_with("1 $0.12"), "{line}");
    assert_eq!((37..42).map(|x| fg(&buf, x, y)).collect::<Vec<_>>(), [C8; 5], "the cost, colour 8");
}

#[test]
fn an_asleep_row_keeps_its_cost_in_colour_8_and_the_rest_in_7() {
    let mut p = ship(ONE, Some(0.12));
    p.sessions[0].asleep_at = Some(WHEN.into());
    let buf = render(&with_plan(p), 44, 3);
    let line = text(&buf, 1);
    assert!(line.ends_with("1 $0.12"), "{line}");
    assert_eq!(fg(&buf, 2, 1), C7, "the dot's mark is 7");
    assert_eq!(fg(&buf, 35, 1), C8, "the counter stays 8");
    assert_eq!((37..42).map(|x| fg(&buf, x, 1)).collect::<Vec<_>>(), [C8; 5], "the cost stays 8, not restyled to 7");
}

#[test]
fn fork_and_manager_rows_draw_no_cost() {
    let mut fork = session("f-one", "ship fork", State::Idle, None, Place::Parked);
    fork.cost_usd = Some(5.0);
    let mut p = plan("ship", "alpha", Phase::Dispatch, ONE, session("l-ship", "ship lead", State::Idle, None, Place::Parked), vec![fork]);
    p.cost_usd = Some(0.12);
    p.sessions[0].cost_usd = Some(3.0);
    let mut manager = session("m-old", "old manager", State::Stopped, None, Place::None);
    manager.cost_usd = Some(7.0);
    manager.archived_at = Some(WHEN.into());
    let mut c = context("alpha", vec![p]);
    c.archived_managers = vec![manager];
    let buf = render(&tree(vec![c]), 44, 6);
    let rows: Vec<String> = (0..6).map(|y| text(&buf, y)).collect();
    let costs = rows.iter().filter(|r| r.contains('$')).count();
    assert_eq!(costs, 1, "only the Plan's row draws a cost: {rows:?}");
    assert!(rows.iter().any(|r| r.contains("1 $0.12")), "{rows:?}");
    assert!(!rows.iter().any(|r| r.contains("$5.00") || r.contains("$3.00") || r.contains("$7.00")), "{rows:?}");
}
