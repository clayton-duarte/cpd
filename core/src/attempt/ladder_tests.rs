//! Red tests of the same-Tier retry (contract §3.4 "Ladder causes", pro-d265): a `crashed` or `merge_conflict`
//! Attempt requeues its Job at its OWN Tier, `no_submit` alone climbs, `retry_cap` still bounds the automatic
//! Attempts, and a crash at the top rung is retried in place.

use serde_json::Value;

use crate::config::{Catalog, TierModel};
use crate::ids::{AttemptId, PlanId};

use super::incident_tests::{Fixture, catalog, fixture};
use super::ledger::{self, Cause, End, Ladder, Stage};

/// Inserts the Attempt `<job>-a<number>`: `running` with a driver, or ended `failed` / `cause` and counted.
fn add(f: &mut Fixture, job: &str, number: u32, tier: &str, ended: Option<&str>) {
    let (status, outcome, cause) = match ended {
        Some(cause) => ("ended", format!("'failed'"), format!("'{cause}'")),
        None => ("running", "NULL".to_owned(), "NULL".to_owned()),
    };
    let ended_at = if ended.is_some() { "'t'" } else { "NULL" };
    f.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, driver_pid, started_at, ended_at)
                   VALUES ('pay', '{job}-a{number}', '{job}', {number}, '{tier}', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--{job}-a{number}', '/w', 4242, 't', {ended_at});"
            ))?;
            Ok(())
        })
        .unwrap();
}

/// Ends `<job>-a<number>` with `cause` through the ledger and returns the Job's `(status, failed_cause)`.
fn end(f: &mut Fixture, job: &str, number: u32, cause: Cause, retry_cap: u32) -> (String, Option<String>) {
    end_on(f, job, number, cause, retry_cap, &catalog())
}

/// [`end`] on a caller's Catalog: the explore test pins `next_tier` on the gappy catalog of
/// contract §9 "Ladder" (hs-d791) without touching `incident_tests::catalog`.
fn end_on(f: &mut Fixture, job: &str, number: u32, cause: Cause, retry_cap: u32, cat: &Catalog) -> (String, Option<String>) {
    let attempt = f
        .store
        .read(|conn| {
            ledger::load(conn, &PlanId::parse("pay").unwrap(), &AttemptId::parse(&format!("{job}-a{number}")).unwrap())
        })
        .unwrap();
    let ladder = Ladder { catalog: cat, retry_cap };
    f.store
        .write("test", |tx| ledger::end(tx, &attempt, &End::Failed { cause, detail: "It failed.".into() }, None, &ladder, Stage::Driver))
        .unwrap();
    f.store
        .read(|conn| {
            Ok(conn.query_row(&format!("SELECT status, failed_cause FROM jobs WHERE plan = 'pay' AND key = '{job}'"), [], |r| {
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
    add(&mut f, "a", 1, tier, None);
    let (status, _) = end(&mut f, "a", 1, cause, retry_cap);
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
    add(&mut f, "a", 1, tier, None);
    let (status, _) = end(&mut f, "a", 1, cause, 2);
    assert_eq!(status, "queued");
    assert_eq!(next_tiers(&mut f), [next], "{cause:?} climbs one rung");
}

#[test]
fn retry_cap_still_bounds_automatic_attempts() {
    for cause in [Cause::Crashed, Cause::MergeConflict] {
        let mut f = fixture();
        add(&mut f, "a", 1, "S2", Some(cause.as_str()));
        add(&mut f, "a", 2, "S2", None);
        let (status, failed_cause) = end(&mut f, "a", 2, cause, 1);
        assert_eq!(status, "failed", "{cause:?}: 1 + retry_cap Attempts at most");
        assert_eq!(failed_cause.as_deref(), Some(cause.as_str()));
        assert!(next_tiers(&mut f).is_empty(), "no attempt-retry at the cap");
    }
}

/// The gappy catalog of contract §9 "Ladder" (hs-d791): H efforts [1,3,5], S efforts [0,1,3,5],
/// O and G as `incident_tests::catalog` has them. Built inline: `incident_tests::catalog` stays dense.
fn gappy_catalog() -> Catalog {
    Catalog {
        pi_version: "1.0.0".into(),
        o: TierModel { model: "o/m".into(), efforts: vec![1, 2, 3] },
        s: TierModel { model: "s/m".into(), efforts: vec![0, 1, 3, 5] },
        h: TierModel { model: "h/m".into(), efforts: vec![1, 3, 5] },
        g: TierModel { model: "g/m".into(), efforts: vec![] },
    }
}

/// Adds the Plan's explore Job `b` of `kind` (`scout`, `research`): `running`, no PR (an explore Job
/// belongs to the Plan), and the fixture's other fields. The fixture's Job `a` stays `work`: `kind` is
/// a Locked field (`jobs_locked` refuses its UPDATE on a running Job), so the test adds its own row.
fn explore_job(f: &mut Fixture, kind: &str) {
    f.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', NULL, 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{kind}', 'running', 0, '{}', cpd_txn(), 't');",
                "f".repeat(64)
            ))?;
            Ok(())
        })
        .unwrap();
}

/// hs-d791, pro-d210: an explore Job's ladder climbs to the next valid effort inside its letter
/// (scout H, research S) and never leaves it; after the top rung a `no_submit` fails the Job.
#[test]
fn explore_kinds_stay_in_their_letter() {
    let gappy = gappy_catalog();

    // A `scout` at H1 with `no_submit` retries at H3: H2 is not a valid effort, and S1 is outside H.
    let mut f = fixture();
    explore_job(&mut f, "scout");
    add(&mut f, "b", 1, "H1", None);
    let (status, _) = end_on(&mut f, "b", 1, Cause::NoSubmit, 2, &gappy);
    assert_eq!(status, "queued", "a scout's no_submit climbs within H");
    assert_eq!(next_tiers(&mut f), ["H3"], "the scout's ladder skips the invalid H2 and stays in H");

    // A `scout` at H3 has no rung above it in H (H5 is not a Worker's Tier): the Job fails.
    let mut f = fixture();
    explore_job(&mut f, "scout");
    add(&mut f, "b", 1, "H3", None);
    let (status, failed_cause) = end_on(&mut f, "b", 1, Cause::NoSubmit, 2, &gappy);
    assert_eq!(status, "failed", "the scout's ladder never leaves H for S1");
    assert_eq!(failed_cause.as_deref(), Some("no_submit"));
    assert!(next_tiers(&mut f).is_empty(), "no attempt-retry when the scout's ladder runs out");

    // A `research` at S1 with `no_submit` retries at S3: S2 is not a valid effort, and S is its last letter.
    let mut f = fixture();
    explore_job(&mut f, "research");
    add(&mut f, "b", 1, "S1", None);
    let (status, _) = end_on(&mut f, "b", 1, Cause::NoSubmit, 2, &gappy);
    assert_eq!(status, "queued", "a research's no_submit climbs within S");
    assert_eq!(next_tiers(&mut f), ["S3"], "the research's ladder skips the invalid S2 and stays in S");
}

#[test]
fn a_crash_at_the_top_rung_is_retried_at_the_top_rung() {
    retried_at(Cause::Crashed, "S3", 2);

    // `no_submit` at the top rung still fails the Job: it climbs, and there is no rung above S3.
    let mut f = fixture();
    add(&mut f, "a", 1, "S3", None);
    let (status, failed_cause) = end(&mut f, "a", 1, Cause::NoSubmit, 2);
    assert_eq!(status, "failed");
    assert_eq!(failed_cause.as_deref(), Some("no_submit"));
}
