//! Red tests of the engine's retry of a failed `open` or `update` Job when its fix merges (pro-d89).

use serde_json::json;

use super::fix_retry::retry_after_fix;
use crate::attempt::ledger::{self, Cause, End, Ladder, Stage};
use crate::config::{Catalog, TierModel};
use crate::home::CpdHome;
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::store::Store;

const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

/// Plans `pay` (PRs `api`, `web`) and `ops` (PR `api`).
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture { _dir: dir, store };
    let mut sql = format!("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');");
    for plan in ["pay", "ops"] {
        sql.push_str(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-{plan}', 'lead', 'ctx', '{plan}', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('{plan}', 'ctx', 'P', 'dispatch', 'l-{plan}', '{T}', '{T}');"
        ));
    }
    for (plan, workflow) in [("pay", "api"), ("pay", "web"), ("ops", "api")] {
        sql.push_str(&format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('{plan}', '{workflow}', '/r', 'main', 'dispatch', 'cpd/{plan}/{workflow}', '/lw', 1, '{T}', '{T}');"
        ));
    }
    f.sql(&sql);
    f
}

impl Fixture {
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// A Job in its own transaction (so `created_txn` grows with each call).
    fn job(&mut self, plan: &str, workflow: &str, key: &str, kind: &str, status: &str) {
        let cause = if status == "failed" { "'crashed'" } else { "NULL" };
        let sha = if status == "merged" { "'abc'" } else { "NULL" };
        let reason = if status == "cancelled" { "'r'" } else { "NULL" };
        let reason_at = if status == "cancelled" { format!("'{T}'") } else { "NULL".to_owned() };
        self.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended,
                               fingerprint, created_txn, created_at, failed_cause, merged_sha, cancel_reason, cancel_requested_at)
               VALUES ('{plan}', '{workflow}', '{key}', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{kind}', '{status}', 0, '{FP}', cpd_txn(), '{T}', {cause}, {sha}, {reason}, {reason_at});"
        ));
    }

    /// The Job's `job-failed` Event, in a transaction of its own.
    fn failure(&mut self, plan: &str, key: &str) {
        self.sql(&format!(
            "INSERT INTO events (txn, at, kind, context, plan, job, text, data)
             VALUES (cpd_txn(), '{T}', 'job-failed', 'ctx', '{plan}', '{key}', 'x', '{{}}');"
        ));
    }

    /// An ended failed Attempt of the Job.
    fn attempt(&mut self, plan: &str, job: &str, number: i64, tier: &str) {
        self.sql(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, driver_pid, started_at, ended_at)
               VALUES ('{plan}', '{job}-a{number}', '{job}', {number}, '{tier}', 'ladder', 'ended', 'failed', 'crashed',
                       'cpd/{plan}--{job}-a{number}', '/w', 1, '{T}', '{T}');"
        ));
    }

    /// A failed Job with one ended Attempt at `S2` and its Event.
    fn failed(&mut self, plan: &str, workflow: &str, key: &str, kind: &str) {
        self.job(plan, workflow, key, kind, "failed");
        self.attempt(plan, key, 1, "S2");
        self.failure(plan, key);
    }

    fn set(&mut self, plan: &str, key: &str, status: &str) {
        let cause = if status == "failed" { "'crashed'" } else { "NULL" };
        let sha = if status == "merged" { "'abc'" } else { "NULL" };
        let reason = if status == "cancelled" { "'r'" } else { "NULL" };
        let reason_at = if status == "cancelled" { format!("'{T}'") } else { "NULL".to_owned() };
        self.sql(&format!("UPDATE jobs SET status = '{status}', failed_cause = {cause}, merged_sha = {sha}, cancel_reason = {reason}, cancel_requested_at = {reason_at} WHERE plan = '{plan}' AND key = '{key}'"));
    }

    /// `retry_after_fix` for `merged` of Plan `plan`, in a write transaction.
    fn retry(&mut self, plan: &str, merged: &str) -> Vec<String> {
        let (plan, merged) = (PlanId::parse(plan).unwrap(), JobKey::parse(merged).unwrap());
        let keys = self.store.write("test", |tx| retry_after_fix(tx, &plan, &merged)).unwrap();
        keys.iter().map(|k| k.as_str().to_owned()).collect()
    }

    fn column(&mut self, plan: &str, key: &str, column: &str) -> Option<String> {
        self.store
            .read(|c| Ok(c.query_row(&format!("SELECT {column} FROM jobs WHERE plan = ?1 AND key = ?2"), [plan, key], |r| r.get(0))?))
            .unwrap()
    }

    fn status(&mut self, plan: &str, key: &str) -> String {
        self.column(plan, key, "status").unwrap()
    }

    /// The `(job, text)` of every `job-retried` Event.
    fn retried(&mut self) -> Vec<(String, String)> {
        self.store
            .read(|c| {
                let mut stmt = c.prepare("SELECT job, text FROM events WHERE kind = 'job-retried' ORDER BY id")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    /// `f` of `pay`/`api` failed, then fix `j` created and merged after it.
    fn failed_then_fixed(&mut self, kind: &str) {
        self.failed("pay", "api", "f", kind);
        self.job("pay", "api", "j", "work", "merged");
    }

    /// A running Job with a `running` (or `merging`) Attempt, for the ledger.
    fn running(&mut self, plan: &str, workflow: &str, key: &str, attempt_status: &str, cancel: bool) {
        self.job(plan, workflow, key, "work", "running");
        if cancel {
            self.sql(&format!("UPDATE jobs SET cancel_reason = 'r', cancel_requested_at = '{T}' WHERE plan = '{plan}' AND key = '{key}'"));
        }
        let (sha, sha_at) = if attempt_status == "merging" { ("'abc'".to_owned(), format!("'{T}'")) } else { ("NULL".to_owned(), "NULL".to_owned()) };
        self.sql(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, submitted_sha, submitted_at, started_at)
               VALUES ('{plan}', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{attempt_status}', 'cpd/{plan}--{key}-a1', '/w', 4242, {sha}, {sha_at}, '{T}');"
        ));
    }

    fn end(&mut self, plan: &str, key: &str, end: End, stage: Stage) {
        let plan = PlanId::parse(plan).unwrap();
        let attempt = self.store.read(|c| ledger::load(c, &plan, &AttemptId::parse(&format!("{key}-a1")).unwrap())).unwrap();
        let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![1, 2, 3] };
        let catalog = Catalog {
            pi_version: "1.0.0".to_owned(),
            o: model("o/m"),
            s: model("s/m"),
            h: model("h/m"),
            g: TierModel { model: "g/m".to_owned(), efforts: vec![] },
        };
        let ladder = Ladder { catalog: &catalog, retry_cap: 3 };
        self.store.write("test", |tx| ledger::end(tx, &attempt, &end, None, &ladder, stage)).unwrap();
    }
}

#[test]
fn fix_retry_requeues_a_failed_open_job_when_a_fix_created_after_its_failure_merges() {
    let mut f = fixture();
    f.failed_then_fixed("open");
    assert_eq!(f.retry("pay", "j"), vec!["f"]);
    assert_eq!(f.status("pay", "f"), "queued");
    assert_eq!(f.column("pay", "f", "failed_cause"), None);
}

#[test]
fn fix_retry_requeues_a_failed_update_job_too() {
    let mut f = fixture();
    f.failed_then_fixed("update");
    assert_eq!(f.retry("pay", "j"), vec!["f"]);
    assert_eq!(f.status("pay", "f"), "queued");
}

#[test]
fn fix_retry_requeues_at_the_tier_of_the_jobs_latest_attempt() {
    let mut f = fixture();
    f.job("pay", "api", "f", "open", "failed");
    f.attempt("pay", "f", 1, "S1");
    f.attempt("pay", "f", 3, "H2");
    f.attempt("pay", "f", 2, "S3");
    f.failure("pay", "f");
    f.job("pay", "api", "j", "work", "merged");
    assert_eq!(f.retry("pay", "j"), vec!["f"]);
    assert_eq!(f.column("pay", "f", "retry_tier").as_deref(), Some("H2"));
}

#[test]
fn fix_retry_writes_one_job_retried_event_naming_the_fix() {
    let mut f = fixture();
    f.failed_then_fixed("open");
    f.retry("pay", "j");
    assert_eq!(
        f.retried(),
        vec![(
            "f".to_owned(),
            "Job f of Plan pay is queued again at Tier S2 after fix j merged. Next: cpd job show f --plan pay".to_owned()
        )]
    );
}

#[test]
fn fix_retry_waits_while_another_job_of_the_pr_created_after_the_failure_is_queued_or_running() {
    let mut f = fixture();
    f.failed("pay", "api", "f", "open");
    f.job("pay", "api", "j", "work", "merged");
    f.job("pay", "api", "other", "work", "queued");
    assert!(f.retry("pay", "j").is_empty(), "another fix is queued");
    f.set("pay", "other", "running");
    assert!(f.retry("pay", "j").is_empty(), "another fix is running");
    assert_eq!(f.status("pay", "f"), "failed");
    assert!(f.retried().is_empty());
    f.set("pay", "other", "merged");
    assert_eq!(f.retry("pay", "j"), vec!["f"], "nothing else is working on the PR");
}

#[test]
fn fix_retry_ignores_a_fix_created_before_the_failure() {
    let mut f = fixture();
    f.job("pay", "api", "j", "work", "merged");
    f.failed("pay", "api", "f", "open");
    assert!(f.retry("pay", "j").is_empty());
    assert_eq!(f.status("pay", "f"), "failed");
    assert!(f.retried().is_empty());
}

#[test]
fn fix_retry_never_retries_a_cancelled_job() {
    let mut f = fixture();
    f.failed_then_fixed("open");
    f.set("pay", "f", "cancelled");
    assert!(f.retry("pay", "j").is_empty());
    assert_eq!(f.status("pay", "f"), "cancelled");
    assert!(f.retried().is_empty());
}

#[test]
fn fix_retry_never_retries_a_job_the_lead_already_retried() {
    let mut f = fixture();
    f.failed_then_fixed("open");
    f.sql("UPDATE jobs SET status = 'queued', retry_tier = 'H1', failed_cause = NULL WHERE plan = 'pay' AND key = 'f'");
    assert!(f.retry("pay", "j").is_empty());
    assert_eq!(f.column("pay", "f", "retry_tier").as_deref(), Some("H1"));
    assert!(f.retried().is_empty());
}

#[test]
fn fix_retry_never_retries_a_failed_work_job() {
    let mut f = fixture();
    f.failed_then_fixed("work");
    assert!(f.retry("pay", "j").is_empty());
    assert_eq!(f.status("pay", "f"), "failed");
    assert!(f.retried().is_empty());
}

#[test]
fn fix_retry_ignores_other_prs_and_other_plans() {
    let mut f = fixture();
    f.failed("pay", "web", "other-pr", "open");
    f.failed("ops", "api", "other-plan", "open");
    f.failed("pay", "api", "f", "update");
    f.job("pay", "api", "j", "work", "merged");
    assert_eq!(f.retry("pay", "j"), vec!["f"], "only the Job of the fix's PR");
    assert_eq!(f.status("pay", "other-pr"), "failed");
    assert_eq!(f.status("ops", "other-plan"), "failed");
    assert_eq!(f.retried().len(), 1);
}

#[test]
fn fix_retry_a_merged_open_or_update_job_retries_nothing() {
    for kind in ["open", "update"] {
        let mut f = fixture();
        f.failed("pay", "api", "f", "open");
        f.job("pay", "api", "j", kind, "merged");
        assert!(f.retry("pay", "j").is_empty(), "a merged {kind} Job is not a fix");
        assert_eq!(f.status("pay", "f"), "failed");
        assert!(f.retried().is_empty());
    }
}

#[test]
fn fix_retry_never_retries_twice_for_one_failure() {
    let mut f = fixture();
    f.failed_then_fixed("open");
    assert_eq!(f.retry("pay", "j"), vec!["f"]);
    assert!(f.retry("pay", "j").is_empty(), "queued now");
    // It fails again: the new `job-failed` Event is newer than `j`.
    f.set("pay", "f", "failed");
    f.failure("pay", "f");
    assert!(f.retry("pay", "j").is_empty(), "j does not postdate the second failure");
    assert_eq!(f.status("pay", "f"), "failed");
    assert_eq!(f.retried().len(), 1);
}

#[test]
fn fix_retry_a_merge_ending_through_the_ledger_retries_in_the_same_transaction() {
    let mut f = fixture();
    f.failed("pay", "api", "f", "open");
    f.running("pay", "api", "j", "merging", false);
    f.end("pay", "j", End::Merged { sha: "abc".to_owned() }, Stage::Merge);
    assert_eq!(f.status("pay", "j"), "merged");
    assert_eq!(f.status("pay", "f"), "queued");
    assert_eq!(f.column("pay", "f", "retry_tier").as_deref(), Some("S2"));
    let retried = f.retried();
    assert_eq!(retried.len(), 1);
    assert!(retried[0].1.contains("after fix j merged"), "{retried:?}");
    let data: String = f.store.read(|c| Ok(c.query_row("SELECT data FROM events WHERE kind = 'job-retried'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&data).unwrap(), json!({"tier": "S2"}));
}

#[test]
fn fix_retry_an_attempt_ending_failed_or_cancelled_retries_nothing() {
    let mut f = fixture();
    f.failed("pay", "api", "f", "open");
    f.running("pay", "api", "bad", "running", false);
    f.end("pay", "bad", End::Failed { cause: Cause::Tampered, detail: "d".to_owned() }, Stage::Driver);
    f.running("pay", "api", "gone", "running", true);
    f.end("pay", "gone", End::Cancelled, Stage::Driver);
    assert_eq!(f.status("pay", "bad"), "failed");
    assert_eq!(f.status("pay", "gone"), "cancelled");
    assert_eq!(f.status("pay", "f"), "failed", "no fix merged");
    assert!(f.retried().is_empty());
    // Control: a merge does retry.
    f.running("pay", "api", "good", "merging", false);
    f.end("pay", "good", End::Merged { sha: "abc".to_owned() }, Stage::Merge);
    assert_eq!(f.status("pay", "f"), "queued");
}
