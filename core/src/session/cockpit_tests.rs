//! Red tests for ensuring the cockpit session before new_window and for reporting
//! a vanished tmux session as stopped.

use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::Value;

use super::{Command, run};
use crate::clock::ManualClock;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::host::host_socket::FakeHostSocket;
use crate::host::runner::FakeRunner;
use crate::host::spawn::FakeSpawn;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Parser)]
struct ContextCli {
    #[command(subcommand)]
    command: crate::context::Command,
}

struct World {
    dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
    ports: Ports,
    runner: FakeRunner,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(
        home.config(),
        "version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"t\"\n",
    )
    .unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            Ok(tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
                   VALUES ('stopped1', 'lead', 'ctx', 'spay', 'Stopped', '/', 'O3', 'stopped', 't', 't', 'pi'),
                          ('working1', 'lead', 'ctx', 'wpay', 'Working', '/', 'O3', 'working', 't', 't', 'pi'),
                          ('mgr1', 'manager', 'ctx', NULL, 'ctx', '/', 'O4', 'stopped', 't', 't', 'pi');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('spay', 'ctx', 'S', 'plan', 'stopped1', 't'), ('wpay', 'ctx', 'W', 'plan', 'working1', 't');",
            )?)
        })
        .unwrap();
    let runner = FakeRunner::with_tmux();
    let ports = Ports {
        clock: Arc::new(ManualClock::new()),
        runner: Arc::new(runner.clone()),
        spawn: Arc::new(FakeSpawn::new()),
        host_socket: Arc::new(FakeHostSocket::new()),
        host_conn: Arc::new(crate::session::tail::FakeHostConn::new()),
    };
    World {
        dir,
        home,
        store,
        ports,
        runner,
    }
}

fn try_cpd(world: &World, words: &[&str]) -> Result<Value, CpdError> {
    let command = Cli::try_parse_from(["cpd"].into_iter().chain(words.iter().copied()))
        .unwrap()
        .command;
    match crate::env::with_fixture_environment(&world.home, world.dir.path(), || {
        Ports::scope(&world.ports, || run(command))
    })? {
        Output::Success(fields) => Ok(Value::Object(fields)),
        _ => panic!("a success"),
    }
}

fn try_context(world: &World, words: &[&str]) -> Result<Value, CpdError> {
    let command = ContextCli::try_parse_from(["context"].into_iter().chain(words.iter().copied()))
        .unwrap()
        .command;
    match crate::env::with_fixture_environment(&world.home, world.dir.path(), || {
        Ports::scope(&world.ports, || crate::context::run(command))
    })? {
        Output::Success(fields) => Ok(Value::Object(fields)),
        _ => panic!("a success"),
    }
}

fn tmux_has_cockpit(w: &World) -> bool {
    Tmux::with_tmpdir("t", w.dir.path())
        .with_runner(Arc::new(w.runner.clone()))
        .has_session("cpd")
        .unwrap()
}

/// Case 1: with the fake having NO cockpit session, `context manager` / `manager start`
/// (core/src/context/manager.rs:218) creates the cockpit session then opens the window.
#[test]
fn manager_start_ensures_cockpit_session() {
    let w = world();
    assert!(!tmux_has_cockpit(&w), "no cockpit to begin with");

    let out = try_context(&w, &["manager", "--context", "ctx"]).unwrap();
    assert_eq!(out["started"], true, "{out}");
    assert!(out["manager"]["pane"].is_string(), "{out}");
    assert!(
        tmux_has_cockpit(&w),
        "the cockpit was created by manager start"
    );
}

/// Case 2: with the fake having NO cockpit session, each of the park and open paths
/// (core/src/session/moves.rs:235, :339, :530) creates the session then opens the window.
#[test]
fn park_and_open_paths_ensure_cockpit_session() {
    // Fork path (moves.rs:235): with no cockpit session, creates the session then opens the window.
    {
        let w = world();
        assert!(!tmux_has_cockpit(&w));
        let out = try_cpd(&w, &["fork", "stopped1"]).unwrap();
        assert!(out["session"]["pane"].is_string(), "{out}");
        assert!(tmux_has_cockpit(&w), "fork ensures cockpit session");
    }

    // Without a chat slot neither operation can swap a pane.
    for command in ["open", "park"] {
        let w = world();
        let err = try_cpd(&w, &[command, "stopped1"]).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::NoCockpit);
        assert!(!tmux_has_cockpit(&w));
        assert!(
            !w.runner
                .calls()
                .iter()
                .any(|call| call.args.iter().any(|arg| arg == "swap-pane"))
        );
    }

    // With a chat slot, opening and parking preserve the normal swap contract.
    {
        let w = world();
        let tmux = Tmux::with_tmpdir("t", w.dir.path()).with_runner(Arc::new(w.runner.clone()));
        let (_, pane) = tmux
            .create_session("cpd", "main", &super::moves::placeholder_spawn())
            .unwrap()
            .unwrap();
        tmux.set_pane_view(&pane, crate::host::View::Chat).unwrap();
        let out = try_cpd(&w, &["open", "stopped1"]).unwrap();
        assert!(out["pane"].is_string(), "{out}");
        let out = try_cpd(&w, &["park", "stopped1"]).unwrap();
        assert!(out["pane"].is_string(), "{out}");
    }
}

/// Case 3: liveness for a Session whose pane lives in a missing tmux session reports stopped
/// (core/src/session/state.rs pane_gone / engine/src/watch/liveness.rs).
#[test]
fn missing_tmux_session_reads_as_stopped() {
    let mut w = world();
    // Record working1 with a pane %10 in the database
    w.store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE sessions SET tmux_pane = '%10', state = 'working' WHERE id = 'working1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    // Tmux server has NO sessions and NO panes (the tmux session is missing / vanished).
    assert!(!tmux_has_cockpit(&w));

    // When the cockpit tmux session is missing / vanished, the session's pane is gone.
    // Liveness and state reconciliation must read/report it as stopped with no pane.
    let row = w
        .store
        .read(|conn| super::row::load(conn, &crate::ids::SessionId::parse("working1").unwrap()))
        .unwrap();

    let tmux = Tmux::with_tmpdir("t", w.dir.path()).with_runner(Arc::new(w.runner.clone()));
    let _panes = tmux.list_panes().unwrap();
    let cockpit_missing = !tmux.has_session("cpd").unwrap();
    assert!(cockpit_missing, "cockpit session is missing in tmux");

    // Liveness checks if tmux session exists; if missing, reports stopped
    let why = "cockpit tmux session is missing";
    let changed = w
        .store
        .write("liveness", |tx| {
            super::state::pane_gone(
                tx,
                &row.id,
                row.state.as_str(),
                row.tmux_pane.as_deref(),
                why,
            )
        })
        .unwrap();
    assert!(changed, "pane_gone transitioned working session to stopped");

    // A session with a missing tmux session must read as stopped in the store and session show:
    // even before a background E2 sweep or when queried directly, it must not read as working.
    let row_after = w
        .store
        .read(|conn| super::row::load(conn, &row.id))
        .unwrap();
    assert_eq!(row_after.state, super::row::State::Stopped);
    assert_eq!(row_after.tmux_pane, None);

    // If a session in the store is working but its tmux session vanished,
    // its effective liveness state must read as stopped:
    w.store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE sessions SET tmux_pane = '%20', state = 'working' WHERE id = 'working1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let out = try_cpd(&w, &["show", "working1"]).unwrap();
    assert_eq!(
        out["session"]["state"], "stopped",
        "missing tmux session reads as stopped"
    );
}

/// Case 4: session show does not report a vanished session as working when its tmux session is missing.
#[test]
fn session_show_does_not_report_vanished_session_as_working() {
    let mut w = world();
    // working1 is recorded in the store as working with pane %10, but tmux has no cockpit or pane %10.
    w.store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE sessions SET tmux_pane = '%10', state = 'working' WHERE id = 'working1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();

    assert!(!tmux_has_cockpit(&w));

    let out = try_cpd(&w, &["show", "working1"]).unwrap();
    let session = &out["session"];
    assert_ne!(
        session["state"], "working",
        "vanished tmux session must not be reported as working by session show: {out}"
    );
    assert_eq!(
        session["state"], "stopped",
        "vanished tmux session reads as stopped in session show: {out}"
    );
}

#[test]
fn fixture_commands_use_their_own_home_and_tmux_dir() {
    let mut first = world();
    first
        .store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE sessions SET name = 'First' WHERE id = 'stopped1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let second = world();
    assert_eq!(
        try_cpd(&first, &["show", "stopped1"]).unwrap()["session"]["name"],
        "First"
    );
    assert_eq!(
        try_cpd(&second, &["show", "stopped1"]).unwrap()["session"]["name"],
        "Stopped"
    );
    for w in [&first, &second] {
        assert!(!tmux_has_cockpit(w));
        let calls = w.runner.calls();
        assert!(!calls.is_empty());
        for call in calls.iter().filter(|call| call.program == "tmux") {
            assert!(call.envs.iter().any(|(key, value)| key == "TMUX_TMPDIR"
                && value.as_deref() == Some(w.dir.path().to_str().unwrap())));
        }
    }
}
