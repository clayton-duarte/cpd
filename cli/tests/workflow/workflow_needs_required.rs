//! `pr add` in phase `dispatch` is refused `needs_required` unless it says `--needs` or `--no-needs`
//! (commands.md §9.4, §1.3; contract). Reuses the `added` fixture of `pr.rs`.

use crate::workflow::{Added, added};
use serde_json::{Value, json};

fn error(run: &crate::common::Run) -> Value {
    let v: Value = serde_json::from_str(run.stdout.trim())
        .unwrap_or_else(|e| panic!("not JSON ({e}): {} / {}", run.stdout, run.stderr));
    v["error"].clone()
}

/// `Added::add` of shape `config` with `--jobs <file.json>`: a Workflow that waits, added after the lock, is created with queued
/// template Jobs (pro-d404), never stub drafts.
fn add_jobs(f: &Added, key: &str, repo: &std::path::Path, extra: &[&str]) -> crate::common::Run {
    let plain = json!({"tier": "S2", "spec": {"goal": "Do it.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": ["Everything else."]}});
    let path = f.fx.env.root().join(format!("jobs-{key}.json"));
    std::fs::write(&path, json!({"config": plain}).to_string()).unwrap();
    f.fx.install_catalog();
    let mut args = extra.to_vec();
    args.extend(["--jobs", path.to_str().unwrap()]);
    f.add(key, repo, &args)
}

fn assert_refused(f: &Added, key: &str, shape: &str) {
    let other =
        f.fx.keyed_repo(&format!("other-{shape}"))
            .canonicalize()
            .unwrap();
    let run = f.fx.cpd(&[
        "workflow",
        "add",
        key,
        "--repo",
        other.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        shape,
        "--plan",
        "pay",
    ]);
    let e = error(&run);
    assert_eq!(e["code"], "needs_required", "{shape}: {e}");
    assert_eq!(run.exit, 3, "{shape}");
    assert_eq!(
        e["details"],
        json!({"plan": "pay", "workflow": key, "phase": "dispatch"}),
        "{shape}"
    );
    let msg = e["message"].as_str().unwrap();
    assert!(
        msg.contains("--needs") && msg.contains("--no-needs"),
        "{msg}"
    );
    // Nothing written.
    let workflows: i64 = f.fx.env.query(&format!(
        "SELECT count(*) FROM workflows WHERE plan = 'pay' AND key = '{key}'"
    ));
    let jobs: i64 = f.fx.env.query(&format!(
        "SELECT count(*) FROM jobs WHERE plan = 'pay' AND workflow = '{key}'"
    ));
    assert_eq!((workflows, jobs), (0, 0), "{shape}");
    assert!(f.pr_added().is_empty(), "{shape}");
}

#[test]
fn workflow_add_in_dispatch_without_needs_or_no_needs_is_refused_needs_required() {
    let f = added("dispatch");
    assert_refused(&f, "fix", "config");
    let stub: i64 =
        f.fx.env
            .query("SELECT count(*) FROM jobs WHERE plan = 'pay' AND key = 'fix-docs'");
    assert_eq!(stub, 0);
}

#[test]
fn workflow_add_in_dispatch_refusal_covers_every_shape() {
    let f = added("dispatch");
    assert_refused(&f, "fix", "docs");
    assert_refused(&f, "fix", "config");
}

#[test]
fn workflow_add_in_dispatch_with_no_needs_adds_a_workflow_without_needs() {
    let f = added("dispatch");
    // Stacked on PR `repo`, which has not merged: a waiting PR after the lock needs `--jobs` (`draft_parked` otherwise).
    let out = add_jobs(&f, "fix", &f.repo, &["--no-needs", "--base", "pay/repo"]).ok();
    assert_eq!(out["workflow"]["needs"], json!([]));
    assert!(f.needs_of("fix").is_empty());
}

#[test]
fn workflow_add_in_dispatch_with_needs_links_the_workflow() {
    let f = added("dispatch");
    let other = f.fx.keyed_repo("other").canonicalize().unwrap();
    let out = add_jobs(&f, "fix", &other, &["--needs", "repo"]).ok();
    assert_eq!(out["workflow"]["needs"], json!(["repo"]));
    assert_eq!(f.needs_of("fix"), ["repo"]);
}

#[test]
fn workflow_add_in_plan_phase_needs_no_flag() {
    let fx = crate::common::fx::Fx::with_ports();
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'plan', 'lead1', 't');",
    );
    let run = fx.cpd(&[
        "workflow",
        "add",
        "fix",
        "--repo",
        repo.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ]);
    assert_eq!(run.json()["workflow"]["needs"], json!([]));
}

#[test]
fn workflow_add_rerun_in_dispatch_without_a_flag_is_already() {
    let f = added("dispatch");
    // Stacked on PR `repo`, which has not merged: a waiting PR after the lock needs `--jobs` (`draft_parked` otherwise).
    add_jobs(&f, "fix", &f.repo, &["--no-needs", "--base", "pay/repo"]).ok();
    assert_eq!(
        add_jobs(&f, "fix", &f.repo, &["--base", "pay/repo"]).ok()["already"],
        true
    );
}

/// A stacked PR (`--base` names a PR of the Plan in its repo, or that PR's Lead Branch) needs neither flag: its base is its link.
fn assert_stacked_add_needs_no_flag(base: &str) {
    let f = added("dispatch");
    let out = add_jobs(&f, "fix", &f.repo, &["--base", base]).ok();
    assert_eq!(out["workflow"]["needs"], json!([]), "{out}");
    assert_eq!(out["workflow"]["base_workflow"], "pay/repo", "{out}");
    assert_eq!(out["workflow"]["status"], "waiting", "{out}");
    assert!(f.needs_of("fix").is_empty());
}

#[test]
fn workflow_add_in_dispatch_needs_neither_flag_for_a_stacked_workflow() {
    assert_stacked_add_needs_no_flag("pay/repo");
}

#[test]
fn workflow_add_in_dispatch_needs_neither_flag_for_a_stacked_workflow_named_by_its_bare_key() {
    assert_stacked_add_needs_no_flag("repo");
}

#[test]
fn workflow_add_in_dispatch_needs_neither_flag_for_a_workflow_based_on_another_workflows_lead_branch()
 {
    // The Lead Branch of PR `repo` in the fixture is `cpd/pay`.
    assert_stacked_add_needs_no_flag("cpd/pay");
}
