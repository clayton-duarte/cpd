//! The overlap warning of `plan render` and the Plan review (commands.md §4.5,
//! §4.6, B4-A3): the built binary against a temporary `$CPD_HOME` with Plans
//! written straight into the database.

mod common;

use std::fs;

use common::TestEnv;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use serde_json::{Value, json};

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

/// A Plan in `phase`, its Lead Session beside it; `archived` as a flag.
fn plan(env: &TestEnv, id: &str, phase: &str, archived: bool) {
    let locked = if matches!(phase, "grounding" | "plan") { "NULL" } else { "'2000-01-01T00:00:00.000Z'" };
    let done = if phase == "done" { "'2000-01-02T00:00:00.000Z'" } else { "NULL" };
    let archived = if archived { "'2000-01-03T00:00:00.000Z'" } else { "NULL" };
    env.sql(
        &format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead-{id}', 'lead', 'ctx', '{id}', '{id}', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at, archived_at)
               VALUES ('{id}', 'ctx', 'Title of {id}', '{phase}', 'lead-{id}', '2000-01-01T00:00:00.000Z', {locked}, {done}, {archived});"
        ),
    );
}

/// A PR of `plan` in `repo`: `waiting` for a Plan still being planned, else `dispatch`, `merged` or `cancelled`.
fn pr(env: &TestEnv, plan: &str, key: &str, repo: &str, status: &str) {
    let none = "NULL, NULL, NULL, NULL";
    let (worktree, started, pull, merged) = match status {
        "waiting" | "cancelled" => ("NULL", "NULL", none.to_owned(), "NULL"),
        "dispatch" => ("'/w'", "'t'", none.to_owned(), "NULL"),
        "merged" => {
            // A merged PR names its stored GitHub row, one number each.
            env.sql(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/api', (SELECT COALESCE(MAX(number), 0) + 1 FROM pull_requests), 'u', 'Old', 0, 'merged', 'none', 'none', 't', 't', 't');",
            );
            ("'/w'", "'t'", "(SELECT MAX(number) FROM pull_requests), 'u', 't', (SELECT MAX(id) FROM pull_requests)".to_owned(), "'t'")
        }
        other => panic!("no fixture for {other}"),
    };
    env.sql(
        &format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                              created_txn, created_at, started_at, merged_at)
             VALUES ('{plan}', '{key}', '{repo}', 'main', '{status}', 'cpd/{plan}/{key}', {worktree}, {pull}, cpd_txn(), 't', {started}, {merged});"
        ),
    );
}

/// A Job of PR `pr` in `status` touching `touches`.
fn job(env: &TestEnv, plan: &str, pr: &str, key: &str, status: &str, touches: &[&str]) {
    env.sql(
        &format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               merged_sha, failed_cause, cancel_reason, cancel_requested_at, created_txn, created_at)
             VALUES ('{plan}', '{pr}', '{key}', 'Title {key}', 'S2', 'Goal.', 'Context.', '{touches}',
                     '[\"cargo test\"]', '[]', '{status}', 0,
                     iif('{status}' IN ('draft', 'cancelled'), NULL, '{FP}'),
                     iif('{status}' = 'merged', 'abc123', NULL), iif('{status}' = 'failed', 'check_failed', NULL),
                     iif('{status}' = 'cancelled', 'x', NULL), iif('{status}' = 'cancelled', 't', NULL), cpd_txn(), 't');",
            touches = json!(touches)
        ),
    );
}

/// Context `ctx`, and Plan `mine` in phase `plan` with one PR `api` in `/r/api` and the draft Job `x` touching `core/src/x.rs`.
fn fixture() -> TestEnv {
    let env = TestEnv::new();
    fs::write(home(&env).config(), "version = 1\nmachine = \"t\"\n").unwrap();
    env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't'), ('other', 'Other', 't', 't');");
    plan(&env, "mine", "plan", false);
    pr(&env, "mine", "api", "/r/api", "waiting");
    job(&env, "mine", "api", "x", "draft", &["core/src/x.rs"]);
    env
}

fn render(env: &TestEnv) -> (Value, String) {
    let out = env.cpd(&["plan", "render", "--plan", "mine"]).ok();
    let text = fs::read_to_string(home(env).plan_md(&PlanId::parse("mine").unwrap())).unwrap();
    (out["overlaps"].clone(), text)
}

const NONE: &str = "No other active Plan touches these files.";

#[test]
fn a_directory_containing_the_path_is_listed_in_the_file_and_the_json() {
    let env = fixture();
    plan(&env, "busy", "dispatch", false);
    pr(&env, "busy", "svc", "/r/api", "dispatch");
    job(&env, "busy", "svc", "y", "queued", &["docs/", "core/src/"]);

    let (overlaps, text) = render(&env);
    assert_eq!(
        overlaps,
        json!([{"job": "x", "path": "core/src/x.rs", "plan": "busy", "pr": "svc", "other_job": "y", "status": "queued", "other_path": "core/src/"}])
    );
    let table = "## Overlaps\n\n| Job | Path | Other Plan | PR | Job | Status |\n|---|---|---|---|---|---|\n| `x` | `core/src/x.rs` | `busy` | `svc` | `y` | queued |\n";
    assert!(text.contains(table), "{text}");
    assert!(!text.contains(NONE), "{text}");
    // After the PRs table, before the PR sections.
    let (prs, overlaps, section) = (text.find("## PRs").unwrap(), text.find("## Overlaps").unwrap(), text.find("## PR api").unwrap());
    assert!(prs < overlaps && overlaps < section, "{text}");
}

#[test]
fn an_equal_path_and_a_file_inside_this_plans_directory_overlap() {
    let env = fixture();
    job(&env, "mine", "api", "dir", "draft", &["docs/"]);
    plan(&env, "busy", "dispatch", false);
    pr(&env, "busy", "svc", "/r/api", "dispatch");
    job(&env, "busy", "svc", "same", "running", &["core/src/x.rs"]);
    job(&env, "busy", "svc", "inside", "failed", &["docs/guide/a.md"]);

    let (overlaps, _) = render(&env);
    assert_eq!(
        overlaps,
        json!([
            {"job": "x", "path": "core/src/x.rs", "plan": "busy", "pr": "svc", "other_job": "same", "status": "running", "other_path": "core/src/x.rs"},
            {"job": "dir", "path": "docs/", "plan": "busy", "pr": "svc", "other_job": "inside", "status": "failed", "other_path": "docs/guide/a.md"},
        ])
    );
}

#[test]
fn a_merged_job_on_an_unmerged_pr_is_listed() {
    let env = fixture();
    plan(&env, "busy", "dispatch", false);
    pr(&env, "busy", "svc", "/r/api", "dispatch");
    job(&env, "busy", "svc", "y", "merged", &["core/src/x.rs"]);
    job(&env, "busy", "svc", "z", "draft", &["core/src/x.rs"]);

    let (overlaps, _) = render(&env);
    let statuses: Vec<&str> = overlaps.as_array().unwrap().iter().map(|o| o["status"].as_str().unwrap()).collect();
    assert_eq!(statuses, ["merged", "draft"]);
}

#[test]
fn no_overlap_says_so() {
    let env = fixture();
    plan(&env, "busy", "dispatch", false);
    pr(&env, "busy", "svc", "/r/api", "dispatch");
    job(&env, "busy", "svc", "y", "queued", &["core/src/y.rs", "core/src", "cli/"]);

    let (overlaps, text) = render(&env);
    assert_eq!(overlaps, json!([]));
    assert!(text.contains(&format!("## Overlaps\n\n{NONE}\n")), "{text}");
    assert!(!text.contains("| Other Plan |"), "{text}");
}

#[test]
fn done_and_archived_plans_merged_and_cancelled_prs_cancelled_jobs_and_other_repos_are_not_listed() {
    let env = fixture();
    let touches: &[&str] = &["core/src/"];
    plan(&env, "finished", "done", false);
    pr(&env, "finished", "svc", "/r/api", "merged");
    job(&env, "finished", "svc", "a", "merged", touches);

    plan(&env, "shelved", "dispatch", true);
    pr(&env, "shelved", "svc", "/r/api", "dispatch");
    job(&env, "shelved", "svc", "b", "queued", touches);

    plan(&env, "shipped", "dispatch", false);
    pr(&env, "shipped", "old", "/r/api", "merged");
    job(&env, "shipped", "old", "c", "merged", touches);
    pr(&env, "shipped", "gone", "/r/api", "cancelled");
    job(&env, "shipped", "gone", "d", "cancelled", touches);

    plan(&env, "pruned", "dispatch", false);
    pr(&env, "pruned", "svc", "/r/api", "dispatch");
    job(&env, "pruned", "svc", "e", "cancelled", touches);

    plan(&env, "elsewhere", "dispatch", false);
    pr(&env, "elsewhere", "svc", "/r/web", "dispatch");
    job(&env, "elsewhere", "svc", "f", "queued", touches);

    let (overlaps, text) = render(&env);
    assert_eq!(overlaps, json!([]));
    assert!(text.contains(NONE), "{text}");
}

#[test]
fn a_plan_in_another_context_is_listed_and_this_plans_own_jobs_are_not() {
    let env = fixture();
    // Another Job of this very Plan, in another PR of the same repo.
    pr(&env, "mine", "second", "/r/api", "waiting");
    job(&env, "mine", "second", "x2", "draft", &["core/src/"]);
    env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-far', 'lead', 'other', 'far', 'far', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('far', 'other', 'Far', 'grounding', 'lead-far', '2000-01-01T00:00:00.000Z', NULL);",
    );
    pr(&env, "far", "svc", "/r/api", "waiting");
    job(&env, "far", "svc", "y", "draft", &["core/src/x.rs"]);

    let (overlaps, _) = render(&env);
    let listed: Vec<(&str, &str)> = overlaps.as_array().unwrap().iter().map(|o| (o["job"].as_str().unwrap(), o["plan"].as_str().unwrap())).collect();
    assert_eq!(listed, [("x", "far"), ("x2", "far")]);
}

#[test]
fn a_cancelled_pr_or_job_of_this_plan_is_not_compared() {
    let env = fixture();
    pr(&env, "mine", "dead", "/r/api", "cancelled");
    job(&env, "mine", "dead", "z", "cancelled", &["docs/a.md"]);
    job(&env, "mine", "api", "w", "cancelled", &["docs/b.md"]);
    plan(&env, "busy", "dispatch", false);
    pr(&env, "busy", "svc", "/r/api", "dispatch");
    job(&env, "busy", "svc", "y", "queued", &["docs/"]);

    let (overlaps, _) = render(&env);
    assert_eq!(overlaps, json!([]));
}

#[test]
fn the_plan_review_file_carries_the_overlaps_and_nothing_is_refused() {
    let env = fixture();
    plan(&env, "busy", "dispatch", false);
    pr(&env, "busy", "svc", "/r/api", "dispatch");
    job(&env, "busy", "svc", "y", "queued", &["core/src/"]);

    let out = env.cpd(&["plan", "review", "--plan", "mine"]).ok();
    let text = fs::read_to_string(out["path"].as_str().unwrap()).unwrap();
    assert!(text.contains("| `x` | `core/src/x.rs` | `busy` | `svc` | `y` | queued |"), "{text}");
}
