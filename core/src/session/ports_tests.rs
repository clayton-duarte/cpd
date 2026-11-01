//! Red tests for `session run`'s loops on virtual time and `FakeSpawn` (Plan tests-mock, Job `ports2-drv-tests`): the wait for a
//! fork's source, the patience with a busy store, `PiProc` (adopt, `try_wait`, `signal`), pi's `SIGTERM` then `SIGKILL`, the
//! adapter watchdog, whether a reload is due, and the reload's `exec` recorded by the `FakeSpawn`. Time is the `ManualClock`
//! (`advance`, `naps`, `elapsed`) and every pid is the `FakeSpawn`'s: no test sleeps, reads the wall clock or starts a
//! process. They are red until `ports2-drv-impl` threads `Ports` through the `_with` forms they call. The server-wait loop of
//! `host/tmux.rs` (`kill_server`, 10 s of 50 ms naps) already runs on the clock: `host::tmux_adapter_tests` pins it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nix::unistd::Pid as NixPid;

use super::fork_wait::{quiet_since_with, wait_for_source_with};
use super::reload::{adapter_missing_with, reload_due_with, reload_with, stop_pi_with};
use super::run::{PiProc, patiently_with};
use crate::clock::{Clock, ManualClock};
use crate::driver::ports_tests::{HookClock, Stubborn, modified_at, ports_with, process};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::exe::Exe;
use crate::host::pi::PiDirs;
use crate::host::spawn::{Launch, Pid, Signal, Spawn};
use crate::ids::SessionId;
use crate::ports::{Fakes, Ports};
use crate::store::Store;

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);
const START: &str = "2026-01-01T00:00:00Z";
const DONE: &str = r#"{"type":"message","message":{"role":"assistant","stopReason":"stop"}}"#;

fn id() -> SessionId {
    SessionId::parse("lead1").unwrap()
}

fn busy() -> CpdError {
    CpdError::new(ErrorCode::StoreBusy, "The store is busy.")
}

// ---- patiently -------------------------------------------------------------------------------------------------------

#[test]
fn a_busy_store_is_waited_out_with_naps_that_start_at_half_a_second_and_double() {
    let clock = ManualClock::new();
    let mut tries = 0;
    let got = patiently_with(&clock, "reading the Session", || {
        tries += 1;
        if tries < 4 { Err(busy()) } else { Ok(tries) }
    });
    assert_eq!(got.unwrap(), 4);
    assert_eq!(
        clock.naps(),
        [Duration::from_millis(500), SECOND, Duration::from_secs(2)]
    );
}

#[test]
fn the_waits_for_a_busy_store_stop_growing_at_five_seconds() {
    let clock = ManualClock::new();
    let mut tries = 0;
    patiently_with(&clock, "reading the Session", || {
        tries += 1;
        if tries < 8 { Err(busy()) } else { Ok(()) }
    })
    .unwrap();
    let five = Duration::from_secs(5);
    assert_eq!(
        clock.naps(),
        [
            Duration::from_millis(500),
            SECOND,
            Duration::from_secs(2),
            Duration::from_secs(4),
            five,
            five,
            five
        ]
    );
}

#[test]
fn a_lock_timeout_is_waited_out_like_a_busy_store() {
    let clock = ManualClock::new();
    let mut tries = 0;
    patiently_with(&clock, "opening the store", || {
        tries += 1;
        if tries < 2 {
            Err(CpdError::new(
                ErrorCode::LockTimeout,
                "The setup lock is held.",
            ))
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(clock.naps(), [Duration::from_millis(500)]);
}

#[test]
fn an_error_that_is_not_transient_is_returned_at_once_with_no_nap() {
    let clock = ManualClock::new();
    let got: Result<(), CpdError> = patiently_with(&clock, "reading the Session", || {
        Err(CpdError::new(ErrorCode::SessionNotFound, "No Session."))
    });
    assert_eq!(got.unwrap_err().code(), ErrorCode::SessionNotFound);
    assert!(clock.naps().is_empty());
}

// ---- PiProc: adopt, try_wait, signal ---------------------------------------------------------------------------------

fn adopted(spawn: &dyn Spawn, pid: Pid) -> PiProc {
    let _ = spawn;
    PiProc::adopt(NixPid::from_raw(pid.0 as i32))
}

#[test]
fn an_adopted_pi_that_runs_is_not_waited_for_yet() {
    let fakes = Fakes::new();
    let pid = process(&fakes.spawn);
    let mut pi = adopted(&*fakes.spawn, pid);
    assert_eq!(pi.id(), pid.0 as i32);
    assert!(pi.try_wait_with(&*fakes.spawn).unwrap().is_none());
}

#[test]
fn an_adopted_pi_that_exited_reports_its_status_and_keeps_reporting_it() {
    let fakes = Fakes::new();
    let pid = process(&fakes.spawn);
    let mut pi = adopted(&*fakes.spawn, pid);
    fakes.spawn.exit(pid, 3);
    assert_eq!(
        pi.try_wait_with(&*fakes.spawn)
            .unwrap()
            .and_then(|s| s.code()),
        Some(3)
    );
    assert_eq!(
        pi.try_wait_with(&*fakes.spawn)
            .unwrap()
            .and_then(|s| s.code()),
        Some(3),
        "waited for once, reported again"
    );
}

#[test]
fn an_adopted_pid_the_spawn_does_not_know_is_an_error() {
    let fakes = Fakes::new();
    let mut pi = PiProc::adopt(NixPid::from_raw(4_000_000));
    assert!(
        pi.try_wait_with(&*fakes.spawn).is_err(),
        "a pid that is not a child fails at the first try_wait"
    );
}

#[test]
fn a_signal_goes_to_the_spawn_while_pi_was_not_waited_for() {
    let fakes = Fakes::new();
    let pid = process(&fakes.spawn);
    let pi = adopted(&*fakes.spawn, pid);
    pi.signal_with(&*fakes.spawn, Signal::Term);
    assert_eq!(fakes.spawn.signals(), [(pid, Signal::Term)]);
}

#[test]
fn no_signal_goes_to_a_pi_that_was_waited_for() {
    let fakes = Fakes::new();
    let pid = process(&fakes.spawn);
    let mut pi = adopted(&*fakes.spawn, pid);
    fakes.spawn.exit(pid, 0);
    assert!(pi.try_wait_with(&*fakes.spawn).unwrap().is_some());
    pi.signal_with(&*fakes.spawn, Signal::Kill);
    assert!(
        fakes.spawn.signals().is_empty(),
        "the pid is no longer pi's"
    );
}

// ---- stop_pi: SIGTERM, the grace, SIGKILL -----------------------------------------------------------------------------

#[test]
fn a_pi_that_exits_on_term_is_stopped_with_no_nap() {
    let fakes = Fakes::new();
    let pid = process(&fakes.spawn);
    let mut pi = adopted(&*fakes.spawn, pid);
    stop_pi_with(&fakes.ports, &mut pi, Duration::from_secs(5));
    assert_eq!(fakes.spawn.signals(), [(pid, Signal::Term)]);
    assert!(fakes.clock.naps().is_empty());
}

#[test]
fn a_pi_that_ignores_term_gets_kill_after_the_grace_in_fifty_millisecond_naps() {
    let fakes = Fakes::new();
    let stubborn = Arc::new(Stubborn {
        inner: fakes.spawn.clone(),
        ignored: Default::default(),
    });
    let ports = ports_with(&fakes, fakes.ports.clock.clone(), stubborn.clone());
    let pid = process(&fakes.spawn);
    let mut pi = adopted(&*fakes.spawn, pid);
    stop_pi_with(&ports, &mut pi, Duration::from_secs(5));
    assert_eq!(*stubborn.ignored.lock().unwrap(), [pid], "SIGTERM first");
    assert_eq!(
        fakes.spawn.signals(),
        [(pid, Signal::Kill)],
        "SIGKILL once the grace is over"
    );
    assert_eq!(
        fakes.clock.naps(),
        [Duration::from_millis(50); 100],
        "a look every 50 ms for the 5 s"
    );
    assert_eq!(fakes.clock.elapsed(), Duration::from_secs(5));
}

#[test]
fn a_pi_that_exits_during_the_grace_is_not_killed() {
    let fakes = Fakes::new();
    let stubborn = Arc::new(Stubborn {
        inner: fakes.spawn.clone(),
        ignored: Default::default(),
    });
    let pid = process(&fakes.spawn);
    let spawn = fakes.spawn.clone();
    let clock: Arc<dyn Clock> = Arc::new(HookClock::new(&fakes.clock, move |n| {
        if n == 3 {
            spawn.exit(pid, 0);
        }
    }));
    let ports = ports_with(&fakes, clock, stubborn);
    let mut pi = adopted(&*fakes.spawn, pid);
    stop_pi_with(&ports, &mut pi, Duration::from_secs(5));
    assert!(fakes.spawn.signals().is_empty(), "no SIGKILL");
    assert_eq!(fakes.clock.naps(), [Duration::from_millis(50); 3]);
}

// ---- reload's exec ---------------------------------------------------------------------------------------------------

/// An `Exe` for a real file in `dir` (only its identity is read) and what its `exec` should launch.
fn installed(dir: &tempfile::TempDir) -> (Exe, String) {
    let path: PathBuf = dir.path().join("cpd");
    std::fs::write(&path, "#!/bin/sh\n").unwrap();
    let launch = path.display().to_string();
    (Exe::at(path).unwrap(), launch)
}

#[test]
fn exec_asks_the_spawn_to_replace_the_process_with_the_installed_cpd_and_returns_its_failure() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let (exe, program) = installed(&dir);
    let err = exe.exec_with(&*fakes.spawn, ["session", "run", "lead1"]);
    assert_eq!(err.code(), ErrorCode::SpawnFailed);
    let args = ["session", "run", "lead1"].map(str::to_owned).to_vec();
    assert_eq!(
        fakes.spawn.launches(),
        [Launch {
            program,
            args,
            ..Launch::default()
        }]
    );
}

#[test]
fn a_reload_without_pi_execs_session_run_for_the_id() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let (exe, program) = installed(&dir);
    let err = reload_with(&fakes.ports, &exe, &id(), None);
    assert_eq!(err.code(), ErrorCode::SpawnFailed);
    let args = ["session", "run", "lead1"].map(str::to_owned).to_vec();
    assert_eq!(
        fakes.spawn.launches(),
        [Launch {
            program,
            args,
            ..Launch::default()
        }]
    );
}

#[test]
fn a_reload_that_keeps_pi_running_execs_with_adopt_and_the_pid() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let (exe, program) = installed(&dir);
    let pid = process(&fakes.spawn);
    let pi = adopted(&*fakes.spawn, pid);
    reload_with(&fakes.ports, &exe, &id(), Some((&pi, false)));
    let args = ["session", "run", "lead1", "--adopt", &pid.0.to_string()]
        .map(str::to_owned)
        .to_vec();
    assert_eq!(
        fakes.spawn.launches().last(),
        Some(&Launch {
            program,
            args,
            ..Launch::default()
        })
    );
}

#[test]
fn a_reload_after_a_new_pi_install_adds_pi_stale() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let (exe, program) = installed(&dir);
    let pid = process(&fakes.spawn);
    let pi = adopted(&*fakes.spawn, pid);
    reload_with(&fakes.ports, &exe, &id(), Some((&pi, true)));
    let args = [
        "session",
        "run",
        "lead1",
        "--adopt",
        &pid.0.to_string(),
        "--pi-stale",
    ]
    .map(str::to_owned)
    .to_vec();
    assert_eq!(
        fakes.spawn.launches().last(),
        Some(&Launch {
            program,
            args,
            ..Launch::default()
        })
    );
}

// ---- Sessions on the clock: the store, the file, the state -------------------------------------------------------------

/// A temporary `$CPD_HOME` with a store holding Session `lead1` (a Manager, `state` since the clock's start, its file
/// recorded when `with_file`) and an `Env` on `ports`.
struct World {
    _dir: tempfile::TempDir,
    env: Env,
    dirs: PiDirs,
    file: PathBuf,
}

fn world(ports: &Ports, state: &str, place: Option<&str>, finished: bool) -> World {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"test\"\n").unwrap();
    let file = dir.path().join("lead1.jsonl");
    std::fs::write(
        &file,
        if finished {
            DONE
        } else {
            r#"{"type":"message","message":{"role":"assistant","stopReason":"toolUse"}}"#
        },
    )
    .unwrap();
    modified_at(&file, ports.clock.utc());
    let mut env = Env::open_home_with(ports, home).unwrap();
    let path = file.display().to_string();
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, name, file, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'manager', 'ctx', 'Lead', '{path}', '/', 'O3', '{state}', '{START}', '{START}');"
            ))?;
            if let Some(place) = place {
                tx.execute_batch(&format!(
                    "INSERT INTO events (txn, at, kind, context, session, text, data)
                     VALUES (cpd_txn(), '{START}', 'session-moved', 'ctx', 'lead1', 'x', '{{\"from\":\"none\",\"place\":\"{place}\"}}');"
                ))?;
            }
            Ok(())
        })
        .unwrap();
    let dirs = PiDirs {
        home: dir.path().to_owned(),
        agent_dir: dir.path().join("agent"),
        session_dir: Some(dir.path().join("sessions")),
    };
    World {
        _dir: dir,
        env,
        dirs,
        file,
    }
}

fn errors(w: &mut World) -> i64 {
    w.env
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM events WHERE kind = 'error'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn a_file_is_quiet_once_the_virtual_time_since_its_write_reaches_the_limit() {
    let fakes = Fakes::new();
    let w = world(&fakes.ports, "idle", None, true);
    fakes.clock.advance(Duration::from_secs(4));
    assert!(!quiet_since_with(
        &*fakes.clock,
        &w.file,
        Duration::from_secs(5)
    ));
    fakes.clock.advance(SECOND);
    assert!(quiet_since_with(
        &*fakes.clock,
        &w.file,
        Duration::from_secs(5)
    ));
}

// ---- the wait for a fork's source -------------------------------------------------------------------------------------

#[test]
fn a_source_that_is_idle_with_a_finished_quiet_file_is_forked_at_the_first_look() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "idle", None, true);
    fakes.clock.advance(Duration::from_secs(5));
    let file = wait_for_source_with(&fakes.ports, &mut w.env, &w.dirs, &id(), "Fork").unwrap();
    assert_eq!(file, Some(w.file.clone()));
    assert!(fakes.clock.naps().is_empty());
}

#[test]
fn a_source_whose_file_was_just_written_is_waited_for_in_two_second_naps_until_it_is_quiet() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "idle", None, true);
    // `fork_quiet_seconds` is 5: quiet at the third 2 s nap (6 s), not at the second (4 s).
    let file = wait_for_source_with(&fakes.ports, &mut w.env, &w.dirs, &id(), "Fork").unwrap();
    assert_eq!(file, Some(w.file.clone()));
    assert_eq!(fakes.clock.naps(), [Duration::from_secs(2); 3]);
}

#[test]
fn a_source_that_is_working_is_waited_for_until_it_is_idle() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "working", None, true);
    let home = w.env.home.clone();
    let clock: Arc<dyn Clock> = Arc::new(HookClock::new(&fakes.clock, move |n| {
        if n == 4 {
            Store::open(&home, 5000)
                .unwrap()
                .write("test", |tx| {
                    tx.execute("UPDATE sessions SET state = 'idle' WHERE id = 'lead1'", [])?;
                    Ok(())
                })
                .unwrap();
        }
    }));
    let ports = ports_with(&fakes, clock, fakes.ports.spawn.clone());
    w.env.ports = ports.clone();
    let file = wait_for_source_with(&ports, &mut w.env, &w.dirs, &id(), "Fork").unwrap();
    assert_eq!(file, Some(w.file.clone()));
    assert_eq!(
        fakes.clock.naps(),
        [Duration::from_secs(2); 4],
        "no time bound while it works"
    );
}

// ---- the adapter watchdog --------------------------------------------------------------------------------------------

#[test]
fn a_session_stopped_for_more_than_a_minute_of_virtual_time_with_pi_running_is_adapter_missing() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "stopped", None, true);
    fakes.clock.advance(MINUTE + SECOND);
    assert!(adapter_missing_with(&fakes.ports, &mut w.env, &id(), 10 * MINUTE).unwrap());
    assert_eq!(errors(&mut w), 1, "the one error Event");
}

#[test]
fn a_session_stopped_for_exactly_a_minute_is_not_adapter_missing() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "stopped", None, true);
    fakes.clock.advance(MINUTE);
    assert!(!adapter_missing_with(&fakes.ports, &mut w.env, &id(), 10 * MINUTE).unwrap());
    assert_eq!(errors(&mut w), 0);
}

#[test]
fn a_stop_the_wrapper_set_before_pi_ran_does_not_count() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "stopped", None, true);
    fakes.clock.advance(60 * MINUTE);
    assert!(
        !adapter_missing_with(&fakes.ports, &mut w.env, &id(), 30 * SECOND).unwrap(),
        "pi ran for 30 s only"
    );
    assert_eq!(errors(&mut w), 0);
}

#[test]
fn a_session_that_is_not_stopped_is_never_adapter_missing() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "working", None, true);
    fakes.clock.advance(60 * MINUTE);
    assert!(!adapter_missing_with(&fakes.ports, &mut w.env, &id(), 60 * MINUTE).unwrap());
}

// ---- whether a reload is due -----------------------------------------------------------------------------------------

#[test]
fn a_parked_idle_lead_reloads_once_its_file_has_been_quiet_for_fork_quiet_seconds_of_virtual_time()
{
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "idle", Some("parked"), true);
    fakes.clock.advance(Duration::from_secs(4));
    assert!(!reload_due_with(&fakes.ports, &mut w.env, &w.dirs, &id()).unwrap());
    fakes.clock.advance(SECOND);
    assert!(reload_due_with(&fakes.ports, &mut w.env, &w.dirs, &id()).unwrap());
}

#[test]
fn the_lead_in_the_chat_slot_reloads_after_ten_virtual_minutes_idle() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "idle", Some("chat"), true);
    fakes.clock.advance(10 * MINUTE - SECOND);
    assert!(!reload_due_with(&fakes.ports, &mut w.env, &w.dirs, &id()).unwrap());
    fakes.clock.advance(SECOND);
    assert!(reload_due_with(&fakes.ports, &mut w.env, &w.dirs, &id()).unwrap());
}

#[test]
fn an_idle_lead_in_the_middle_of_a_turn_never_reloads() {
    let fakes = Fakes::new();
    let mut w = world(&fakes.ports, "idle", Some("parked"), false);
    fakes.clock.advance(60 * MINUTE);
    assert!(!reload_due_with(&fakes.ports, &mut w.env, &w.dirs, &id()).unwrap());
}
