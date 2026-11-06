//! Red tests for `cpd cockpit serve` (commands.md §11a, contract §5.5).
//!
//! No process, tmux, sleep or clock: the store is a temp `$CPD_HOME`, time is the `Input::Tick`s of the script, and
//! the `ChangeSource` is a fake that commits Events through a second handle on the same store when its script says so.
//! Serving reads everything through its own `Env`, never the process `$CPD_HOME`.

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

fn plan_list_args() -> crate::plan::ListArgs {
    match PlanCli::try_parse_from(["cpd", "list"]).unwrap().command {
        crate::plan::Command::List(args) => args,
        _ => unreachable!("list"),
    }
}

fn plan_show_args(plan: &str) -> crate::plan::ShowArgs {
    match PlanCli::try_parse_from(["cpd", "show", &format!("--plan={plan}")])
        .unwrap()
        .command
    {
        crate::plan::Command::Show(args) => args,
        _ => unreachable!("show"),
    }
}

fn session_list_args() -> crate::session::ListArgs {
    match SessionCli::try_parse_from(["cpd", "list"]).unwrap().command {
        crate::session::Command::List(args) => args,
        _ => unreachable!("list"),
    }
}

fn inbox_list_args() -> crate::inbox::cmd::ListArgs {
    match InboxCli::try_parse_from(["cpd", "list"]).unwrap().command {
        crate::inbox::cmd::Command::List(args) => args,
        _ => unreachable!("list"),
    }
}

#[test]
fn snapshot_panes_equal_the_list_readers() {
    let mut rig = rig();
    let env = &mut rig.env;
    let expect = json!({
        "context": printed(crate::context::list::list_in(env)),
        "plan": printed(crate::plan::list::list_in(env, plan_list_args())),
        "session": printed(crate::session::row::list_in(env, session_list_args())),
        "inbox": printed(crate::inbox::cmd::list_in(env, inbox_list_args())),
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
    let expect = printed(crate::plan::show::show_in(
        &mut rig.env,
        plan_show_args("pay"),
    ));
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

#[test]
fn serve_reads_its_own_env_not_process_cpd_home() {
    let mut rig = rig();
    let frames = run_quiet(&mut rig, vec![]);
    assert_eq!(frames[1]["t"], "snapshot");
    assert_eq!(frames[1]["panes"]["plan"]["plans"][0]["id"], "pay");
}

#[test]
fn plan_show_reads_the_serving_env_not_process_cpd_home() {
    let mut rig = rig();
    let input = vec![
        line(json!({"t": "plan_show", "id": "r1", "plan": "pay"})),
        line(json!({"t": "plan_show", "id": "r2", "plan": "no-such-plan"})),
        line(json!({"t": "refresh"})),
    ];
    let frames = run_quiet(&mut rig, input);
    assert_eq!(
        kinds(&frames),
        ["hello", "snapshot", "plan_show", "error", "snapshot"]
    );
    assert_eq!(frames[2]["value"]["plan"]["id"], "pay");
    assert_eq!(frames[3]["id"], "r2");
    assert_eq!(frames[3]["error"]["code"], "plan_not_found");
}

// ---- Restart in place on a binary upgrade (decision pro-d450) ----

/// Rig `ports()` with the `FakeSpawn` kept, so a test sets `self_exe` and reads the launches.
fn rig_with_spawn() -> (Rig, Arc<FakeSpawn>) {
    let mut rig = rig();
    let clock = ManualClock::new();
    let spawn = Arc::new(FakeSpawn::new());
    rig.env.ports = Ports {
        clock: Arc::new(clock.clone()),
        runner: Arc::new(FakeRunner::new(clock)),
        spawn: spawn.clone(),
        ..Ports::real()
    };
    (rig, spawn)
}

/// An installed cpd (`<dir>/cpd`) that a new install replaces by a rename, so the path holds another inode.
struct Install {
    _dir: TempDir,
    path: std::path::PathBuf,
}

impl Install {
    fn new() -> Install {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cpd");
        fs::write(&path, "v1").unwrap();
        Install { _dir: dir, path }
    }

    fn upgrade(&self) {
        let next = self.path.with_file_name("cpd.new");
        fs::write(&next, "v2").unwrap();
        fs::rename(&next, &self.path).unwrap();
    }
}

/// What a newer cpd does to the store: a migration this binary lacks (`breaking`), recorded under `cpd_txn()`.
fn newer_cpd_migrates(home: &CpdHome) {
    use rusqlite::functions::FunctionFlags;
    let conn = rusqlite::Connection::open(home.db()).unwrap();
    let txn: i64 = conn
        .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
        .unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn))
        .unwrap();
    conn.execute_batch("BEGIN; CREATE TABLE newer_things (id INTEGER PRIMARY KEY) STRICT;")
        .unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
         VALUES ('29991231T2359-newer', 'breaking', lower(hex(zeroblob(32))), (SELECT max(seq) + 1 FROM schema_migrations), ?1, 0)",
        rusqlite::params![txn],
    )
    .unwrap();
    conn.execute_batch("COMMIT;").unwrap();
}

/// At its first poll: upgrades the binary (when `install`) and migrates the store (when `migrate`); reports a change
/// when it migrated, as the store's `data_version` does.
struct Upgrade<'a> {
    home: CpdHome,
    install: Option<&'a Install>,
    migrate: bool,
    polls: usize,
}

impl ChangeSource for Upgrade<'_> {
    fn poll(&mut self) -> Option<i64> {
        self.polls += 1;
        if self.polls > 1 {
            return None;
        }
        if let Some(install) = self.install {
            install.upgrade();
        }
        if self.migrate {
            newer_cpd_migrates(&self.home);
            return Some(1);
        }
        None
    }
}

/// Serves `n` ticks; returns the frames and whether serve returned an error.
fn serve_ticks(rig: &mut Rig, changes: &mut dyn ChangeSource, n: usize) -> (Vec<Value>, bool) {
    let mut out = Vec::new();
    let result = serve(&mut rig.env, &mut ticks(n).into_iter(), &mut out, changes);
    let frames = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (frames, result.is_err())
}

fn assert_one_exec_of(spawn: &FakeSpawn, install: &Install) {
    let launches = spawn.launches();
    assert_eq!(launches.len(), 1, "one exec: {launches:?}");
    assert_eq!(launches[0].program, install.path.display().to_string());
    assert_eq!(launches[0].args, ["cockpit", "serve"]);
}

#[test]
fn cockpit_serve_execs_replaced_binary_on_tick_and_writes_no_error_frame() {
    let (mut rig, spawn) = rig_with_spawn();
    let install = Install::new();
    spawn.set_self_exe(&install.path);
    let mut changes = Upgrade {
        home: rig.home.clone(),
        install: Some(&install),
        migrate: false,
        polls: 0,
    };
    let (frames, _) = serve_ticks(&mut rig, &mut changes, 4);
    assert_one_exec_of(&spawn, &install);
    assert!(!kinds(&frames).contains(&"error"), "{frames:?}");
}

#[test]
fn cockpit_serve_execs_replaced_binary_on_store_too_new_and_writes_no_error_frame() {
    let (mut rig, spawn) = rig_with_spawn();
    let install = Install::new();
    spawn.set_self_exe(&install.path);
    let mut changes = Upgrade {
        home: rig.home.clone(),
        install: Some(&install),
        migrate: true,
        polls: 0,
    };
    let (frames, _) = serve_ticks(&mut rig, &mut changes, 6);
    assert_one_exec_of(&spawn, &install);
    assert!(!kinds(&frames).contains(&"error"), "{frames:?}");
}

#[test]
fn cockpit_serve_store_too_new_with_unchanged_binary_still_emits_error_frame() {
    let (mut rig, spawn) = rig_with_spawn();
    let install = Install::new();
    spawn.set_self_exe(&install.path);
    let mut changes = Upgrade {
        home: rig.home.clone(),
        install: None,
        migrate: true,
        polls: 0,
    };
    let (frames, failed) = serve_ticks(&mut rig, &mut changes, 6);
    assert!(failed);
    let last = frames.last().unwrap();
    assert_eq!(last["t"], "error");
    assert_eq!(last["id"], Value::Null);
    assert_eq!(last["error"]["code"], "store_too_new");
    assert!(spawn.launches().is_empty());
}
