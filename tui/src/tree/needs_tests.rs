use std::collections::BTreeSet;

use cpd_core::inbox::model::{Kind, Urgency};
use cpd_core::plan::Phase;
use cpd_core::session::State;

use super::fixture::{context, plan, session, tree};
use super::inbox::item;
use super::needs;
use super::view::View;
use crate::read::Place;

fn lead_tree(state: State) -> crate::read::Tree {
    let lead = session("l-ship", "ship lead", state, None, Place::Parked);
    tree(vec![context("alpha", vec![plan("ship", "alpha", Phase::Dispatch, [0; 6], lead, vec![])])])
}

fn with_item(mut t: crate::read::Tree, subject: &str) -> crate::read::Tree {
    let mut i = item(1, Kind::PrReady, Urgency::White, "ready", "2026-01-01T00:00:00.000Z");
    i.subject = subject.into();
    t.inbox.push(i);
    t
}

#[test]
fn a_tree_with_nothing_animated_and_an_empty_inbox_needs_no_clock() {
    let n = needs(&View::new(lead_tree(State::Idle), true));
    assert!(!n.frame);
    assert!(!n.second);
    assert!(n.at.is_none());
}

#[test]
fn an_inbox_with_items_needs_the_second_clock_and_no_frame_clock_when_nothing_spins() {
    let n = needs(&View::new(with_item(lead_tree(State::Idle), "pr:o/r#1"), true));
    assert!(n.second);
    assert!(!n.frame);
    assert!(n.at.is_none());
}

#[test]
fn a_working_lead_needs_the_frame_clock_and_no_second_clock_with_an_empty_inbox() {
    let n = needs(&View::new(lead_tree(State::Working), true));
    assert!(n.frame);
    assert!(!n.second);
    assert!(n.at.is_none());
}

#[test]
fn a_merging_inbox_item_needs_both_clocks() {
    let mut view = View::new(with_item(lead_tree(State::Idle), "pr:o/r#1"), true);
    view.set_merging(BTreeSet::from(["o/r#1".to_owned()]));
    let n = needs(&view);
    assert!(n.frame);
    assert!(n.second);
}
