//! Red tests for the Plan-scoped engine hold (contract §3.7 E3/E4/E7, Job inc-v2-engine, hs-d643):
//! while one `incident` row exists, E3 starts only a Job of a Plan in `incident_plans` or an
//! explicitly urgent Job; E4 and E7 act only on the PRs of Plans in `incident_plans`; E9, E10 and
//! E11 still hold machine-wide.
//!
//! The inc-v2-engine Job implements the two stubs these tests call
//! ([`schedule::incident_starts`] and [`engine::incident_holds_plan`]) and wires them into the steps.

use std::os::unix::fs::PermissionsExt;

use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::ids::{PlanId, PrKey};

use super::engine::{self, Engine};
use super::report::Step;
use super::{open, schedule};

/// A test engine on a temporary home, with a tmux program that exits at once.
fn engine_fixture() -> (Engine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("config.toml"),
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n\n[tmux]\nsocket = \"incident-tests\"\n",
    )
    .unwrap();
    let program = dir.path().join("tmux");
    std::fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let home = CpdHome::resolve(Some(root.into()), None).unwrap();
    (Engine::for_test(home, Tmux::new("incident-tests").with_program(&program)), dir)
}

/// One `incident` row: incident mode is on.
fn incident_on(engine: &mut Engine) {
    engine
        .store
        .write("test", |tx| {
            tx.execute(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn())",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

/// Puts Plan `plan` in the incident (`incident_plans`), so the engine does not hold it.
fn incident_includes(engine: &mut Engine, plan: &str) {
    let plan = plan.to_owned();
    engine
        .store
        .write("test", move |tx| {
            tx.execute("INSERT INTO incident_plans (plan, started_txn) VALUES (?1, cpd_txn())", [plan.as_str()])?;
            Ok(())
        })
        .unwrap();
}

/// A Plan `pay` in `dispatch`: the `dispatch` PR `api` (E4 has an `open` Job to add) and the
/// `waiting` PR `ship` (E7 has it to start).
fn seed_dispatch_pr(engine: &mut Engine) {
    engine
        .store
        .write("test", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', '/r', 'main', 'dispatch', 'cpd/pay', '/lead', 't', cpd_txn(), 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pay', 'ship', '/r', 'main', 'waiting', 'cpd/pay/ship', cpd_txn(), 't');",
            )?;
            Ok(())
        })
        .unwrap();
}

/// A second Plan `ops` in `dispatch`: the `dispatch` PR `web` and the `waiting` PR `app`.
fn seed_held_dispatch_pr(engine: &mut Engine) {
    engine
        .store
        .write("test", |tx| {
            tx.execute_batch(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead2', 'lead', 'ctx', 'ops', 'Lead 2', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('ops', 'ctx', 'Ops', 'dispatch', 'lead2', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('ops', 'web', '/r', 'main', 'dispatch', 'cpd/ops', '/lead2', 't', cpd_txn(), 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('ops', 'app', '/r', 'main', 'waiting', 'cpd/ops/app', cpd_txn(), 't');",
            )?;
            Ok(())
        })
        .unwrap();
}

/// The `open` Jobs written so far, as `(plan, pr)`.
fn open_jobs(engine: &mut Engine) -> Vec<(String, String)> {
    engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT plan, pr FROM jobs WHERE kind = 'open' ORDER BY plan, pr")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// Whether incident mode holds Plan `plan` for E4 and E7 (the scoping seam).
fn held(engine: &mut Engine, plan: &str) -> bool {
    let plan = PlanId::parse(plan).unwrap();
    engine.store.read(|conn| Ok(engine::incident_holds_plan(conn, &plan))).unwrap()
}

#[test]
fn incident_mode_runs_only_incident_plans_and_explicit_urgent_jobs() {
    let (mut engine, _dir) = engine_fixture();
    seed_dispatch_pr(&mut engine);
    seed_held_dispatch_pr(&mut engine);
    let pay = PlanId::parse("pay").unwrap();
    let ops = PlanId::parse("ops").unwrap();

    // Incident off: every Ready Job may start, whatever its Plan or urgency.
    engine
        .store
        .write("test", |tx| {
            assert!(schedule::incident_starts(tx, &pay, false), "off: a non-urgent Job starts");
            assert!(schedule::incident_starts(tx, &ops, false), "off: a non-urgent Job of any Plan starts");
            assert!(schedule::incident_starts(tx, &pay, true), "off: an urgent Job starts");
            assert!(schedule::incident_starts(tx, &ops, true), "off: an urgent Job of any Plan starts");
            Ok(())
        })
        .unwrap();

    incident_on(&mut engine);
    incident_includes(&mut engine, "pay");

    // Incident on: a Job runs when its Plan is in `incident_plans` or it is urgent; otherwise it holds.
    engine
        .store
        .write("test", |tx| {
            assert!(schedule::incident_starts(tx, &pay, false), "on: a non-urgent Job of an incident Plan starts");
            assert!(schedule::incident_starts(tx, &pay, true), "on: an urgent Job of an incident Plan starts");
            assert!(!schedule::incident_starts(tx, &ops, false), "on: a non-urgent Job of a held Plan waits");
            assert!(schedule::incident_starts(tx, &ops, true), "on: an explicit urgent Job still starts");
            Ok(())
        })
        .unwrap();
}

#[test]
fn incident_mode_holds_e4_and_e7_for_plans_outside_the_set() {
    let (mut engine, _dir) = engine_fixture();
    seed_dispatch_pr(&mut engine);
    seed_held_dispatch_pr(&mut engine);
    incident_on(&mut engine);
    incident_includes(&mut engine, "pay");

    // The scoping seam E4 and E7 use: `pay` is in the incident, `ops` is held.
    assert!(!held(&mut engine, "pay"), "a Plan in the incident is not held");
    assert!(held(&mut engine, "ops"), "a Plan outside the incident is held");

    // E4 acts on the incident Plan's `dispatch` PR only: the held Plan's PR gets no `open` Job.
    open::run(&mut engine);
    assert_eq!(open_jobs(&mut engine), vec![("pay".to_owned(), "api".to_owned())], "E4 adds the open Job only for the incident Plan's PR");

    // E7 selects only the incident Plan's `waiting` PR; the held Plan's PR is left alone.
    let startable = engine::startable_prs(&mut engine).unwrap();
    assert_eq!(
        startable,
        vec![(PlanId::parse("pay").unwrap(), PrKey::parse("ship").unwrap())],
        "E7 starts only the incident Plan's PR"
    );

    // E9, E10 and E11 still hold machine-wide; the other steps run.
    assert!(engine::incident_holds(&mut engine, Step::E9), "E9 holds");
    assert!(engine::incident_holds(&mut engine, Step::E10), "E10 holds");
    assert!(engine::incident_holds(&mut engine, Step::E11), "E11 holds");
    for step in [Step::E1, Step::E2, Step::E4, Step::E5, Step::E7, Step::E8, Step::E12, Step::E13] {
        assert!(!engine::incident_holds(&mut engine, step), "{step} is not held machine-wide");
    }
}
