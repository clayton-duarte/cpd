//! Red tests for `cpd cockpit serve` (commands.md §11a, contract §5.5).
//!
//! No process, tmux, sleep or clock: the store is a temp `$CPD_HOME`, time is the `Input::Tick`s of the script, and
//! the `ChangeSource` is a fake that commits Events through a second handle on the same store when its script says so.
//! `snapshot_panes_equal_the_list_readers` and `plan_show_request_returns_the_plan_value` read the list and show
//! commands through `$CPD_HOME` (each sets it to its own temp dir; nextest runs one test per process).

use std::collections::BTreeSet;
use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{COALESCE_MS, ChangeSource, Frame, Input, POLL_MS, serve};
use crate::clock::ManualClock;
use crate::env::Env;
use crate::events::{self, NewEvent};
use crate::home::CpdHome;
use crate::host::runner::FakeRunner;
use crate::host::spawn::FakeSpawn;
use crate::ids::PlanId;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

fn ports() -> Ports {
    let clock = ManualClock::new();
    Ports {
        clock: Arc::new(clock.clone()),
        runner: Arc::new(FakeRunner::new(clock)),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    }
}

/// A temp store with one Context, one Plan `pay` and one Event, and the `Env` over it.
struct Rig {
    _tmp: TempDir,
    home: CpdHome,
    env: Env,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(tmp.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');",
            )?;
            events::append(tx, NewEvent::plan_phase("seed event", "plan", "dispatch").plan(PlanId::parse("pay").unwrap()))?;
            Ok(())
        })
        .unwrap();
    drop(store);
    let env = Env::open_home_with(&ports(), home.clone()).unwrap();
    Rig {
        _tmp: tmp,
        home,
        env,
    }
}

/// Commits one Event through its own handle on the store.
fn commit_event(store: &mut Store, text: &str) {
    store
        .write("fixture event", |tx| {
            events::append(
                tx,
                NewEvent::plan_phase(text, "plan", "dispatch").plan(PlanId::parse("pay").unwrap()),
            )?;
            Ok(())
        })
        .unwrap();
}

/// Reports a change at the polls listed in `at` (0-based) and commits one Event at each of them.
struct Script {
    store: Store,
    at: BTreeSet<usize>,
    polls: usize,
}

impl Script {
    fn new(home: &CpdHome, at: &[usize]) -> Script {
        Script {
            store: Store::open(home, 5000).unwrap(),
            at: at.iter().copied().collect(),
            polls: 0,
        }
    }
}

impl ChangeSource for Script {
    fn poll(&mut self) -> Option<i64> {
        let n = self.polls;
        self.polls += 1;
        if !self.at.contains(&n) {
            return None;
        }
        commit_event(&mut self.store, &format!("event at poll {n}"));
        Some(n as i64 + 1)
    }
}

fn line(value: Value) -> Input {
    Input::Line(value.to_string())
}

fn ticks(n: usize) -> Vec<Input> {
    (0..n).map(|_| Input::Tick).collect()
}

/// Runs `serve` on `input` and returns its frames (every output line, parsed) and its result.
fn run(rig: &mut Rig, changes: &mut dyn ChangeSource, input: Vec<Input>) -> Vec<Value> {
    let mut out = Vec::new();
    serve(&mut rig.env, &mut input.into_iter(), &mut out, changes)
        .expect("serve ends Ok at end of input");
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l:?}: {e}")))
        .collect()
}

/// [`run`] with a source that never changes.
fn run_quiet(rig: &mut Rig, input: Vec<Input>) -> Vec<Value> {
    let mut changes = Script::new(&rig.home.clone(), &[]);
    run(rig, &mut changes, input)
}

fn kinds(frames: &[Value]) -> Vec<&str> {
    frames.iter().map(|f| f["t"].as_str().unwrap()).collect()
}

#[derive(Parser)]
struct ContextCli {
    #[command(subcommand)]
    command: crate::context::Command,
}
#[derive(Parser)]
struct PlanCli {
    #[command(subcommand)]
    command: crate::plan::Command,
}
#[derive(Parser)]
struct SessionCli {
    #[command(subcommand)]
    command: crate::session::Command,
}
#[derive(Parser)]
struct InboxCli {
    #[command(subcommand)]
    command: crate::inbox::cmd::Command,
}

/// What the CLI prints for a success: `{"ok": true, …fields}`.
fn printed(out: Result<Output, crate::error::CpdError>) -> Value {
    let Output::Success(fields) = out.unwrap() else {
        panic!("a success")
    };
    let mut value = json!({"ok": true});
    value.as_object_mut().unwrap().extend(fields);
    value
}

#[test]
fn snapshot_panes_equal_the_list_readers() {
    let mut rig = rig();
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let ports = ports();
    let expect = Ports::scope(&ports, || {
        json!({
            "context": printed(crate::context::run(ContextCli::try_parse_from(["cpd", "list"]).unwrap().command)),
            "plan": printed(crate::plan::run(PlanCli::try_parse_from(["cpd", "list"]).unwrap().command)),
            "session": printed(crate::session::run(SessionCli::try_parse_from(["cpd", "list"]).unwrap().command)),
            "inbox": printed(crate::inbox::cmd::run(InboxCli::try_parse_from(["cpd", "list"]).unwrap().command)),
        })
    });
    let frames = run_quiet(&mut rig, vec![]);
    assert_eq!(frames[1]["t"], "snapshot");
    assert_eq!(frames[1]["panes"], expect);
}

#[test]
fn hello_is_the_first_frame_then_a_snapshot() {
    let mut rig = rig();
    let frames = run_quiet(&mut rig, vec![]);
    assert_eq!(frames[0], json!({"t": "hello", "protocol": 1}));
    assert_eq!(Frame::Hello.to_line(), "{\"t\":\"hello\",\"protocol\":1}\n");
    assert_eq!(frames[1]["t"], "snapshot");
    assert_eq!(frames[1]["seq"], 1);
    assert_eq!(kinds(&frames), ["hello", "snapshot"]);
}

#[test]
fn committed_event_is_framed_before_the_next_snapshot() {
    let mut rig = rig();
    let mut changes = Script::new(&rig.home.clone(), &[0]);
    let frames = run(&mut rig, &mut changes, ticks(4));
    assert_eq!(kinds(&frames), ["hello", "snapshot", "event", "snapshot"]);
    assert_eq!(frames[2]["event"]["id"], 2);
    assert_eq!(frames[2]["event"]["kind"], "plan-phase");
    assert_eq!(frames[2]["event"]["text"], "event at poll 0");
    assert_eq!(frames[3]["seq"], 2);
}

#[test]
fn events_before_start_are_not_replayed() {
    let mut rig = rig();
    let mut changes = Script::new(&rig.home.clone(), &[0]);
    let frames = run(&mut rig, &mut changes, ticks(4));
    let events: Vec<&Value> = frames.iter().filter(|f| f["t"] == "event").collect();
    // The seed Event (id 1) predates the start; only the Event committed after it is framed.
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"]["id"], 2);
}

#[test]
fn changes_within_the_coalesce_window_make_one_snapshot() {
    // The windows are measured in ticks of POLL_MS: a change one tick after the first is inside COALESCE_MS.
    const { assert!(POLL_MS < COALESCE_MS && 2 * POLL_MS >= COALESCE_MS) };
    let mut rig = rig();
    // Polls 0 and 1 change (one batch); poll 4 changes again, after the window closed (a second batch).
    let mut changes = Script::new(&rig.home.clone(), &[0, 1, 4]);
    let frames = run(&mut rig, &mut changes, ticks(8));
    assert_eq!(
        kinds(&frames),
        [
            "hello", "snapshot", "event", "event", "snapshot", "event", "snapshot"
        ]
    );
    let seqs: Vec<&Value> = frames
        .iter()
        .filter(|f| f["t"] == "snapshot")
        .map(|f| &f["seq"])
        .collect();
    assert_eq!(seqs, [1, 2, 3]);
}

#[test]
fn plan_show_request_returns_the_plan_value() {
    let mut rig = rig();
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let expect = Ports::scope(&ports(), || {
        printed(crate::plan::run(
            PlanCli::try_parse_from(["cpd", "show", "--plan", "pay"])
                .unwrap()
                .command,
        ))
    });
    let input = vec![line(json!({"t": "plan_show", "id": "r1", "plan": "pay"}))];
    let frames = run_quiet(&mut rig, input);
    assert_eq!(kinds(&frames), ["hello", "snapshot", "plan_show"]);
    assert_eq!(
        frames[2],
        json!({"t": "plan_show", "id": "r1", "value": expect})
    );
}

#[test]
fn unknown_plan_returns_an_error_frame_and_serving_continues() {
    let mut rig = rig();
    let input = vec![
        line(json!({"t": "plan_show", "id": "r2", "plan": "no-such-plan"})),
        line(json!({"t": "refresh"})),
    ];
    let frames = run_quiet(&mut rig, input);
    assert_eq!(kinds(&frames), ["hello", "snapshot", "error", "snapshot"]);
    assert_eq!(frames[2]["id"], "r2");
    assert_eq!(frames[2]["error"]["code"], "plan_not_found");
    assert!(frames[2]["error"]["message"].is_string());
    assert_eq!(frames[3]["seq"], 2);
}

#[test]
fn malformed_line_returns_an_error_frame_with_null_id() {
    let mut rig = rig();
    let input = vec![
        Input::Line("not json".into()),
        line(json!({"t": "refresh"})),
    ];
    let frames = run_quiet(&mut rig, input);
    assert_eq!(kinds(&frames), ["hello", "snapshot", "error", "snapshot"]);
    assert_eq!(frames[2]["id"], Value::Null);
    assert_eq!(frames[2]["error"]["code"], "usage");
    assert_eq!(frames[3]["seq"], 2);
}

#[test]
fn end_of_input_ends_serving_cleanly() {
    let mut rig = rig();
    let mut out = Vec::new();
    let mut changes = Script::new(&rig.home.clone(), &[]);
    let result = serve(
        &mut rig.env,
        &mut std::iter::empty(),
        &mut out,
        &mut changes,
    );
    assert!(result.is_ok());
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 2, "only hello and snapshot 1: {text}");
}
