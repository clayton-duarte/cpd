//! The cost toggle of the tree view (pro-d189, pro-d196, commands.md §13.2 "Cost counters" and the `$` key): by default no
//! Context or Plan row draws a cost; `$` shows them and a second `$` hides them; the flag is the view's, so a new view is off.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Modifier;

use super::fixture::{context, plan, session, tree};
use super::view::{Effect, View};
use crate::palette::IDLE;
use crate::read::{Place, Tree};
use cpd_core::metrics::format_cost;
use cpd_core::plan::Phase;
use cpd_core::session::State;

/// The width of the pane drawn; the row area is 2 cells narrower (one blank margin column on each side).
const W: u16 = 40;
/// The width of the pane of the cut test, narrow enough to cut a 23-letter id with or without the cost.
const NARROW: u16 = 30;
const COUNTERS: &str = "2⋅2⋅5";

/// A Context `alpha` costing 45.6 with one Plan `ship` costing 12.3 (no Job counters).
fn priced() -> Tree {
    named("ship", [0; 6])
}

/// The same, the Plan named `id` with `counts` (`[draft, queued, running, merging, failed, merged]`).
fn named(id: &str, counts: [usize; 6]) -> Tree {
    let mut p = plan(id, "alpha", Phase::Dispatch, counts, session("l-ship", "lead", State::Idle, None, Place::Parked), vec![]);
    p.cost_usd = Some(12.3);
    let mut c = context("alpha", vec![p]);
    c.cost_usd = Some(45.6);
    tree(vec![c])
}

fn press(view: &mut View, c: char) -> Effect {
    view.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

fn draw(view: &mut View) -> Buffer {
    draw_in(view, W)
}

fn draw_in(view: &mut View, width: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    terminal.backend().buffer().clone()
}

/// The drawn rows, trailing blanks trimmed.
fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()).collect()
}

/// The row that holds `text`, from the first column of the row area.
fn row(buf: &Buffer, text: &str) -> String {
    let all = rows(buf);
    let found = all.iter().find(|r| r.contains(text)).unwrap_or_else(|| panic!("no row with {text:?}: {all:?}"));
    found.chars().skip(1).collect()
}

fn has_cost(buf: &Buffer) -> bool {
    rows(buf).iter().any(|r| r.contains('$'))
}

#[test]
fn the_tree_draws_no_cost_by_default() {
    let mut view = View::new(priced(), true);
    let buf = draw(&mut view);
    assert!(!has_cost(&buf), "no cost on any row: {:?}", rows(&buf));
}

#[test]
fn the_dollar_key_shows_the_costs_and_a_second_press_hides_them() {
    let mut view = View::new(priced(), true);
    assert_eq!(press(&mut view, '$'), Effect::None);
    let buf = draw(&mut view);
    assert!(row(&buf, "alpha").contains(&format!("alpha {}", format_cost(45.6))), "the Context's cost: {:?}", rows(&buf));
    assert!(row(&buf, "ship").contains(&format!("ship {}", format_cost(12.3))), "the Plan's cost: {:?}", rows(&buf));
    assert_eq!(press(&mut view, '$'), Effect::None);
    let buf = draw(&mut view);
    assert!(!has_cost(&buf), "a second press hides them: {:?}", rows(&buf));
}

#[test]
fn the_dollar_key_needs_no_cursor_and_does_not_show_it() {
    let mut view = View::new(priced(), false); // not focused, no cursor
    let reversed = |buf: &Buffer| buf.content().iter().any(|c| c.modifier.contains(Modifier::REVERSED));
    let before = draw(&mut view);
    assert!(!reversed(&before) && !has_cost(&before), "no cursor and no cost before the key: {:?}", rows(&before));
    assert_eq!(press(&mut view, '$'), Effect::None);
    let buf = draw(&mut view);
    assert!(has_cost(&buf), "the key toggled with no cursor: {:?}", rows(&buf));
    assert!(!reversed(&buf), "and the cursor stays hidden: no reversed cell");
}

#[test]
fn a_new_view_starts_with_the_costs_off() {
    let mut view = View::new(priced(), true);
    press(&mut view, '$');
    assert!(has_cost(&draw(&mut view)), "on");
    let mut again = View::new(priced(), true);
    let buf = draw(&mut again);
    assert!(!has_cost(&buf), "a new view is off again: {:?}", rows(&buf));
}

#[test]
fn a_long_plan_name_takes_the_room_of_the_hidden_cost() {
    let id = "abcdefghijklmnopqrstuvw";
    let area = usize::from(NARROW) - 2;
    let counts = [0, 2, 1, 1, 0, 5]; // draws as COUNTERS
    let c = COUNTERS.chars().count();
    let k = format_cost(12.3).chars().count();
    // §13.2: the name is cut with `…` to `width - 6 - R` cells, R = (1 + k when the cost shows) + (c + 1)
    let cut = |keep: usize| -> String { id.chars().take(keep - 1).chain(['…']).collect() };
    let row_of = |left: String| format!("{left:<w$}{COUNTERS}", w = area - c);

    let mut view = View::new(named(id, counts), true);
    let off = cut(area - 6 - (c + 1));
    let buf = draw_in(&mut view, NARROW);
    assert_eq!(row(&buf, "abc").trim_end(), row_of(format!("╰─{IDLE} {off}")), "costs off: cut by the counters only");

    press(&mut view, '$');
    let on = cut(area - 6 - (1 + k) - (c + 1));
    let buf = draw_in(&mut view, NARROW);
    assert_eq!(row(&buf, "abc").trim_end(), row_of(format!("╰─{IDLE} {on} {}", format_cost(12.3))), "costs on: cut before the cost too");
    assert_eq!(off.chars().count(), on.chars().count() + 1 + k, "off shows the room of the hidden cost more");
}
