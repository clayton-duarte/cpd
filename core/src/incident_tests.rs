//! Red tests for incident mode (contract §2.2 "Incident", §3.7, §5.2; commands.md §25, hs-d634):
//! the Manager-only `cpd incident start`/`end`, the readable `status`, the `incident` row and the
//! two Events. The inc-core Job implements the stubs in `incident.rs` and `events/typed.rs` and
//! makes these pass.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use crate::error::ErrorCode;
use crate::events::{Kind, NewEvent, PROTOCOL, Set};
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::incident;
use crate::output::Output;
use crate::store::Store;

/// A temporary store with a live Manager `mgr`, a Lead `lead1`, a Worker `w1` and an archived
/// Manager `mgr-old`, a Plan `pay` with its PR `api`, the Job `a` and its running Attempt `a-a1`.
struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let fingerprint = "f".repeat(64);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                   VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                   VALUES ('w1', 'worker', 'ctx', 'pay', 'a-a1', 'Worker', '/', 'O3', 'working', 't', 't');
                 INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, archived_at)
                   VALUES ('mgr-old', 'manager', 'ctx', 'Old', '/', 'O3', 'stopped', 't', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pay', 'api', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fingerprint}', cpd_txn(), 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                   VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 't');"
            ))?;
            Ok(())
        })
        .unwrap();
    Fixture { _dir: dir, store }
}

/// [`fixture`] plus the Attempts the stop-request rule distinguishes: `b-a1` (`starting`, no
/// request), `c-a1` (`merging`, no request), `d-a1` (`running`, `timed_out`) and `e-a1`
/// (`running`, `cancelled`).
fn fixture_with_attempts() -> Fixture {
    let mut f = fixture();
    let fingerprint = "f".repeat(64);
    let mut sql = String::new();
    for key in ["b", "c", "d", "e"] {
        sql.push_str(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', 'api', '{key}', '{key}', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fingerprint}', cpd_txn(), 't');\n"
        ));
    }
    sql.push_str(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'b-a1', 'b', 1, 'S2', 'ladder', 'starting', 'cpd/pay--b-a1', '/w', 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, submitted_sha, submitted_at)
           VALUES ('pay', 'c-a1', 'c', 1, 'S2', 'ladder', 'merging', 'cpd/pay--c-a1', '/w', 't', 'sha', 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, stop_request)
           VALUES ('pay', 'd-a1', 'd', 1, 'S2', 'ladder', 'running', 'cpd/pay--d-a1', '/w', 't', 'timed_out');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, stop_request)
           VALUES ('pay', 'e-a1', 'e', 1, 'S2', 'ladder', 'running', 'cpd/pay--e-a1', '/w', 't', 'cancelled');",
    );
    f.store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok(())
        })
        .unwrap();
    f
}

fn fields(out: Output) -> serde_json::Map<String, Value> {
    match out {
        Output::Success(fields) => fields,
        Output::Owned => panic!("incident prints the success envelope"),
    }
}

/// The one `incident` row as `(started_at, started_by, reason, started_txn)`, if any.
fn incident_row(store: &mut Store) -> Option<(String, String, String, i64)> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT started_at, started_by, reason, started_txn FROM incident WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?)
        })
        .unwrap()
}

/// Every Event as `(kind, data)`, in id order.
fn events(store: &mut Store) -> Vec<(String, Value)> {
    store
        .read(|c| {
            let mut stmt = c.prepare("SELECT kind, data FROM events ORDER BY id")?;
            Ok(stmt
                .query_map([], |r| {
                    let data: String = r.get(1)?;
                    Ok((r.get::<_, String>(0)?, serde_json::from_str(&data).expect("data is JSON")))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// Every Attempt's `stop_request`, by id.
fn stop_requests(store: &mut Store) -> BTreeMap<String, Option<String>> {
    store
        .read(|c| {
            let mut stmt = c.prepare("SELECT id, stop_request FROM attempts")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?
                .collect::<rusqlite::Result<BTreeMap<_, _>>>()?)
        })
        .unwrap()
}

#[test]
fn incident_start_turns_it_on_and_writes_the_row() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();

    let out = incident::start(&mut f.store, Some(&mgr), "database migration in progress").unwrap();
    let out = fields(out);
    assert_eq!(out["changed"], json!(true));
    assert_eq!(out["incident"]["by"], json!("mgr"));
    assert_eq!(out["incident"]["reason"], json!("database migration in progress"));
    let started_at = out["incident"]["started_at"].as_str().expect("started_at is a string").to_owned();
    assert!(!started_at.is_empty(), "started_at is set");

    let row = incident_row(&mut f.store).expect("one incident row");
    assert_eq!((row.0.as_str(), row.1.as_str(), row.2.as_str()), (started_at.as_str(), "mgr", "database migration in progress"));
    assert!(row.3 >= 1, "started_txn is the inserting transaction");

    assert_eq!(
        events(&mut f.store),
        vec![(
            "incident-started".to_owned(),
            json!({"by": "mgr", "reason": "database migration in progress", "started_at": started_at}),
        )],
        "one incident-started Event with by, reason and started_at"
    );

    // Idempotent: a second start while on writes nothing and reports `changed` false.
    let out = fields(incident::start(&mut f.store, Some(&mgr), "a different reason").unwrap());
    assert_eq!(out["changed"], json!(false));
    assert_eq!(out["incident"]["started_at"], json!(started_at));
    assert_eq!(out["incident"]["by"], json!("mgr"));
    assert_eq!(out["incident"]["reason"], json!("database migration in progress"), "the first reason stays");
    assert_eq!(events(&mut f.store).len(), 1, "no second incident-started Event");
}

#[test]
fn incident_start_refuses_a_non_manager() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let lead = SessionId::parse("lead1").unwrap();
    let worker = SessionId::parse("w1").unwrap();
    let archived = SessionId::parse("mgr-old").unwrap();
    let unknown = SessionId::parse("nope").unwrap();

    for caller in [None, Some(&lead), Some(&worker), Some(&archived), Some(&unknown)] {
        let err = incident::start(&mut f.store, caller, "why").expect_err("only a live Manager may start incident mode");
        assert_eq!(err.code(), ErrorCode::ManagerOnly, "caller {caller:?}");
        assert_eq!(err.exit().code(), 3, "manager_only is exit 3");
    }
    assert!(incident_row(&mut f.store).is_none(), "a refused start writes no row");
    assert!(events(&mut f.store).is_empty(), "a refused start appends no Event");

    // A live Manager is allowed.
    assert!(incident::start(&mut f.store, Some(&mgr), "why").is_ok());
}

#[test]
fn incident_end_clears_it_and_is_idempotent() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let lead = SessionId::parse("lead1").unwrap();

    // Incident mode on: the row `incident start` writes.
    f.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn())",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    // Only a live Manager may end it.
    let err = incident::end(&mut f.store, Some(&lead)).expect_err("a Lead may not end incident mode");
    assert_eq!(err.code(), ErrorCode::ManagerOnly);
    assert!(incident_row(&mut f.store).is_some(), "a refused end deletes nothing");

    let out = fields(incident::end(&mut f.store, Some(&mgr)).unwrap());
    assert_eq!(out["changed"], json!(true));
    assert_eq!(out["incident"], json!(null));
    assert!(incident_row(&mut f.store).is_none(), "the row is deleted");

    let ended = events(&mut f.store);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0, "incident-ended");
    assert_eq!(ended[0].1["by"], json!("mgr"));
    let ended_at = ended[0].1["ended_at"].as_str().expect("ended_at is a string");
    assert!(!ended_at.is_empty(), "ended_at is set");

    // Idempotent: ending an off incident writes nothing and reports `changed` false.
    let out = fields(incident::end(&mut f.store, Some(&mgr)).unwrap());
    assert_eq!(out["changed"], json!(false));
    assert_eq!(out["incident"], json!(null));
    assert_eq!(events(&mut f.store).len(), 1, "no second incident-ended Event");
}

#[test]
fn incident_start_requests_stop_on_running_and_leaves_merging() {
    let mut f = fixture_with_attempts();
    let mgr = SessionId::parse("mgr").unwrap();

    incident::start(&mut f.store, Some(&mgr), "why").unwrap();

    let stops = stop_requests(&mut f.store);
    assert_eq!(stops.get("a-a1").and_then(|v| v.as_deref()), Some("cancelled"), "a running Attempt is asked to stop");
    assert_eq!(stops.get("b-a1").and_then(|v| v.as_deref()), Some("cancelled"), "a starting Attempt is asked to stop");
    assert_eq!(stops.get("c-a1").and_then(|v| v.as_deref()), None, "a merging Attempt is untouched");
    assert_eq!(stops.get("d-a1").and_then(|v| v.as_deref()), Some("timed_out"), "an existing stop_request stays");
    assert_eq!(stops.get("e-a1").and_then(|v| v.as_deref()), Some("cancelled"), "an existing cancelled stays");
}

#[test]
fn incident_status_reports_on_and_off() {
    let mut f = fixture();

    let off = fields(incident::status(&mut f.store).unwrap());
    assert_eq!(off["on"], json!(false));
    assert_eq!(off["incident"], json!(null));

    f.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, '2026-10-14T21:10:00.000Z', 'mgr', 'database migration in progress', cpd_txn())",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    let on = fields(incident::status(&mut f.store).unwrap());
    assert_eq!(on["on"], json!(true));
    assert_eq!(on["incident"]["started_at"], json!("2026-10-14T21:10:00.000Z"));
    assert_eq!(on["incident"]["by"], json!("mgr"));
    assert_eq!(on["incident"]["reason"], json!("database migration in progress"));
    assert!(events(&mut f.store).is_empty(), "status writes nothing");
}

#[test]
fn incident_started_and_ended_events_are_known_kinds() {
    let started: Kind = "incident-started".parse().expect("incident-started is a known kind");
    let ended: Kind = "incident-ended".parse().expect("incident-ended is a known kind");
    for (kind, required) in [
        (started, ["by", "reason", "started_at"].as_slice()),
        (ended, ["by", "ended_at"].as_slice()),
    ] {
        let spec = kind.spec();
        assert_eq!(
            (spec.context, spec.plan, spec.job, spec.attempt, spec.session),
            (Set::No, Set::No, Set::No, Set::No, Set::No),
            "{kind} is about no scope column"
        );
        assert_eq!(spec.required, required, "{kind} requires its data fields");
    }
    assert_eq!(PROTOCOL, 14, "hs-d634 raised the protocol for the two incident kinds");

    let started_event =
        NewEvent::incident_started("Incident mode on.", "mgr", "database migration in progress", "2026-10-14T21:10:00.000Z");
    assert_eq!(started_event.kind(), Kind::IncidentStarted);
    assert_eq!(
        started_event.data(),
        &json!({"by": "mgr", "reason": "database migration in progress", "started_at": "2026-10-14T21:10:00.000Z"})
    );

    let ended_event = NewEvent::incident_ended("Incident mode off.", "mgr", "2026-10-14T22:00:00.000Z");
    assert_eq!(ended_event.kind(), Kind::IncidentEnded);
    assert_eq!(ended_event.data(), &json!({"by": "mgr", "ended_at": "2026-10-14T22:00:00.000Z"}));
}

/// `incident::elapsed` (hs-d640, contract §6.1): the coarse human duration from the row's
/// `started_at` to now — under 60 s `{n}s`; under 1 h `{n}m`; under 1 d `{n}h {n}m`; otherwise
/// `{n}d {n}h`; values truncated, a `started_at` in the future reads `0s`.
#[test]
fn elapsed_is_a_coarse_human_duration() {
    use time::Duration;
    use time::macros::datetime;

    let started = "2000-01-01T00:00:00.000Z";
    let base = datetime!(2000-01-01 00:00:00 UTC);
    let cases = [
        (Duration::ZERO, "0s"),
        (Duration::seconds(42), "42s"),
        (Duration::seconds(59), "59s"),
        (Duration::minutes(1), "1m"),
        (Duration::minutes(5), "5m"),
        (Duration::minutes(59), "59m"),
        (Duration::hours(1), "1h 0m"),
        (Duration::hours(1) + Duration::minutes(23), "1h 23m"),
        (Duration::days(1), "1d 0h"),
        (Duration::days(2) + Duration::hours(4), "2d 4h"),
        (Duration::seconds(-1), "0s"),
    ];
    for (i, (ago, want)) in cases.into_iter().enumerate() {
        assert_eq!(incident::elapsed(started, base + ago).unwrap(), want, "case {i}");
    }
}
