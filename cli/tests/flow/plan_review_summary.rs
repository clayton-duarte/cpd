//! `plan review` with the full render deleted (hs-d739, commands.md §4.5): it writes only `summary-<review>.md`, and
//! the summary ends with the line `Approve | Change`. Written from the docs, before the code changes.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::Store;
use serde_json::Value;

fn home(fx: &Fx) -> CpdHome {
    CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap()
}

/// `run/<plan>/<name>`, beside `plan.md`.
fn run_file(fx: &Fx, plan: &str, name: &str) -> PathBuf {
    home(fx)
        .plan_md(&PlanId::parse(plan).unwrap())
        .with_file_name(name)
}

/// A Plan `pay` in phase `plan` with one draft Job `a`, outlined for review.
fn plan_with_one_draft(fx: &Fx) -> PathBuf {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    let repo = fx.keyed_repo("pay");
    fx.cpd(&[
        "plan",
        "new",
        "pay",
        "--context",
        "ctx",
        "--title",
        "Plan pay",
    ])
    .ok();
    fx.github_origin(&repo);
    fx.cpd(&[
        "workflow",
        "add",
        "pay",
        "--repo",
        repo.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "job",
        "cancel",
        "pay-config",
        "--reason",
        "a stub of the template",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "job",
        "cancel",
        "open-pay",
        "--reason",
        "a stub of the template",
        "--plan",
        "pay",
    ])
    .ok();
    fx.drop_unreachable_origin(&repo);
    fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    let mut store = Store::open(&home(fx), 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, created_txn, created_at)
                 VALUES ('pay', (SELECT key FROM workflows WHERE plan = 'pay'), 'a', 'Job a', 'S2', 'Goal of a.', 'Context of a.', '[\"src/a.rs\"]',
                         '[\"cargo test\"]', '[]', 'draft', 0, {}, 't');",
                tx.id()
            ))?;
            Ok(())
        })
        .unwrap();
    fx.outline_for_review("pay");
    repo
}

fn review(fx: &Fx) -> Value {
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok()
}

fn last_line(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_owned()
}

#[test]
fn plan_review_writes_the_summary_copy_and_no_full_render() {
    let fx = Fx::with_ports();
    plan_with_one_draft(&fx);
    review(&fx);
    for name in ["summary-r1.md", "plan.md", "summary.md"] {
        assert!(run_file(&fx, "pay", name).exists(), "run/pay/{name} exists");
    }
    assert!(
        !run_file(&fx, "pay", "plan-r1.md").exists(),
        "the full render plan-r1.md is deleted (hs-d739)"
    );
}

#[test]
fn plan_review_output_has_no_path_and_the_summary_ends_with_approve_change() {
    let fx = Fx::with_ports();
    plan_with_one_draft(&fx);
    let out = review(&fx);
    for key in ["review", "summary", "next"] {
        assert!(out.get(key).is_some(), "`{key}` in {out}");
    }
    assert!(out.get("path").is_none(), "no `path` key: {out}");
    let summary = PathBuf::from(out["summary"].as_str().expect("summary is a path string"));
    assert_eq!(last_line(&summary), "Approve | Change");

    let again = review(&fx);
    assert_eq!(again["already"], true, "{again}");
    assert!(
        again.get("path").is_none(),
        "no `path` key on the pending review again: {again}"
    );
}
