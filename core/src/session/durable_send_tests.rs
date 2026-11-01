//! `session send` and `session show` / `session list` with the stored runner (commands.md §10.1, §10.2, §10.13; contract
//! §3.6): a send to a stopped `durable` Lead appends its `session-message` Event and starts no pane; a stopped `pi` Lead is
//! still started; both commands print `runner`. On the fake ports and the in-process fake tmux: no process starts.

use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::Value;

use super::{Command, run};
use crate::clock::ManualClock;
use crate::home::CpdHome;
use crate::host::Tmux;
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

/// A home with Lead `dlead` (runner `durable`) and Lead `plead` (runner `pi`), both `stopped` with no pane, each of its own
/// Plan, and a fake tmux server with the cockpit session `cpd`. `$CPD_HOME` is this test's (nextest runs one test per process).
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
                   VALUES ('dlead', 'lead', 'ctx', 'dpay', 'Durable', '/', 'O3', 'stopped', 't', 't', 'durable'),
                          ('plead', 'lead', 'ctx', 'ppay', 'Pi', '/', 'O3', 'stopped', 't', 't', 'pi');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('dpay', 'ctx', 'D', 'plan', 'dlead', 't'), ('ppay', 'ctx', 'P', 'plan', 'plead', 't');",
            )?)
        })
        .unwrap();
    let clock = ManualClock::new();
    let runner = FakeRunner::with_tmux();
    let ports = Ports {
        clock: Arc::new(clock),
        runner: Arc::new(runner.clone()),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    // The fake tmux keys its server by `$TMUX_TMPDIR` and the socket: the same directory for every `Tmux` of this process.
    unsafe { std::env::set_var("TMUX_TMPDIR", dir.path()) };
    Tmux::new("t")
        .with_runner(Arc::new(runner.clone()))
        .run(&["new-session", "-d", "-s", "cpd", "sleep", "600"].map(str::to_owned))
        .unwrap();
    // The one set of this process's environment: the home, and no sender (the command then runs as the User).
    unsafe {
        std::env::set_var("CPD_HOME", home.root());
        std::env::remove_var("CPD_SESSION");
        std::env::remove_var("PI_SESSION_ID");
    }
    World {
        _dir: dir,
        store,
        ports,
        runner,
    }
}

fn cpd(world: &World, words: &[&str]) -> Value {
    let command = Cli::try_parse_from(["cpd"].into_iter().chain(words.iter().copied()))
        .unwrap()
        .command;
    match Ports::scope(&world.ports, || run(command)).unwrap() {
        Output::Success(fields) => Value::Object(fields),
        _ => panic!("a success"),
    }
}

fn messages(world: &mut World, session: &str) -> i64 {
    world
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM events WHERE kind = 'session-message' AND session = ?1",
                [session],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn new_windows(world: &World) -> usize {
    world
        .runner
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a == "new-window"))
        .count()
}

#[test]
fn send_to_a_stopped_durable_lead_appends_the_event_and_starts_no_pane() {
    let mut w = world();
    let out = cpd(&w, &["send", "dlead", "wake up", "--unblocks", "u"]);
    assert_eq!(messages(&mut w, "dlead"), 1, "the Event is appended");
    assert_eq!(new_windows(&w), 0, "no window");
    assert_eq!(
        (&out["started"], &out["pane"]),
        (&Value::Bool(false), &Value::Null),
        "{out}"
    );
    let pane: Option<String> = w
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT tmux_pane FROM sessions WHERE id = 'dlead'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(pane, None);
}

#[test]
fn send_to_a_stopped_pi_lead_still_starts_its_pane() {
    let mut w = world();
    let out = cpd(&w, &["send", "plead", "wake up", "--unblocks", "u"]);
    assert_eq!(messages(&mut w, "plead"), 1, "the Event is appended");
    assert_eq!(new_windows(&w), 1, "one parked window");
    assert_eq!(out["started"], Value::Bool(true), "{out}");
    assert!(out["pane"].is_string(), "{out}");
}

#[test]
fn session_show_and_list_carry_the_runner() {
    let w = world();
    let list = cpd(&w, &["list"]);
    let runner_of = |id: &str| {
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap_or_else(|| panic!("{id} in {list}"))["runner"]
            .clone()
    };
    assert_eq!(
        (runner_of("dlead"), runner_of("plead")),
        (Value::from("durable"), Value::from("pi"))
    );
    for (id, runner) in [("dlead", "durable"), ("plead", "pi")] {
        let shown = cpd(&w, &["show", id]);
        assert_eq!(shown["session"]["runner"], Value::from(runner), "{shown}");
    }
}
