//! The overlap warning of `plan render` and the Plan review (commands.md §4.5,
//! §4.6, B4-A3, hs-n201): the built binary against a temporary `$CPD_HOME` with
//! Plans written straight into the database.

use crate::common;

use std::fs;

use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use serde_json::{Value, json};

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub(super) fn home(fx: &Fx) -> CpdHome {
    CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap()
}

/// A Plan in `phase`, its Lead Session beside it; `archived` as a flag.
pub(super) fn plan(fx: &Fx, id: &str, phase: &str, archived: bool) {
    let locked = if matches!(phase, "grounding" | "plan") {
        "NULL"
    } else {
        "'2000-01-01T00:00:00.000Z'"
    };
    let done = if phase == "done" {
        "'2000-01-02T00:00:00.000Z'"
    } else {
        "NULL"
    };
    let archived = if archived {
        "'2000-01-03T00:00:00.000Z'"
    } else {
        "NULL"
    };
    fx.env.sql(
        &format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead-{id}', 'lead', 'ctx', '{id}', '{id}', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at, archived_at)
               VALUES ('{id}', 'ctx', 'Title of {id}', '{phase}', 'lead-{id}', '2000-01-01T00:00:00.000Z', {locked}, {done}, {archived});"
        ),
    );
}

/// A PR of `plan` in `repo`: `waiting` for a Plan still being planned, else `dispatch`, `merged` or `cancelled`.
pub(super) fn pr(fx: &Fx, plan: &str, key: &str, repo: &str, status: &str) {
    let none = "NULL, NULL, NULL, NULL";
    let (worktree, started, pull, merged) = match status {
        "waiting" | "cancelled" => ("NULL", "NULL", none.to_owned(), "NULL"),
        "dispatch" => ("'/w'", "'t'", none.to_owned(), "NULL"),
        "merged" => {
            // A merged PR names its stored GitHub row, one number each.
            fx.env.sql(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/api', (SELECT COALESCE(MAX(number), 0) + 1 FROM pull_requests), 'u', 'Old', 0, 'merged', 'none', 'none', 't', 't', 't');",
            );
            ("'/w'", "'t'", "(SELECT MAX(number) FROM pull_requests), 'u', 't', (SELECT MAX(id) FROM pull_requests)".to_owned(), "'t'")
        }
        other => panic!("no fixture for {other}"),
    };
    fx.env.sql(
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                              created_txn, created_at, started_at, merged_at)
             VALUES ('{plan}', '{key}', '{repo}', 'main', '{status}', 'cpd/{plan}/{key}', {worktree}, {pull}, cpd_txn(), 't', {started}, {merged});"
        ),
    );
}

/// A Job of PR `pr` in `status` touching `touches`.
pub(super) fn job(fx: &Fx, plan: &str, pr: &str, key: &str, status: &str, touches: &[&str]) {
    fx.env.sql(
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
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
fn fixture() -> Fx {
    let fx = Fx::new();
    fs::write(home(&fx).config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't'), ('other', 'Other', 't', 't');");
    plan(&fx, "mine", "plan", false);
    pr(&fx, "mine", "api", "/r/api", "waiting");
    job(&fx, "mine", "api", "x", "draft", &["core/src/x.rs"]);
    fx
}

fn render(fx: &Fx) -> (Value, String) {
    let out = fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    let text = fs::read_to_string(home(fx).plan_md(&PlanId::parse("mine").unwrap())).unwrap();
    (out["overlaps"].clone(), text)
}

const NONE: &str = "No other active Plan touches these files.";

/// The table of `## Overlaps` up to its rows: one row per this Job, other Plan and other PR.
const HEADER: &str =
    "| Job | Other Plan | PR | Its Jobs (status) | Paths |\n|---|---|---|---|---|\n";

#[test]
fn a_directory_containing_the_path_is_listed_in_the_file_and_the_json() {
    let fx = fixture();
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(&fx, "busy", "svc", "y", "queued", &["docs/", "core/src/"]);

    let (overlaps, text) = render(&fx);
    assert_eq!(
        overlaps,
        json!([{"job": "x", "plan": "busy", "pr": "svc", "other_jobs": [{"key": "y", "status": "queued"}], "paths": ["core/src/x.rs"]}])
    );
    let table = format!(
        "## Overlaps\n\n{HEADER}| `x` | `busy` | `svc` | `y` (queued) | `core/src/x.rs` |\n"
    );
    assert!(text.contains(&table), "{text}");
    assert!(!text.contains(NONE), "{text}");
    // After the PRs table, before the PR sections.
    let (prs, overlaps, section) = (text.find("## Workflows").unwrap(), text.find("## Overlaps").unwrap(), text.find("## Workflow api").unwrap());
    assert!(prs < overlaps && overlaps < section, "{text}");
}

#[test]
fn an_equal_path_and_a_file_inside_this_plans_directory_overlap() {
    let fx = fixture();
    job(&fx, "mine", "api", "dir", "draft", &["docs/"]);
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(&fx, "busy", "svc", "same", "running", &["core/src/x.rs"]);
    job(&fx, "busy", "svc", "inside", "failed", &["docs/guide/a.md"]);

    // By this Job's key: `dir` was added after `x`, and comes first.
    let (overlaps, _) = render(&fx);
    assert_eq!(
        overlaps,
        json!([
            {"job": "dir", "plan": "busy", "pr": "svc", "other_jobs": [{"key": "inside", "status": "failed"}], "paths": ["docs/"]},
            {"job": "x", "plan": "busy", "pr": "svc", "other_jobs": [{"key": "same", "status": "running"}], "paths": ["core/src/x.rs"]},
        ])
    );
}

#[test]
fn a_merged_job_on_an_unmerged_pr_is_listed() {
    let fx = fixture();
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(&fx, "busy", "svc", "y", "merged", &["core/src/x.rs"]);

    let (overlaps, _) = render(&fx);
    assert_eq!(
        overlaps,
        json!([{"job": "x", "plan": "busy", "pr": "svc", "other_jobs": [{"key": "y", "status": "merged"}], "paths": ["core/src/x.rs"]}])
    );
}

#[test]
fn two_overlapping_jobs_of_one_other_pr_give_one_row_listing_both() {
    let fx = fixture();
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    // In the order they were added, not by key; `w` overlaps nothing.
    job(&fx, "busy", "svc", "z", "running", &["core/src/"]);
    job(&fx, "busy", "svc", "y", "queued", &["core/src/x.rs"]);
    job(&fx, "busy", "svc", "w", "queued", &["cli/"]);

    let (overlaps, text) = render(&fx);
    assert_eq!(
        overlaps,
        json!([{"job": "x", "plan": "busy", "pr": "svc", "other_jobs": [{"key": "z", "status": "running"}, {"key": "y", "status": "queued"}], "paths": ["core/src/x.rs"]}])
    );
    let table = format!(
        "## Overlaps\n\n{HEADER}| `x` | `busy` | `svc` | `z` (running), `y` (queued) | `core/src/x.rs` |\n"
    );
    assert!(text.contains(&table), "{text}");
}

#[test]
fn paths_are_listed_once_and_the_table_caps_them_at_five() {
    let fx = fixture();
    job(
        &fx,
        "mine",
        "api",
        "wide",
        "draft",
        &[
            "a/1.rs",
            "a/2.rs",
            "a/1.rs",
            "a/3.rs",
            "elsewhere/",
            "a/4.rs",
            "a/5.rs",
            "a/6.rs",
            "a/7.rs",
        ],
    );
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(&fx, "busy", "svc", "dir", "queued", &["a/"]);
    job(&fx, "busy", "svc", "same", "queued", &["a/1.rs", "a/2.rs"]);

    // Two Jobs overlap it on many paths, and it is one row: the JSON has every path once, the table five and the count of the rest.
    let (overlaps, text) = render(&fx);
    assert_eq!(
        overlaps,
        json!([{
            "job": "wide", "plan": "busy", "pr": "svc",
            "other_jobs": [{"key": "dir", "status": "queued"}, {"key": "same", "status": "queued"}],
            "paths": ["a/1.rs", "a/2.rs", "a/3.rs", "a/4.rs", "a/5.rs", "a/6.rs", "a/7.rs"],
        }])
    );
    let table = format!(
        "## Overlaps\n\n{HEADER}| `wide` | `busy` | `svc` | `dir` (queued), `same` (queued) | `a/1.rs`, `a/2.rs`, `a/3.rs`, `a/4.rs`, `a/5.rs`, +2 |\n"
    );
    assert!(text.contains(&table), "{text}");
}

#[test]
fn rows_are_ordered_by_this_jobs_key_then_the_other_plan_then_the_pr() {
    let fx = fixture();
    // `x` (the fixture) was added first; `b` and `a` come after it.
    job(&fx, "mine", "api", "b", "draft", &["core/src/"]);
    job(&fx, "mine", "api", "a", "draft", &["docs/"]);
    // The other Plans and PRs are added in the reverse of their order.
    plan(&fx, "beta", "dispatch", false);
    pr(&fx, "beta", "p2", "/r/api", "dispatch");
    job(&fx, "beta", "p2", "n", "queued", &["core/src/"]);
    pr(&fx, "beta", "p1", "/r/api", "dispatch");
    job(&fx, "beta", "p1", "m", "queued", &["core/src/"]);
    plan(&fx, "alpha", "dispatch", false);
    pr(&fx, "alpha", "p9", "/r/api", "dispatch");
    job(&fx, "alpha", "p9", "l", "queued", &["core/src/"]);

    let (overlaps, _) = render(&fx);
    let listed: Vec<(&str, &str, &str)> = overlaps
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["job"].as_str().unwrap(),
                o["plan"].as_str().unwrap(),
                o["pr"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("b", "alpha", "p9"),
            ("b", "beta", "p1"),
            ("b", "beta", "p2"),
            ("x", "alpha", "p9"),
            ("x", "beta", "p1"),
            ("x", "beta", "p2")
        ]
    );
}

#[test]
fn the_jobs_of_this_plans_merged_pr_are_not_checked() {
    let fx = fixture();
    let touches: &[&str] = &["core/src/"];
    // On its base already: `m` is no overlap, whatever the other Plan does.
    pr(&fx, "mine", "old", "/r/api", "merged");
    job(&fx, "mine", "old", "m", "merged", touches);
    // A Job merged on the Lead Branch of a PR that is not merged is not on its base yet.
    pr(&fx, "mine", "live", "/r/api", "dispatch");
    job(&fx, "mine", "live", "landed", "merged", touches);
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(&fx, "busy", "svc", "y", "queued", touches);

    let (overlaps, text) = render(&fx);
    let listed: Vec<&str> = overlaps
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["job"].as_str().unwrap())
        .collect();
    assert_eq!(listed, ["landed", "x"]);
    let table = format!(
        "## Overlaps\n\n{HEADER}| `landed` | `busy` | `svc` | `y` (queued) | `core/src/` |\n| `x` | `busy` | `svc` | `y` (queued) | `core/src/x.rs` |\n"
    );
    assert!(text.contains(&table), "{text}");
}

#[test]
fn no_overlap_says_so() {
    let fx = fixture();
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(
        &fx,
        "busy",
        "svc",
        "y",
        "queued",
        &["core/src/y.rs", "core/src", "cli/"],
    );

    let (overlaps, text) = render(&fx);
    assert_eq!(overlaps, json!([]));
    assert!(text.contains(&format!("## Overlaps\n\n{NONE}\n")), "{text}");
    assert!(!text.contains("| Other Plan |"), "{text}");
}

#[test]
fn done_and_archived_plans_merged_and_cancelled_prs_cancelled_jobs_and_other_repos_are_not_listed()
{
    let fx = fixture();
    let touches: &[&str] = &["core/src/"];
    plan(&fx, "finished", "done", false);
    pr(&fx, "finished", "svc", "/r/api", "merged");
    job(&fx, "finished", "svc", "a", "merged", touches);

    plan(&fx, "shelved", "dispatch", true);
    pr(&fx, "shelved", "svc", "/r/api", "dispatch");
    job(&fx, "shelved", "svc", "b", "queued", touches);

    plan(&fx, "shipped", "dispatch", false);
    pr(&fx, "shipped", "old", "/r/api", "merged");
    job(&fx, "shipped", "old", "c", "merged", touches);
    pr(&fx, "shipped", "gone", "/r/api", "cancelled");
    job(&fx, "shipped", "gone", "d", "cancelled", touches);

    plan(&fx, "pruned", "dispatch", false);
    pr(&fx, "pruned", "svc", "/r/api", "dispatch");
    job(&fx, "pruned", "svc", "e", "cancelled", touches);

    plan(&fx, "elsewhere", "dispatch", false);
    pr(&fx, "elsewhere", "svc", "/r/web", "dispatch");
    job(&fx, "elsewhere", "svc", "f", "queued", touches);

    let (overlaps, text) = render(&fx);
    assert_eq!(overlaps, json!([]));
    assert!(text.contains(NONE), "{text}");
}

#[test]
fn a_plan_in_another_context_is_listed_and_this_plans_own_jobs_are_not() {
    let fx = fixture();
    // Another Job of this very Plan, in another PR of the same repo.
    pr(&fx, "mine", "second", "/r/api", "waiting");
    job(&fx, "mine", "second", "x2", "draft", &["core/src/"]);
    fx.env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-far', 'lead', 'other', 'far', 'far', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('far', 'other', 'Far', 'grounding', 'lead-far', '2000-01-01T00:00:00.000Z', NULL);",
    );
    pr(&fx, "far", "svc", "/r/api", "waiting");
    job(&fx, "far", "svc", "y", "draft", &["core/src/x.rs"]);

    let (overlaps, _) = render(&fx);
    assert_eq!(
        overlaps,
        json!([
            {"job": "x", "plan": "far", "pr": "svc", "other_jobs": [{"key": "y", "status": "draft"}], "paths": ["core/src/x.rs"]},
            {"job": "x2", "plan": "far", "pr": "svc", "other_jobs": [{"key": "y", "status": "draft"}], "paths": ["core/src/"]},
        ])
    );
}

#[test]
fn a_cancelled_pr_or_job_of_this_plan_is_not_compared() {
    let fx = fixture();
    pr(&fx, "mine", "dead", "/r/api", "cancelled");
    job(&fx, "mine", "dead", "z", "cancelled", &["docs/a.md"]);
    job(&fx, "mine", "api", "w", "cancelled", &["docs/b.md"]);
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(&fx, "busy", "svc", "y", "queued", &["docs/"]);

    let (overlaps, _) = render(&fx);
    assert_eq!(overlaps, json!([]));
}

#[test]
fn the_plan_review_summary_carries_the_overlaps_and_nothing_is_refused() {
    let fx = fixture();
    plan(&fx, "busy", "dispatch", false);
    pr(&fx, "busy", "svc", "/r/api", "dispatch");
    job(&fx, "busy", "svc", "y", "queued", &["core/src/"]);

    fx.env.outline_for_review("mine");
    let out = fx
        .cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ])
        .ok();
    let text = fs::read_to_string(out["summary"].as_str().unwrap()).unwrap();
    assert!(
        text.contains("\nOverlaps: 1\n- core/src/x.rs: busy/svc/y (queued)\n"),
        "{text}"
    );
}
