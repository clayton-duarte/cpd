//! `job dispatch` and the description of a PR (Plan desc-gate): an `open` Job whose PR has no stored description and no `head_ref`
//! is refused `description_required` before any Attempt or driver; an adopted PR's open Job and an `update` Job start.

use crate::common;

use super::driver_open::{Live, open_live, open_pr_live};

const UNDESCRIBED: &str = "UPDATE workflows SET description = NULL WHERE key = 'repo';";

fn attempts(live: &Live) -> i64 {
    live.value("SELECT count(*) FROM attempts")
}

#[test]
fn job_dispatch_refuses_an_open_job_whose_pr_has_no_description() {
    let live = open_live();
    live.fixture(UNDESCRIBED);

    let run = live.dispatch("open-repo");

    let error = common::refused(&run, "description_required", 3);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("cpd workflow describe --workflow repo"),
        "{error}"
    );
    assert_eq!(attempts(&live), 0, "no Attempt row");
    assert_eq!(
        live.fx.fakes.spawn.launches().len(),
        0,
        "no driver launched"
    );
    assert_eq!(live.job_status("open-repo"), "queued");
}

/// Guard: an adopted PR (`head_ref` set) keeps its own body; its open Job is dispatched.
#[test]
fn job_dispatch_starts_the_open_job_of_an_adopted_pr_without_a_description() {
    let live = open_live();
    // `head_ref` is fixed by a trigger: lift it for the update and put back the very same trigger.
    let trigger: String =
        live.value("SELECT sql FROM sqlite_master WHERE name = 'workflows_head_ref_fixed'");
    live.fixture(&format!("{UNDESCRIBED} DROP TRIGGER workflows_head_ref_fixed; UPDATE workflows SET head_ref = 'feature' WHERE key = 'repo'; {trigger};"));
    let out = live.dispatch("open-repo").ok();
    assert_eq!(out["attempt"], "open-repo-a1", "{out}");
    assert_eq!(attempts(&live), 1);
}

/// Guard: an `update` Job needs no description.
#[test]
fn job_dispatch_starts_an_update_job_whose_pr_has_no_description() {
    let live = open_pr_live();
    live.add_cpd_job("up-repo", "update");
    live.fixture(UNDESCRIBED);
    let out = live.dispatch("up-repo").ok();
    assert_eq!(out["attempt"], "up-repo-a1", "{out}");
}

/// Guard: a described PR starts its open Job.
#[test]
fn job_dispatch_starts_the_open_job_of_a_described_pr() {
    let live = open_live();
    let out = live.dispatch("open-repo").ok();
    assert_eq!(out["attempt"], "open-repo-a1", "{out}");
}
