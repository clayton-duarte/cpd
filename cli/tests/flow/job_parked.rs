//! The door of `job add --draft` and `job edit` after the lock (contract §2.2 "A parked draft", commands.md §5.1, §5.2; hs-d731):
//! a stub draft (no Tier or no Spec) that waits for nothing is refused `draft_parked`. Run by the command tree (`Fx`);
//! Plans and the states only the engine reaches are fixtures (`super::job`).

use serde_json::json;

use super::job::{count, engine, events, fixture, job, job_add, job_row, plan, setup, spec_file};
use crate::common::fx::Fx;
use crate::common::{Run, refused};

/// Plan `pl` in phase `dispatch` (PR `repo`) with Job `a` queued.
fn locked() -> Fx {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "dispatch");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    fx
}

/// `job add <key> --draft --title <t>`, a stub, in Plan `pl` with `extra` flags.
fn stub(fx: &Fx, key: &str, extra: &[&str]) -> Run {
    let title = format!("Title of {key}");
    let mut args = vec![
        "job", "add", key, "--draft", "--title", &title, "--plan", "pl",
    ];
    args.extend(extra);
    fx.cpd(&args)
}

/// A stub draft written straight into the database: what the door would refuse, as an existing state.
fn parked(fx: &Fx, key: &str, kind: &str) {
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, kind, status, appended, created_txn, created_at)
             VALUES ('pl', 'repo', '{key}', 'Title of {key}', '{kind}', 'draft', 1, cpd_txn(), 't');"
        ),
    );
}

/// PR `web` of Plan `pl`, waiting, that needs PR `repo` (which is not merged): it waits for another PR.
fn web_waits_for_repo(fx: &Fx) {
    fixture(
        &fx.env,
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('pl', 'web', '/web', 'main', 'waiting', 'cpd/pl/web', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pl', 'web', 'repo');",
    );
}

fn writes(fx: &Fx) -> (i64, i64, i64) {
    (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    )
}

// ---------- refused ----------

#[test]
fn stub_without_need_after_lock_is_refused() {
    let fx = locked();
    let e = refused(&stub(&fx, "k", &[]), "draft_parked", 3);
    assert_eq!(
        e["details"],
        json!({"plan": "pl", "job": "k", "pr": "repo", "needs": []}),
        "{e}"
    );
    let message = e["message"].as_str().unwrap();
    for form in ["--needs", "cpd_plan_review", "--draft"] {
        assert!(message.contains(form), "the message names {form}: {e}");
    }
    // A Tier alone, or a Spec alone, is still a stub.
    refused(&stub(&fx, "t", &["--tier", "S2"]), "draft_parked", 3);
    refused(
        &stub(&fx, "s", &["--spec", &spec_file(&fx.env)]),
        "draft_parked",
        3,
    );
}

#[test]
fn refusal_writes_nothing() {
    let fx = locked();
    let before = writes(&fx);
    let history = count(&fx.env, "history");
    refused(&stub(&fx, "k", &[]), "draft_parked", 3);
    assert_eq!(writes(&fx), before, "no txn, no Event, no Job");
    assert_eq!(count(&fx.env, "history"), history);
    assert_eq!(events(&fx, "job-added").len(), 1, "only `a`");
}

#[test]
fn stub_needing_merged_job_is_refused() {
    let fx = locked();
    engine(&fx.env, "pl", "a", "merged");
    let e = refused(&stub(&fx, "k", &["--needs", "a"]), "draft_parked", 3);
    assert_eq!(
        e["details"],
        json!({"plan": "pl", "job": "k", "pr": "repo", "needs": ["a"]}),
        "{e}"
    );
}

#[test]
fn stub_needing_parked_stub_is_refused() {
    let fx = locked();
    parked(&fx, "p", "work");
    let before = writes(&fx);
    let e = refused(&stub(&fx, "k", &["--needs", "p"]), "draft_parked", 3);
    assert_eq!(
        e["details"],
        json!({"plan": "pl", "job": "k", "pr": "repo", "needs": ["p"]}),
        "{e}"
    );
    assert_eq!(writes(&fx), before);
}

#[test]
fn edit_no_needs_of_stub_after_lock_is_refused() {
    let fx = locked();
    stub(&fx, "k", &["--needs", "a"]).ok();
    let before = (writes(&fx), count(&fx.env, "history"));
    let e = refused(
        &job(&fx, &["edit", "k", "--no-needs"], "pl"),
        "draft_parked",
        3,
    );
    assert_eq!(
        e["details"],
        json!({"plan": "pl", "job": "k", "pr": "repo", "needs": []}),
        "{e}"
    );
    assert_eq!((writes(&fx), count(&fx.env, "history")), before);
    assert_eq!(
        fx.env
            .query::<String>("SELECT group_concat(needs) FROM dependencies WHERE job = 'k'"),
        "a",
        "unchanged"
    );
    assert_eq!(job_row(&fx.env, "pl", "k", "status"), "draft");
}

// ---------- accepted (guards that pass before the door too) ----------

#[test]
fn stub_needing_a_queued_running_or_failed_job_is_accepted() {
    let fx = locked();
    let out = stub(&fx, "q", &["--needs", "a"]).ok();
    assert_eq!(
        (&out["job"]["status"], &out["job"]["needs"]),
        (&json!("draft"), &json!(["a"])),
        "queued need"
    );
    job_add(&fx, "pl", "r", "S2", &[]).ok();
    engine(&fx.env, "pl", "r", "running");
    stub(&fx, "on-running", &["--needs", "r"]).ok();
    job_add(&fx, "pl", "f", "S2", &[]).ok();
    engine(&fx.env, "pl", "f", "failed");
    // `job add` and `job edit` refuse a new need on a failed Job (`needs_unavailable`), so the stub that needs it
    // is a fixture (it was added while `f` was running); retrying `f` recovers it, so the stub waits: any edit is accepted.
    parked(&fx, "on-failed", "work");
    fixture(
        &fx.env,
        "INSERT INTO dependencies (plan, job, needs) VALUES ('pl', 'on-failed', 'f');",
    );
    job(&fx, &["edit", "on-failed", "--title", "Other"], "pl").ok();
}

#[test]
fn stub_that_needs_a_waiting_stub_is_accepted() {
    let fx = locked();
    stub(&fx, "w", &["--needs", "a"]).ok();
    stub(&fx, "k", &["--needs", "w"]).ok();
    assert_eq!(job_row(&fx.env, "pl", "k", "status"), "draft");
}

#[test]
fn stub_in_a_pr_that_waits_for_another_pr_is_accepted() {
    let fx = locked();
    web_waits_for_repo(&fx);
    let out = stub(&fx, "k", &["--workflow", "web"]).ok();
    assert_eq!((&out["job"]["pr"], &out["job"]["status"]), (&json!("web"), &json!("draft")));
}

#[test]
fn full_draft_with_no_needs_is_accepted() {
    let fx = locked();
    let spec = spec_file(&fx.env);
    let out = stub(&fx, "k", &["--tier", "S2", "--spec", &spec]).ok();
    assert_eq!(
        (&out["job"]["status"], &out["job"]["needs"]),
        (&json!("draft"), &json!([]))
    );
}

#[test]
fn edit_giving_a_parked_stub_its_tier_and_spec_is_accepted() {
    let fx = locked();
    parked(&fx, "p", "work");
    let spec = spec_file(&fx.env);
    let out = job(&fx, &["edit", "p", "--tier", "S2", "--spec", &spec], "pl").ok();
    assert_eq!(out["job"]["status"], "draft");
    assert_eq!(job_row(&fx.env, "pl", "p", "tier"), "S2");
}

#[test]
fn in_phase_plan_a_stub_with_no_needs_is_accepted() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let out = stub(&fx, "k", &[]).ok();
    assert_eq!(out["job"]["status"], "draft");
    job(&fx, &["edit", "k", "--title", "Other"], "pl").ok();
}

#[test]
fn the_open_jobs_draft_never_makes_a_plan_refuse_anything() {
    let fx = locked();
    parked(&fx, "open-repo", "open");
    // The open Job's draft is exempt: with it there, a stub that waits, a queued Job and a full draft are added as ever.
    stub(&fx, "k", &["--needs", "a"]).ok();
    job_add(&fx, "pl", "b", "S2", &["a"]).ok();
    let spec = spec_file(&fx.env);
    stub(&fx, "full", &["--tier", "S2", "--spec", &spec]).ok();
    // Editing a Job never reports the open Job as a parked draft.
    let run = job(&fx, &["edit", "open-repo", "--title", "Open it"], "pl");
    assert_ne!(run.json()["error"]["code"], "draft_parked", "{run:?}");
}
