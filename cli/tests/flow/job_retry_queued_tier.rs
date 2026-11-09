//! `job retry --tier` on a QUEUED Job whose effective Tier fails the worker range (`tier_not_allowed`) requeues it
//! at the given Tier instead of refusing `job_not_failed` (pro-d484).

use std::fs;

use serde_json::json;

use super::job::{drafted, job, job_row, lock};
use crate::common::Run;

#[test]
fn retry_tier_on_queued_tier_not_allowed_job_succeeds() {
    let fx = crate::common::fx::Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    // The worker floor was raised to O1 after Job `a` was stamped S2: it is queued and below the floor.
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\nworker_tier = \"O1\"\n",
    )
    .unwrap();
    let out: Run = job(&fx, &["retry", "a", "--tier", "O1"], "pl");
    assert_eq!(
        out.ok(),
        json!({"ok": true, "job": "a", "status": "queued", "tier": "O1"})
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "retry_tier"), "O1");
}
