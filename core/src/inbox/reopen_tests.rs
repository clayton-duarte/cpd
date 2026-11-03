//! The authored pull request rule for a reopened pull request (pro-d362): a legacy cancelled Workflow (`pull_request` NULL) is
//! rebound by `workflow::reopen::reopen`, and the `pr-ready` item then names its Context, Plan and Lead Session. The join
//! of the rule itself (`inbox/pr.rs`) is unchanged.

use time::macros::datetime;

use super::model::{Condition, Kind};
use super::rules::conditions_with;
use crate::janitor_tests::Rig;
use crate::store::Store;
use crate::workflow::reopen_tests::{cancelled, gh, publish, rig, run, stored_pull, twins};

/// The `pr-ready` item of `acme/pay#801`.
fn ready(rig: &Rig) -> Condition {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let found = conditions_with(
        &mut store,
        &rig.home,
        datetime!(2026-01-02 00:00 UTC),
        20,
        1,
        None,
    )
    .unwrap();
    let mut ready: Vec<Condition> = found
        .into_iter()
        .filter(|c| c.kind == Kind::PrReady)
        .collect();
    assert_eq!(ready.len(), 1);
    ready.remove(0)
}

fn owner(item: &Condition) -> [Option<&str>; 3] {
    [
        item.context.as_ref().map(|c| c.as_str()),
        item.plan.as_ref().map(|p| p.as_str()),
        item.session.as_ref().map(|s| s.as_str()),
    ]
}

#[test]
fn inbox_pr_ready_of_a_reopened_cancelled_workflow_carries_its_plan_and_session() {
    // Two cancelled Workflows share the Lead Branch: nothing is rebound, the item has no owner (no guess).
    let two = rig("deploy");
    twins(&two);
    stored_pull(&two, 801, "open");
    gh(&two, &[("cpd/pay", 801, "OPEN")]);
    publish(&two, "cpd/pay");
    assert!(run(&two, "pay", "api").is_err());
    assert_eq!(owner(&ready(&two)), [None, None, None]);

    // Exactly one: it is rebound by `reopen`, and the item names the Plan and its Lead.
    let one = rig("deploy");
    cancelled(&one, "pay", "api", None);
    stored_pull(&one, 801, "open");
    gh(&one, &[("cpd/pay/api", 801, "OPEN")]);
    publish(&one, "cpd/pay/api");

    run(&one, "pay", "api").unwrap();

    assert_eq!(
        owner(&ready(&one)),
        [Some("ctx"), Some("pay"), Some("lead1")]
    );
}
