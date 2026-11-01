//! `pr add --shape` after the lock (commands.md §9.4 "The template", hs-d731): a PR that would start at once is refused
//! `draft_parked` without `--jobs`, `--jobs <file.json>` writes the template's non-open Jobs `queued`, and before the lock
//! `--jobs` is refused `usage`. Reuses the world of `pr_crud.rs`: Plan `pay-1` with repos `pay` and `web`.

use std::fs;

use serde_json::{Value, json};

use crate::common::{Run, refused};
use crate::pr_crud::{Crud, crud};

/// The Spec object `job add --spec` takes.
fn spec() -> Value {
    json!({"goal": "Do it.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": ["Everything else."]})
}

/// The `--jobs` object of `shape`: every suffix of the template with Tier `S2` and a Spec (a test Job's with `tests` and `test_files`).
fn jobs_object(shape: &str) -> Value {
    let plain = json!({"tier": "S2", "spec": spec()});
    let mut test = spec();
    test["tests"] = json!(["pr_x::it_works"]);
    test["test_files"] = json!(["cli/tests/pr/pr_x.rs"]);
    match shape {
        "tdd" => json!({"docs": plain, "tests": {"tier": "S2", "spec": test}, "impl": plain}),
        "docs" => json!({"docs": plain}),
        "config" => json!({"config": plain}),
        other => panic!("no shape {other}"),
    }
}

impl Crud {
    fn jobs_file(&self, name: &str, value: &Value) -> String {
        let path = self.env.root().join(name);
        fs::write(&path, value.to_string()).unwrap();
        path.to_str().unwrap().to_owned()
    }

    /// `pr add k --shape <shape> <extra…>` on repo `pay`, in Plan `pay-1`.
    fn add_k(&self, shape: &str, extra: &[&str]) -> Run {
        let mut args = vec![
            "pr",
            "add",
            "k",
            "--repo",
            self.pay.to_str().unwrap(),
            "--title",
            "Add retries",
            "--shape",
            shape,
        ];
        args.extend(extra);
        args.extend(["--plan", "pay-1"]);
        self.cpd(&args)
    }

    /// An unmerged PR `base` of the Plan, as the lock leaves one: what a PR that `--needs base` waits for.
    fn base_pr(&self) {
        self.env.sql(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, shape, lead_branch, lead_worktree, started_at, created_txn, created_at)
               VALUES ('pay-1', 'base', '{}', 'main', 'dispatch', 'free', 'cpd/pay-1/base', '/w/base', 't', cpd_txn(), 't');",
            self.pay.display()
        ));
    }

    /// The counts of `txns`, `events`, `prs` and `jobs`: unchanged by a refusal.
    fn rows(&self) -> (i64, i64, i64, i64) {
        let n = |table: &str| {
            self.env
                .query::<i64>(&format!("SELECT count(*) FROM {table}"))
        };
        (n("txns"), n("events"), n("prs"), n("jobs"))
    }

    /// `[key, kind, title, status, appended, has tier, has fingerprint, needs]` of every Job of PR `k`, in creation order.
    fn rows_of_k(&self) -> Value {
        let keys: String = self.env.query("SELECT group_concat(key, ',') FROM (SELECT key FROM jobs WHERE pr = 'k' ORDER BY rowid)");
        let rows: Vec<Value> = keys
            .split(',')
            .map(|key| {
                let q = |col: &str| self.env.query::<Option<String>>(&format!("SELECT {col} FROM jobs WHERE plan = 'pay-1' AND key = '{key}'"));
                let appended: i64 = self.env.query(&format!("SELECT appended FROM jobs WHERE plan = 'pay-1' AND key = '{key}'"));
                let needs: Option<String> = self.env.query(&format!("SELECT group_concat(needs) FROM dependencies WHERE plan = 'pay-1' AND job = '{key}'"));
                json!([key, q("kind"), q("title"), q("status"), appended, q("tier").is_some(), q("fingerprint").is_some(), needs.unwrap_or_default()])
            })
            .collect();
        json!(rows)
    }
}

fn assert_parked(shape: &str, stubs: &[&str]) {
    let fx = crud("dispatch");
    let before = fx.rows();
    let run = fx.add_k(shape, &["--no-needs"]);
    let error = refused(&run, "draft_parked", 3);
    assert_eq!(
        error["details"],
        json!({"plan": "pay-1", "pr": "k", "jobs": stubs}),
        "{shape}"
    );
    assert_eq!(fx.rows(), before, "{shape}: a refusal wrote something");
}

#[test]
fn tdd_after_lock_without_jobs_is_refused() {
    assert_parked("tdd", &["k-docs", "k-tests", "k-impl"]);
}

#[test]
fn docs_after_lock_without_jobs_is_refused() {
    assert_parked("docs", &["k-docs"]);
}

#[test]
fn config_after_lock_without_jobs_is_refused() {
    assert_parked("config", &["k-config"]);
}

#[test]
fn refusal_writes_nothing() {
    for phase in ["dispatch", "deploy", "done"] {
        let fx = crud(phase);
        let before = fx.rows();
        let plan: String = fx.env.query("SELECT phase FROM plans WHERE id = 'pay-1'");
        refused(&fx.add_k("tdd", &["--no-needs"]), "draft_parked", 3);
        assert_eq!(fx.rows(), before, "{phase}");
        // Not even the reopening of the Plan.
        assert_eq!(
            fx.env
                .query::<String>("SELECT phase FROM plans WHERE id = 'pay-1'"),
            plan,
            "{phase}"
        );
        assert_eq!(
            fx.env
                .query::<i64>("SELECT count(*) FROM jobs WHERE pr = 'k'"),
            0,
            "{phase}"
        );
    }
}

#[test]
fn jobs_queue_the_template_after_lock() {
    let fx = crud("dispatch");
    let file = fx.jobs_file("jobs.json", &jobs_object("tdd"));
    let out = fx.add_k("tdd", &["--no-needs", "--jobs", &file]).ok();
    assert_eq!(
        out["pr"]["jobs"],
        json!(["k-docs", "k-tests", "k-impl", "open-k"])
    );
    assert_eq!(
        fx.rows_of_k(),
        json!([
            [
                "k-docs",
                "docs",
                "Docs: Add retries",
                "queued",
                1,
                true,
                true,
                ""
            ],
            [
                "k-tests",
                "test",
                "Red tests: Add retries",
                "queued",
                1,
                true,
                true,
                "k-docs"
            ],
            [
                "k-impl",
                "impl",
                "Implement: Add retries",
                "queued",
                1,
                true,
                true,
                "k-tests"
            ],
            [
                "open-k",
                "open",
                "Open the pull request: Add retries",
                "draft",
                1,
                true,
                false,
                ""
            ],
        ])
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT tests FROM jobs WHERE key = 'k-tests'"),
        "[\"pr_x::it_works\"]"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT test_files FROM jobs WHERE key = 'k-tests'"),
        "[\"cli/tests/pr/pr_x.rs\"]"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT goal FROM jobs WHERE key = 'k-impl'"),
        "Do it."
    );
    // One `job-added` Event per Job, the three queued, the opener a draft.
    let added = fx.cpd(&["events", "list", "--kind", "job-added"]).ok();
    let statuses: Vec<(Value, Value)> = added["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["job"].clone(), e["data"]["status"].clone()))
        .collect();
    assert_eq!(
        statuses,
        [
            (json!("k-docs"), json!("queued")),
            (json!("k-tests"), json!("queued")),
            (json!("k-impl"), json!("queued")),
            (json!("open-k"), json!("draft"))
        ]
    );
}

#[test]
fn jobs_with_missing_key_is_refused() {
    let fx = crud("dispatch");
    let before = fx.rows();
    let mut object = jobs_object("tdd");
    object.as_object_mut().unwrap().remove("tests");
    let file = fx.jobs_file("jobs.json", &object);
    // The missing key is the file's fault, not the flag's: not `usage`.
    let run = fx.add_k("tdd", &["--no-needs", "--jobs", &file]);
    let error = run.error();
    assert_ne!(error["code"], "usage", "{run:?}");
    assert_ne!(error["code"], "draft_parked", "{run:?}");
    assert_eq!(run.exit, 2, "{run:?}");
    assert_eq!(fx.rows(), before);
}

#[test]
fn jobs_with_an_extra_key_is_refused() {
    let fx = crud("dispatch");
    let before = fx.rows();
    let mut object = jobs_object("docs");
    object["impl"] = json!({"tier": "S2", "spec": spec()});
    let file = fx.jobs_file("jobs.json", &object);
    let run = fx.add_k("docs", &["--no-needs", "--jobs", &file]);
    let error = run.error();
    assert_ne!(error["code"], "usage", "{run:?}");
    assert_eq!(run.exit, 2, "{run:?}");
    assert_eq!(fx.rows(), before);
}

#[test]
fn jobs_with_a_bad_tier_is_refused_tier_invalid() {
    let fx = crud("dispatch");
    let before = fx.rows();
    let mut object = jobs_object("docs");
    object["docs"]["tier"] = json!("Z9");
    let file = fx.jobs_file("jobs.json", &object);
    refused(
        &fx.add_k("docs", &["--no-needs", "--jobs", &file]),
        "tier_invalid",
        2,
    );
    assert_eq!(fx.rows(), before);
}

#[test]
fn jobs_with_a_bad_spec_is_refused_spec_invalid() {
    let fx = crud("dispatch");
    let before = fx.rows();
    let mut object = jobs_object("docs");
    object["docs"]["spec"] = json!({"goal": "Do it."});
    let file = fx.jobs_file("jobs.json", &object);
    refused(
        &fx.add_k("docs", &["--no-needs", "--jobs", &file]),
        "spec_invalid",
        2,
    );
    assert_eq!(fx.rows(), before);
}

#[test]
fn jobs_before_lock_is_refused() {
    for phase in ["plan", "grounding"] {
        let fx = crud(phase);
        let before = fx.rows();
        let file = fx.jobs_file("jobs.json", &jobs_object("tdd"));
        let error = refused(
            &fx.add_k("tdd", &["--no-needs", "--jobs", &file]),
            "usage",
            2,
        );
        // The flag exists: the refusal is the phase's, not the parser's `unexpected argument`.
        let message = error["message"].as_str().unwrap();
        assert!(
            message.contains("--jobs") && !message.contains("unexpected argument"),
            "{phase}: {message}"
        );
        assert_eq!(fx.rows(), before, "{phase}: the template is unchanged");
    }
}

#[test]
fn jobs_on_a_pr_that_waits_for_another_pr_queue_the_jobs_too() {
    let fx = crud("dispatch");
    fx.base_pr();
    let file = fx.jobs_file("jobs.json", &jobs_object("tdd"));
    fx.add_k("tdd", &["--needs", "base", "--jobs", &file]).ok();
    let statuses: String = fx.env.query("SELECT group_concat(key || '=' || status) FROM (SELECT key, status FROM jobs WHERE pr = 'k' ORDER BY rowid)");
    assert_eq!(
        statuses,
        "k-docs=queued,k-tests=queued,k-impl=queued,open-k=draft"
    );
}

// Guards: these pass before the change and must keep passing.

#[test]
fn a_pr_that_waits_for_another_pr_still_lays_out_the_stubs() {
    let fx = crud("dispatch");
    fx.base_pr();
    fx.add_k("tdd", &["--needs", "base"]).ok();
    assert_eq!(
        fx.rows_of_k(),
        json!([
            [
                "k-docs",
                "docs",
                "Docs: Add retries",
                "draft",
                1,
                false,
                false,
                ""
            ],
            [
                "k-tests",
                "test",
                "Red tests: Add retries",
                "draft",
                1,
                false,
                false,
                "k-docs"
            ],
            [
                "k-impl",
                "impl",
                "Implement: Add retries",
                "draft",
                1,
                false,
                false,
                "k-tests"
            ],
            [
                "open-k",
                "open",
                "Open the pull request: Add retries",
                "draft",
                1,
                true,
                false,
                ""
            ],
        ])
    );
}

#[test]
fn in_phase_plan_the_template_is_unchanged() {
    let fx = crud("plan");
    fx.add_k("tdd", &["--no-needs"]).ok();
    assert_eq!(
        fx.rows_of_k(),
        json!([
            [
                "k-docs",
                "docs",
                "Docs: Add retries",
                "draft",
                0,
                false,
                false,
                ""
            ],
            [
                "k-tests",
                "test",
                "Red tests: Add retries",
                "draft",
                0,
                false,
                false,
                "k-docs"
            ],
            [
                "k-impl",
                "impl",
                "Implement: Add retries",
                "draft",
                0,
                false,
                false,
                "k-tests"
            ],
            [
                "open-k",
                "open",
                "Open the pull request: Add retries",
                "draft",
                0,
                true,
                false,
                ""
            ],
        ])
    );
}
