//! `session resume` of a stopped pi Session (commands.md §10.17, issue #1207): a stopped Session is restarted in a parked
//! window running `session run <id>`, creating the cockpit when it is missing; a running Session is a no-op; an unknown
//! key is refused. On the fake ports and the in-process fake tmux: no process starts.

use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::Value;

use super::{Command, run};
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
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

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    ports: Ports,
    runner: FakeRunner,
}

/// A home with pi Lead `stopped1` (`stopped`, no pane) and pi Lead `running1` (`idle`), and a fake tmux server with no
/// cockpit. `$CPD_HOME` is this test's (nextest runs one test per process).
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
                          ('running1', 'lead', 'ctx', 'rpay', 'Running', '/', 'O3', 'idle', 't', 't', 'pi');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('spay', 'ctx', 'S', 'plan', 'stopped1', 't'), ('rpay', 'ctx', 'R', 'plan', 'running1', 't');",
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
    unsafe {
        std::env::set_var("TMUX_TMPDIR", dir.path());
        std::env::set_var("CPD_HOME", home.root());
        std::env::remove_var("CPD_SESSION");
        std::env::remove_var("CPD_ATTEMPT");
        std::env::remove_var("PI_SESSION_ID");
    }
    World {
        _dir: dir,
        store,
        ports,
        runner,
    }
}

fn try_cpd(world: &World, words: &[&str]) -> Result<Value, CpdError> {
    let command = Cli::try_parse_from(["cpd"].into_iter().chain(words.iter().copied()))
        .unwrap()
        .command;
    match Ports::scope(&world.ports, || run(command))? {
        Output::Success(fields) => Ok(Value::Object(fields)),
        _ => panic!("a success"),
    }
}

fn tmux_calls(w: &World) -> Vec<String> {
    w.runner.calls().iter().map(|c| c.args.join(" ")).collect()
}

/// The calls that start a pane: a new session or a new window.
fn pane_starts(w: &World) -> Vec<String> {
    tmux_calls(w)
        .into_iter()
        .filter(|c| c.contains("new-session") || c.contains("new-window"))
        .collect()
}

fn tmux_has_cockpit(w: &World) -> bool {
    Tmux::new("t")
        .with_runner(Arc::new(w.runner.clone()))
        .has_session("cpd")
        .unwrap()
}

fn count(w: &mut World, sql: &str) -> i64 {
    w.store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn resume_creates_cockpit_and_starts_pane_for_stopped_session() {
    let mut w = world();
    assert!(!tmux_has_cockpit(&w), "no cockpit to begin with");
    let out = try_cpd(&w, &["resume", "stopped1"]).unwrap();
    assert_eq!(out["session"], "stopped1", "{out}");
    assert!(out["pane"].is_string(), "a new pane: {out}");
    assert_eq!(out["state"], "stopped", "{out}");
    assert!(tmux_has_cockpit(&w), "the cockpit was created");
    let starts = pane_starts(&w);
    assert_eq!(starts.len(), 1, "one pane started: {starts:?}");
    assert!(
        starts[0].ends_with("session run stopped1"),
        "the pane runs `session run <id>`: {starts:?}"
    );
    assert_eq!(
        count(
            &mut w,
            "SELECT count(*) FROM sessions WHERE id = 'stopped1' AND tmux_pane IS NOT NULL AND state = 'stopped'"
        ),
        1,
        "the pane is recorded and the state stays stopped"
    );
}

#[test]
fn resume_is_noop_for_running_session() {
    let mut w = world();
    let out = try_cpd(&w, &["resume", "running1"]).unwrap();
    assert_eq!(out["already"], Value::Bool(true), "{out}");
    assert_eq!(out["pane"], Value::Null, "{out}");
    assert!(
        pane_starts(&w).is_empty(),
        "no new pane: {:?}",
        pane_starts(&w)
    );
    assert_eq!(
        count(
            &mut w,
            "SELECT count(*) FROM events WHERE session = 'running1'"
        ),
        0,
        "writes nothing"
    );
}

#[test]
fn resume_refuses_unknown_key() {
    let mut w = world();
    let err = try_cpd(&w, &["resume", "nosuchkey"]).expect_err("an unknown key is refused");
    assert_eq!(err.code(), ErrorCode::SessionNotFound, "{}", err.message());
    assert_eq!(err.code().exit().code(), 4);
    assert!(pane_starts(&w).is_empty(), "no pane: {:?}", pane_starts(&w));
    assert_eq!(count(&mut w, "SELECT count(*) FROM events"), 0, "no Event");
    // The refusal leaves the world intact: a known stopped Session still resumes.
    let out = try_cpd(&w, &["resume", "stopped1"]).unwrap();
    assert!(out["pane"].is_string(), "{out}");
}

#[test]
fn resume_keeps_state_stopped_until_the_adapter_reports() {
    let mut w = world();
    try_cpd(&w, &["resume", "stopped1"]).unwrap();
    assert_eq!(
        count(
            &mut w,
            "SELECT count(*) FROM sessions WHERE id = 'stopped1' AND state = 'stopped'"
        ),
        1,
        "resume does not write idle: the adapter watchdog needs stopped"
    );
    assert_eq!(
        count(
            &mut w,
            "SELECT count(*) FROM events WHERE session = 'stopped1' AND kind = 'session-state'"
        ),
        0,
        "no session-state Event"
    );
}

#[test]
fn resume_replaces_the_live_wrapper_waiting_for_enter() {
    let w = world();
    try_cpd(&w, &["resume", "stopped1"]).unwrap();
    // pi exited: the wrapper pane stays alive, the Session is stopped.
    let out = try_cpd(&w, &["resume", "stopped1"]).unwrap();
    assert_eq!(out["already"], Value::Bool(false), "{out}");
    assert!(out["pane"].is_string(), "a restarted pane: {out}");
    assert_eq!(pane_starts(&w).len(), 2, "{:?}", pane_starts(&w));
    assert!(
        tmux_calls(&w).iter().any(|c| c.contains("kill-pane")),
        "the old wrapper pane was killed: {:?}",
        tmux_calls(&w)
    );
}

#[test]
fn resume_refuses_a_durable_session_before_any_side_effect() {
    let mut w = world();
    w.store
        .write("fixture", |tx| {
            Ok(tx.execute_batch("UPDATE sessions SET runner = 'durable' WHERE id = 'stopped1'")?)
        })
        .unwrap();
    let err = try_cpd(&w, &["resume", "stopped1"]).expect_err("durable is refused");
    assert_eq!(err.code(), ErrorCode::DurableSession, "{}", err.message());
    assert!(pane_starts(&w).is_empty(), "no pane: {:?}", pane_starts(&w));
    assert_eq!(count(&mut w, "SELECT count(*) FROM events"), 0, "no Event");
}

#[test]
fn resume_replacing_the_wrapper_in_the_chat_slot_keeps_the_chat_slot() {
    let w = world();
    let tmux = Tmux::new("t").with_runner(Arc::new(w.runner.clone()));
    let (_, placeholder) = tmux
        .create_session("cpd", "main", &super::moves::placeholder_spawn())
        .unwrap()
        .unwrap();
    tmux.set_pane_view(&placeholder, crate::host::View::Chat)
        .unwrap();
    try_cpd(&w, &["resume", "stopped1"]).unwrap();
    // The wrapper (pi exited) holds the chat slot: swapped into cpd:main.
    let wrapper = tmux
        .list_panes()
        .unwrap()
        .into_iter()
        .find(|p| p.cpd_session.is_some())
        .unwrap()
        .pane;
    tmux.swap_pane(&wrapper, &placeholder).unwrap();
    let slot =
        |panes: &[crate::host::Pane]| crate::host::chat_slot(panes, "cpd").map(|p| p.pane.clone());
    assert_eq!(slot(&tmux.list_panes().unwrap()), Some(wrapper.clone()));

    try_cpd(&w, &["resume", "stopped1"]).unwrap();

    let panes = tmux.list_panes().unwrap();
    assert!(
        slot(&panes).is_some(),
        "the chat slot still exists: {panes:?}"
    );
    assert!(
        panes.iter().all(|p| p.pane != wrapper),
        "the old wrapper is gone"
    );
    assert!(
        panes
            .iter()
            .any(|p| !p.dead && p.cpd_session.is_some() && Some(&p.pane) != slot(&panes).as_ref()),
        "the restarted pi runs parked"
    );
}
