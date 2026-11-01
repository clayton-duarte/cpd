//! Review Jobs are retired (Context record pro-d163; commands.md §5.1 `job add`, §5.2 `job show`, contract §5.2 `job-added`):
//! `job add` has no `--review` flag, and no Job output or `job-added` Event carries a `review` field. The column
//! `jobs.review` stays, inert: nothing reads or writes it.

use std::collections::BTreeSet;

use serde_json::Value;

use super::job::{count, drafted, events, job, job_add, lock, plan, setup, spec_file};
use crate::common::fx::Fx;
use crate::common::refused;

/// The keys of the JSON object `value`.
fn keys(value: &Value) -> BTreeSet<&str> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("not an object: {value}"))
        .keys()
        .map(String::as_str)
        .collect()
}

fn set(names: &[&'static str]) -> BTreeSet<&'static str> {
    names.iter().copied().collect()
}

#[test]
fn job_add_refuses_the_review_flag_and_writes_nothing() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let spec = spec_file(&fx.env);
    let before = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    let run = fx.cpd(&[
        "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec, "--review", "--plan",
        "pl",
    ]);
    refused(&run, "usage", 2);

    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        before,
        "a refusal writes nothing"
    );
}

#[test]
fn job_add_and_its_job_added_event_carry_no_review_field() {
    let output = set(&[
        "key", "pr", "kind", "status", "appended", "tier", "needs", "urgent",
    ]);
    let data = set(&[
        "pr", "kind", "status", "appended", "tier", "needs", "urgent",
    ]);
    for (phase, status) in [("plan", "draft"), ("dispatch", "queued")] {
        let fx = Fx::new();
        setup(&fx.env);
        plan(&fx.env, "pl", phase);
        let out = job_add(&fx, "pl", "k", "S2", &[]).ok();
        assert_eq!(out["job"]["status"], status, "{phase}");
        assert_eq!(
            keys(&out["job"]),
            output,
            "{phase}: the job object of `job add`"
        );
        let added = events(&fx, "job-added");
        let event = added
            .last()
            .unwrap_or_else(|| panic!("{phase}: no job-added Event"));
        assert_eq!(
            keys(&event["data"]),
            data,
            "{phase}: the data of the job-added Event"
        );
    }
}

#[test]
fn job_show_and_plan_show_carry_no_review_field() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");

    let shown = job(&fx, &["show", "b"], "pl").ok();
    assert!(
        !keys(&shown["job"]).contains("review"),
        "job show: {}",
        shown["job"]
    );

    let plan_show = fx.cpd(&["plan", "show", "--plan", "pl"]).ok();
    let jobs = plan_show["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    for item in jobs {
        assert!(!keys(item).contains("review"), "plan show: {item}");
    }

    let listed = job(&fx, &["list"], "pl").ok();
    let rows = listed["jobs"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert!(!keys(row).contains("review"), "job list: {row}");
    }
}
