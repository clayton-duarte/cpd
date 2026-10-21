//! `plan delete` (commands.md §4.14; contract §2.2 "Deleted", hs-d527), run as the built binary against a temporary
//! `$CPD_HOME`, a fake `pi`-less cockpit and a local bare repo that stands in for `origin`. The Plan's rows that only the
//! engine reaches are written straight into the database, as `plan_cancel.rs` does.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{TestEnv, WATCH_OK, refused};
use serde_json::Value;

// ---------- fixtures (shared with context_delete.rs) ----------

/// What a Plan's world is made of, on disk and in the database.
pub struct World {
    pub plan: String,
    pub lead: String,
    pub repo: PathBuf,
    pub bare: PathBuf,
    pub lead_worktree: PathBuf,
    pub attempt_worktree: PathBuf,
    pub run_dir: PathBuf,
}

impl World {
    pub fn lead_branch(&self) -> String {
        format!("cpd/{}/api", self.plan)
    }

    pub fn attempt_branch(&self) -> String {
        format!("cpd/{}--m-a1", self.plan)
    }
}

/// The config, the fakes and the Context `ctx` (owner `acme`).
pub fn base(env: &TestEnv) {
    env.write_tmux_config("");
    env.fake_cpd_panes(WATCH_OK);
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
}

pub fn fixture(env: &TestEnv, sql: &str) {
    let mut store = env.store();
    common::sql(&mut store, sql);
}

fn branches(env: &TestEnv, repo: &Path) -> Vec<String> {
    env.git(repo, &["branch", "--format=%(refname:short)"]).lines().map(str::to_owned).collect()
}

pub fn local_branches(env: &TestEnv, w: &World) -> Vec<String> {
    branches(env, &w.repo)
}

pub fn remote_branches(env: &TestEnv, w: &World) -> Vec<String> {
    branches(env, &w.bare)
}

/// Plan `plan` of Context `ctx`, finished: its one PR `api` is `merged` (pull request `acme/<plan>#11`), its one Job `m` is
/// merged with its Attempt `m-a1` ended, a Lead worktree and an Attempt worktree exist with their local branches, the
/// Lead Branch is also on the local bare `origin`, and `run/<plan>/` holds files. The Lead is `stopped`, not archived.
pub fn world(env: &TestEnv, plan: &str) -> World {
    world_ending(env, plan, true)
}

/// [`world`] whose PR ends `merged` or, when `merged` is false, `open` with its pull request open (a PR never goes back from `merged`).
fn world_ending(env: &TestEnv, plan: &str, merged: bool) -> World {
    let lead = env.cpd(&["plan", "new", plan, "--context", "ctx", "--title", &format!("Plan {plan}")]).ok()["lead"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let repo = env.keyed_repo(plan);
    let bare = env.root().join("origins").join(format!("{plan}.git"));
    fs::create_dir_all(&bare).unwrap();
    env.git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    env.local_origin(&repo, &bare);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    env.cpd(&["pr", "add", "api", "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", plan]).ok();
    env.cpd(&["job", "cancel", "api-config", "--reason", "a stub of the template", "--plan", plan]).ok();

    let (lead_worktree, attempt_worktree) = (env.root().join("wt").join(format!("{plan}-api")), env.root().join("wt").join(format!("{plan}-m-a1")));
    let w = World {
        plan: plan.to_owned(),
        lead,
        run_dir: env.cpd_home().join("run").join(plan),
        repo,
        bare,
        lead_worktree,
        attempt_worktree,
    };
    env.git(&w.repo, &["worktree", "add", "-q", "-b", &w.lead_branch(), w.lead_worktree.to_str().unwrap(), "main"]);
    env.git(&w.repo, &["push", "-q", "origin", &w.lead_branch()]);
    env.git(&w.repo, &["worktree", "add", "-q", "-b", &w.attempt_branch(), w.attempt_worktree.to_str().unwrap(), "main"]);
    fs::create_dir_all(w.run_dir.join("specs")).unwrap();
    fs::write(w.run_dir.join("summary.md"), "summary\n").unwrap();
    fs::write(w.run_dir.join("specs/m-a1.md"), "spec\n").unwrap();

    let fingerprint = "f".repeat(64);
    fixture(
        env,
        &format!(
            "UPDATE plans SET phase = 'plan' WHERE id = '{plan}';
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
               VALUES ('{plan}', 'api', 'm', 'Job m', 'S2', 'Goal.', 'Context.', '[\"src/m.rs\"]', '[\"cargo test\"]', '[]', 'draft', 0, cpd_txn(), 't');
             UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = '{plan}';
             UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE plan = '{plan}' AND key = 'm';
             UPDATE prs SET status = 'dispatch', lead_worktree = '{lead_wt}', started_at = 't' WHERE plan = '{plan}' AND key = 'api';
             UPDATE jobs SET status = 'running' WHERE plan = '{plan}' AND key = 'm';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, merged_sha, branch, worktree, started_at, ended_at)
               VALUES ('{plan}', 'm-a1', 'm', 1, 'S2', 'ladder', 'ended', 'merged', 'merged', 'c0ffee', '{branch}', '{attempt_wt}', 't', 't');
             UPDATE jobs SET status = 'merged', merged_sha = 'c0ffee' WHERE plan = '{plan}' AND key = 'm';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/{plan}', 11, 'https://github.com/acme/{plan}/pull/11', 'T', 0, '{state}', 'none', 'none', 't', {merged_at}, 't');
             UPDATE prs SET status = 'open', pr_number = 11, pr_url = 'https://github.com/acme/{plan}/pull/11', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{plan}' AND number = 11)
              WHERE plan = '{plan}' AND key = 'api';
             {finish}",
            finish = if merged { format!("UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = '{plan}' AND key = 'api';") } else { String::new() },
            state = if merged { "merged" } else { "open" },
            merged_at = if merged { "'t'" } else { "NULL" },
            lead_wt = w.lead_worktree.display(),
            attempt_wt = w.attempt_worktree.display(),
            branch = w.attempt_branch(),
        ),
    );
    w
}

/// [`world`] whose PR is still `open`, with its pull request open.
pub fn world_with_open_pr(env: &TestEnv, plan: &str) -> World {
    world_ending(env, plan, false)
}

pub fn archive(env: &TestEnv, plan: &str) {
    env.cpd(&["plan", "archive", "--plan", plan]).ok();
}

pub fn count(env: &TestEnv, sql: &str) -> i64 {
    env.query(sql)
}

/// The rows the Plan owns, in the tables the delete empties.
pub fn plan_rows(env: &TestEnv, plan: &str) -> i64 {
    // the Lead's row and the `plan-deleted` Event stay (commands.md §4.14): they are not what the delete empties
    ["jobs", "attempts", "prs"].iter().map(|t| count(env, &format!("SELECT count(*) FROM {t} WHERE plan = '{plan}'"))).sum::<i64>()
        + count(env, &format!("SELECT count(*) FROM events WHERE plan = '{plan}' AND kind <> 'plan-deleted'"))
        + count(env, &format!("SELECT count(*) FROM sessions WHERE plan = '{plan}' AND id NOT IN (SELECT lead_session FROM plans)"))
}

pub fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

// ---------- tests ----------

#[test]
fn without_yes_lists_what_goes_and_deletes_nothing() {
    let env = TestEnv::new();
    base(&env);
    let w = world(&env, "pay");
    archive(&env, "pay");
    let before = plan_rows(&env, "pay");
    let events_before = count(&env, "SELECT count(*) FROM events");

    let out = env.cpd(&["plan", "delete", "pay"]).ok();
    assert_eq!(out["preview"], true, "{out}");
    assert_eq!(out["jobs"], 2, "the Job `m` and the cancelled stub `api-config` {out}");
    assert_eq!(out["attempts"], 1, "{out}");
    assert_eq!(out["sessions"], 1, "the Lead {out}");
    assert_eq!(out["worktrees"], 2, "the Lead worktree and the Attempt worktree {out}");
    assert!(out["disk_freed_mb"].is_number(), "{out}");

    assert_eq!(plan_rows(&env, "pay"), before, "nothing deleted");
    assert_eq!(count(&env, "SELECT count(*) FROM events"), events_before, "no Event written");
    assert!(w.lead_worktree.exists() && w.attempt_worktree.exists() && w.run_dir.exists());
    assert!(local_branches(&env, &w).contains(&w.attempt_branch()));
    assert_eq!(env.query::<i64>("SELECT count(*) FROM plans WHERE id = 'pay' AND deleted_at IS NULL"), 1);
}

#[test]
fn refuses_a_plan_that_is_not_archived_or_asleep() {
    let env = TestEnv::new();
    base(&env);
    let w = world(&env, "pay");

    let error = refused(&env.cpd(&["plan", "delete", "pay", "--yes"]), "archive_blocked", 3);
    assert!(error["details"]["blockers"].as_array().is_some_and(|b| !b.is_empty()), "{error}");
    refused(&env.cpd(&["plan", "delete", "pay"]), "archive_blocked", 3);
    refused(&env.cpd(&["plan", "delete", "nope", "--yes"]), "plan_not_found", 4);

    assert!(w.lead_worktree.exists() && w.run_dir.exists());
    assert!(plan_rows(&env, "pay") > 0);
}

#[test]
fn refuses_with_live_work_or_an_unmerged_pr() {
    let env = TestEnv::new();
    base(&env);
    let busy = world(&env, "busy");
    archive(&env, "busy");
    // a Session working after the archive: nothing running is a precondition of the delete itself
    fixture(&env, &format!("UPDATE sessions SET state = 'working' WHERE id = '{}'", busy.lead));
    let error = refused(&env.cpd(&["plan", "delete", "busy", "--yes"]), "archive_blocked", 3);
    assert!(error["details"]["blockers"].as_array().is_some_and(|b| !b.is_empty()), "{error}");

    let open = world_with_open_pr(&env, "openpr");
    archive(&env, "openpr");
    let error = refused(&env.cpd(&["plan", "delete", "openpr", "--yes"]), "archive_blocked", 3);
    assert!(error["details"]["blockers"].as_array().is_some_and(|b| !b.is_empty()), "{error}");

    assert!(busy.run_dir.exists() && open.run_dir.exists() && open.lead_worktree.exists());
    assert!(plan_rows(&env, "busy") > 0 && plan_rows(&env, "openpr") > 0);
}

#[test]
fn deletes_rows_files_and_worktrees_of_an_archived_plan() {
    let env = TestEnv::new();
    base(&env);
    let w = world(&env, "pay");
    let other = world(&env, "other");
    archive(&env, "pay");

    let out = env.cpd(&["plan", "delete", "pay", "--yes"]).ok();
    assert_eq!((out["preview"].clone(), out["already"].clone()), (Value::Bool(false), Value::Bool(false)), "{out}");

    assert_eq!(plan_rows(&env, "pay"), 0, "the Plan's Jobs, Attempts, PRs, Events and Sessions are gone");
    assert_eq!(count(&env, "SELECT count(*) FROM escalations WHERE plan = 'pay'"), 0);
    assert_ne!(env.query::<Option<String>>("SELECT deleted_at FROM plans WHERE id = 'pay'"), None, "the Plan row is marked, never deleted");
    assert!(!w.run_dir.exists(), "$CPD_HOME/run/pay/");
    assert!(!w.lead_worktree.exists() && !w.attempt_worktree.exists(), "the worktrees");
    let local = local_branches(&env, &w);
    assert!(!local.contains(&w.lead_branch()) && !local.contains(&w.attempt_branch()), "the local branches: {local:?}");

    let deleted = events(&env, "plan-deleted");
    assert_eq!(deleted.len(), 1, "{deleted:?}");
    assert_eq!(deleted[0]["data"]["attempts"], 1);

    // another Plan is untouched
    assert!(plan_rows(&env, "other") > 0 && other.run_dir.exists() && other.lead_worktree.exists());
    assert!(local_branches(&env, &other).contains(&other.attempt_branch()));

    // idempotent: a rerun writes nothing
    let events_before = count(&env, "SELECT count(*) FROM events");
    let again = env.cpd(&["plan", "delete", "pay", "--yes"]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(count(&env, "SELECT count(*) FROM events"), events_before);
}

#[test]
fn deletes_the_plan_of_an_asleep_lead() {
    let env = TestEnv::new();
    base(&env);
    let w = world(&env, "pay");
    // asleep is a timestamp of the Session (contract §2.2), not a state: the Plan is not archived
    fixture(&env, &format!("UPDATE sessions SET asleep_at = 't' WHERE id = '{}'", w.lead));
    assert_eq!(env.query::<i64>("SELECT count(*) FROM plans WHERE id = 'pay' AND archived_at IS NULL"), 1);

    env.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    assert_eq!(plan_rows(&env, "pay"), 0);
    assert!(!w.run_dir.exists() && !w.lead_worktree.exists());
}

#[test]
fn keeps_context_records_and_remote_branches() {
    let env = TestEnv::new();
    base(&env);
    env.cpd(&["context", "note", "Mind the ledger.", "--context", "ctx"]).ok();
    let w = world(&env, "pay");
    archive(&env, "pay");
    let records = count(&env, "SELECT count(*) FROM context_records WHERE context = 'ctx'");
    assert!(records > 0);
    assert!(remote_branches(&env, &w).contains(&w.lead_branch()));

    env.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    assert_eq!(count(&env, "SELECT count(*) FROM context_records WHERE context = 'ctx'"), records, "Context records stay");
    assert_eq!(env.query::<i64>("SELECT count(*) FROM contexts WHERE id = 'ctx' AND deleted_at IS NULL"), 1, "the Context stays");
    assert!(remote_branches(&env, &w).contains(&w.lead_branch()), "the remote branch is left alone");
    assert!(env.calls("gh").iter().all(|c| c.first().map(String::as_str) != Some("pr") || c.get(1).map(String::as_str) != Some("close")), "GitHub is left alone");
}

#[test]
fn tamper_check_stays_clean_after_a_delete() {
    let env = TestEnv::new();
    base(&env);
    world(&env, "pay");
    let other = world(&env, "other");
    archive(&env, "pay");

    env.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    // every DELETE is in `history` (the tamper check's record of the deleted ids), and the check finds nothing
    assert!(count(&env, "SELECT count(*) FROM history WHERE tbl = 'jobs' AND op = 'delete'") > 0);
    assert_eq!(env.store().tamper().unwrap(), Vec::<Value>::new());
    // the guard is not weakened for any other write: another Plan's rows cannot be deleted outside cpd
    let raw = rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    assert!(raw.execute("DELETE FROM jobs WHERE plan = 'other'", []).is_err(), "a raw delete is still refused");
    assert!(raw.execute("DELETE FROM events WHERE plan = 'other'", []).is_err(), "Events stay append-only");
    assert!(plan_rows(&env, "other") > 0 && other.run_dir.exists());
    // the store still writes, and the deleted id is never reused
    env.cpd(&["context", "note", "After the delete.", "--context", "ctx"]).ok();
    let again = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Again"]);
    assert_ne!(again.exit, 0, "a deleted id is never reused: {again:?}");
}
