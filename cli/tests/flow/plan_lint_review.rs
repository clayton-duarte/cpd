//! `plan review` and `plan render` with the lint (commands.md §4.5, §4.6, §4.15, contract §2.7): the `pr_empty`
//! refusal, the `warnings` key, and an error keeping its own code. The built binary against a temporary `$CPD_HOME`.

use super::plan_lint::{base, outline, pair, stub};
use super::plan_overlap::{job, pr};
use serde_json::json;

use crate::common::refused;

fn review(fx: &crate::common::fx::Fx) -> crate::common::Run {
    fx.cpd(&["plan", "review", "--plan", "mine", "--title", "T"])
}

#[test]
fn review_refuses_pr_empty_in_phase_plan_and_writes_nothing() {
    let fx = base();
    pr(&fx, "mine", "web", "/r/web", "waiting");
    job(&fx, "mine", "api", "a", "draft", &["src/a.rs"]);
    outline(&fx, &["api"]);

    let error = refused(&review(&fx), "pr_empty", 3);
    assert_eq!(error["details"]["prs"], json!(["web"]), "{error}");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plan_reviews"), 0);
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM events WHERE kind = 'plan-review'"),
        0
    );
    let summary = super::plan_overlap::home(&fx)
        .plan_md(&cpd_core::ids::PlanId::parse("mine").unwrap())
        .with_file_name("summary-r1.md");
    assert!(!summary.exists(), "{}", summary.display());
}

#[test]
fn review_does_not_refuse_pr_empty_after_the_lock() {
    let fx = base();
    fx.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'mine';");
    pr(&fx, "mine", "web", "/r/web", "waiting");
    job(&fx, "mine", "api", "a", "draft", &["src/a.rs"]);
    job(&fx, "mine", "web", "l", "running", &["src/l.rs"]);
    outline(&fx, &["api"]);

    let out = review(&fx).ok();
    assert_eq!(out["review"]["status"], "pending", "{out}");
}

#[test]
fn review_returns_the_warnings_and_is_not_refused_by_them() {
    let fx = pair();
    outline(&fx, &["api"]);

    let out = review(&fx).ok();
    assert!(out.get("review").is_some(), "{out}");
    let warnings = out["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("{out}"));
    assert_eq!(warnings.len(), 1, "{out}");
    let f = &warnings[0];
    assert_eq!(f["rule"], "parallel_write_overlap", "{f}");
    assert_eq!(
        (&f["job"], &f["other"], &f["paths"]),
        (&json!("a"), &json!("b"), &json!(["src/a.rs"])),
        "{f}"
    );

    let again = review(&fx).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(again["warnings"], out["warnings"], "{again}");
}

#[test]
fn render_returns_the_warnings_and_never_refuses() {
    let fx = pair();
    outline(&fx, &["api"]);
    let out = fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    let warnings = out["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("{out}"));
    assert_eq!(warnings.len(), 1, "{out}");
    assert_eq!(warnings[0]["rule"], "parallel_write_overlap", "{out}");

    drop(fx);
    let fx = base();
    stub(&fx, "api", "s");
    let out = fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    assert_eq!(out["warnings"], json!([]), "{out}");
}

#[test]
fn review_refuses_an_error_with_its_own_code_never_lint_failed() {
    let fx = base();
    stub(&fx, "api", "s");
    let error = refused(&review(&fx), "draft_incomplete", 3);
    assert_eq!(
        error["details"]["jobs"],
        json!([{"key": "s", "missing": ["tier", "spec"]}]),
        "{error}"
    );
}

#[test]
fn the_review_summary_and_plan_md_carry_the_warning() {
    let fx = pair();
    outline(&fx, &["api"]);
    let out = review(&fx).ok();
    assert_eq!(out["review"]["status"], "pending", "{out}");
    let home = super::plan_overlap::home(&fx);
    let plan_md = home.plan_md(&cpd_core::ids::PlanId::parse("mine").unwrap());
    let summary = std::fs::read_to_string(plan_md.with_file_name("summary-r1.md")).unwrap();
    let lines: Vec<&str> = summary.lines().collect();
    assert!(lines.contains(&"Lint: 1"), "{summary}");
    assert!(
        lines.contains(
            &"- parallel_write_overlap: a and b both name src/a.rs (a hint, not a conflict prediction)"
        ),
        "{summary}"
    );
    let plan_md = std::fs::read_to_string(&plan_md).unwrap();
    assert!(plan_md.contains("\n## Lint\n\n| Rule | PR | Job | Other | Paths | Message |\n|---|---|---|---|---|---|\n| `parallel_write_overlap` | `api` | `a` | `b` | `src/a.rs` | a and b have no dependency path"), "{plan_md}");

    drop(fx);
    let fx = base();
    stub(&fx, "api", "s");
    fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    let plan_md =
        super::plan_overlap::home(&fx).plan_md(&cpd_core::ids::PlanId::parse("mine").unwrap());
    let summary = std::fs::read_to_string(plan_md.with_file_name("summary.md")).unwrap();
    assert!(summary.lines().any(|l| l == "Lint: none"), "{summary}");
}
