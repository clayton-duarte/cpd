//! The pro-d88 rule ([`failed_worked_on`]) and its two readers of the core: `stuck_jobs`.

use std::collections::BTreeSet;
use std::time::Duration;

use time::OffsetDateTime;
use time::macros::datetime;

use super::worked_on::failed_worked_on;
use crate::home::CpdHome;
use crate::ids::{JobKey, PlanId};
use crate::inbox::rules::stuck_jobs;
use crate::store::Store;

const T: &str = "2026-01-01T00:00:00.000Z";
const NOW: OffsetDateTime = datetime!(2026-01-01 00:20 UTC);
const TEN: Duration = Duration::from_secs(600);
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

/// Context `ctx`; Plans `pay` and `ops`, each with a Lead (`l-pay`, `l-ops`) in `lead_state`; PRs `p1`, `p2` of `pay` and `p1` of `ops`.
fn fixture(lead_state: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture { _dir: dir, store };
    let label = if lead_state == "waiting" || lead_state == "blocked" { "'Which?'" } else { "NULL" };
    let mut sql = format!("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');");
    for plan in ["pay", "ops"] {
        sql.push_str(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_label, state_at, created_at)
               VALUES ('l-{plan}', 'lead', 'ctx', '{plan}', 'Lead', '/', 'O3', '{lead_state}', {label}, '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('{plan}', 'ctx', 'P', 'dispatch', 'l-{plan}', '{T}', '{T}');"
        ));
    }
    for (plan, pr) in [("pay", "p1"), ("pay", "p2"), ("ops", "p1")] {
        sql.push_str(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('{plan}', '{pr}', '/r', 'main', 'dispatch', 'cpd/{plan}/{pr}', '/lw', 1, '{T}', '{T}');"
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
    fn job(&mut self, plan: &str, pr: &str, key: &str, status: &str) {
        let cause = if status == "failed" { "'crashed'" } else { "NULL" };
        let sha = if status == "merged" { "'abc'" } else { "NULL" };
        let reason = if status == "cancelled" { "'r'" } else { "NULL" };
        let reason_at = if status == "cancelled" { format!("'{T}'") } else { "NULL".to_owned() };
        let fp = if status == "draft" { "NULL".to_owned() } else { format!("'{FP}'") };
        self.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, failed_cause, merged_sha, cancel_reason, cancel_requested_at)
               VALUES ('{plan}', '{pr}', '{key}', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, {fp}, cpd_txn(), '{T}', {cause}, {sha}, {reason}, {reason_at});"
        ));
    }

    /// The Job's `job-failed` Event, in a transaction of its own.
    fn failure(&mut self, plan: &str, key: &str) {
        self.sql(&format!(
            "INSERT INTO events (txn, at, kind, context, plan, job, text, data)
             VALUES (cpd_txn(), '{T}', 'job-failed', 'ctx', '{plan}', '{key}', 'x', '{{}}');"
        ));
    }

    /// A failed Job with its Event.
    fn failed(&mut self, plan: &str, pr: &str, key: &str) {
        self.job(plan, pr, key, "failed");
        self.failure(plan, key);
    }

    fn set(&mut self, plan: &str, key: &str, status: &str) {
        let cause = if status == "failed" { "'crashed'" } else { "NULL" };
        let sha = if status == "merged" { "'abc'" } else { "NULL" };
        let reason = if status == "cancelled" { "'r'" } else { "NULL" };
        let reason_at = if status == "cancelled" { format!("'{T}'") } else { "NULL".to_owned() };
        self.sql(&format!("UPDATE jobs SET status = '{status}', failed_cause = {cause}, merged_sha = {sha}, cancel_reason = {reason}, cancel_requested_at = {reason_at} WHERE plan = '{plan}' AND key = '{key}'"));
    }

    fn lead(&mut self, plan: &str, state: &str) {
        let label = if state == "waiting" || state == "blocked" { "'Which?'" } else { "NULL" };
        self.sql(&format!("UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = 'l-{plan}'"));
    }

    fn worked(&mut self, plan: Option<&str>) -> BTreeSet<(String, String)> {
        let plan = plan.map(|p| PlanId::parse(p).unwrap());
        let set = self.store.read(|c| failed_worked_on(c, plan.as_ref())).unwrap();
        set.into_iter().map(|(p, j)| (p.as_str().to_owned(), j.as_str().to_owned())).collect()
    }

    fn holds(&mut self, key: &str) -> bool {
        self.worked(Some("pay")).contains(&("pay".to_owned(), key.to_owned()))
    }

    fn stuck(&mut self) -> Vec<(String, i64, String, i64)> {
        stuck_jobs(&mut self.store, NOW, TEN).unwrap().into_iter().map(|s| (s.job.as_str().to_owned(), s.minutes, s.cause, s.event)).collect()
    }
}

#[allow(dead_code)]
fn _types(_: JobKey) {}

#[test]
fn worked_on_holds_for_a_failed_job_while_its_lead_works() {
    let mut f = fixture("working");
    f.failed("pay", "p1", "a");
    assert!(f.holds("a"), "a working Lead is working on it");
    for state in ["idle", "waiting", "blocked", "stopped"] {
        f.lead("pay", state);
        assert!(!f.holds("a"), "a {state} Lead is not working on it");
    }
}

#[test]
fn worked_on_holds_for_a_failed_job_with_a_queued_or_running_job_of_its_pr_created_after_the_failure() {
    let mut f = fixture("idle");
    f.failed("pay", "p1", "a");
    assert!(!f.holds("a"));
    f.job("pay", "p1", "fix", "queued");
    assert!(f.holds("a"), "a queued fix");
    f.set("pay", "fix", "running");
    assert!(f.holds("a"), "a running fix");
    // A second failure: the fix of the first one no longer counts; one created after the second does.
    f.set("pay", "fix", "merged");
    f.job("pay", "p1", "between", "queued");
    f.set("pay", "a", "queued");
    f.set("pay", "a", "failed");
    f.failure("pay", "a");
    assert!(!f.holds("a"), "a Job created before the second failure does not count");
    f.job("pay", "p1", "again", "queued");
    assert!(f.holds("a"), "a Job created after the second failure counts");
}

#[test]
fn worked_on_ignores_jobs_created_before_the_failure_other_prs_and_jobs_that_are_not_queued_or_running() {
    let mut f = fixture("idle");
    f.job("pay", "p1", "early", "queued");
    f.failed("pay", "p1", "a");
    f.job("pay", "p2", "other", "queued");
    for (key, status) in [("d", "draft"), ("m", "merged"), ("f", "failed"), ("c", "cancelled")] {
        f.job("pay", "p1", key, status);
    }
    assert!(!f.holds("a"));
    // Control: a queued Job of its PR created after the failure does.
    f.job("pay", "p1", "fix", "queued");
    assert!(f.holds("a"));
}

#[test]
fn worked_on_never_holds_for_a_job_that_is_not_failed() {
    let mut f = fixture("working");
    for (key, status) in [("q", "queued"), ("r", "running"), ("m", "merged"), ("c", "cancelled"), ("d", "draft")] {
        f.job("pay", "p1", key, status);
        f.failure("pay", key);
    }
    assert_eq!(f.worked(None), BTreeSet::new());
    // Control: a failed Job with its Lead working is in the set, and only it.
    f.failed("pay", "p1", "x");
    assert_eq!(f.worked(None), BTreeSet::from([("pay".to_owned(), "x".to_owned())]));
}

#[test]
fn worked_on_stops_holding_when_the_fix_ends_and_the_lead_is_idle() {
    for end in ["merged", "cancelled", "failed"] {
        let mut f = fixture("idle");
        f.failed("pay", "p1", "a");
        f.job("pay", "p1", "fix", "queued");
        assert!(f.holds("a"), "queued");
        f.set("pay", "fix", "running");
        assert!(f.holds("a"), "running");
        f.set("pay", "fix", end);
        assert!(!f.holds("a"), "the fix {end}");
    }
}

#[test]
fn worked_on_reads_one_plan_or_every_plan() {
    let mut f = fixture("idle");
    f.failed("pay", "p1", "a");
    f.failed("ops", "p1", "b");
    f.job("pay", "p1", "fix-a", "queued");
    f.job("ops", "p1", "fix-b", "queued");
    let a = ("pay".to_owned(), "a".to_owned());
    let b = ("ops".to_owned(), "b".to_owned());
    assert_eq!(f.worked(Some("pay")), BTreeSet::from([a.clone()]));
    assert_eq!(f.worked(Some("ops")), BTreeSet::from([b.clone()]));
    assert_eq!(f.worked(None), BTreeSet::from([a, b.clone()]));
    // A queued Job of `pay` does not help the failed Job of `ops` once its own fix is gone.
    f.set("ops", "fix-b", "cancelled");
    assert_eq!(f.worked(Some("ops")), BTreeSet::new());
    assert!(!f.worked(None).contains(&b));
}

#[test]
fn a_failed_job_is_not_stuck_while_a_fix_job_of_its_pr_is_queued_or_running() {
    let mut f = fixture("idle");
    f.failed("pay", "p1", "a");
    assert_eq!(f.stuck().len(), 1, "no fix yet");
    f.job("pay", "p2", "other", "queued");
    assert_eq!(f.stuck().len(), 1, "a Job of another PR does not help");
    f.job("pay", "p1", "fix", "queued");
    assert_eq!(f.stuck(), [], "a queued fix");
    f.set("pay", "fix", "running");
    assert_eq!(f.stuck(), [], "a running fix");
}

#[test]
fn a_failed_job_is_stuck_when_its_fix_job_ended_and_its_lead_is_idle() {
    for end in ["cancelled", "failed", "merged"] {
        let mut f = fixture("idle");
        f.failed("pay", "p1", "a");
        let before = f.stuck();
        assert_eq!(before.len(), 1);
        f.job("pay", "p1", "fix", "queued");
        assert_eq!(f.stuck(), [], "the fix is underway");
        if end != "cancelled" {
            f.set("pay", "fix", "running");
        }
        f.set("pay", "fix", end);
        assert_eq!(f.stuck(), before, "the fix {end}: stuck again, same event, minutes and cause");
    }
}
