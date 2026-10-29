//! The rail's spacing (commands.md §17, contract §6.1.1, pro-d186): a Context block's rows end at column W-2 of the pane,
//! the pane's inner edge, the column W-1 being the blank margin.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use super::fixture::{context, plan, session, tree};
use super::view::View;
use crate::read::Place;
use cpd_core::plan::Phase;
use cpd_core::session::State;

const W: u16 = 40;

fn cells(buf: &Buffer, y: u16) -> Vec<String> {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_owned()).collect()
}

/// The column of the last cell of row `y` that is not blank.
fn last_column(buf: &Buffer, y: u16) -> usize {
    cells(buf, y).iter().rposition(|c| c.trim() != "").expect("a row with text")
}

fn draw(t: crate::read::Tree) -> Buffer {
    let mut view = View::new(t, true);
    let mut terminal = Terminal::new(TestBackend::new(W, 12)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn a_context_block_row_ends_at_the_panes_inner_edge() {
    // a Plan row of the Context block is the one that is cut (its name) and right-aligned (its counters)
    let long = plan(
        "a-very-long-plan-id-xxxx",
        "cc",
        Phase::Dispatch,
        [0, 36, 0, 0, 1, 5],
        session("l-long", "lead", State::Idle, None, Place::Parked),
        vec![],
    );
    let buf = draw(tree(vec![context("cc", vec![long])]));
    let y = (0..12).find(|&y| cells(&buf, y).iter().any(|c| c == "…")).expect("the Plan's name is cut with an ellipsis");
    assert_eq!(last_column(&buf, y), usize::from(W) - 2, "the row ends at column W-2: {:?}", cells(&buf, y));
    assert_eq!(buf[(W - 1, y)].symbol(), " ", "the margin column is blank");
}

#[test]
fn a_long_context_id_and_its_cost_end_at_the_panes_inner_edge() {
    let mut c = context(&"a".repeat(60), vec![]);
    c.cost_usd = Some(12.3);
    let buf = draw(tree(vec![c]));
    let y = (0..12).find(|&y| cells(&buf, y).iter().any(|c| c == "…")).expect("the id is cut with an ellipsis");
    let line: String = cells(&buf, y).concat();
    assert!(line.trim_end().ends_with(&format!(" {}", cpd_core::metrics::format_cost(12.3))), "{line}");
    assert_eq!(last_column(&buf, y), usize::from(W) - 2, "the row ends at column W-2: {line}");
    assert_eq!(buf[(W - 1, y)].symbol(), " ", "the margin column is blank");
}
