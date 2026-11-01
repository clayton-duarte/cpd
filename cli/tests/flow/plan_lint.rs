//! `cpd plan lint` (commands.md §4.15, contract §2.7): the built binary against a temporary `$CPD_HOME` with Plans
//! written straight into the database.

use crate::common;

use std::fs;
use std::path::Path;

use super::plan_overlap::{home, job, plan, pr};
use common::fx::Fx;
use common::{Run, refused};
use cpd_core::ids::PlanId;
use serde_json::{Value, json};

const MESSAGE: &str = "a and b have no dependency path and both name src/a.rs. A hint, not a conflict prediction: on the ledger of 2026-10-30, 5 of 209 such pairs (2.4%) hit a merge conflict. Settle it with cpd job edit a --needs b, or narrow touches.";

/// Context `ctx` and Plan `mine` in phase `plan` with one waiting PR `api`.
fn base() -> Fx {
    let fx = Fx::new();
    fs::write(home(&fx).config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');");
    plan(&fx, "mine", "plan", false);
    pr(&fx, "mine", "api", "/r/api", "waiting");
    fx
}

/// [`base`] with the overlap pair `a` and `b`, both touching `src/a.rs`.
fn pair() -> Fx {
    let fx = base();
    job(&fx, "mine", "api", "a", "draft", &["src/a.rs"]);
    job(&fx, "mine", "api", "b", "draft", &["src/a.rs"]);
    fx
}

/// The outline of the Plan, set by SQL, its `done_when` covering `prs`.
fn outline(fx: &Fx, prs: &[&str]) {
    let done: serde_json::Map<String, Value> = prs.iter().map(|k| ((*k).to_owned(), json!(["Done"]))).collect();
    let text = json!({"goal": "Goal", "assumptions": [], "decided": [], "decisions": [], "risks": [], "out_of_scope": [], "done_when": done}).to_string();
    fx.env.sql(&format!("UPDATE plans SET outline = '{text}' WHERE id = 'mine';"));
}

/// A stub draft: no Tier, no Spec.
fn stub(fx: &Fx, pr: &str, key: &str) {
    fx.env.sql(&format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('mine', '{pr}', '{key}', 'Title {key}', NULL, NULL, NULL, NULL, NULL, NULL, 'draft', 0, cpd_txn(), 't');"
    ));
}

fn lint(fx: &Fx) -> Run {
    fx.cpd(&["plan", "lint", "--plan", "mine"])
}

fn finding_keys(f: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = f.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    keys
}

#[test]
fn an_overlap_is_a_warning_and_the_exit_is_zero() {
    let fx = pair();
    outline(&fx, &["api"]);

    let out = lint(&fx).ok();
    let mut keys: Vec<&str> = out.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["errors", "findings", "ok", "plan", "warnings"], "{out}");
    assert_eq!((&out["ok"], &out["plan"], &out["errors"], &out["warnings"]), (&json!(true), &json!("mine"), &json!(0), &json!(1)));
    let findings = out["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{out}");
    let f = &findings[0];
    assert_eq!(finding_keys(f), ["details", "job", "message", "other", "paths", "pr", "rule", "severity"], "{f}");
    assert_eq!((&f["rule"], &f["severity"], &f["pr"]), (&json!("parallel_write_overlap"), &json!("warn"), &json!("api")));
    assert_eq!((&f["job"], &f["other"], &f["paths"], &f["details"]), (&json!("a"), &json!("b"), &json!(["src/a.rs"]), &json!({})));
    let message = f["message"].as_str().unwrap();
    for part in ["not a conflict prediction", "5 of 209", "2.4%"] {
        assert!(message.contains(part), "{message}");
    }
    assert_eq!(message, MESSAGE);
}

#[test]
fn an_error_finding_is_refused_lint_failed_with_every_finding() {
    let fx = pair();
    stub(&fx, "api", "s");

    let run = lint(&fx);
    let error = refused(&run, "lint_failed", 3);
    assert_eq!(
        error["message"],
        "Plan mine has 2 lint errors: draft_incomplete, outline_missing. Fix them, then ask again."
    );
    let d = &error["details"];
    assert_eq!((&d["plan"], &d["errors"], &d["warnings"]), (&json!("mine"), &json!(2), &json!(1)), "{d}");
    let findings = d["findings"].as_array().unwrap();
    let rules: Vec<&Value> = findings.iter().map(|f| &f["rule"]).collect();
    assert_eq!(rules, ["draft_incomplete", "outline_missing", "parallel_write_overlap"], "{d}");
    assert_eq!(findings[0]["severity"], "error");
    assert_eq!(findings[0]["details"]["jobs"], json!([{"key": "s", "missing": ["tier", "spec"]}]));
    assert_eq!(findings[1]["severity"], "error");
    assert_eq!(findings[1]["details"]["next"], "cpd plan outline --file <outline.json>");
    for f in &findings[..2] {
        assert_eq!((&f["job"], &f["other"], &f["paths"]), (&Value::Null, &Value::Null, &json!([])), "{f}");
    }
    assert_eq!(findings[2]["severity"], "warn");
}

#[test]
fn pr_empty_is_reported_in_phase_plan_and_not_in_dispatch() {
    let fx = base();
    outline(&fx, &[]);
    let error = refused(&lint(&fx), "lint_failed", 3);
    let findings = error["details"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{error}");
    assert_eq!((&findings[0]["rule"], &findings[0]["severity"]), (&json!("pr_empty"), &json!("error")));
    assert_eq!(findings[0]["details"]["prs"], json!(["api"]));

    let fx = Fx::new();
    fs::write(home(&fx).config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');");
    plan(&fx, "mine", "dispatch", false);
    pr(&fx, "mine", "api", "/r/api", "dispatch");
    outline(&fx, &[]);
    let out = lint(&fx).ok();
    assert_eq!((&out["errors"], &out["findings"]), (&json!(0), &json!([])), "{out}");
}

#[test]
fn a_dependency_cycle_is_an_error_finding_with_the_cycle() {
    let fx = pair();
    outline(&fx, &["api"]);
    fx.env.sql("INSERT INTO dependencies (plan, job, needs) VALUES ('mine', 'a', 'b'), ('mine', 'b', 'a');");

    let error = refused(&lint(&fx), "lint_failed", 3);
    let findings = error["details"]["findings"].as_array().unwrap();
    let f = findings.iter().find(|f| f["rule"] == "dependency_cycle").unwrap_or_else(|| panic!("{error}"));
    assert_eq!(f["severity"], "error");
    let cycle = f["details"]["cycle"].as_array().unwrap();
    assert_eq!(cycle.first(), cycle.last(), "{f}");
    for key in ["a", "b"] {
        assert!(cycle.contains(&json!(key)), "{f}");
    }
}

#[test]
fn a_grounding_plan_is_illegal_phase_and_an_unknown_plan_is_not_found() {
    let fx = base();
    fx.env.sql("UPDATE plans SET phase = 'grounding' WHERE id = 'mine';");
    let error = refused(&lint(&fx), "illegal_phase", 3);
    assert_eq!((&error["details"]["plan"], &error["details"]["phase"]), (&json!("mine"), &json!("grounding")), "{error}");

    refused(&fx.cpd(&["plan", "lint", "--plan", "nope"]), "plan_not_found", 4);
    refused(&fx.cpd(&["plan", "lint"]), "plan_required", 2);
}

fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(read) = fs::read_dir(dir) else { return out };
    for entry in read {
        let path = entry.unwrap().path();
        out.push(path.display().to_string());
        if path.is_dir() {
            out.extend(files(&path));
        }
    }
    out.sort();
    out
}

#[test]
fn lint_is_read_only_and_prints_the_same_twice() {
    let fx = pair();
    outline(&fx, &["api"]);
    let dir = home(&fx).plan_dir(&PlanId::parse("mine").unwrap());
    let state = |fx: &Fx| {
        (
            fx.env.query::<i64>("SELECT count(*) FROM events"),
            fx.env.query::<i64>("SELECT count(*) FROM txns"),
            files(&dir),
        )
    };
    let before = state(&fx);

    let first = lint(&fx);
    let second = lint(&fx);
    first.ok();
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(state(&fx), before);
    assert!(before.2.is_empty(), "{:?}", before.2);
}
