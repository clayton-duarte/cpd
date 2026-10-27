use cpd_core::plan::Phase;
use cpd_core::session::State;

use super::fixture::{context, managed, plan, session, tree};
use super::needs;
use super::view::View;
use crate::read::{Dot, Place};

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

fn managed_tree(dot: Dot, tweak: impl FnOnce(&mut crate::read::ContextNode)) -> crate::read::Tree {
    let lead = session("l-ship", "ship lead", State::Idle, None, Place::Parked);
    let mut c = managed(context("alpha", vec![plan("ship", "alpha", Phase::Dispatch, [0; 6], lead, vec![])]), dot);
    tweak(&mut c);
    tree(vec![c])
}

#[test]
fn a_working_manager_with_no_working_lead_needs_the_frame_clock() {
    let n = needs(&View::new(managed_tree(Dot::Working(None), |_| {}), true));
    assert!(n.frame);
    assert!(!n.second);
    assert!(n.at.is_none());
}

#[test]
fn a_fixing_manager_needs_the_frame_clock() {
    let n = needs(&View::new(managed_tree(Dot::Fixing, |_| {}), true));
    assert!(n.frame);
    assert!(!n.second);
    assert!(n.at.is_none());
}

#[test]
fn an_archived_context_with_a_working_manager_dot_needs_no_clock() {
    let n = needs(&View::new(managed_tree(Dot::Working(None), |c| c.archived_at = Some("2026-02-01T00:00:00.000Z".into())), true));
    assert!(!n.frame);
}

#[test]
fn a_sleeping_manager_with_a_working_dot_needs_no_clock() {
    let n = needs(&View::new(managed_tree(Dot::Working(None), |c| c.manager_asleep = true), true));
    assert!(!n.frame);
}
