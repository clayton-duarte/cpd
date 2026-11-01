//! When `session run` restarts pi (contract §6.7 "Reload on install",
//! "Adapter watchdog"; commands.md §10.8): whether a new install may be
//! exec'd, whether the Lead is at a safe point, and the adapter watchdog.

use std::time::Duration;

use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::Env;
use super::fork_wait::{quiet_since_with, turn_finished};
use super::row::{self, State};
use super::run::{PiProc, existing_file};
use super::state::scoped;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::host::exe::{Exe, Refusal};
use crate::host::limit::limit;
use crate::host::pi::PiDirs;
use crate::host::spawn::Signal as SpawnSignal;
use crate::ids::SessionId;
use crate::ports::Ports;

/// How often a reload looks whether pi exited after `SIGTERM`.
const STOP_POLL: Duration = Duration::from_millis(50);
/// How long the Lead in the chat slot must have been idle before a reload
/// restarts its pi without it being parked (hs-d49).
const CHAT_IDLE: Duration = Duration::from_secs(10 * 60);
/// How long a Session may stay `stopped` while its pi runs before the wrapper
/// restarts pi: the adapter is missing (contract §6.7 "Adapter watchdog", hs-d141).
/// Tests shorten it with `CPD_TEST_ADAPTER_WATCHDOG` ([`limit`]).
const ADAPTER_WATCHDOG: Duration = Duration::from_secs(60);

/// Whether the wrapper may `exec` the cpd at its path: nothing replaced it (a
/// reload for pi's sake), or what replaced it can open the store. A refused install is reported once per
/// installed file and the wrapper carries on as if nothing was installed (hs-d151).
pub(super) fn may_exec(exe: &Exe) -> bool {
    if !exe.replaced() {
        return true;
    }
    match exe.check_store() {
        Ok(()) => true,
        Err(Refusal::New(reason)) => {
            eprintln!("cpd: reload skipped: {} {reason}", exe.path().display());
            false
        }
        Err(Refusal::Known) => false,
    }
}

/// Sends pi `SIGTERM` and waits for it to exit; if it has not exited after
/// `grace`, sends `SIGKILL` and waits again (§6.7 "Reload on install"). `SIGTERM` and `SIGKILL` go to `ports.spawn`, the
/// 50 ms polls and the grace are naps and time of `ports.clock`.
pub(super) fn stop_pi_with(ports: &Ports, pi: &mut PiProc, grace: Duration) {
    let (clock, spawn) = (&*ports.clock, &*ports.spawn);
    pi.signal_with(spawn, SpawnSignal::Term);
    let deadline = clock.now() + grace;
    while clock.now() < deadline {
        if !matches!(pi.try_wait_with(spawn), Ok(None)) {
            return;
        }
        clock.sleep(STOP_POLL);
    }
    pi.signal_with(spawn, SpawnSignal::Kill);
    while matches!(pi.try_wait_with(spawn), Ok(None)) {
        clock.sleep(STOP_POLL);
    }
}

/// The adapter watchdog (contract §6.7 "Adapter watchdog"): whether the
/// Session has been `stopped` for more than [`ADAPTER_WATCHDOG`] while the pi
/// started `running` ago runs. If so, appends the one `error` Event, in the
/// transaction that re-checks the state, and returns true. `now` is `ports.clock.utc()`.
pub(super) fn adapter_missing_with(
    ports: &Ports,
    env: &mut Env,
    id: &SessionId,
    running: Duration,
) -> Result<bool, CpdError> {
    let row = env.store.read(|conn| row::load(conn, id))?;
    if row.state != State::Stopped {
        return Ok(false);
    }
    let since = OffsetDateTime::parse(&row.state_at, &Rfc3339).map_err(|e| {
        CpdError::bad_row(format!("{} has state_at {:?}: {e}", row.id, row.state_at))
    })?;
    // A `stopped` the wrapper set when the previous pi exited does not count while no pi ran.
    let stopped_for = Duration::try_from(ports.clock.utc() - since)
        .unwrap_or_default()
        .min(running);
    if stopped_for <= limit("ADAPTER_WATCHDOG", ADAPTER_WATCHDOG) {
        return Ok(false);
    }
    let seconds = stopped_for.as_secs();
    env.store.write("session run", |tx| {
        let now = row::load(tx, id)?;
        if now.state != State::Stopped || now.state_at != row.state_at {
            return Ok(false);
        }
        let message = format!(
            "Session {}: pi has run for {seconds} s with the adapter reporting stopped; the wrapper restarts pi.",
            row.name
        );
        let details = json!({"session": row.id, "state_since": row.state_at, "seconds": seconds});
        let event = NewEvent::error(message.clone(), "session run", ErrorCode::AdapterMissing.as_str(), &message, details);
        append(tx, scoped(event, &row))?;
        Ok(true)
    })
}

/// Replaces this process with the cpd at its path (§10.8), `exec` through `ports.spawn` (`Exe::exec_with`). With `adopt` (pi,
/// and whether its install already changed) pi keeps running and the new
/// process adopts it by pid; without, no pi runs and the new process starts
/// one. Returns only the error of failing to.
pub(super) fn reload_with(
    ports: &Ports,
    exe: &Exe,
    id: &SessionId,
    adopt: Option<(&PiProc, bool)>,
) -> CpdError {
    let mut args = vec!["session".to_owned(), "run".to_owned(), id.to_string()];
    if let Some((pi, stale)) = adopt {
        args.extend(["--adopt".to_owned(), pi.id().to_string()]);
        if stale {
            args.push("--pi-stale".to_owned());
        }
    }
    exe.exec_with(&*ports.spawn, args)
}

/// A new pi is installed and the Lead is at a safe point (§6.7 "Reload on
/// install"): reads the Session and its file, and decides with [`reload_now`]. The idle time and the quiet of the file are
/// read on `ports.clock`.
pub(super) fn reload_due_with(
    ports: &Ports,
    env: &mut Env,
    dirs: &PiDirs,
    id: &SessionId,
) -> Result<bool, CpdError> {
    let row = env.store.read(|conn| row::load(conn, id))?;
    // Cheap first: only an idle Lead can be at a safe point.
    if row.state != State::Idle {
        return Ok(false);
    }
    let Some(file) = existing_file(dirs, &row) else {
        return Ok(false);
    };
    let quiet = Duration::from_secs(u64::from(env.config.timeouts.fork_quiet_seconds));
    let since = OffsetDateTime::parse(&row.state_at, &Rfc3339).map_err(|e| {
        CpdError::bad_row(format!("{} has state_at {:?}: {e}", row.id, row.state_at))
    })?;
    let idle_for = Duration::try_from(ports.clock.utc() - since).unwrap_or_default();
    let finished = std::fs::read_to_string(&file).is_ok_and(|text| turn_finished(&text));
    Ok(reload_now(
        row.state,
        &row.place,
        idle_for,
        finished,
        quiet_since_with(&*ports.clock, &file, quiet),
    ))
}

/// Whether a Lead whose pi runs reloads it now (hs-d49): it is idle, its last
/// turn is finished and its session file is quiet (the rule of `session fork`),
/// and it is parked, or in the chat slot and has been idle for [`CHAT_IDLE`] (so
/// unsent typing is not lost). A `waiting`, `blocked` or `working` Lead never reloads.
fn reload_now(
    state: State,
    place: &str,
    idle_for: Duration,
    turn_finished: bool,
    file_quiet: bool,
) -> bool {
    state == State::Idle
        && turn_finished
        && file_quiet
        && (place != "chat" || idle_for >= CHAT_IDLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn a_parked_idle_lead_with_a_finished_quiet_turn_reloads_at_once() {
        assert!(reload_now(
            State::Idle,
            "parked",
            Duration::ZERO,
            true,
            true
        ));
        assert!(reload_now(State::Idle, "none", Duration::ZERO, true, true));
    }

    #[test]
    fn a_lead_that_is_not_idle_never_reloads() {
        for state in [
            State::Working,
            State::Waiting,
            State::Blocked,
            State::Stopped,
        ] {
            for place in ["parked", "chat", "none"] {
                assert!(
                    !reload_now(state, place, 60 * MINUTE, true, true),
                    "{state:?} in {place}"
                );
            }
        }
    }

    #[test]
    fn an_idle_lead_waits_for_a_finished_turn_and_a_quiet_file() {
        assert!(!reload_now(State::Idle, "parked", 60 * MINUTE, false, true));
        assert!(!reload_now(State::Idle, "parked", 60 * MINUTE, true, false));
        assert!(!reload_now(State::Idle, "chat", 60 * MINUTE, false, false));
    }

    #[test]
    fn the_lead_in_the_chat_slot_waits_ten_minutes() {
        assert!(!reload_now(State::Idle, "chat", Duration::ZERO, true, true));
        assert!(!reload_now(
            State::Idle,
            "chat",
            10 * MINUTE - Duration::from_secs(1),
            true,
            true
        ));
        assert!(reload_now(State::Idle, "chat", 10 * MINUTE, true, true));
        assert!(reload_now(State::Idle, "chat", 11 * MINUTE, true, true));
        assert!(
            !reload_now(State::Idle, "chat", 11 * MINUTE, false, true),
            "ten minutes do not skip the turn rule"
        );
    }
}
