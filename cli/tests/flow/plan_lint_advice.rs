//! The lint rule `tier_advice` (contract §2.7, §3.4b; commands.md §4.5, §4.6, §4.15) through the built binary: the
//! warning, its place in review and render, the catalog read only with history, and the loud failure.

use super::job::setup;
use super::plan_lint::{base, outline};
use super::plan_overlap::{home, job, plan, pr};
use crate::common::fx::Fx;
use crate::common::{Run, refused};
use serde_json::{Value, json};

/// One finished `impl` Job of the earlier Plan `old` with one first Attempt at `tier`.
fn history_job(fx: &Fx, key: &str, tier: &str, merged: bool, cost: &str) {
    let (status, failed, sha) = if merged {
        ("merged", "NULL", "'abc'")
    } else {
        ("failed", "'gate_failed'", "NULL")
    };
    let (outcome, cause) = if merged {
        ("merged", "merged")
    } else {
        ("failed", "no_submit")
    };
    fx.env.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, kind, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, failed_cause, merged_sha, created_txn, created_at)
         VALUES ('old', 'old', '{key}', 'T', 'impl', '{tier}', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 1, '{}', {failed}, {sha}, cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, ended_at, branch, worktree, started_at, metrics, merged_sha)
         VALUES ('old', '{key}-a1', '{key}', 1, '{tier}', 'ladder', 'ended', '{outcome}', '{cause}', 't', 'cpd/old--{key}-a1', '/w', 't', '{{\"cost_usd\": {cost}}}', {sha});",
        "f".repeat(64)
    ));
}

/// A merged Plan `old` with 40 first Attempts at S1 (38 merged, 2 failed, $0.20) and 20 at S2 (all merged, $0.50):
/// the advice for `impl` is S1.
fn history(fx: &Fx) {
    plan(fx, "old", "done", false);
    pr(fx, "old", "old", "/r/old", "merged");
    for n in 0..40 {
        history_job(fx, &format!("s1-{n}"), "S1", n >= 2, "0.20");
    }
    for n in 0..20 {
        history_job(fx, &format!("s2-{n}"), "S2", true, "0.50");
    }
}

/// The Plan `mine` with a draft `impl` Job `i` at `tier` and the test Job `t` it needs, an outline and a `done_when`.
/// With `catalog`, a fake pi install and a seeded Tier catalog (`setup`, which also makes Context `ctx`).
fn mine(tier: &str, catalog: bool) -> Fx {
    let fx = if catalog {
        let fx = Fx::new();
        setup(&fx.env);
        plan(&fx, "mine", "plan", false);
        pr(&fx, "mine", "api", "/r/api", "waiting");
        fx
    } else {
        base()
    };
    job(&fx, "mine", "api", "t", "draft", &["src/t.rs"]);
    job(&fx, "mine", "api", "i", "draft", &["src/i.rs"]);
    fx.env.sql(&format!(
        "UPDATE jobs SET kind = 'test' WHERE plan = 'mine' AND key = 't';
         UPDATE jobs SET kind = 'impl', tier = '{tier}' WHERE plan = 'mine' AND key = 'i';
         INSERT INTO dependencies (plan, job, needs) VALUES ('mine', 'i', 't');"
    ));
    outline(&fx, &["api"]);
    fx
}

fn lint(fx: &Fx) -> Run {
    fx.cpd(&["plan", "lint", "--plan", "mine"])
}

fn review(fx: &Fx) -> Run {
    fx.cpd(&["plan", "review", "--plan", "mine", "--title", "T"])
}

fn render(fx: &Fx) -> Run {
    fx.cpd(&["plan", "render", "--plan", "mine"])
}

/// The `tier_advice` findings of `warnings` (a list of findings) or of `findings`.
fn advice(findings: &Value) -> Vec<&Value> {
    findings
        .as_array()
        .unwrap_or_else(|| panic!("{findings}"))
        .iter()
        .filter(|f| f["rule"] == "tier_advice")
        .collect()
}

fn assert_the_warning(f: &Value) {
    assert_eq!(f["severity"], "warn", "{f}");
    assert_eq!(
        (&f["pr"], &f["job"], &f["other"], &f["paths"], &f["details"]),
        (
            &json!("api"),
            &json!("i"),
            &Value::Null,
            &json!([]),
            &json!({})
        ),
        "{f}"
    );
    let message = f["message"].as_str().unwrap();
    for part in [
        "history favours S1",
        "not what the Tier causes",
        "cpd job edit",
    ] {
        assert!(message.contains(part), "{message}");
    }
}

#[test]
fn lint_warns_when_a_draft_is_above_the_advised_tier() {
    let fx = mine("S2", true);
    history(&fx);

    let out = lint(&fx).ok();
    assert_eq!(
        (&out["errors"], &out["warnings"]),
        (&json!(0), &json!(1)),
        "{out}"
    );
    let found = advice(&out["findings"]);
    assert_eq!(found.len(), 1, "{out}");
    assert_the_warning(found[0]);
}

#[test]
fn review_and_render_return_the_tier_advice_warning() {
    let fx = mine("S2", true);
    history(&fx);

    let out = review(&fx).ok();
    assert_eq!(out["review"]["status"], "pending", "{out}");
    let found = advice(&out["warnings"]);
    assert_eq!(found.len(), 1, "{out}");
    assert_the_warning(found[0]);

    let out = render(&fx).ok();
    let found = advice(&out["warnings"]);
    assert_eq!(found.len(), 1, "{out}");
    assert_the_warning(found[0]);
}

#[test]
fn a_draft_at_the_advised_tier_or_below_has_no_warning() {
    for tier in ["S1", "S0"] {
        let fx = mine(tier, true);
        history(&fx);
        let out = lint(&fx).ok();
        assert_eq!(out["warnings"], json!(0), "{tier}: {out}");
        assert!(advice(&out["findings"]).is_empty(), "{tier}: {out}");
    }
}

#[test]
fn no_history_means_no_catalog_read() {
    let fx = mine("S2", false);
    let out = lint(&fx).ok();
    assert!(advice(&out["findings"]).is_empty(), "{out}");
    let out = review(&fx).ok();
    assert!(advice(&out["warnings"]).is_empty(), "{out}");
    let out = render(&fx).ok();
    assert!(advice(&out["warnings"]).is_empty(), "{out}");
}

#[test]
fn a_qualifying_history_with_no_readable_catalog_fails_loud() {
    let fx = mine("S2", false);
    history(&fx);
    // No install: the harness puts a deny shim for `pi` first on PATH, so no real pi is reachable.
    for run in [lint(&fx), review(&fx), render(&fx)] {
        let error = refused(&run, "pi_not_found", 1);
        assert_eq!(error["code"], "pi_not_found", "{error}");
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plan_reviews"), 0);
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM events WHERE kind = 'plan-review'"),
        0
    );
    let plan_md = home(&fx).plan_md(&cpd_core::ids::PlanId::parse("mine").unwrap());
    assert!(!plan_md.with_file_name("summary-r1.md").exists());
}

#[test]
fn the_catalog_cache_is_the_only_write_of_plan_lint() {
    let fx = mine("S2", true);
    history(&fx);
    let events = |fx: &Fx| fx.env.query::<i64>("SELECT count(*) FROM events");
    let cache = |fx: &Fx| fx.env.query::<i64>("SELECT count(*) FROM tier_catalog");

    let before = events(&fx);
    let first = lint(&fx);
    assert_eq!(first.ok()["warnings"], json!(1), "{first:?}");
    let cached = cache(&fx);
    let second = lint(&fx);
    second.ok();
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(events(&fx), before);
    assert_eq!(cache(&fx), cached);
}
