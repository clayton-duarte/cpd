//! `plan render` shows cancelled Jobs and PRs (commands.md §4.5, hs-d518): muted, with their status and reason;
//! the counts, the summary and the fingerprint leave them out. The built binary against a temporary `$CPD_HOME`,
//! with the Plan written straight into the database and cancelled through `job cancel` and `pr cancel`.
//!
//! The fixture helpers are shared with `plan_render_waiting` and `context_pending_waiting`.

use crate::common;

use std::fs;

use common::TestEnv;
use common::fx::Fx;
use serde_json::Value;

/// Context `ctx`, Plan `pay` in `dispatch` with its Lead, on an [`Fx`]: the commands run in this process.
pub(crate) fn fx() -> Fx {
    let fx = Fx::new();
    seed(&fx.env);
    fx
}

/// [`fx`] on a [`TestEnv`] (the binary), for a test that needs it (`context_pending_waiting`).
pub(crate) fn env() -> TestEnv {
    let env = TestEnv::new();
    seed(&env);
    env
}

fn seed(env: &TestEnv) {
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"t\"\n").unwrap();
    env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'dispatch', 'lead1', '1999-12-31T00:00:00.000Z', '2000-01-01T00:00:00.000Z');",
    );
}

/// A PR of `pay`: `waiting` (not started) or `dispatch` (started); `base` is a branch, or the Lead Branch of a PR it is stacked on.
pub(crate) fn pr(env: &TestEnv, key: &str, status: &str, base: &str, needs: &[&str]) {
    let started = if status == "waiting" { "NULL, NULL" } else { "'/w', 't'" };
    // the needs are written in the transaction that creates the PR: after it they are Locked
    let needs: String =
        needs.iter().map(|n| format!("INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay', '{key}', '{n}');")).collect();
    env.sql(&format!(
        "INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
         VALUES ('pay', '{key}', '/r/{key}', '{key}', '{base}', '{status}', 'cpd/pay/{key}', {started}, cpd_txn(), 't');{needs}"
    ));
}

/// A Job of `pay`'s PR `pr` in `status`, of `kind`, needing `needs`: written as a draft, given its Dependencies (a Locked Job
/// takes none), then moved to its status one legal step at a time.
pub(crate) fn job_of_kind(env: &TestEnv, pr: &str, key: &str, kind: &str, status: &str, needs: &[&str]) {
    env.sql(&format!(
        "INSERT INTO jobs (plan, pr, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', '{pr}', '{key}', '{kind}', 'Job {key}', 'S2', 'Goal of {key}.', 'Context of {key}.', '[\"src/{key}.rs\"]', '[\"cargo test\"]', '[]',
                 'draft', 0, cpd_txn(), 't');"
    ));
    for need in needs {
        env.sql(&format!("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');"));
    }
    let to = |status: &str| format!("UPDATE jobs SET status = '{status}' WHERE plan = 'pay' AND key = '{key}';");
    match status {
        "draft" => {}
        "queued" => env.sql(&format!("UPDATE jobs SET status = 'queued', fingerprint = printf('%064d', 1) WHERE plan = 'pay' AND key = '{key}';")),
        "merged" => env.sql(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = printf('%064d', 1) WHERE plan = 'pay' AND key = '{key}';{}UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'pay' AND key = '{key}';",
            to("running")
        )),
        other => panic!("no fixture for {other}"),
    }
}

pub(crate) fn job(env: &TestEnv, pr: &str, key: &str, status: &str, needs: &[&str]) {
    job_of_kind(env, pr, key, "work", status, needs);
}

/// `plan render --plan pay`: its output and the text of `plan.md`.
pub(crate) fn render(fx: &Fx) -> (Value, String) {
    let out = fx.cpd(&["plan", "render", "--plan", "pay"]).ok();
    let text = fs::read_to_string(out["path"].as_str().unwrap()).unwrap();
    (out, text)
}

/// The lines of `text` that mention `` `key` `` (or the bare key) as a table row's first cell.
pub(crate) fn row<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(&format!("| `{key}` |")) || l.starts_with(&format!("| {key} |")) || l.starts_with(&format!("| [`{key}`]")))
        .unwrap_or_else(|| panic!("no table row for {key}:\n{text}"))
}

/// `api` (dispatch) with a live Job `a` and a Job `gone` cancelled with a reason.
fn with_cancelled_job() -> Fx {
    let fx = fx();
    pr(&fx.env, "api", "dispatch", "main", &[]);
    job(&fx.env, "api", "a", "queued", &[]);
    job(&fx.env, "api", "gone", "queued", &[]);
    fx.cpd(&["job", "cancel", "gone", "--reason", "not needed any more", "--plan", "pay"]).ok();
    fx
}

#[test]
fn cancelled_job_row_shows_status_and_reason() {
    let fx = with_cancelled_job();
    let (_, text) = render(&fx);
    let gone = row(&text, "gone");
    assert!(gone.contains("cancelled"), "the status: {gone}");
    assert!(gone.contains("not needed any more"), "the reason: {gone}");
    // it keeps its place, in creation order, beside the live Job
    assert!(text.find(row(&text, "a")).unwrap() < text.find(gone).unwrap(), "{text}");
    // it is drawn in no graph, and has no Spec section
    assert!(!text.contains("[\"gone\"]"), "not in the Mermaid graph:\n{text}");
    assert!(!text.contains("### `gone`"), "no Spec section:\n{text}");
}

/// `api` live; `old` a started PR whose Jobs are all cancelled, cancelled by `pr cancel`.
fn with_cancelled_pr(superseded_by: Option<&str>) -> Fx {
    let fx = fx();
    pr(&fx.env, "api", "dispatch", "main", &[]);
    job(&fx.env, "api", "a", "queued", &[]);
    pr(&fx.env, "old", "dispatch", "main", &[]);
    job(&fx.env, "old", "o1", "queued", &[]);
    fx.cpd(&["job", "cancel", "o1", "--reason", "dropped with its PR", "--plan", "pay"]).ok();
    let mut args = vec!["pr", "cancel", "old", "--plan", "pay"];
    if let Some(by) = superseded_by {
        args.extend(["--superseded-by", by]);
    }
    fx.cpd(&args).ok();
    fx
}

#[test]
fn cancelled_pr_is_listed_with_its_jobs() {
    let fx = with_cancelled_pr(None);
    let (_, text) = render(&fx);
    let old = row(&text, "old");
    assert!(old.contains("cancelled"), "the PR's status in its row: {old}");
    assert!(text.find(row(&text, "api")).unwrap() < text.find(old).unwrap(), "after the live PRs:\n{text}");
    // its Job is shown, cancelled, with its reason
    let job = row(&text, "o1");
    assert!(job.contains("cancelled") && job.contains("dropped with its PR"), "{job}");
    // it is in no graph of needs: with one live PR there is no graph at all
    assert!(!text.contains("[\"old\"]"), "{text}");
}

#[test]
fn superseded_pr_names_its_superseder() {
    let fx = with_cancelled_pr(Some("api"));
    let (_, text) = render(&fx);
    let old = row(&text, "old");
    assert!(old.contains("cancelled"), "{old}");
    assert!(old.contains("superseded by api"), "{old}");
}

#[test]
fn summary_counts_leave_cancelled_out() {
    let fx = with_cancelled_pr(None);
    job(&fx.env, "api", "gone", "queued", &[]);
    fx.cpd(&["job", "cancel", "gone", "--reason", "not needed any more", "--plan", "pay"]).ok();
    let before = fx.cpd(&["plan", "show", "--plan", "pay"]).ok();
    let (out, text) = render(&fx);
    // the PRs counted are those that are not cancelled; the fingerprint is the one of the Plan as before
    assert_eq!(out["prs"], 1, "{out}");
    assert_eq!(out["draft_jobs"], 0, "{out}");
    assert_eq!(before["counts"]["cancelled"], 2, "{before}");
    assert_eq!(before["counts"]["queued"], 1, "{before}");
    let summary = fs::read_to_string(out["summary"].as_str().unwrap()).unwrap();
    // the "Full plan (<path>)" line holds a random temp dir that may contain "o1"; only the other lines are checked
    let body: String = summary.lines().filter(|l| !l.starts_with("Full plan (")).collect::<Vec<_>>().join("\n");
    assert!(!body.contains("o1") && !body.contains("gone"), "the summary does not count them: {summary}");
}
