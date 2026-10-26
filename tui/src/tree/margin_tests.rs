//! The rail's spacing (commands.md §17, contract §6.1.1): a Context block's rows end at column W-4 of the pane, the columns
//! W-3 to W-1 being blank (the padding, the box's wall column, the margin).

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

#[test]
fn a_context_block_row_is_cut_three_columns_before_the_panes_edge() {
    // a Plan row of the Context block is the one that is cut (its name) and right-aligned (its counters)
    let long = plan(
        "a-very-long-plan-id-xxxx",
        "cc",
        Phase::Dispatch,
        [0, 36, 0, 0, 1, 5],
        session("l-long", "lead", State::Idle, None, Place::Parked),
        vec![],
    );
    let mut view = View::new(tree(vec![context("cc", vec![long])]), true);
    let mut terminal = Terminal::new(TestBackend::new(W, 12)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    let buf = terminal.backend().buffer().clone();
    let y = (0..12).find(|&y| cells(&buf, y).iter().any(|c| c == "…")).expect("the Plan's name is cut with an ellipsis");
    assert_eq!(last_column(&buf, y), usize::from(W) - 4, "the row ends at column W-4: {:?}", cells(&buf, y));
}
