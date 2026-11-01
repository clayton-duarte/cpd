//! The pane moves and a `durable` Session (commands.md §10.3 to §10.5, §10.9, §10.10, §10.12; contract §3.6, §6.19): `park`,
//! `history` and `fork` refuse it with `durable_session` before any write or tmux call; `open` no longer refuses it: it
//! resumes the Session and waits for its host's socket, opening no pane; `close` succeeds as `already` and `archive` writes
//! `archived_at`, both without a tmux call; a `pi` Session is still opened. On the fake ports and the in-process fake
//! tmux: no process starts.

use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::Value;

use super::{Command, run};
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::host::host_socket::{FakeHostSocket, Reach};
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
    host_socket: Arc<FakeHostSocket>,
    /// The tmux calls made while the world was built.
    base: usize,
}

/// A home with Leads `dlead` (runner `durable`) and `plead` (runner `pi`), both `stopped` with no pane, each of its own Plan,
/// and a fake tmux server with the cockpit session `cpd`. `$CPD_HOME` is this test's (nextest runs one test per process).
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"t\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            Ok(tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
                   VALUES ('dlead', 'lead', 'ctx', 'dpay', 'Durable', '/', 'O3', 'stopped', 't', 't', 'durable'),
                          ('plead', 'lead', 'ctx', 'ppay', 'Pi', '/', 'O3', 'stopped', 't', 't', 'pi');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('dpay', 'ctx', 'D', 'plan', 'dlead', 't'), ('ppay', 'ctx', 'P', 'plan', 'plead', 't');",
            )?)
        })
        .unwrap();
    let runner = FakeRunner::with_tmux();
    let host_socket = Arc::new(FakeHostSocket::new());
    let ports = Ports {
        clock: Arc::new(ManualClock::new()),
        runner: Arc::new(runner.clone()),
        spawn: Arc::new(FakeSpawn::new()),
        host_socket: host_socket.clone(),
        host_conn: Arc::new(crate::session::tail::FakeHostConn::new()),
    };
    unsafe { std::env::set_var("TMUX_TMPDIR", dir.path()) };
    Tmux::new("t")
        .with_runner(Arc::new(runner.clone()))
        .run(&["new-session", "-d", "-s", "cpd", "-n", "main", "sleep", "600"].map(str::to_owned))
        .unwrap();
    // The chat slot: the placeholder pane of `cpd:main`.
    Tmux::new("t")
        .with_runner(Arc::new(runner.clone()))
        .run(&["set", "-p", "-t", "cpd:main", "@cpd_view", "chat"].map(str::to_owned))
        .unwrap();
    unsafe {
        std::env::set_var("CPD_HOME", home.root());
        std::env::remove_var("CPD_SESSION");
        std::env::remove_var("PI_SESSION_ID");
    }
    let base = runner.calls().len();
    World { _dir: dir, store, ports, runner, host_socket, base }
}

fn try_cpd(world: &World, words: &[&str]) -> Result<Value, CpdError> {
    let command = Cli::try_parse_from(["cpd"].into_iter().chain(words.iter().copied())).unwrap().command;
    match Ports::scope(&world.ports, || run(command))? {
        Output::Success(fields) => Ok(Value::Object(fields)),
        _ => panic!("a success"),
    }
}

fn tmux_calls_since_start(w: &World) -> Vec<String> {
    w.runner.calls().iter().skip(w.base).map(|c| c.args.join(" ")).collect()
}

fn new_windows(w: &World) -> usize {
    w.runner.calls().iter().filter(|c| c.args.iter().any(|a| a == "new-window")).count()
}

fn refused(w: &World, words: &[&str]) {
    let err = try_cpd(w, words).expect_err("a durable Session is refused");
    assert_eq!(err.code(), ErrorCode::DurableSession, "{}", err.message());
    assert_eq!(err.code().exit().code(), 3);
    assert!(tmux_calls_since_start(w).is_empty(), "no tmux call: {:?}", tmux_calls_since_start(w));
}

fn count(w: &mut World, sql: &str) -> i64 {
    w.store.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

#[test]
fn open_of_a_durable_session_waits_for_the_socket_and_starts_no_pane() {
    let w = world();
    // The socket is absent on the first probe and accepts on the second (contract §6.19).
    w.host_socket.script(vec![Reach::Absent, Reach::Accepted]);
    let out = try_cpd(&w, &["open", "dlead"]).unwrap();
    assert_eq!(out["session"], "dlead");
    assert_eq!(out["pane"], Value::Null, "a durable Session never enters a pane");
    assert_eq!(new_windows(&w), 0, "no pane");
    assert!(tmux_calls_since_start(&w).is_empty(), "no tmux call: {:?}", tmux_calls_since_start(&w));
    assert_eq!(w.host_socket.paths().len(), 2, "probed until it accepted");
}

#[test]
fn park_refuses_a_durable_session_with_durable_session() {
    let w = world();
    refused(&w, &["park", "dlead"]);
}

#[test]
fn history_refuses_a_durable_session_with_durable_session() {
    let mut w = world();
    w.store.write("fixture", |tx| Ok(tx.execute("UPDATE sessions SET archived_at = 't' WHERE id = 'dlead'", [])?)).unwrap();
    refused(&w, &["history", "dlead"]);
    assert_eq!(new_windows(&w), 0, "no history pane");
}

#[test]
fn fork_refuses_a_durable_session_with_durable_session() {
    let mut w = world();
    refused(&w, &["fork", "dlead"]);
    assert_eq!(count(&mut w, "SELECT count(*) FROM sessions"), 2, "no Session written");
    assert_eq!(new_windows(&w), 0, "no pane");
}

#[test]
fn close_of_a_durable_session_succeeds_as_already_and_calls_no_tmux() {
    let mut w = world();
    let out = try_cpd(&w, &["close", "dlead"]).unwrap();
    assert_eq!(out["already"], Value::Bool(true), "{out}");
    assert!(tmux_calls_since_start(&w).is_empty(), "no tmux call: {:?}", tmux_calls_since_start(&w));
    assert_eq!(count(&mut w, "SELECT count(*) FROM events WHERE session = 'dlead'"), 0, "writes nothing");
}

#[test]
fn archive_of_a_durable_session_writes_archived_at_and_calls_no_tmux() {
    let mut w = world();
    // A Plan's Lead is archived with its Plan: archive a durable side fork of it.
    w.store
        .write("fixture", |tx| {
            Ok(tx.execute(
                "INSERT INTO sessions (id, role, context, plan, forked_from, name, cwd, tier, state, state_at, created_at, runner)
                   VALUES ('dfork', 'lead', 'ctx', 'dpay', 'dlead', 'Fork', '/', 'O3', 'stopped', 't', 't', 'durable')",
                [],
            )?)
        })
        .unwrap();
    try_cpd(&w, &["archive", "dfork"]).unwrap();
    assert_eq!(count(&mut w, "SELECT count(*) FROM sessions WHERE id = 'dfork' AND archived_at IS NOT NULL"), 1);
    assert!(tmux_calls_since_start(&w).is_empty(), "no tmux call: {:?}", tmux_calls_since_start(&w));
}

#[test]
fn open_still_starts_a_pane_for_a_pi_session() {
    let w = world();
    let out = try_cpd(&w, &["open", "plead"]).unwrap();
    assert!(new_windows(&w) >= 1, "a pane is started: {out}");
}
