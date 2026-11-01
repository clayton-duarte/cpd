//! The 30 s hold after a provider_error Attempt (decision pro-d102): time moves only on a `ManualClock`.

use std::time::Duration;

use cpd_core::clock::{Clock, ManualClock, timestamp};
use cpd_core::ids::{JobKey, PlanId};
use rusqlite::{Connection, params};

use super::provider_hold::holding_with;

fn db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE attempts (plan TEXT, id TEXT, job TEXT, number INTEGER, status TEXT, outcome TEXT, cause TEXT, driver_pid INTEGER, ended_at TEXT);",
    )
    .unwrap();
    conn
}

/// An Attempt that ended at the clock's current time (or is still running when `cause` is `None` and `status` is "running").
fn end(
    conn: &Connection,
    clock: &ManualClock,
    plan: &str,
    job: &str,
    number: i64,
    outcome: &str,
    cause: Option<&str>,
) {
    conn.execute(
        "INSERT INTO attempts (plan, id, job, number, status, outcome, cause, driver_pid, ended_at) VALUES (?1, ?1 || ?2 || ?3, ?2, ?3, 'ended', ?4, ?5, 7, ?6)",
        params![plan, job, number, outcome, cause, timestamp(clock.utc())],
    )
    .unwrap();
}

fn running(conn: &Connection, plan: &str, job: &str, number: i64) {
    conn.execute(
        "INSERT INTO attempts (plan, id, job, number, status, outcome, cause, driver_pid, ended_at) VALUES (?1, ?1 || ?2 || ?3, ?2, ?3, 'running', NULL, NULL, 7, NULL)",
        params![plan, job, number],
    )
    .unwrap();
}

fn held(conn: &Connection, clock: &ManualClock, plan: &str, job: &str) -> bool {
    holding_with(
        conn,
        &PlanId::parse(plan).unwrap(),
        &JobKey::parse(job).unwrap(),
        clock,
    )
    .unwrap()
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn a_job_whose_latest_attempt_ended_provider_error_is_held_until_30_seconds_have_passed() {
    let (conn, clock) = (db(), ManualClock::new());
    end(
        &conn,
        &clock,
        "pay",
        "api",
        1,
        "failed",
        Some("provider_error"),
    );
    assert!(held(&conn, &clock, "pay", "api"));
    clock.advance(secs(29));
    assert!(held(&conn, &clock, "pay", "api"));
    clock.advance(secs(1));
    assert!(!held(&conn, &clock, "pay", "api"));
}

#[test]
fn a_job_with_no_attempt_or_no_provider_error_attempt_is_never_held() {
    let (conn, clock) = (db(), ManualClock::new());
    assert!(!held(&conn, &clock, "pay", "api"));
    end(&conn, &clock, "pay", "api", 1, "failed", Some("crashed"));
    clock.advance(secs(1));
    assert!(!held(&conn, &clock, "pay", "api"));
    end(&conn, &clock, "pay", "web", 1, "merged", None);
    clock.advance(secs(1));
    assert!(!held(&conn, &clock, "pay", "web"));
    // Control: only a provider_error Attempt holds.
    end(
        &conn,
        &clock,
        "pay",
        "web",
        2,
        "failed",
        Some("provider_error"),
    );
    assert!(held(&conn, &clock, "pay", "web"));
}

#[test]
fn a_later_attempt_of_another_cause_ends_the_hold() {
    let (conn, clock) = (db(), ManualClock::new());
    end(
        &conn,
        &clock,
        "pay",
        "api",
        1,
        "failed",
        Some("provider_error"),
    );
    clock.advance(secs(1));
    assert!(held(&conn, &clock, "pay", "api"));
    end(&conn, &clock, "pay", "api", 2, "failed", Some("crashed"));
    assert!(!held(&conn, &clock, "pay", "api"));
}

#[test]
fn the_hold_is_per_job() {
    let (conn, clock) = (db(), ManualClock::new());
    end(
        &conn,
        &clock,
        "pay",
        "a",
        1,
        "failed",
        Some("provider_error"),
    );
    assert!(held(&conn, &clock, "pay", "a"));
    assert!(!held(&conn, &clock, "pay", "b"));
    assert!(!held(&conn, &clock, "other", "a"));
}

#[test]
fn the_hold_does_not_grow_with_the_number_of_provider_errors() {
    let (conn, clock) = (db(), ManualClock::new());
    for n in 1..=3 {
        end(
            &conn,
            &clock,
            "pay",
            "api",
            n,
            "failed",
            Some("provider_error"),
        );
        if n < 3 {
            clock.advance(secs(30));
        }
    }
    assert!(held(&conn, &clock, "pay", "api"));
    clock.advance(secs(29));
    assert!(held(&conn, &clock, "pay", "api"));
    clock.advance(secs(1));
    assert!(!held(&conn, &clock, "pay", "api"));
}

#[test]
fn an_attempt_still_running_does_not_hold() {
    let (conn, clock) = (db(), ManualClock::new());
    end(
        &conn,
        &clock,
        "pay",
        "api",
        1,
        "failed",
        Some("provider_error"),
    );
    clock.advance(secs(1));
    assert!(held(&conn, &clock, "pay", "api"));
    running(&conn, "pay", "api", 2);
    assert!(!held(&conn, &clock, "pay", "api"));
}
