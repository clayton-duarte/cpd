//! The durable host supervisor (contract §6.19, P1.10): the engine starts one host per live durable Session,
//! restarts a dead one within a bound and stops it when the Session sleeps or is archived. On the fake ports:
//! time only through `clock.advance`, no process, no socket, no tmux.

use std::sync::Arc;
use std::time::Duration;

use cpd_core::clock::ManualClock;
use cpd_core::error::ErrorCode;
use cpd_core::host::runner::FakeRunner;
use cpd_core::host::spawn::{FakeSpawn, Pid, Signal, Spawn};
use cpd_core::ids::SessionId;
use cpd_core::ports::Ports;

use super::testkit::{Fx, ago_seconds};
use super::{archive_leads, hosts, sleep};

/// An engine on a temporary home whose fakes are a `ManualClock`, a `FakeSpawn` and a `FakeRunner`.
fn live_fx() -> (Fx, Arc<FakeSpawn>, Arc<ManualClock>) {
    let mut fx = Fx::new("");
    let spawn = Arc::new(FakeSpawn::new());
    let clock = Arc::new(ManualClock::new());
    fx.engine.ports = Ports {
        clock: clock.clone(),
        runner: Arc::new(FakeRunner::new(ManualClock::new())),
        spawn: spawn.clone(),
        ..Ports::real()
    };
    (fx, spawn, clock)
}

/// A Lead `id` in Context `ctx` with the stored `runner`, `cwd` and optional `plan`, `stopped` and never asleep.
fn add_lead(fx: &mut Fx, id: &str, runner: &str, cwd: &str, plan: Option<&str>) {
    let plan = plan
        .map(|p| format!("'{p}'"))
        .unwrap_or_else(|| "NULL".to_owned());
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
           VALUES ('{id}', 'lead', 'ctx', {plan}, '{id}', '{cwd}', 'O3', 'stopped', 't', 't', '{runner}');"
    ));
}

/// The one live host pid `spawn` holds for `id`.
fn one_host(spawn: &FakeSpawn, id: &str) -> Pid {
    let mut pids = spawn.with_env("CPD_SESSION", id);
    assert_eq!(pids.len(), 1, "one host for {id}: {pids:?}");
    pids.pop().unwrap()
}

#[test]
fn live_durable_session_starts_the_host() {
    let (mut fx, spawn, _clock) = live_fx();
    let cwd = fx.engine.home.root().join("lead-cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    add_lead(
        &mut fx,
        "lead1",
        "durable",
        &cwd.display().to_string(),
        None,
    );

    hosts::run(&mut fx.engine);

    let launches = spawn.launches();
    assert_eq!(launches.len(), 1, "one host: {launches:?}");
    let launch = &launches[0];
    assert_eq!(
        launch.program,
        cpd_core::config::catalog::node().display().to_string()
    );
    assert!(
        launch
            .args
            .first()
            .is_some_and(|a| a.ends_with("durable-host/main.ts")),
        "{:?}",
        launch.args
    );
    let session = launch
        .args
        .iter()
        .position(|a| a == "--session")
        .expect("--session: {:?}");
    assert_eq!(
        launch.args.get(session + 1).map(String::as_str),
        Some("lead1")
    );
    let env = |key: &str| {
        launch
            .env_set
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        env("CPD_HOME"),
        Some(fx.engine.home.root().display().to_string())
    );
    assert_eq!(env("CPD_SESSION"), Some("lead1".to_owned()));
    let path = env("PATH").expect("PATH is set");
    assert!(
        path.starts_with(&format!("{}:", fx.engine.home.root().join("bin").display())),
        "PATH starts with $CPD_HOME/bin: {path}"
    );
    assert_eq!(launch.cwd.as_ref(), Some(&cwd), "cwd is the Session's cwd");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn no_host_for_a_pi_session() {
    let (mut fx, spawn, _clock) = live_fx();
    add_lead(&mut fx, "pi1", "pi", "/", None);
    add_lead(&mut fx, "dlead", "durable", "/", None);

    hosts::run(&mut fx.engine);

    let launches = spawn.launches();
    assert_eq!(
        launches.len(),
        1,
        "only the durable Session starts a host: {launches:?}"
    );
    assert!(
        launches[0]
            .env_set
            .iter()
            .any(|(k, v)| k == "CPD_SESSION" && v == "dlead"),
        "the pi Session launches nothing: {launches:?}"
    );
}

#[test]
fn one_host_per_live_durable_session() {
    let (mut fx, spawn, _clock) = live_fx();
    add_lead(&mut fx, "a", "durable", "/", None);
    add_lead(&mut fx, "b", "durable", "/", None);

    hosts::run(&mut fx.engine);
    let first = spawn.launches();
    assert_eq!(first.len(), 2, "one host per Session: {first:?}");
    let sessions: Vec<&str> = first
        .iter()
        .filter_map(|l| {
            l.env_set
                .iter()
                .find(|(k, _)| k == "CPD_SESSION")
                .map(|(_, v)| v.as_str())
        })
        .collect();
    assert!(
        sessions.contains(&"a") && sessions.contains(&"b"),
        "{sessions:?}"
    );

    hosts::run(&mut fx.engine);
    assert_eq!(
        spawn.launches().len(),
        2,
        "never a second host for a live Session: {:?}",
        spawn.launches()
    );
}

#[test]
fn restarts_a_dead_host_while_still_live() {
    let (mut fx, spawn, _clock) = live_fx();
    add_lead(&mut fx, "lead1", "durable", "/", None);
    hosts::run(&mut fx.engine);
    assert_eq!(spawn.launches().len(), 1);

    spawn.exit(one_host(&spawn, "lead1"), 1);
    hosts::run(&mut fx.engine);

    assert_eq!(
        spawn.launches().len(),
        2,
        "the host is relaunched: {:?}",
        spawn.launches()
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn restart_bound_in_the_window_stops_the_session() {
    let (mut fx, spawn, _clock) = live_fx();
    add_lead(&mut fx, "lead1", "durable", "/", None);
    hosts::run(&mut fx.engine);
    for _ in 0..3 {
        spawn.exit(one_host(&spawn, "lead1"), 1);
        hosts::run(&mut fx.engine);
    }
    assert_eq!(
        spawn.launches().len(),
        4,
        "the initial start and three restarts: {:?}",
        spawn.launches()
    );

    spawn.exit(one_host(&spawn, "lead1"), 1);
    hosts::run(&mut fx.engine);

    assert_eq!(
        spawn.launches().len(),
        4,
        "a fourth restart inside the window is refused: {:?}",
        spawn.launches()
    );
    let (state, label): (String, Option<String>) = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, state_label FROM sessions WHERE id = 'lead1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(state, "stopped");
    assert_eq!(label.as_deref(), Some("host crash loop"));
    assert_eq!(
        fx.count("session-state"),
        1,
        "{:?}",
        fx.events("session-state")
    );
}

#[test]
fn restart_window_resets_after_ten_minutes() {
    let (mut fx, spawn, clock) = live_fx();
    add_lead(&mut fx, "lead1", "durable", "/", None);
    hosts::run(&mut fx.engine);
    for _ in 0..3 {
        spawn.exit(one_host(&spawn, "lead1"), 1);
        hosts::run(&mut fx.engine);
    }
    assert_eq!(spawn.launches().len(), 4);

    spawn.exit(one_host(&spawn, "lead1"), 1);
    clock.advance(Duration::from_secs(10 * 60));
    hosts::run(&mut fx.engine);

    assert_eq!(
        spawn.launches().len(),
        5,
        "a death after the window starts a fresh bound: {:?}",
        spawn.launches()
    );
    let label: Option<String> = fx.query("SELECT state_label FROM sessions WHERE id = 'lead1'");
    assert_eq!(
        label, None,
        "the Session did not reach the crash-loop bound"
    );
}

#[test]
fn stops_the_host_when_the_session_sleeps_or_is_archived() {
    // Sleep: a durable Lead of a finished Plan, due, starts a host; `sleep::run` stops it.
    let (mut fx, spawn, _clock) = live_fx();
    let old = "2020-01-01T00:00:00.000Z";
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{old}', '{old}', 'durable');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'lead1', '{old}', '{old}', '{old}');"
    ));
    hosts::run(&mut fx.engine);
    assert_eq!(spawn.launches().len(), 1, "the host starts");

    sleep::run(&mut fx.engine);
    assert!(
        spawn.signals().iter().any(|(_, s)| *s == Signal::Term),
        "the running host is stopped: {:?}",
        spawn.signals()
    );
    let launches = spawn.launches().len();
    hosts::run(&mut fx.engine);
    assert_eq!(
        spawn.launches().len(),
        launches,
        "an asleep Session is never started"
    );

    // Archive: a durable Lead asleep past the threshold; `archive_leads::run` stops the host it archives.
    let (mut fx, spawn, _clock) = live_fx();
    let asleep = ago_seconds(2 * 24 * 3600);
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, asleep_at, runner)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', '{old}', '{old}', '{asleep}', 'durable');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'lead1', '{old}', '{old}', '{old}');"
    ));
    let id = SessionId::parse("lead1").unwrap();
    let (_pid, _lock) = hosts::start(&mut fx.engine, &id).expect("the host starts");
    archive_leads::run(&mut fx.engine);
    assert!(
        spawn.signals().iter().any(|(_, s)| *s == Signal::Term),
        "the archived Session stops its host: {:?}",
        spawn.signals()
    );
}

#[test]
fn the_flock_file_exists_after_the_start() {
    let (mut fx, _spawn, _clock) = live_fx();
    add_lead(&mut fx, "lead1", "durable", "/", None);
    let id = SessionId::parse("lead1").unwrap();

    let (_pid, lock) = hosts::start(&mut fx.engine, &id).expect("the first start takes the lock");
    let path = fx.engine.home.run_dir().join("host").join("lead1.lock");
    assert!(path.exists(), "{} exists after the start", path.display());

    let err = hosts::start(&mut fx.engine, &id).unwrap_err();
    assert_eq!(err.code(), ErrorCode::HostRunning, "{}", err.message());
    drop(lock);
}
