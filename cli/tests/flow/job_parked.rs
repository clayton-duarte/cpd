//! The door of `job add --draft` and `job edit` after the lock (contract §2.2 "A parked draft", commands.md §5.1, §5.2; hs-d822):
//! a complete draft (Tier and Spec) that waits for a queued, running or failed Job, directly or through other drafts, or
//! for another PR, is refused `draft_parked`; a stub (no Tier or no Spec) is legal on its own. Run by the command tree
//! (`Fx`); Plans and the states only the engine reaches are fixtures (`super::job`).

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

/// `job add <key> --draft --title <t>` in Plan `pl` with `extra` flags: a stub unless `extra` gives `--tier` and `--spec`.
fn stub(fx: &Fx, key: &str, extra: &[&str]) -> Run {
    let title = format!("Title of {key}");
    let mut args = vec![
        "job", "add", key, "--draft", "--title", &title, "--plan", "pl",
    ];
    args.extend(extra);
    fx.cpd(&args)
}

/// An incomplete draft (no Tier, no Spec) written straight into the database: the legal stub the door accepts and the
/// state the engine's `parked_drafts` never counts.
fn incomplete_draft(fx: &Fx, key: &str, kind: &str) {
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, pr, key, title, kind, status, appended, created_txn, created_at)
             VALUES ('pl', 'repo', '{key}', 'Title of {key}', '{kind}', 'draft', 1, cpd_txn(), 't');"
        ),
    );
}

/// A complete draft (Tier and Spec) written straight into the database, with `needs`: a legal state (for example when a
/// review covers it), never a state the door would write after the lock while it waits.
fn complete_draft(fx: &Fx, key: &str, needs: &[&str]) {
    let mut sql = format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pl', 'repo', '{key}', 'Title of {key}', 'S2', 'Do it.', 'Because.', '[\"src/a.rs\"]', '[\"cargo test\"]', '[]', 'draft', 1, cpd_txn(), 't');"
    );
    for need in needs {
        sql += &format!(
            "INSERT INTO dependencies (plan, job, needs) VALUES ('pl', '{key}', '{need}');"
        );
    }
    fixture(&fx.env, &sql);
}

/// PR `web` of Plan `pl`, waiting, that needs PR `repo` (which is not merged): it waits for another PR.
fn web_waits_for_repo(fx: &Fx) {
    fixture(
        &fx.env,
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('pl', 'web', '/web', 'main', 'waiting', 'cpd/pl/web', cpd_txn(), 't');
         INSERT INTO pr_needs (plan, pr, needs) VALUES ('pl', 'web', 'repo');",
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
fn complete_draft_that_waits_is_refused() {
    let fx = locked();
    let spec = spec_file(&fx.env);
    let e = refused(
        &stub(&fx, "k", &["--tier", "S2", "--spec", &spec, "--needs", "a"]),
        "draft_parked",
        3,
    );
    assert_eq!(
        e["details"],
        json!({"plan": "pl", "job": "k", "pr": "repo", "needs": ["a"]}),
        "{e}"
    );
    let message = e["message"].as_str().unwrap();
    for form in ["--needs", "cpd_plan_review", "--draft"] {
        assert!(message.contains(form), "the message names {form}: {e}");
    }
    // A Tier alone, or a Spec alone, is still a stub, and a stub that waits for nothing is accepted.
    stub(&fx, "stub", &[]).ok();
    stub(&fx, "tier-only", &["--tier", "S2"]).ok();
    stub(&fx, "spec-only", &["--spec", &spec]).ok();
    // A complete draft that waits for nothing is accepted too.
    stub(&fx, "full", &["--tier", "S2", "--spec", &spec]).ok();
}

#[test]
fn refusal_writes_nothing() {
    let fx = locked();
    let spec = spec_file(&fx.env);
    let before = writes(&fx);
    let history = count(&fx.env, "history");
    refused(
        &stub(&fx, "k", &["--tier", "S2", "--spec", &spec, "--needs", "a"]),
        "draft_parked",
        3,
    );
    assert_eq!(writes(&fx), before, "no txn, no Event, no Job");
    assert_eq!(count(&fx.env, "history"), history);
    assert_eq!(events(&fx, "job-added").len(), 1, "only `a`");
}

#[test]
fn complete_draft_needing_a_merged_job_is_accepted() {
    let fx = locked();
    engine(&fx.env, "pl", "a", "merged");
    let spec = spec_file(&fx.env);
    let out = stub(&fx, "k", &["--tier", "S2", "--spec", &spec, "--needs", "a"]).ok();
    assert_eq!(
        (&out["job"]["status"], &out["job"]["needs"]),
        (&json!("draft"), &json!(["a"])),
        "a merged need does not wait"
    );
}

#[test]
fn complete_draft_needing_a_draft_that_waits_is_refused() {
    let fx = locked();
    // The stub `p` waits for the queued Job `a`, so a complete draft that needs `p` waits through it.
    incomplete_draft(&fx, "p", "work");
    fixture(
        &fx.env,
        "INSERT INTO dependencies (plan, job, needs) VALUES ('pl', 'p', 'a');",
    );
    let spec = spec_file(&fx.env);
    let before = writes(&fx);
    let e = refused(
        &stub(&fx, "k", &["--tier", "S2", "--spec", &spec, "--needs", "p"]),
        "draft_parked",
        3,
    );
    assert_eq!(
        e["details"],
        json!({"plan": "pl", "job": "k", "pr": "repo", "needs": ["p"]}),
        "{e}"
    );
    assert_eq!(writes(&fx), before);
    // `p` waits for nothing: a complete draft that needs it is accepted.
    fixture(
        &fx.env,
        "DELETE FROM dependencies WHERE plan = 'pl' AND job = 'p';",
    );
    stub(
        &fx,
        "ok",
        &["--tier", "S2", "--spec", &spec, "--needs", "p"],
    )
    .ok();
}

#[test]
fn edit_removing_a_waiting_need_is_accepted() {
    let fx = locked();
    complete_draft(&fx, "k", &["a"]);
    let out = job(&fx, &["edit", "k", "--no-needs"], "pl").ok();
    assert_eq!(out["job"]["status"], "draft");
    assert_eq!(
        count(&fx.env, "dependencies"),
        0,
        "the waiting need is gone"
    );
    assert_eq!(job_row(&fx.env, "pl", "k", "status"), "draft");
    // Putting the waiting need back is refused: the door refuses the complete draft it would make.
    refused(
        &job(&fx, &["edit", "k", "--needs", "a"], "pl"),
        "draft_parked",
        3,
    );
    assert_eq!(count(&fx.env, "dependencies"), 0);
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
    incomplete_draft(&fx, "on-failed", "work");
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
    let out = stub(&fx, "k", &["--pr", "web"]).ok();
    assert_eq!(
        (&out["job"]["pr"], &out["job"]["status"]),
        (&json!("web"), &json!("draft"))
    );
    // A complete draft in the same waiting PR is refused: the PR's wait is a wait.
    let spec = spec_file(&fx.env);
    refused(
        &stub(
            &fx,
            "full",
            &["--tier", "S2", "--spec", &spec, "--pr", "web"],
        ),
        "draft_parked",
        3,
    );
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
fn edit_completing_a_stub_without_needs_is_accepted() {
    let fx = locked();
    incomplete_draft(&fx, "p", "work");
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
    incomplete_draft(&fx, "open-repo", "open");
    // The open Job's draft is exempt: with it there, a stub that waits, a queued Job and a full draft are added as ever.
    stub(&fx, "k", &["--needs", "a"]).ok();
    job_add(&fx, "pl", "b", "S2", &["a"]).ok();
    let spec = spec_file(&fx.env);
    stub(&fx, "full", &["--tier", "S2", "--spec", &spec]).ok();
    // Editing a Job never reports the open Job as a parked draft.
    let run = job(&fx, &["edit", "open-repo", "--title", "Open it"], "pl");
    assert_ne!(run.json()["error"]["code"], "draft_parked", "{run:?}");
}
