//! Cost counters on the tree's Context and Plan rows (pro-d179, pro-d186, commands.md §13.2 "Rows", F5): a Context row
//! draws its `cost_usd` one space after its id, a Plan row one space after its name with the Job counters right-aligned,
//! both as `format_cost` text in colour 8, the row ending at the area's last cell. Fork, Manager and Worker rows draw none.

use std::collections::HashSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use tui_tree_widget::TreeState;

use super::fixture::{WHEN, context, plan, session, tree};
use super::rows::{Scroll, draw, open_new};
use crate::palette::numbered::{C7, C8, C10, C11, C15};
use crate::palette::{self, IDLE, SPINNER};
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
    terminal.draw(|f| draw(f, f.area(), t, &mut state, false, 0, true, &mut Scroll::default())).unwrap();
    let full = terminal.backend().buffer();
    let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, width, height));
    for y in 0..height {
        for x in 0..width {
            buf[(x, y)] = full[(x, y + 1)].clone(); // row 0 is the ▲ line
        }
    }
    buf
}

/// A row of an area `width` wide: `left`, then `right`, which ends at the row's last cell.
fn row_of(left: &str, right: &str, width: usize) -> String {
    format!("{left:<w$}{right}", w = width - right.chars().count())
}

/// The width in cells of the cost text of `usd`.
fn k(usd: f64) -> usize {
    format_cost(usd).chars().count()
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
fn a_plan_row_draws_its_cost_one_space_after_its_name_and_its_counters_at_the_edge() {
    let mut p = plan("ship", "alpha", Phase::Dispatch, TWO_TWO_FIVE, session("l-ship", "ship lead", State::Working, None, Place::Parked), vec![]);
    p.cost_usd = Some(0.12);
    let buf = render(&with_plan(p), 44, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{WORKING} ship {}", format_cost(0.12)), "2⋅2⋅5", 44));
    let buf = render(&with_plan(ship(ONE, Some(1234.0))), 30, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship {}", format_cost(1234.0)), "1", 30));
}

#[test]
fn a_plan_row_with_no_counter_draws_its_cost_beside_its_name() {
    let buf = render(&with_plan(ship(NONE, Some(0.004))), 30, 3);
    assert_eq!(text(&buf, 1), format!("╰─{IDLE} ship {}", format_cost(0.004)));
    let buf = render(&with_plan(ship(NONE, Some(0.12))), 30, 3);
    assert_eq!(text(&buf, 1), format!("╰─{IDLE} ship {}", format_cost(0.12)));
}

#[test]
fn a_plan_row_with_no_cost_is_what_it_was() {
    let t = tree(vec![context("alpha", vec![ship(TWO_TWO_FIVE, None), {
        let mut p = plan("rel", "alpha", Phase::Dispatch, NONE, session("l-rel", "rel", State::Idle, None, Place::Parked), vec![]);
        p.cost_usd = Some(0.12);
        p
    }])]);
    let buf = render(&t, 44, 4);
    assert_eq!(text(&buf, 1), row_of(&format!("├─{IDLE} ship"), "2⋅2⋅5", 44), "no cost: the counters end at the last cell");
    assert_eq!(text(&buf, 2), format!("╰─{IDLE} rel {}", format_cost(0.12)), "its neighbour with a cost draws it after the name");
}

#[test]
fn a_context_row_draws_its_cost_one_space_after_its_id() {
    let mut c = context("alpha", vec![]);
    c.cost_usd = Some(12.3);
    let buf = render(&tree(vec![c]), 30, 2);
    assert_eq!(text(&buf, 0), format!("╭{IDLE} alpha {}", format_cost(12.3)));
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
    assert_eq!(beta, &format!("╭{IDLE} beta {}", format_cost(12.3)), "a priced Context draws it after the id: {rows:?}");
}

#[test]
fn a_cost_of_zero_draws_dollar_zero() {
    let zero = format_cost(0.0);
    assert_eq!(zero, "$0.00", "the shared formatter's zero");
    let buf = render(&with_plan(ship(NONE, Some(0.0))), 30, 3);
    assert_eq!(text(&buf, 1), format!("╰─{IDLE} ship {zero}"));
    let mut c = context("alpha", vec![]);
    c.cost_usd = Some(0.0);
    let buf = render(&tree(vec![c]), 30, 2);
    assert_eq!(text(&buf, 0), format!("╭{IDLE} alpha {zero}"));
}

#[test]
fn the_cost_text_is_the_shared_formatters() {
    for usd in [0.0, 0.004, 0.0099, 0.01, 0.12, 4.5, 999.99, 1000.0, 1234.0, 15_400.0] {
        let buf = render(&with_plan(ship(NONE, Some(usd))), 30, 3);
        assert_eq!(text(&buf, 1), format!("╰─{IDLE} ship {}", format_cost(usd)), "{usd}");
    }
}

#[test]
fn the_cost_is_colour_8_and_the_counters_keep_their_colours() {
    let mut c = context("alpha", vec![ship(TWO_TWO_FIVE, Some(0.12))]);
    c.cost_usd = Some(12.3);
    let buf = render(&tree(vec![c]), 44, 3);
    // Plan row: the name `ship` is at 4..8, the space at 8, the cost from 9; the counters "2⋅2⋅5" end at column 43
    assert_eq!([39, 41, 43].map(|x| fg(&buf, x, 1)), [C15, C11, C10], "the counters keep their colours");
    let cost = 9..9 + u16::try_from(k(0.12)).unwrap();
    assert_eq!(cost.clone().map(|x| fg(&buf, x, 1)).collect::<Vec<_>>(), vec![C8; k(0.12)], "the cost is muted");
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship {}", format_cost(0.12)), "2⋅2⋅5", 44));
    // Context row: the id `alpha` is at 3..8, the space at 8, the cost from 9; not bold
    let cost = 9..9 + u16::try_from(k(12.3)).unwrap();
    assert_eq!(cost.clone().map(|x| fg(&buf, x, 0)).collect::<Vec<_>>(), vec![C8; k(12.3)], "the Context's cost is muted");
    assert!(cost.clone().all(|x| !buf[(x, 0)].modifier.contains(Modifier::BOLD)), "and not bold");
    assert!((3..8).all(|x| buf[(x, 0)].modifier.contains(Modifier::BOLD)), "the id is");
    assert_eq!(text(&buf, 0), format!("╭{IDLE} alpha {}", format_cost(12.3)));
}

#[test]
fn a_long_plan_name_is_cut_before_its_cost_and_counters() {
    let mut p = plan(
        "abcdefghijklmnopqrstuvwx",
        "alpha",
        Phase::Dispatch,
        TWO_TWO_FIVE,
        session("l-long", "lead", State::Idle, None, Place::Parked),
        vec![],
    );
    p.cost_usd = Some(0.12);
    let (width, c) = (30, "2⋅2⋅5".chars().count());
    let name: String = "abcdefghijklmnopqrstuvwx".chars().take(width - 6 - (1 + k(0.12)) - (c + 1) - 1).chain(['…']).collect();
    assert_eq!(name.chars().count(), width - 6 - (1 + k(0.12)) - (c + 1));
    let buf = render(&with_plan(p), 30, 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} {name} {}", format_cost(0.12)), "2⋅2⋅5", 30), "3 blank cells before the counters");
}

#[test]
fn a_long_context_id_is_cut_before_its_cost() {
    let id = "a".repeat(30);
    let mut c = context(&id, vec![]);
    c.cost_usd = Some(12.3);
    let width = 30;
    let kept = width - 4 - k(12.3);
    let cut = format!("{}…", "a".repeat(kept - 1));
    let buf = render(&tree(vec![c]), 30, 2);
    let line = text(&buf, 0);
    assert_eq!(line, format!("╭{IDLE} {cut} {}", format_cost(12.3)));
    assert_eq!(line.chars().count(), width, "the row fills the area exactly");
}

#[test]
fn a_narrow_plan_row_drops_the_cost_and_keeps_the_counters() {
    let (kc, c) = (k(0.12), 1);
    let t = with_plan(ship(ONE, Some(0.12)));
    // the name keeps width - 6 - (1 + k) - (c + 1) >= 1 cells: the pair is drawn from width 9 + k + c
    let fits = 9 + kc + c;
    let buf = render(&t, u16::try_from(fits).unwrap(), 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} … {}", format_cost(0.12)), "1", fits));
    let below = fits - 1;
    let buf = render(&t, u16::try_from(below).unwrap(), 3);
    assert_eq!(text(&buf, 1), row_of(&format!("╰─{IDLE} ship"), "1", below), "the cost is dropped first");
    // the counters alone are drawn from width c + 5
    let alone = c + 5;
    let line = text(&render(&t, u16::try_from(alone).unwrap(), 3), 1);
    assert!(line.ends_with(" 1") && !line.contains('$'), "{line}");
    let line = text(&render(&t, u16::try_from(alone - 1).unwrap(), 3), 1);
    assert!(!line.contains('$') && !line.contains('1'), "neither: {line}");
}

#[test]
fn a_narrow_row_drops_a_lone_cost_that_leaves_no_room() {
    let kc = k(0.12);
    let cost = format_cost(0.12);
    // a Plan: the name keeps width - 6 - (1 + k) >= 1 cells
    let t = with_plan(ship(NONE, Some(0.12)));
    let plan_fits = 8 + kc;
    assert_eq!(text(&render(&t, u16::try_from(plan_fits).unwrap(), 3), 1), format!("╰─{IDLE} … {cost}"));
    let line = text(&render(&t, u16::try_from(plan_fits - 1).unwrap(), 3), 1);
    assert!(!line.contains('$') && !line.contains('¢'), "{line}");
    // a Context: the id keeps width - 4 - k >= 1 cells
    let mut c = context("alpha", vec![]);
    c.cost_usd = Some(0.12);
    let ctx_fits = kc + 5;
    assert_eq!(text(&render(&tree(vec![c.clone()]), u16::try_from(ctx_fits).unwrap(), 2), 0), format!("╭{IDLE} … {cost}"));
    let line = text(&render(&tree(vec![c]), u16::try_from(ctx_fits - 1).unwrap(), 2), 0);
    assert!(!line.contains('$') && !line.contains('¢'), "{line}");
}

#[test]
fn an_archived_row_keeps_its_cost_in_colour_8() {
    let mut p = ship(ONE, Some(0.12));
    p.head.archived_at = Some(WHEN.into());
    p.sessions[0].archived_at = Some(WHEN.into());
    let buf = render(&with_plan(p), 44, 6);
    let y = (0..6).find(|&y| text(&buf, y).contains("ship")).expect("the archived Plan's row shows");
    let line = text(&buf, y);
    assert!(line.contains(&format!("ship {}", format_cost(0.12))) && line.ends_with(" 1") && line.chars().count() == 44, "{line}");
    let cost = 9..9 + u16::try_from(k(0.12)).unwrap();
    assert_eq!(cost.map(|x| fg(&buf, x, y)).collect::<Vec<_>>(), vec![C8; k(0.12)], "the cost, colour 8, beside the name");
}

#[test]
fn an_asleep_row_keeps_its_cost_in_colour_8_and_the_rest_in_7() {
    let mut p = ship(ONE, Some(0.12));
    p.sessions[0].asleep_at = Some(WHEN.into());
    let buf = render(&with_plan(p), 44, 3);
    let line = text(&buf, 1);
    assert!(line.ends_with(" 1") && line.chars().count() == 44, "the counter at the last cell: {line}");
    assert_eq!(fg(&buf, 2, 1), C7, "the dot's mark is 7");
    assert_eq!(fg(&buf, 43, 1), C8, "the counter stays 8");
    let cost = 9..9 + u16::try_from(k(0.12)).unwrap();
    assert_eq!(cost.map(|x| fg(&buf, x, 1)).collect::<Vec<_>>(), vec![C8; k(0.12)], "the cost stays 8, not restyled to 7, beside the name");
    assert!(line.contains(&format!("ship {}", format_cost(0.12))), "{line}");
}

#[test]
fn a_selected_plan_row_never_draws_its_cost_blue() {
    let mut p = plan("ship", "alpha", Phase::Dispatch, NONE, session("l-ship", "ship lead", State::Idle, None, Place::Chat), vec![]);
    p.cost_usd = Some(0.12);
    let buf = render(&with_plan(p), 30, 3);
    let blue = palette::selected_name().fg.unwrap();
    assert_eq!(text(&buf, 1), format!("╰─{IDLE} ship {}", format_cost(0.12)));
    assert!((4..8).all(|x| fg(&buf, x, 1) == blue), "the name is the selected blue");
    let cost = 9..9 + u16::try_from(k(0.12)).unwrap();
    assert_eq!(cost.clone().map(|x| fg(&buf, x, 1)).collect::<Vec<_>>(), vec![C8; k(0.12)], "the cost directly after the name is 8");
    assert!((0..30).filter(|x| !(4..8).contains(x)).all(|x| fg(&buf, x, 1) != blue), "no other cell of the row is blue");
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
    assert!(rows.iter().any(|r| r.contains("ship $0.12")), "{rows:?}");
    assert!(!rows.iter().any(|r| r.contains("$5.00") || r.contains("$3.00") || r.contains("$7.00")), "{rows:?}");
}
