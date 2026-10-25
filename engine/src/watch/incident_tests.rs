//! Red tests for the engine's incident-mode pause (contract §3.7 E3–E11, Job inc-docs, hs-d634):
//! while one `incident` row exists, E3 starts only urgent Ready Jobs and E4, E7, E9, E10 and E11
//! hold (they start nothing and write nothing); E1, E2, E5, E8, E12 and E13 run as usual.
//!
//! The inc-engine Job implements the two stubs these tests call
//! ([`schedule::incident_starts`] and [`engine::incident_holds`]) and wires them into the steps.

use std::os::unix::fs::PermissionsExt;

use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;

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

/// A Plan in `dispatch` with a PR in `dispatch`: E4 has an `open` Job to add.
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
                   VALUES ('pay', 'api', '/r', 'main', 'dispatch', 'cpd/pay', '/lead', 't', cpd_txn(), 't');",
            )?;
            Ok(())
        })
        .unwrap();
}

/// The `job-added` Events written so far.
fn job_added(engine: &mut Engine) -> i64 {
    engine
        .store
        .read(|conn| Ok(conn.query_row("SELECT count(*) FROM events WHERE kind = 'job-added'", [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn incident_mode_starts_only_urgent_jobs() {
    let (mut engine, _dir) = engine_fixture();

    // Incident off: every Ready Job may start.
    engine
        .store
        .write("test", |tx| {
            assert!(schedule::incident_starts(tx, false), "off: a non-urgent Job starts");
            assert!(schedule::incident_starts(tx, true), "off: an urgent Job starts");
            Ok(())
        })
        .unwrap();

    incident_on(&mut engine);

    // Incident on: a non-urgent Ready Job waits, an urgent one still starts.
    engine
        .store
        .write("test", |tx| {
            assert!(!schedule::incident_starts(tx, false), "on: a non-urgent Job waits");
            assert!(schedule::incident_starts(tx, true), "on: an urgent Job still starts");
            Ok(())
        })
        .unwrap();
}

#[test]
fn incident_mode_holds_open_jobs_and_pr_starts() {
    let (mut engine, _dir) = engine_fixture();
    seed_dispatch_pr(&mut engine);
    incident_on(&mut engine);

    // E4 holds: it adds no `open` Job while incident mode is on (the red state adds one).
    open::run(&mut engine);
    assert_eq!(job_added(&mut engine), 0, "E4 adds no open Job while incident mode is on");

    // The hold decision E4, E7, E9, E10 and E11 use; the other steps run.
    assert!(engine::incident_holds(&mut engine, Step::E4), "E4 holds");
    assert!(engine::incident_holds(&mut engine, Step::E7), "E7 holds");
    assert!(engine::incident_holds(&mut engine, Step::E9), "E9 holds");
    assert!(engine::incident_holds(&mut engine, Step::E10), "E10 holds");
    assert!(engine::incident_holds(&mut engine, Step::E11), "E11 holds");
    for step in [Step::E1, Step::E2, Step::E5, Step::E8, Step::E12, Step::E13] {
        assert!(!engine::incident_holds(&mut engine, step), "{step} still runs");
    }

    // Incident mode off: E4 adds the open Job again.
    engine
        .store
        .write("test", |tx| {
            tx.execute("DELETE FROM incident", [])?;
            Ok(())
        })
        .unwrap();
    open::run(&mut engine);
    assert_eq!(job_added(&mut engine), 1, "off: E4 adds the open Job");
}
