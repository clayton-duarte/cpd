//! Red tests for the incident hold guard on `cpd pr open`/`push`/`rebase` (contract §7.6,
//! commands.md §9.1/§9.3/§9.9, hs-d643): an agent is refused `incident_hold` for a held Plan.
//! Job inc-v2-guard implements `hold_refused` in `core/src/pr/open.rs` and makes these pass.
//!
//! The three commands share one guard, so the tests call `hold_refused` directly, as the Spec
//! says: the call sites only have to pass `agent` = `CPD_SESSION` set and non-empty, which no
//! test here sets (the tests never touch `std::env`).

use serde_json::json;

use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::{PlanId, WorkflowKey};
use crate::store::Store;

use super::open::hold_refused;

/// A store with a live incident over the Plan `pay` (the incident Plan) and the Plan `ops`
/// outside it (held); each has one Workflow (`api`, `web`).
struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('ops', 'ctx', 'Ops', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pay', 'api', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('ops', 'web', '/r', 'main', 'waiting', 'cpd/ops', cpd_txn(), 't');
                 INSERT INTO incident (id, started_at, started_by, reason, started_txn)
                   VALUES (1, 't', 'mgr', 'why', cpd_txn());
                 INSERT INTO incident_plans (plan, started_txn) VALUES ('pay', cpd_txn());",
            )?;
            Ok(())
        })
        .unwrap();
    Fixture { _dir: dir, store }
}

/// The incident hold refusal for `plan`/`pr` from an agent, as its `CpdError`.
fn refused(f: &mut Fixture, plan: &PlanId, workflow: &WorkflowKey) -> crate::error::CpdError {
    f.store
        .read(|c| hold_refused(c, plan, workflow, true))
        .expect_err("a held Plan is refused for an agent")
}

#[test]
fn pr_open_refuses_a_held_plan_for_an_agent() {
    let mut f = fixture();
    let ops = PlanId::parse("ops").unwrap();
    let web = WorkflowKey::parse("web").unwrap();

    let err = refused(&mut f, &ops, &web);
    assert_eq!(
        err.code(),
        ErrorCode::IncidentHold,
        "the code is incident_hold"
    );
    assert_eq!(err.exit().code(), 3, "incident_hold is exit 3");
    assert_eq!(err.details()["plan"], json!("ops"), "details.plan names the held Plan");
    assert_eq!(err.details()["pr"], json!("web"), "details.pr names the Workflow the caller resolved");
    assert_eq!(err.details()["incident"]["reason"], json!("why"), "details.incident is the incident");
}

#[test]
fn pr_push_refuses_a_held_plan_for_an_agent() {
    let mut f = fixture();
    let ops = PlanId::parse("ops").unwrap();
    let web = WorkflowKey::parse("web").unwrap();

    // The agent is refused; a caller that is not an agent is not (the User's own push).
    let err = refused(&mut f, &ops, &web);
    assert_eq!(
        err.code(),
        ErrorCode::IncidentHold,
        "the code is incident_hold"
    );
    assert_eq!(err.exit().code(), 3, "incident_hold is exit 3");
    f.store
        .read(|c| hold_refused(c, &ops, &web, false))
        .expect("a caller that is not an agent is not held");
}

#[test]
fn pr_rebase_refuses_a_held_plan_for_an_agent() {
    let mut f = fixture();
    let ops = PlanId::parse("ops").unwrap();
    let web = WorkflowKey::parse("web").unwrap();

    let err = refused(&mut f, &ops, &web);
    assert_eq!(
        err.code(),
        ErrorCode::IncidentHold,
        "the code is incident_hold"
    );
    assert_eq!(err.exit().code(), 3, "incident_hold is exit 3");
}

#[test]
fn pr_open_allows_an_incident_plan() {
    let mut f = fixture();
    let pay = PlanId::parse("pay").unwrap();
    let api = WorkflowKey::parse("api").unwrap();

    // A Plan in the incident is not held, even for an agent.
    f.store
        .read(|c| hold_refused(c, &pay, &api, true))
        .expect("a Plan in the incident is allowed");

    // With the incident off, every Plan is allowed.
    f.store
        .write("incident end", |tx| {
            tx.execute("DELETE FROM incident", [])?;
            tx.execute("DELETE FROM incident_plans", [])?;
            Ok(())
        })
        .unwrap();
    let ops = PlanId::parse("ops").unwrap();
    let web = WorkflowKey::parse("web").unwrap();
    f.store
        .read(|c| hold_refused(c, &ops, &web, true))
        .expect("with the incident off no Plan is held");
}
