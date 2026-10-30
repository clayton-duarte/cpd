//! Red tests of the same-Tier retry (contract §3.4 "Ladder causes", pro-d265): a `crashed` or `merge_conflict`
//! Attempt requeues its Job at its OWN Tier, `no_submit` alone climbs, `retry_cap` still bounds the automatic
//! Attempts, and a crash at the top rung is retried in place.

use serde_json::Value;

use crate::ids::{AttemptId, PlanId};

use super::incident_tests::{Fixture, catalog, fixture};
use super::ledger::{self, Cause, End, Ladder, Stage};

/// Inserts the Attempt `a-a<number>` of Job `a`: `running` with a driver, or ended `failed` / `cause` and counted.
fn add(f: &mut Fixture, number: u32, tier: &str, ended: Option<&str>) {
    let (status, outcome, cause) = match ended {
        Some(cause) => ("ended", format!("'failed'"), format!("'{cause}'")),
        None => ("running", "NULL".to_owned(), "NULL".to_owned()),
    };
    let ended_at = if ended.is_some() { "'t'" } else { "NULL" };
    f.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, driver_pid, started_at, ended_at)
                   VALUES ('pay', 'a-a{number}', 'a', {number}, '{tier}', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--a-a{number}', '/w', 4242, 't', {ended_at});"
            ))?;
            Ok(())
        })
        .unwrap();
}

/// Ends `a-a<number>` with `cause` through the ledger and returns the Job's `(status, failed_cause)`.
fn end(f: &mut Fixture, number: u32, cause: Cause, retry_cap: u32) -> (String, Option<String>) {
    let attempt = f
        .store
        .read(|conn| ledger::load(conn, &PlanId::parse("pay").unwrap(), &AttemptId::parse(&format!("a-a{number}")).unwrap()))
        .unwrap();
    let catalog = catalog();
    let ladder = Ladder { catalog: &catalog, retry_cap };
    f.store
        .write("test", |tx| ledger::end(tx, &attempt, &End::Failed { cause, detail: "It failed.".into() }, None, &ladder, Stage::Driver))
        .unwrap();
    f.store
        .read(|conn| {
            Ok(conn.query_row("SELECT status, failed_cause FROM jobs WHERE plan = 'pay' AND key = 'a'", [], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?)
        })
        .unwrap()
}

/// The `next_tier` of each `attempt-retry` Event, oldest first.
fn next_tiers(f: &mut Fixture) -> Vec<String> {
    f.store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = 'attempt-retry' ORDER BY id")?;
            Ok(stmt
                .query_map([], |r| {
                    let data: String = r.get(0)?;
                    let v: Value = serde_json::from_str(&data).expect("data is JSON");
                    Ok(v["next_tier"].as_str().unwrap_or_default().to_owned())
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn retried_at(cause: Cause, tier: &str, retry_cap: u32) {
    let mut f = fixture();
    add(&mut f, 1, tier, None);
    let (status, _) = end(&mut f, 1, cause, retry_cap);
    assert_eq!(status, "queued", "{cause:?} at {tier} requeues the Job");
    assert_eq!(next_tiers(&mut f), [tier], "{cause:?} is retried at the same Tier");
}

#[test]
fn crashed_retries_at_the_same_tier() {
    retried_at(Cause::Crashed, "S2", 2);
}

#[test]
fn merge_conflict_retries_at_the_same_tier() {
    retried_at(Cause::MergeConflict, "S2", 2);
}

#[test]
fn no_submit_still_climbs() {
    retried_at_next(Cause::NoSubmit, "S1", "S2");
}

fn retried_at_next(cause: Cause, tier: &str, next: &str) {
    let mut f = fixture();
    add(&mut f, 1, tier, None);
    let (status, _) = end(&mut f, 1, cause, 2);
    assert_eq!(status, "queued");
    assert_eq!(next_tiers(&mut f), [next], "{cause:?} climbs one rung");
}

#[test]
fn retry_cap_still_bounds_automatic_attempts() {
    for cause in [Cause::Crashed, Cause::MergeConflict] {
        let mut f = fixture();
        add(&mut f, 1, "S2", Some(cause.as_str()));
        add(&mut f, 2, "S2", None);
        let (status, failed_cause) = end(&mut f, 2, cause, 1);
        assert_eq!(status, "failed", "{cause:?}: 1 + retry_cap Attempts at most");
        assert_eq!(failed_cause.as_deref(), Some(cause.as_str()));
        assert!(next_tiers(&mut f).is_empty(), "no attempt-retry at the cap");
    }
}

#[test]
fn a_crash_at_the_top_rung_is_retried_at_the_top_rung() {
    retried_at(Cause::Crashed, "S3", 2);

    // `no_submit` at the top rung still fails the Job: it climbs, and there is no rung above S3.
    let mut f = fixture();
    add(&mut f, 1, "S3", None);
    let (status, failed_cause) = end(&mut f, 1, Cause::NoSubmit, 2);
    assert_eq!(status, "failed");
    assert_eq!(failed_cause.as_deref(), Some("no_submit"));
}
