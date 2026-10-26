use cpd_core::plan::Phase;
use cpd_core::session::State;

use super::fixture::{context, plan, session, tree};
use super::needs;
use super::view::View;
use crate::read::Place;

fn lead_tree(state: State) -> crate::read::Tree {
    let lead = session("l-ship", "ship lead", state, None, Place::Parked);
    tree(vec![context("alpha", vec![plan("ship", "alpha", Phase::Dispatch, [0; 6], lead, vec![])])])
}

#[test]
fn a_tree_with_nothing_animated_and_an_empty_inbox_needs_no_clock() {
    let n = needs(&View::new(lead_tree(State::Idle), true));
    assert!(!n.frame);
    assert!(!n.second);
    assert!(n.at.is_none());
}

#[test]
fn a_working_lead_needs_the_frame_clock_and_no_second_clock_with_an_empty_inbox() {
    let n = needs(&View::new(lead_tree(State::Working), true));
    assert!(n.frame);
    assert!(!n.second);
    assert!(n.at.is_none());
}
