//! Red tests of how `plan lock`, `plan review`, `plan show` and `plan cancel` treat explore Jobs (Plan `explore-roles`,
//! R2, R3; contract §2.2, §2.3, §3.1; commands.md §4): an explore Job belongs to the Plan, has a NULL `pr`, is `queued` at
//! once, is in no plan fingerprint and is invisible to review and lock.
//!
//! The commands read the process environment (`CPD_HOME`): a test takes `LOCK`, points it at a temp home and `Drop` puts it back.
//! No test lets `plan lock` or `plan cancel` reach git, tmux or the engine: the lock is driven to an error that comes before
//! git (no draft Job; not approved; a PR with no draft Job), and the cancel is the Jobs step (`cancel_jobs`) alone.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::Value;

use super::cancel::cancel_jobs;
use super::fingerprint::plan_fingerprint;
use super::review::latest_approved_for;
use super::{LockArgs, ReviewArgs, ShowArgs};
use crate::args::{PlanArg, PrTitle};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::output::Output;

const T: &str = "2026-01-01T00:00:00.000Z";

static LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _dir: tempfile::TempDir,
    env: Env,
    saved: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: `LOCK` serializes these tests, and nextest gives each a process of its own.
        unsafe {
            match &self.saved {
                Some(v) => std::env::set_var("CPD_HOME", v),
                None => std::env::remove_var("CPD_HOME"),
            }
        }
    }
}

fn plan_id() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn plan_arg() -> PlanArg {
    PlanArg { plan: Some(plan_id()) }
}

/// Plan `pay` in `phase` with PR `api` (`waiting`, or `dispatch` after the lock) and the Lead; `extra_pr` adds the PR `web`.
fn fixture(phase: &str, extra_pr: bool) -> Fixture {
    let lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let owner = home.config().parent().unwrap().join("policy").join("acme");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("owner.toml"), "").unwrap();
    let saved = std::env::var_os("CPD_HOME");
    // SAFETY: as in `Drop`.
    unsafe { std::env::set_var("CPD_HOME", home.root()) };
    let env = Env::open_home(home).unwrap();
    let locked = if phase == "plan" { "NULL".to_owned() } else { format!("'{T}'") };
    let (status, worktree, started) = if phase == "plan" { ("waiting", "NULL".to_owned(), "NULL".to_owned()) } else { ("dispatch", "'/lw'".to_owned(), format!("'{T}'")) };
    let mut sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on, owner) VALUES ('ctx', 'Ctx', '{T}', 'm1', 'acme');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, outline)
           VALUES ('pay', 'ctx', 'P', '{phase}', 'l-pay', '{T}', {locked},
                   '{{\"assumptions\":[],\"decided\":[],\"decisions\":[],\"done_when\":{{\"api\":[\"works\"]}},\"goal\":\"g\",\"out_of_scope\":[],\"risks\":[]}}');"
    );
    for pr in if extra_pr { vec!["api", "web"] } else { vec!["api"] } {
        sql.push_str(&format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{pr}', '/r', 'main', '{status}', 'cpd/pay/{pr}', {worktree}, 1, '{T}', {started});"
        ));
    }
    let mut f = Fixture { _dir: dir, env, saved, _lock: lock };
    f.sql(&sql);
    f
}

impl Fixture {
    fn sql(&mut self, sql: &str) {
        self.env
            .store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// A complete draft `work` Job of PR `api`.
    fn draft(&mut self, key: &str) {
        self.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, created_txn, created_at)
               VALUES ('pay', 'api', '{key}', 'Draft {key}', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'work', 'draft', 0, cpd_txn(), '{T}');"
        ));
    }

    /// An explore Job as `job add` makes it: no PR, `queued`, a fingerprint, the derived touches.
    fn explore(&mut self, key: &str, kind: &str, status: &str) {
        let tier = if kind == "scout" { "H2" } else { "S2" };
        self.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', NULL, '{key}', 'Explore {key}', '{tier}', 'g', 'c', '[\"run/pay/explore/{key}.md\"]', '[\"b\"]', '[]', '{kind}', 'queued', 1, '{}', cpd_txn(), '{T}');",
            "e".repeat(64)
        ));
        if status == "running" {
            self.sql(&format!("UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';"));
            self.sql(&format!(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
                   VALUES ('pay', '{key}-a1', '{key}', 1, '{tier}', 'ladder', 'running', 'cpd/pay--{key}-a1', '/w', 4242, '{T}');"
            ));
        }
    }

    fn fingerprint(&mut self) -> String {
        self.env.store.read(|c| plan_fingerprint(c, &plan_id())).unwrap()
    }

    fn approve_current(&mut self) {
        let fp = self.fingerprint();
        self.sql(&format!(
            "INSERT INTO plan_reviews (plan, id, number, fingerprint, title, status, requested_at, decided_at)
               VALUES ('pay', 'r1', 1, '{fp}', 'T', 'approved', '{T}', '{T}');"
        ));
    }

    fn rows(&mut self, sql: &str) -> Vec<(String, Option<String>)> {
        self.env
            .store
            .read(|c| {
                let mut stmt = c.prepare(sql)?;
                let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    fn status_and_fingerprint(&mut self) -> Vec<(String, Option<String>)> {
        self.rows("SELECT key || ':' || status, fingerprint FROM jobs WHERE plan = 'pay' ORDER BY key")
    }
}

fn fields(out: Result<Output, CpdError>) -> Value {
    match out.unwrap_or_else(|e| panic!("expected a success, got {e}")) {
        Output::Success(f) => Value::Object(f),
        Output::Owned => panic!("a success"),
    }
}

fn refused(out: Result<Output, CpdError>) -> CpdError {
    match out {
        Ok(_) => panic!("expected a refusal, got a success"),
        Err(e) => e,
    }
}

fn lock() -> Result<Output, CpdError> {
    super::lock::lock(LockArgs { plan: plan_arg() })
}

fn review() -> Result<Output, CpdError> {
    super::review::review(ReviewArgs { plan: plan_arg(), title: "Pay".parse::<PrTitle>().unwrap() })
}

#[test]
fn plan_lock_ignores_explore_jobs() {
    // A Plan whose only Jobs are explore Jobs fails as a Plan with no draft Job does, whatever they are.
    let mut f = fixture("plan", false);
    f.explore("s1", "scout", "queued");
    f.explore("r1", "research", "queued");
    let before = f.status_and_fingerprint();
    let e = refused(lock());
    assert_eq!(e.code(), ErrorCode::NoDraftJobs, "{e}");
    assert_eq!(f.status_and_fingerprint(), before, "the explore Jobs stay queued, with the fingerprint they were added with");
    drop(f);

    // Beside a draft, they are in no plan fingerprint and are not re-queued: the lock gets as far as the review check.
    let mut f = fixture("plan", false);
    f.draft("d1");
    let alone = f.fingerprint();
    f.explore("s1", "scout", "queued");
    f.explore("r1", "research", "queued");
    assert_eq!(f.fingerprint(), alone, "no explore Job is in the plan fingerprint");
    let before = f.status_and_fingerprint();
    let e = refused(lock());
    assert_eq!(e.code(), ErrorCode::PlanNotApproved, "{e}");
    assert_eq!(e.details()["fingerprint"], alone.as_str(), "the fingerprint the lock checks has no explore Job");
    assert_eq!(f.status_and_fingerprint(), before);
    assert!(before.iter().any(|(s, fp)| s == "s1:queued" && fp.as_deref() == Some("e".repeat(64).as_str())));
}

#[test]
fn plan_review_counts_no_explore_job() {
    // Only explore Jobs: there is nothing to review.
    let mut f = fixture("plan", false);
    f.explore("s1", "scout", "queued");
    assert_eq!(refused(review()).code(), ErrorCode::NoDraftJobs);
    drop(f);

    // Beside a draft: the review is of one Job, and the files and the Event name no explore Job.
    let mut f = fixture("plan", false);
    f.draft("d1");
    f.explore("s1", "scout", "queued");
    f.explore("r1", "research", "queued");
    let out = fields(review());
    let summary = std::fs::read_to_string(out["summary"].as_str().unwrap()).unwrap();
    for text in [&summary] {
        assert!(text.contains("d1"), "the draft is listed: {text}");
        assert!(!text.contains("Explore s1") && !text.contains("Explore r1"), "no explore Job in the review: {text}");
    }
    let events = f.rows("SELECT text, NULL FROM events WHERE kind = 'plan-review'");
    assert_eq!(events.len(), 1);
    assert!(events[0].0.contains("1 draft Jobs"), "one draft counted, not three Jobs: {}", events[0].0);
    let reviewed = out["review"]["fingerprint"].as_str().unwrap().to_owned();
    drop(f);
    let mut g = fixture("plan", false);
    g.draft("d1");
    assert_eq!(reviewed, g.fingerprint(), "the review covers the draft alone");
}

#[test]
fn adding_an_explore_job_keeps_an_approved_review_approved() {
    // The PR `web` has no draft Job, so a lock that passes the approval check ends in `pr_empty`, before git.
    let mut f = fixture("plan", true);
    f.draft("d1");
    f.approve_current();
    let approved = f.fingerprint();
    f.explore("s1", "scout", "queued");
    f.explore("r1", "research", "queued");
    assert_eq!(f.fingerprint(), approved);
    let fp = approved.clone();
    let still = f.env.store.read(|c| latest_approved_for(c, &plan_id(), &fp)).unwrap();
    assert_eq!(still.as_deref(), Some("r1"), "the review is still approved for the current plan");
    let e = refused(lock());
    assert_ne!(e.code(), ErrorCode::PlanNotApproved, "{e}");
    assert_eq!(e.code(), ErrorCode::PrEmpty, "{e}");
}

#[test]
fn plan_show_lists_explore_jobs_with_a_null_pr() {
    let mut f = fixture("dispatch", true);
    f.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'api', 'w1', 'Work', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'work', 'queued', 0, '{}', cpd_txn(), '{T}');",
        "f".repeat(64)
    ));
    f.explore("s1", "scout", "queued");
    f.explore("r1", "research", "queued");
    let out = fields(super::show::show(ShowArgs { plan: plan_arg() }));
    let jobs = out["jobs"].as_array().unwrap();
    for (key, kind) in [("s1", "scout"), ("r1", "research")] {
        let job = jobs.iter().find(|j| j["key"] == key).unwrap_or_else(|| panic!("{key} is listed in jobs: {jobs:?}"));
        assert_eq!(job["pr"], Value::Null, "{key}");
        assert_eq!(job["kind"], kind, "{key}");
        assert_eq!(job["status"], "queued", "{key}");
    }
    let work = jobs.iter().find(|j| j["key"] == "w1").unwrap();
    assert_eq!(work["pr"], "api");
    for pr in out["workflows"].as_array().unwrap() {
        let listed: Vec<&str> = pr["jobs"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(!listed.contains(&"s1") && !listed.contains(&"r1"), "no PR row mentions an explore Job: {pr}");
    }
    assert_eq!(out["counts"]["queued"], 3);
}

#[test]
fn plan_cancel_cancels_explore_jobs() {
    let mut f = fixture("dispatch", false);
    f.explore("q1", "scout", "queued");
    f.explore("r1", "research", "running");
    let out = cancel_jobs(&mut f.env.store, &plan_id(), "stop").unwrap();
    assert_eq!(out, ["q1", "r1"], "both are named: one cancelled, one asked to stop");
    let rows = f.rows("SELECT key || ':' || status, cancel_reason FROM jobs WHERE plan = 'pay' ORDER BY key");
    assert_eq!(rows, [("q1:cancelled".to_owned(), Some("stop".to_owned())), ("r1:running".to_owned(), Some("stop".to_owned()))]);
    let requested = f.rows("SELECT key, cancel_requested_at FROM jobs WHERE plan = 'pay' AND key = 'r1'");
    assert!(requested[0].1.is_some(), "the cancel of the running Job is requested, for the engine to stop its Attempt");
    let events = f.rows("SELECT kind, NULL FROM events WHERE kind IN ('job-cancelled', 'job-cancel-requested') ORDER BY id");
    assert_eq!(events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-cancelled", "job-cancel-requested"]);
}
