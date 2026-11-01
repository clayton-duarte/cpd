//! `cpd context pending` (commands.md §3.11, hs-d159 P1): the command tree, in this process (`Fx`),
//! against a temporary `$CPD_HOME` with Plans written straight into the database.

use crate::common;

use std::fs;

use common::TestEnv;
use common::fx::Fx;
use cpd_core::home::CpdHome;
use serde_json::{Value, json};

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A Plan of `context` in `phase`, its Lead Session beside it; `archived` and `done` as its phase needs.
fn plan(env: &TestEnv, context: &str, id: &str, phase: &str, archived: bool) {
    let done = if phase == "done" { "'2000-01-02T00:00:00.000Z'" } else { "NULL" };
    let archived = if archived { "'2000-01-03T00:00:00.000Z'" } else { "NULL" };
    env.sql(
        &format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead-{id}', 'lead', '{context}', '{id}', '{id}', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at, archived_at)
               VALUES ('{id}', '{context}', 'Title of {id}', '{phase}', 'lead-{id}', '2000-01-01T00:00:0{n}.000Z',
                       '2000-01-01T00:00:00.000Z', {done}, {archived});",
            n = id.len() % 10
        ),
    );
}

/// A PR of `plan`; `open` and `merged` ones have a pull request number, `open` a stored GitHub row too.
fn pr(env: &TestEnv, plan: &str, key: &str, status: &str) {
    let (worktree, started, pull, merged) = match status {
        "cancelled" => ("NULL", "NULL", "NULL, NULL, NULL, NULL", "NULL"),
        "dispatch" => ("'/w'", "'t'", "NULL, NULL, NULL, NULL", "NULL"),
        "open" => (
            "'/w'",
            "'t'",
            "7, 'https://github.com/acme/api/pull/7', 't', (SELECT id FROM pull_requests WHERE number = 7)",
            "NULL",
        ),
        "merged" => (
            "'/w'",
            "'t'",
            "8, 'https://github.com/acme/api/pull/8', 't', (SELECT id FROM pull_requests WHERE number = 8)",
            "'t'",
        ),
        other => panic!("no fixture for {other}"),
    };
    if status == "open" {
        env.sql(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add api', 0, 'open', 'none', 'none', 't', NULL, 't');",
        );
    }
    if status == "merged" {
        // One GitHub row per merged PR: the `number` is unique per repo.
        env.sql(
            &format!(
                "INSERT OR IGNORE INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/api', 8, 'https://github.com/acme/api/pull/8', 'Old', 0, 'merged', 'none', 'none', 't', 't', 't');"
            ),
        );
    }
    env.sql(
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                              created_txn, created_at, started_at, merged_at)
             VALUES ('{plan}', '{key}', '/r/{key}', 'main', '{status}', 'cpd/{plan}/{key}', {worktree}, {pull}, cpd_txn(), 't', {started}, {merged});"
        ),
    );
}

/// A Job of PR `pr` in `status`, with the columns that status requires; its title is `Title <key>`, it touches `src/<key>.rs`.
fn job(env: &TestEnv, plan: &str, pr: &str, key: &str, status: &str) {
    env.sql(
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               merged_sha, failed_cause, cancel_reason, cancel_requested_at, created_txn, created_at)
             VALUES ('{plan}', '{pr}', '{key}', 'Title {key}', 'S2', 'Goal.', 'Context.', '[\"src/{key}.rs\", \"docs/{key}.md\"]',
                     '[\"cargo test\"]', '[]', '{status}', 0,
                     iif('{status}' IN ('draft', 'cancelled'), NULL, '{FP}'),
                     iif('{status}' = 'merged', 'abc123', NULL), iif('{status}' = 'failed', 'check_failed', NULL),
                     iif('{status}' = 'cancelled', 'x', NULL), iif('{status}' = 'cancelled', 't', NULL), cpd_txn(), 't');"
        ),
    );
}

/// An explore Job of `plan` (its `pr` NULL): `kind` `scout` or `research` at `tier`, stored `status`, touching `explore/<key>.md`,
/// and `needs` of other explore Jobs. A `running` one gets a running Attempt.
fn explore(env: &TestEnv, plan: &str, key: &str, kind: &str, tier: &str, status: &str, needs: &[&str]) {
    let mut sql = format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
         VALUES ('{plan}', NULL, '{key}', 'Title {key}', '{tier}', 'Goal.', 'Context.', '[\"explore/{key}.md\"]', '[\"ok\"]', '[]', '{kind}', '{status}', 1, '{FP}', cpd_txn(), 't');"
    );
    for need in needs {
        sql.push_str(&format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{key}', '{need}');"));
    }
    if status == "running" {
        sql.push_str(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
             VALUES ('{plan}', '{key}-a1', '{key}', 1, '{tier}', 'ladder', 'running', 'cpd/{plan}--{key}-a1', '/w', 4242, 't');"
        ));
    }
    env.sql(&sql);
}

fn env() -> Fx {
    let fx = Fx::new();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't'), ('other', 'Other', 't', 't'), ('empty', 'Empty', 't', 't');",
    );
    fx
}

/// Plan `live` has PRs in every status and Jobs in every status; `ctx` also has a done and an archived Plan,
/// and `other` has a Plan of its own.
fn fixture() -> Fx {
    let fx = env();
    plan(&fx.env, "ctx", "live", "dispatch", false);
    pr(&fx.env, "live", "api", "dispatch");
    for (key, status) in [("a-merged", "merged"), ("b-queued", "queued"), ("c-running", "running"), ("d-failed", "failed"), ("e-draft", "draft"), ("f-gone", "cancelled")] {
        job(&fx.env, "live", "api", key, status);
    }
    pr(&fx.env, "live", "ui", "open");
    job(&fx.env, "live", "ui", "g-ui", "queued");
    pr(&fx.env, "live", "old", "merged");
    job(&fx.env, "live", "old", "h-old", "merged");
    pr(&fx.env, "live", "gone", "cancelled");
    job(&fx.env, "live", "gone", "i-gone", "cancelled");

    plan(&fx.env, "ctx", "finished", "done", false);
    pr(&fx.env, "finished", "api", "merged");
    plan(&fx.env, "ctx", "shelved", "dispatch", true);
    pr(&fx.env, "shelved", "api", "dispatch");
    job(&fx.env, "shelved", "api", "j-shelved", "queued");

    plan(&fx.env, "other", "foreign", "dispatch", false);
    pr(&fx.env, "foreign", "api", "dispatch");
    job(&fx.env, "foreign", "api", "k-foreign", "queued");
    fx
}

fn keys(values: &Value) -> Vec<&str> {
    values.as_array().unwrap().iter().map(|v| v["key"].as_str().unwrap()).collect()
}

#[test]
fn lists_only_active_plans_unmerged_prs_and_their_live_jobs() {
    let fx = fixture();
    let out = fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    assert_eq!(out["context"], "ctx");
    let plans = out["plans"].as_array().unwrap();
    // Not `finished` (done), not `shelved` (archived), not `foreign` (another Context).
    assert_eq!(plans.len(), 1, "{out}");
    let live = &plans[0];
    assert_eq!(
        (&live["id"], &live["title"], &live["phase"], &live["lead_session"]),
        (&json!("live"), &json!("Title of live"), &json!("dispatch"), &json!("lead-live"))
    );
    // Not `old` (merged) and not `gone` (cancelled), in key order.
    assert_eq!(keys(&live["workflows"]), ["api", "ui"]);
    let api = &live["workflows"][0];
    assert_eq!(
        (&api["repo"], &api["base"], &api["status"], &api["lead_branch"], &api["pull_request"]),
        (&json!("/r/api"), &json!("main"), &json!("dispatch"), &json!("cpd/live/api"), &Value::Null)
    );
    // Not `f-gone` (cancelled); the Job merged on the Lead Branch is there, with its title and touches.
    assert_eq!(keys(&api["jobs"]), ["a-merged", "b-queued", "c-running", "d-failed", "e-draft"]);
    assert_eq!(
        api["jobs"][0],
        json!({"key": "a-merged", "status": "merged", "title": "Title a-merged", "touches": ["src/a-merged.rs", "docs/a-merged.md"]})
    );
    let statuses: Vec<&str> = api["jobs"].as_array().unwrap().iter().map(|j| j["status"].as_str().unwrap()).collect();
    assert_eq!(statuses, ["merged", "queued", "running", "failed", "draft"]);
    let ui = &live["workflows"][1];
    assert_eq!(ui["pull_request"], json!({"number": 7, "url": "https://github.com/acme/api/pull/7", "state": "open"}));
    assert_eq!(keys(&ui["jobs"]), ["g-ui"]);
}

#[test]
fn the_context_is_the_selected_plans_by_default() {
    let fx = fixture();
    let out = fx.cpd(&["context", "pending", "--plan", "live"]).ok();
    assert_eq!(out["context"], "ctx");
    assert_eq!(out["plans"][0]["id"], "live");
}

#[test]
fn a_context_without_an_active_plan_has_no_plans() {
    let fx = fixture();
    let out = fx.cpd(&["context", "pending", "--context", "empty"]).ok();
    assert_eq!(out["plans"], json!([]));
    // `--markdown` prints on the process's own stdout (`print!`): the binary, which Fx cannot read.
    let text = fx.env.cpd(&["context", "pending", "--context", "empty", "--markdown"]);
    assert_eq!((text.exit, text.stdout.as_str()), (0, "No pending work.\n"));
    // A Context whose only Plans are done or archived has none either.
    plan(&fx.env, "empty", "fin2", "done", false);
    plan(&fx.env, "empty", "shelf2", "dispatch", true);
    pr(&fx.env, "shelf2", "api", "dispatch");
    assert_eq!(fx.cpd(&["context", "pending", "--context", "empty"]).ok()["plans"], json!([]));
}

#[test]
fn markdown_prints_the_same_content() {
    let fx = fixture();
    // `--markdown` prints on the process's own stdout (`print!`): the binary, which Fx cannot read.
    let run = fx.env.cpd(&["context", "pending", "--context", "ctx", "--markdown"]);
    assert_eq!(run.exit, 0, "{run:?}");
    let expected = "# Pending work in ctx\n\
\n## live (dispatch): Title of live\n\
\n### Workflow api (/r/api, base main): dispatch\n\
\nno pull request\n\
\n- a-merged merged: Title a-merged (src/a-merged.rs, docs/a-merged.md)\n\
- b-queued queued: Title b-queued (src/b-queued.rs, docs/b-queued.md)\n\
- c-running running: Title c-running (src/c-running.rs, docs/c-running.md)\n\
- d-failed failed: Title d-failed (src/d-failed.rs, docs/d-failed.md)\n\
- e-draft draft: Title e-draft (src/e-draft.rs, docs/e-draft.md)\n\
\n### Workflow ui (/r/ui, base main): open\n\
\n#7 open https://github.com/acme/api/pull/7\n\
\n- g-ui queued: Title g-ui (src/g-ui.rs, docs/g-ui.md)\n";
    assert_eq!(run.stdout, expected);
}

#[test]
/// commands.md §3.12 (Plan `explore-view`): a Plan `live` with PR `api` and explore Jobs shows a plan-level `jobs`
/// array beside `prs`, in `seq` order, while the PR's own `jobs` array is unchanged.
#[test]
fn context_pending_a_plans_plan_level_explore_jobs_in_json() {
    let fx = env();
    plan(&fx.env, "ctx", "live", "dispatch", false);
    pr(&fx.env, "live", "api", "dispatch");
    job(&fx.env, "live", "api", "w1", "queued");
    explore(&fx.env, "live", "look", "scout", "H2", "queued", &[]);
    explore(&fx.env, "live", "look2", "research", "S2", "running", &[]);
    let out = fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    let plans = out["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 1, "{out}");
    let live = &plans[0];
    assert_eq!(live["id"], "live");
    assert!(live["jobs"].is_array(), "the Plan carries a plan-level jobs array: {out}");
    assert_eq!(keys(&live["jobs"]), ["look", "look2"], "{out}");
    assert_eq!(
        live["jobs"][0],
        json!({"key": "look", "kind": "scout", "status": "queued", "tier": "H2", "title": "Title look", "touches": ["explore/look.md"]})
    );
    assert_eq!(
        live["jobs"][1],
        json!({"key": "look2", "kind": "research", "status": "running", "tier": "S2", "title": "Title look2", "touches": ["explore/look2.md"]})
    );
    // The PR's own `jobs` array is unchanged.
    assert_eq!(keys(&live["workflows"]), ["api"]);
    assert_eq!(keys(&live["workflows"][0]["jobs"]), ["w1"]);
    assert_eq!(live["workflows"][0]["jobs"][0]["status"], "queued");
}

/// commands.md §3.12: an explore Job that needs another explore Job is displayed `waiting` with its `waiting_on`.
#[test]
fn context_pending_an_explore_job_waiting_on_another_explore_job() {
    let fx = env();
    plan(&fx.env, "ctx", "live", "dispatch", false);
    explore(&fx.env, "live", "scan", "scout", "H2", "running", &[]);
    explore(&fx.env, "live", "dig", "research", "S2", "queued", &["scan"]);
    let out = fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    let jobs = out["plans"][0]["jobs"].as_array().expect("the Plan's plan-level jobs");
    assert_eq!(jobs[0]["key"], "scan");
    assert_eq!(jobs[0]["status"], "running");
    assert_eq!(
        jobs[1],
        json!({"key": "dig", "kind": "research", "status": "waiting", "tier": "S2", "title": "Title dig", "touches": ["explore/dig.md"], "waiting_on": ["scan"]})
    );
}

/// commands.md §3.12: a Plan with only explore Jobs and no PR is listed, with a non-empty `jobs` and an empty `prs`.
#[test]
fn context_pending_a_plan_with_only_explore_jobs_is_not_empty() {
    let fx = env();
    plan(&fx.env, "ctx", "solo", "dispatch", false);
    explore(&fx.env, "solo", "scan", "scout", "H2", "queued", &[]);
    let out = fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    let plans = out["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 1, "{out}");
    assert_eq!(plans[0]["id"], "solo");
    assert_eq!(plans[0]["workflows"], json!([]));
    assert_eq!(keys(&plans[0]["jobs"]), ["scan"], "{out}");
}

/// commands.md §3.12: `--markdown` renders the Plan-level `### Jobs` section after the Plan's PR sections, one line
/// per explore Job `- <key> <kind> <status>: <title> (<touches>)`, a blocked one `waiting on <keys>`.
#[test]
fn context_pending_markdown_the_plan_level_jobs_section() {
    let fx = env();
    plan(&fx.env, "ctx", "live", "dispatch", false);
    pr(&fx.env, "live", "api", "dispatch");
    job(&fx.env, "live", "api", "w1", "queued");
    explore(&fx.env, "live", "look", "scout", "H2", "queued", &[]);
    explore(&fx.env, "live", "look2", "research", "S2", "running", &[]);
    explore(&fx.env, "live", "dig", "research", "S2", "queued", &["look2"]);
    let run = fx.env.cpd(&["context", "pending", "--context", "ctx", "--markdown"]);
    assert_eq!(run.exit, 0, "{run:?}");
    let expected = "# Pending work in ctx\n\
\n## live (dispatch): Title of live\n\
\n### Workflow api (/r/api, base main): dispatch\n\
\nno pull request\n\
\n- w1 queued: Title w1 (src/w1.rs, docs/w1.md)\n\
\n### Jobs\n\
\n- look scout queued: Title look (explore/look.md)\n\
- look2 research running: Title look2 (explore/look2.md)\n\
- dig research waiting on look2: Title dig (explore/dig.md)\n";
    assert_eq!(run.stdout, expected);
}

fn it_writes_nothing() {
    let fx = fixture();
    let events = || fx.cpd(&["events", "list"]).ok()["events"].as_array().unwrap().len();
    let before = events();
    fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    assert_eq!(events(), before);
}

#[test]
fn refusals() {
    let fx = fixture();
    let run = fx.cpd(&["context", "pending", "--context", "nope"]);
    assert_eq!((run.error()["code"].clone(), run.exit), (json!("context_not_found"), 4));
    let run = fx.cpd(&["context", "pending"]);
    assert_eq!((run.error()["code"].clone(), run.exit), (json!("context_required"), 2));
}
