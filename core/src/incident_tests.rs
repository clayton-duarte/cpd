//! Red tests for incident mode v2 (contract §2.2 "Incident", §3.7, §5.2; commands.md §25,
//! hs-d643): the Manager-only `cpd incident start`/`end`, the readable `status`, the Plan-scoped
//! `incident`/`incident_plans` state, `is_held`, `--until-workflow` and the auto-end on the until-Workflow
//! merge. The inc-v2-impl Job implements the stubs in `incident.rs`, `events/typed.rs` and
//! `events/kind.rs` and makes these pass.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use crate::error::ErrorCode;
use crate::events::{Kind, NewEvent, PROTOCOL, Set};
use crate::home::CpdHome;
use crate::ids::{PlanId, SessionId};
use crate::incident;
use crate::output::Output;
use crate::store::Store;

/// A temporary store with a live Manager `mgr`, the Leads `lead1` (`pay`) and `lead2` (`ops`), a
/// Worker `w1` and an archived Manager `mgr-old`; the Plans `pay` (its Workflows `api` and `ship`) and
/// `ops` (its Workflow `web`); the Job `a` of `pay` and its running Attempt `a-a1`.
///
/// `ship` is `open` with a `pull_requests` row, so a test can record its merge with one update;
/// `api` and `web` are `waiting`.
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
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead2', 'lead', 'ctx', 'ops', 'Lead 2', '/', 'O3', 'working', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                   VALUES ('w1', 'worker', 'ctx', 'pay', 'a-a1', 'Worker', '/', 'O3', 'working', 't', 't');
                 INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, archived_at)
                   VALUES ('mgr-old', 'manager', 'ctx', 'Old', '/', 'O3', 'stopped', 't', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('ops', 'ctx', 'Ops', 'dispatch', 'lead2', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pay', 'api', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('ops', 'web', '/r', 'main', 'waiting', 'cpd/ops', cpd_txn(), 't');
                 INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, mergeable, merge_state, authored, review_requested)
                   VALUES (7, 'o/r', 7, 'https://example/7', 'Ship', 0, 'open', 'none', 'none', 't', NULL, 't', 'unknown', 'unknown', 0, 0);
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at)
                   VALUES ('pay', 'ship', '/r', 'main', 'open', 'cpd/pay/ship', '/w', 7, 'https://example/7', 't', 7, cpd_txn(), 't', 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
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
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
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

/// The one `incident` row's `lead` column, if any (raw SQL: the column the migration adds).
fn incident_lead(store: &mut Store) -> Option<String> {
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT lead FROM incident WHERE id = 1", [], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .optional()?,
            )
        })
        .unwrap()
        .flatten()
}

/// The one `incident` row's `(until_plan, until_pr)`, if any.
fn incident_until(store: &mut Store) -> Option<(Option<String>, Option<String>)> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT until_plan, until_workflow FROM incident WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })
        .unwrap()
}

/// Every `incident_plans` row as `plan -> started_txn`.
fn incident_plans(store: &mut Store) -> BTreeMap<String, i64> {
    store
        .read(|c| {
            let mut stmt = c.prepare("SELECT plan, started_txn FROM incident_plans")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .collect::<rusqlite::Result<BTreeMap<_, _>>>()?)
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
                    Ok((
                        r.get::<_, String>(0)?,
                        serde_json::from_str(&data).expect("data is JSON"),
                    ))
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
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
                })?
                .collect::<rusqlite::Result<BTreeMap<_, _>>>()?)
        })
        .unwrap()
}

/// The Plan ids of a `plans` array, sorted: the tests never pin the order the set is reported in.
fn plan_array(value: &Value) -> Vec<String> {
    let mut plans: Vec<String> = value
        .as_array()
        .expect("plans is an array")
        .iter()
        .map(|p| p.as_str().expect("a Plan id").to_owned())
        .collect();
    plans.sort();
    plans
}

/// `incident::is_held` in a read transaction.
fn held(store: &mut Store, plan: &PlanId) -> bool {
    store.read(|c| Ok(incident::is_held(c, plan))).unwrap()
}

/// `incident::plans` in a read transaction, sorted.
fn plan_set(store: &mut Store) -> Vec<String> {
    let mut plans = store
        .read(|c| {
            Ok(incident::plans(c)?
                .into_iter()
                .map(|p| p.as_str().to_owned())
                .collect::<Vec<_>>())
        })
        .unwrap();
    plans.sort();
    plans
}

#[test]
fn incident_start_requires_at_least_one_plan() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();

    let err =
        incident::start(&mut f.store, Some(&mgr), &[], "why", None).expect_err("no Plan is usage");
    assert_eq!(err.code(), ErrorCode::Usage, "no --plan is usage");
    assert_eq!(err.exit().code(), 2, "usage is exit 2");
    assert!(
        incident_row(&mut f.store).is_none(),
        "a refused start writes no row"
    );
    assert!(
        events(&mut f.store).is_empty(),
        "a refused start appends no Event"
    );

    // One Plan is enough.
    let out = fields(incident::start(&mut f.store, Some(&mgr), &[pay], "why", None).unwrap());
    assert_eq!(out["changed"], json!(true));
}

#[test]
fn incident_start_records_the_plan_set_and_status_reports_it() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    // A well-formed Plan that does not exist is plan_not_found (exit 4), and writes nothing.
    let missing = PlanId::parse("gone").unwrap();
    let err = incident::start(
        &mut f.store,
        Some(&mgr),
        &[pay.clone(), missing],
        "why",
        None,
    )
    .expect_err("an unknown Plan is refused");
    assert_eq!(err.code(), ErrorCode::PlanNotFound);
    assert_eq!(err.exit().code(), 4);
    assert!(
        incident_row(&mut f.store).is_none(),
        "a refused start writes no row"
    );
    assert!(
        incident_plans(&mut f.store).is_empty(),
        "a refused start writes no incident_plans row"
    );

    let out = fields(
        incident::start(
            &mut f.store,
            Some(&mgr),
            &[pay.clone(), ops.clone()],
            "why",
            None,
        )
        .unwrap(),
    );
    assert_eq!(out["changed"], json!(true));
    assert_eq!(
        plan_array(&out["incident"]["plans"]),
        ["ops", "pay"],
        "the named Plans are reported"
    );
    assert_eq!(
        out["incident"]["until_pr"],
        json!(null),
        "no until_pr by default"
    );
    let started_at = out["incident"]["started_at"]
        .as_str()
        .expect("started_at is a string")
        .to_owned();

    let (_, by, reason, started_txn) = incident_row(&mut f.store).expect("one incident row");
    assert_eq!((by.as_str(), reason.as_str()), ("mgr", "why"));
    let set = incident_plans(&mut f.store);
    assert_eq!(
        set.keys().cloned().collect::<Vec<_>>(),
        ["ops", "pay"],
        "one incident_plans row per named Plan"
    );
    assert!(
        set.values().all(|txn| *txn == started_txn),
        "each incident_plans row names the start transaction ({started_txn})"
    );

    let started = events(&mut f.store)
        .into_iter()
        .find(|(kind, _)| kind == "incident-started")
        .expect("one incident-started Event");
    assert_eq!(
        plan_array(&started.1["plans"]),
        ["ops", "pay"],
        "incident-started data carries plans"
    );

    let on = fields(incident::status(&mut f.store).unwrap());
    assert_eq!(on["on"], json!(true));
    assert_eq!(
        plan_array(&on["incident"]["plans"]),
        ["ops", "pay"],
        "status reports the Plan set"
    );
    assert_eq!(on["incident"]["until_pr"], json!(null));
    assert_eq!(on["incident"]["started_at"], json!(started_at));
    assert_eq!(on["incident"]["by"], json!("mgr"));
}

#[test]
fn incident_is_held_for_plans_outside_the_set_only() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    // Off: no Plan is held and the set is empty.
    assert!(!held(&mut f.store, &pay), "off holds nothing");
    assert!(!held(&mut f.store, &ops), "off holds nothing");
    assert!(plan_set(&mut f.store).is_empty());

    incident::start(
        &mut f.store,
        Some(&mgr),
        std::slice::from_ref(&pay),
        "why",
        None,
    )
    .unwrap();

    assert!(
        !held(&mut f.store, &pay),
        "a Plan in the incident is not held"
    );
    assert!(
        held(&mut f.store, &ops),
        "a Plan outside the incident is held"
    );
    assert_eq!(
        plan_set(&mut f.store),
        ["pay"],
        "incident::plans returns the set"
    );
}

#[test]
fn incident_start_records_until_pr_and_refuses_a_foreign_plan() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();

    // A Workflow of a named Plan is stored and reported.
    let out = fields(
        incident::start(
            &mut f.store,
            Some(&mgr),
            &[pay.clone()],
            "why",
            Some("pay/ship"),
        )
        .unwrap(),
    );
    assert_eq!(out["incident"]["until_pr"], json!("pay/ship"));
    assert_eq!(
        incident_until(&mut f.store),
        Some((Some("pay".to_owned()), Some("pay/ship".to_owned()))),
        "until_plan and until_pr are stored"
    );
    let on = fields(incident::status(&mut f.store).unwrap());
    assert_eq!(
        on["incident"]["until_pr"],
        json!("pay/ship"),
        "status reports until_pr"
    );

    incident::end(&mut f.store, Some(&mgr)).unwrap();
    assert!(incident_row(&mut f.store).is_none());

    // A Workflow of a Plan not named is until_pr_not_incident (exit 3).
    let err = incident::start(
        &mut f.store,
        Some(&mgr),
        &[pay.clone()],
        "why",
        Some("ops/web"),
    )
    .expect_err("a foreign Plan's Workflow is refused");
    assert_eq!(err.code(), ErrorCode::UntilWorkflowNotIncident);
    assert_eq!(err.exit().code(), 3);
    assert!(
        incident_row(&mut f.store).is_none(),
        "a refused start writes no row"
    );

    // A Workflow of a named Plan that does not exist is pr_not_found (exit 4).
    let err = incident::start(
        &mut f.store,
        Some(&mgr),
        &[pay.clone()],
        "why",
        Some("pay/nope"),
    )
    .expect_err("an unknown Workflow is refused");
    assert_eq!(err.code(), ErrorCode::WorkflowNotFound);
    assert_eq!(err.exit().code(), 4);

    // A value that is not `<plan>/<key>` is usage (exit 2).
    for bad in ["nope", "pay/", "/ship", "pay/ship/x"] {
        let err = incident::start(&mut f.store, Some(&mgr), &[pay.clone()], "why", Some(bad))
            .expect_err("a malformed until-workflow is refused");
        assert_eq!(err.code(), ErrorCode::Usage, "{bad:?}");
        assert_eq!(err.exit().code(), 2, "{bad:?}");
    }
    assert!(
        incident_row(&mut f.store).is_none(),
        "every refusal writes no row"
    );
}

#[test]
fn incident_ends_when_the_until_pr_merges() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();

    incident::start(
        &mut f.store,
        Some(&mgr),
        std::slice::from_ref(&pay),
        "why",
        Some("pay/ship"),
    )
    .unwrap();

    // The until-Workflow is not merged: the auto-end does nothing.
    let merged = f
        .store
        .write("test", |tx| incident::end_if_until_workflow_merged(tx))
        .unwrap();
    assert!(
        !merged,
        "an unmerged until-Workflow does not end the incident"
    );
    assert!(
        incident_row(&mut f.store).is_some(),
        "the incident stays on"
    );
    assert_eq!(plan_set(&mut f.store), ["pay"], "the Plan set stays");

    // The merge is recorded (as E5's poll does, in the same transaction): the incident ends.
    let merged = f.store
        .write("test", |tx| {
            tx.execute("UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE id = 7", [])?;
            tx.execute("UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'pay' AND key = 'ship'", [])?;
            incident::end_if_until_workflow_merged(tx)
        })
        .unwrap();
    assert!(merged, "a merged until-Workflow ends the incident");
    assert!(
        incident_row(&mut f.store).is_none(),
        "the incident row is deleted"
    );
    assert!(
        incident_plans(&mut f.store).is_empty(),
        "every incident_plans row is deleted"
    );

    let ended = events(&mut f.store)
        .into_iter()
        .find(|(kind, _)| kind == "incident-ended")
        .expect("one incident-ended Event");
    assert_eq!(
        ended.1["cause"],
        json!("until-workflow"),
        "the auto-end names its cause"
    );
    assert_eq!(ended.1["by"], json!("engine"), "the engine ends it");

    // Nothing left to end: a second call is false and appends nothing.
    let before = events(&mut f.store).len();
    let again = f
        .store
        .write("test", |tx| incident::end_if_until_workflow_merged(tx))
        .unwrap();
    assert!(!again);
    assert_eq!(
        events(&mut f.store).len(),
        before,
        "no second incident-ended Event"
    );
}

#[test]
fn incident_start_turns_it_on_and_writes_the_row() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();

    let out = incident::start(
        &mut f.store,
        Some(&mgr),
        std::slice::from_ref(&pay),
        "database migration in progress",
        None,
    )
    .unwrap();
    let out = fields(out);
    assert_eq!(out["changed"], json!(true));
    assert_eq!(out["incident"]["by"], json!("mgr"));
    assert_eq!(
        out["incident"]["reason"],
        json!("database migration in progress")
    );
    assert_eq!(plan_array(&out["incident"]["plans"]), ["pay"]);
    let started_at = out["incident"]["started_at"]
        .as_str()
        .expect("started_at is a string")
        .to_owned();
    assert!(!started_at.is_empty(), "started_at is set");

    let row = incident_row(&mut f.store).expect("one incident row");
    assert_eq!(
        (row.0.as_str(), row.1.as_str(), row.2.as_str()),
        (started_at.as_str(), "mgr", "database migration in progress")
    );
    assert!(row.3 >= 1, "started_txn is the inserting transaction");

    assert_eq!(
        events(&mut f.store),
        vec![(
            "incident-started".to_owned(),
            json!({"by": "mgr", "reason": "database migration in progress", "started_at": started_at, "plans": ["pay"]}),
        )],
        "one incident-started Event with by, reason, started_at and plans"
    );

    // Idempotent: a second start while on writes nothing and reports `changed` false.
    let out = fields(
        incident::start(&mut f.store, Some(&mgr), &[pay], "a different reason", None).unwrap(),
    );
    assert_eq!(out["changed"], json!(false));
    assert_eq!(out["incident"]["started_at"], json!(started_at));
    assert_eq!(out["incident"]["by"], json!("mgr"));
    assert_eq!(
        out["incident"]["reason"],
        json!("database migration in progress"),
        "the first reason stays"
    );
    assert_eq!(
        events(&mut f.store).len(),
        1,
        "no second incident-started Event"
    );
}

#[test]
fn incident_start_refuses_a_non_manager() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let lead = SessionId::parse("lead1").unwrap();
    let worker = SessionId::parse("w1").unwrap();
    let archived = SessionId::parse("mgr-old").unwrap();
    let unknown = SessionId::parse("nope").unwrap();
    let pay = PlanId::parse("pay").unwrap();

    for caller in [
        None,
        Some(&lead),
        Some(&worker),
        Some(&archived),
        Some(&unknown),
    ] {
        let err = incident::start(
            &mut f.store,
            caller,
            std::slice::from_ref(&pay),
            "why",
            None,
        )
        .expect_err("only a live Manager may start incident mode");
        assert_eq!(err.code(), ErrorCode::ManagerOnly, "caller {caller:?}");
        assert_eq!(err.exit().code(), 3, "manager_only is exit 3");
    }
    assert!(
        incident_row(&mut f.store).is_none(),
        "a refused start writes no row"
    );
    assert!(
        events(&mut f.store).is_empty(),
        "a refused start appends no Event"
    );

    // A live Manager is allowed.
    assert!(incident::start(&mut f.store, Some(&mgr), &[pay], "why", None).is_ok());
}

#[test]
fn incident_end_clears_it_and_is_idempotent() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let lead = SessionId::parse("lead1").unwrap();

    // Incident mode on: the rows `incident start` writes.
    f.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn())",
                [],
            )?;
            tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn())", [])?;
            Ok(())
        })
        .unwrap();

    // Only a live Manager may end it.
    let err =
        incident::end(&mut f.store, Some(&lead)).expect_err("a Lead may not end incident mode");
    assert_eq!(err.code(), ErrorCode::ManagerOnly);
    assert!(
        incident_row(&mut f.store).is_some(),
        "a refused end deletes nothing"
    );

    let out = fields(incident::end(&mut f.store, Some(&mgr)).unwrap());
    assert_eq!(out["changed"], json!(true));
    assert_eq!(out["incident"], json!(null));
    assert!(incident_row(&mut f.store).is_none(), "the row is deleted");
    assert!(
        incident_plans(&mut f.store).is_empty(),
        "every incident_plans row is deleted"
    );

    let ended = events(&mut f.store);
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].0, "incident-ended");
    assert_eq!(ended[0].1["by"], json!("mgr"));
    assert_eq!(
        ended[0].1["cause"],
        json!("manual"),
        "a commanded end names its cause"
    );
    let ended_at = ended[0].1["ended_at"]
        .as_str()
        .expect("ended_at is a string");
    assert!(!ended_at.is_empty(), "ended_at is set");

    // Idempotent: ending an off incident writes nothing and reports `changed` false.
    let out = fields(incident::end(&mut f.store, Some(&mgr)).unwrap());
    assert_eq!(out["changed"], json!(false));
    assert_eq!(out["incident"], json!(null));
    assert_eq!(
        events(&mut f.store).len(),
        1,
        "no second incident-ended Event"
    );
}

#[test]
fn incident_start_requests_stop_on_running_and_leaves_merging() {
    let mut f = fixture_with_attempts();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();

    incident::start(&mut f.store, Some(&mgr), &[pay], "why", None).unwrap();

    let stops = stop_requests(&mut f.store);
    assert_eq!(
        stops.get("a-a1").and_then(|v| v.as_deref()),
        Some("cancelled"),
        "a running Attempt is asked to stop"
    );
    assert_eq!(
        stops.get("b-a1").and_then(|v| v.as_deref()),
        Some("cancelled"),
        "a starting Attempt is asked to stop"
    );
    assert_eq!(
        stops.get("c-a1").and_then(|v| v.as_deref()),
        None,
        "a merging Attempt is untouched"
    );
    assert_eq!(
        stops.get("d-a1").and_then(|v| v.as_deref()),
        Some("timed_out"),
        "an existing stop_request stays"
    );
    assert_eq!(
        stops.get("e-a1").and_then(|v| v.as_deref()),
        Some("cancelled"),
        "an existing cancelled stays"
    );
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
            tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn())", [])?;
            Ok(())
        })
        .unwrap();

    let on = fields(incident::status(&mut f.store).unwrap());
    assert_eq!(on["on"], json!(true));
    assert_eq!(
        on["incident"]["started_at"],
        json!("2026-10-14T21:10:00.000Z")
    );
    assert_eq!(on["incident"]["by"], json!("mgr"));
    assert_eq!(
        on["incident"]["reason"],
        json!("database migration in progress")
    );
    assert_eq!(plan_array(&on["incident"]["plans"]), ["pay"]);
    assert!(events(&mut f.store).is_empty(), "status writes nothing");
}

#[test]
fn incident_started_and_ended_events_are_known_kinds() {
    let started: Kind = "incident-started"
        .parse()
        .expect("incident-started is a known kind");
    let ended: Kind = "incident-ended"
        .parse()
        .expect("incident-ended is a known kind");
    for (kind, required, optional) in [
        (
            started,
            ["by", "reason", "started_at", "plans"].as_slice(),
            ["until_pr"].as_slice(),
        ),
        (ended, ["by", "ended_at", "cause"].as_slice(), [].as_slice()),
    ] {
        let spec = kind.spec();
        assert_eq!(
            (
                spec.context,
                spec.plan,
                spec.job,
                spec.attempt,
                spec.session
            ),
            (Set::No, Set::No, Set::No, Set::No, Set::No),
            "{kind} is about no scope column"
        );
        assert_eq!(spec.required, required, "{kind} requires its data fields");
        assert_eq!(
            spec.optional, optional,
            "{kind} may carry its optional fields"
        );
    }
    let protocol = PROTOCOL;
    assert!(
        protocol >= 14,
        "hs-d634 raised the protocol to 14 for the two incident kinds, and it only goes up"
    );

    let started_event = NewEvent::incident_started(
        "Incident mode on.",
        "mgr",
        "database migration in progress",
        "2026-10-14T21:10:00.000Z",
        ["pay"],
        None,
    );
    assert_eq!(started_event.kind(), Kind::IncidentStarted);
    assert_eq!(
        started_event.data(),
        &json!({"by": "mgr", "reason": "database migration in progress", "started_at": "2026-10-14T21:10:00.000Z", "plans": ["pay"]})
    );

    let until = NewEvent::incident_started(
        "Incident mode on.",
        "mgr",
        "fixing Workflow ci",
        "2026-10-14T21:10:00.000Z",
        ["pay"],
        Some("pay/ship"),
    );
    assert_eq!(
        until.data(),
        &json!({"by": "mgr", "reason": "fixing Workflow ci", "started_at": "2026-10-14T21:10:00.000Z", "plans": ["pay"], "until_pr": "pay/ship"})
    );

    let ended_event = NewEvent::incident_ended(
        "Incident mode off.",
        "mgr",
        "2026-10-14T22:00:00.000Z",
        "manual",
    );
    assert_eq!(ended_event.kind(), Kind::IncidentEnded);
    assert_eq!(
        ended_event.data(),
        &json!({"by": "mgr", "ended_at": "2026-10-14T22:00:00.000Z", "cause": "manual"})
    );
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
        assert_eq!(
            incident::elapsed(started, base + ago).unwrap(),
            want,
            "case {i}"
        );
    }
}

/// Assumption A1 (hs-d664): a start on a live incident replaces `incident_plans` with exactly the
/// named Plans, so one Plan may leave while another joins.
#[test]
fn incident_start_on_a_live_incident_replaces_the_plan_set() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    incident::start(
        &mut f.store,
        Some(&mgr),
        std::slice::from_ref(&pay),
        "why",
        None,
    )
    .unwrap();
    assert_eq!(plan_set(&mut f.store), ["pay"], "the fresh start names pay");

    // `ops` joins and `pay` leaves: the named list replaces the set exactly.
    let out = fields(
        incident::start(
            &mut f.store,
            Some(&mgr),
            std::slice::from_ref(&ops),
            "why",
            None,
        )
        .unwrap(),
    );
    assert_eq!(
        out["changed"],
        json!(true),
        "a changed set is not idempotent"
    );
    assert_eq!(
        plan_array(&out["incident"]["plans"]),
        ["ops"],
        "the output reports the new set only"
    );
    assert_eq!(
        plan_set(&mut f.store),
        ["ops"],
        "incident::plans returns exactly the named Plans"
    );
    assert_eq!(
        incident_plans(&mut f.store)
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        ["ops"],
        "one incident_plans row per named Plan, and pay's is gone"
    );
    assert!(!held(&mut f.store, &ops), "the named Plan is not held");
    assert!(
        held(&mut f.store, &pay),
        "the Plan that left the incident is held"
    );
}

/// Assumption A2 (hs-d664): an update with `--reason` omitted (the empty string) leaves the
/// stored `started_at`, `started_by` and `reason` alone.
#[test]
fn incident_start_on_a_live_incident_keeps_started_at_by_and_reason() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    let first = fields(
        incident::start(
            &mut f.store,
            Some(&mgr),
            &[pay],
            "database migration in progress",
            None,
        )
        .unwrap(),
    );
    let started_at = first["incident"]["started_at"]
        .as_str()
        .expect("started_at is a string")
        .to_owned();

    // An update names `ops` and gives no --reason ("" means omitted).
    let out = fields(incident::start(&mut f.store, Some(&mgr), &[ops], "", None).unwrap());
    assert_eq!(out["changed"], json!(true), "the set changed");
    assert_eq!(
        out["incident"]["started_at"],
        json!(started_at),
        "started_at stays"
    );
    assert_eq!(out["incident"]["by"], json!("mgr"), "started_by stays");
    assert_eq!(
        out["incident"]["reason"],
        json!("database migration in progress"),
        "the stored reason stays"
    );
    assert_eq!(plan_set(&mut f.store), ["ops"], "the set did change");

    let (row_at, by, reason, _) = incident_row(&mut f.store).expect("the live incident row");
    assert_eq!(
        (row_at.as_str(), by.as_str(), reason.as_str()),
        (started_at.as_str(), "mgr", "database migration in progress"),
        "the row keeps started_at, started_by and reason"
    );
}

/// Assumption A3 (hs-d664): an update that names the same set and the same `until_pr` reports
/// `changed:false` and appends no Event.
#[test]
fn incident_start_on_a_live_incident_is_idempotent() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();

    incident::start(
        &mut f.store,
        Some(&mgr),
        std::slice::from_ref(&pay),
        "why",
        None,
    )
    .unwrap();
    let before = events(&mut f.store).len();
    assert_eq!(
        before, 1,
        "the fresh start appends one incident-started Event"
    );

    // The same Plan, no --reason and no --until-workflow: nothing changed.
    let out = fields(incident::start(&mut f.store, Some(&mgr), &[pay.clone()], "", None).unwrap());
    assert_eq!(
        out["changed"],
        json!(false),
        "an unchanged update reports changed false"
    );
    assert_eq!(plan_array(&out["incident"]["plans"]), ["pay"]);
    assert_eq!(plan_set(&mut f.store), ["pay"]);
    assert_eq!(
        events(&mut f.store).len(),
        before,
        "an unchanged update appends no Event"
    );
}

/// Assumption A2 (hs-d664): a new `--until-workflow` replaces the stored one, and an absent
/// `--until-workflow` whose stored `until_plan` is not in the new set clears both columns.
#[test]
fn incident_start_on_a_live_incident_replaces_and_clears_until_pr() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    // Fresh start with `pay` and `ops` and `ops/web` as the auto-end Workflow.
    incident::start(
        &mut f.store,
        Some(&mgr),
        &[pay.clone(), ops.clone()],
        "why",
        Some("ops/web"),
    )
    .unwrap();
    assert_eq!(
        incident_until(&mut f.store),
        Some((Some("ops".to_owned()), Some("ops/web".to_owned()))),
        "the fresh start stores until_plan and until_pr"
    );

    // A new --until-workflow replaces the stored one.
    let out = fields(
        incident::start(
            &mut f.store,
            Some(&mgr),
            &[pay.clone(), ops.clone()],
            "why",
            Some("pay/api"),
        )
        .unwrap(),
    );
    assert_eq!(out["changed"], json!(true));
    assert_eq!(
        out["incident"]["until_pr"],
        json!("pay/api"),
        "the output reports the new until_pr"
    );
    assert_eq!(
        incident_until(&mut f.store),
        Some((Some("pay".to_owned()), Some("pay/api".to_owned()))),
        "a new until-workflow replaces both columns"
    );

    // `ops` leaves; with no --until-workflow its stored until_plan is not in the new set, so both clear.
    let out =
        fields(incident::start(&mut f.store, Some(&mgr), &[pay.clone()], "why", None).unwrap());
    assert_eq!(out["changed"], json!(true));
    assert_eq!(
        out["incident"]["until_pr"],
        json!(null),
        "the output reports no until_pr"
    );
    assert_eq!(
        incident_until(&mut f.store),
        Some((None, None)),
        "an absent until-workflow whose stored until_plan left clears both columns"
    );
    assert_eq!(plan_set(&mut f.store), ["pay"]);
}

/// Assumption A3 (hs-d664): a changed set appends one `incident-plans` Event whose `data` carries
/// `by` and the new `plans`.
#[test]
fn incident_start_on_a_live_incident_appends_an_incident_plans_event() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    incident::start(&mut f.store, Some(&mgr), &[pay], "why", None).unwrap();
    let before = events(&mut f.store);
    assert_eq!(
        before.len(),
        1,
        "the fresh start appends one incident-started Event"
    );
    assert_eq!(before[0].0, "incident-started");

    // The set changes: one incident-plans Event, after the incident-started one.
    let out = fields(incident::start(&mut f.store, Some(&mgr), &[ops], "why", None).unwrap());
    assert_eq!(out["changed"], json!(true));
    let after = events(&mut f.store);
    assert_eq!(
        after.len(),
        2,
        "exactly one incident-plans Event is appended"
    );
    let (kind, data) = after
        .into_iter()
        .find(|(kind, _)| kind == "incident-plans")
        .expect("one incident-plans Event");
    assert_eq!(kind, "incident-plans");
    assert_eq!(data["by"], json!("mgr"), "incident-plans data carries by");
    assert_eq!(
        plan_array(&data["plans"]),
        ["ops"],
        "incident-plans data carries the new plans"
    );
}

/// Assumption A3 (hs-d664): an update runs no stop-request sweep, so a running or merging Attempt
/// keeps the `stop_request` it had (a fresh start's sweep is not repeated).
#[test]
fn incident_start_on_a_live_incident_does_not_interrupt_running_attempts() {
    let mut f = fixture_with_attempts();
    let mgr = SessionId::parse("mgr").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    // A live incident set up directly, with every Attempt's stop_request as the fixture left it.
    f.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn())",
                [],
            )?;
            tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn())", [])?;
            Ok(())
        })
        .unwrap();
    let before = stop_requests(&mut f.store);

    // The update changes the set but runs no stop-request sweep.
    let out = fields(incident::start(&mut f.store, Some(&mgr), &[ops], "why", None).unwrap());
    assert_eq!(out["changed"], json!(true));
    assert_eq!(
        plan_set(&mut f.store),
        ["ops"],
        "the update replaced the set"
    );
    assert_eq!(
        stop_requests(&mut f.store),
        before,
        "an update requests no stop"
    );
}

/// Assumption A3 (hs-d664): a non-Manager is refused even when incident mode is already live, and
/// the refused update changes nothing.
#[test]
fn incident_start_on_a_live_incident_refuses_a_non_manager() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let lead = SessionId::parse("lead1").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    incident::start(
        &mut f.store,
        Some(&mgr),
        std::slice::from_ref(&pay),
        "why",
        None,
    )
    .unwrap();
    let before = events(&mut f.store).len();

    let err = incident::start(&mut f.store, Some(&lead), &[ops], "", None)
        .expect_err("only a live Manager may update incident mode");
    assert_eq!(err.code(), ErrorCode::ManagerOnly);
    assert_eq!(err.exit().code(), 3, "manager_only is exit 3");
    assert_eq!(
        plan_set(&mut f.store),
        ["pay"],
        "a refused update leaves the set"
    );
    assert_eq!(
        events(&mut f.store).len(),
        before,
        "a refused update appends no Event"
    );
}

/// D1 (hs-d678): a fresh start stores the `lead_session` of the first `--plan` argument's Plan in
/// `incident.lead`; a live start whose set is unchanged (only the argument order differs) leaves it.
#[test]
fn incident_start_stores_the_first_plans_lead() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    // The first named Plan is `pay` (`lead_session` `lead1`).
    incident::start(
        &mut f.store,
        Some(&mgr),
        &[pay.clone(), ops.clone()],
        "why",
        None,
    )
    .unwrap();
    assert_eq!(
        incident_lead(&mut f.store).as_deref(),
        Some("lead1"),
        "the first --plan's lead_session"
    );

    // The same set reordered is no set change: the stored lead stays.
    incident::start(&mut f.store, Some(&mgr), &[ops, pay], "why", None).unwrap();
    assert_eq!(
        incident_lead(&mut f.store).as_deref(),
        Some("lead1"),
        "a reordered set is unchanged"
    );
}

/// D1 (hs-d678): a live start whose set changes recomputes `lead` from the new first `--plan`
/// argument's Plan; a live start whose set is only reordered does not.
#[test]
fn incident_start_on_a_live_incident_recomputes_the_lead_when_the_set_changes() {
    let mut f = fixture();
    let mgr = SessionId::parse("mgr").unwrap();
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    // `pay` leaves, `ops` stays: the new first Plan is `ops` (`lead_session` `lead2`).
    incident::start(
        &mut f.store,
        Some(&mgr),
        &[pay.clone(), ops.clone()],
        "why",
        None,
    )
    .unwrap();
    assert_eq!(
        incident_lead(&mut f.store).as_deref(),
        Some("lead1"),
        "the fresh start's lead"
    );
    incident::start(
        &mut f.store,
        Some(&mgr),
        std::slice::from_ref(&ops),
        "why",
        None,
    )
    .unwrap();
    assert_eq!(
        incident_lead(&mut f.store).as_deref(),
        Some("lead2"),
        "the changed set recomputes the lead"
    );

    // `pay` joins again: the set changed, and its first Plan is `pay` (`lead1`).
    incident::start(
        &mut f.store,
        Some(&mgr),
        &[pay.clone(), ops.clone()],
        "why",
        None,
    )
    .unwrap();
    assert_eq!(
        incident_lead(&mut f.store).as_deref(),
        Some("lead1"),
        "the changed set recomputes the lead again"
    );

    // The same set reordered is not a change: the stored lead stays.
    incident::start(&mut f.store, Some(&mgr), &[ops, pay], "why", None).unwrap();
    assert_eq!(
        incident_lead(&mut f.store).as_deref(),
        Some("lead1"),
        "a reordered set is unchanged"
    );
}

/// A2 (hs-d678): a row written before the migration has a null `lead`; the click resolves the
/// alphabetically first Plan's `lead_session` from `incident_plans`. With no incident row, none.
#[test]
fn the_incident_lead_falls_back_to_the_alphabetically_first_plan() {
    let mut f = fixture();

    f.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn())",
                [],
            )?;
            tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn())", [])?;
            tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES ('ops', cpd_txn())", [])?;
            Ok(())
        })
        .unwrap();

    // `ops` sorts before `pay`, so its `lead_session` `lead2` is the fallback (`pay`'s is `lead1`).
    let lead = f.store.read(incident::lead).unwrap();
    assert_eq!(
        lead.map(|s| s.as_str().to_owned()).as_deref(),
        Some("lead2"),
        "the alphabetically first Plan's Lead"
    );

    // With no incident row there is no Lead to resolve.
    f.store
        .write("fixture", |tx| {
            tx.execute("DELETE FROM incident_plans", [])?;
            tx.execute("DELETE FROM incident WHERE id = 1", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        f.store.read(incident::lead).unwrap(),
        None,
        "no incident row, no Lead"
    );
}
