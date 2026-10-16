//! `session run` (commands.md §10.8, contract §6.7): the program in every
//! Lead pane. It owns its terminal: it runs pi in the foreground, and when pi
//! exits it offers to resume it.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::moves::first_message;
use super::port;
use super::row::{self, Role, Row, State};
use super::{Env, RunArgs, bad_row, not_a_lead, state};
use crate::config::{Letter, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::host::exe::Exe;
use crate::host::pi::{self, LeadLaunch, LeadStart, PiDirs, Plannotator};
use crate::host::{PaneId, Tmux};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;

/// How often the fork waits for its source to be idle (§6.7 step 1).
const SOURCE_POLL: Duration = Duration::from_secs(2);
/// How often it looks for pi's session file while pi runs.
const FILE_POLL: Duration = Duration::from_millis(500);
/// How often a reload looks whether pi exited after `SIGTERM`.
const STOP_POLL: Duration = Duration::from_millis(50);
/// How long the Lead in the chat slot must have been idle before a reload
/// restarts its pi without it being parked (hs-d49).
const CHAT_IDLE: Duration = Duration::from_secs(10 * 60);
/// How long a reload waits for pi to exit after `SIGTERM` before it sends `SIGKILL`.
const PI_STOP: Duration = Duration::from_secs(10);
/// How long it waits for `new-window`'s caller to set `@cpd_session` on its pane.
const PANE_WAIT: Duration = Duration::from_secs(5);

pub(super) fn run(args: RunArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let row = env.store.read(|conn| row::load(conn, &args.id))?;
    if row.role != Role::Lead {
        return Err(not_a_lead(&row.id, row.role));
    }
    check_pane(&env.tmux(), &row.id)?;
    let exe = Exe::current()?;
    let dirs = PiDirs::from_env()?;
    // From here on this process owns the terminal (commands.md §1.2).
    loop {
        let row = env.store.read(|conn| row::load(conn, &args.id))?;
        // A Lead with a Plan gets the Plan's port; a grounded Lead gets none (§6.5).
        let port = match &row.plan {
            Some(plan) => match assign_port(&mut env, plan)? {
                Some(port) => Some(port),
                None => return Ok(Output::Owned),
            },
            None => None,
        };
        let start = start(&mut env, &dirs, &row)?;
        let candidates = pi::lead_candidates(&env.config.pi_adapter_dir()?, &env.config.lead_pi_packages);
        let extensions = pi::extensions_to_pass(&candidates, Path::new(&row.cwd), &dirs, &mut std::io::stderr());
        // `PLANNOTATOR_REMOTE` comes from cpd's environment; the port is the Plan's, whatever the environment holds.
        let plannotator = Plannotator { port: port.map(|p| p.to_string()), remote: Plannotator::from_env().remote };
        let launch = pi::lead(&LeadLaunch {
            home: &env.home,
            session: &row.id,
            plan: row.plan.as_ref(),
            extensions: &extensions,
            plannotator: &plannotator,
            start,
        });
        let mut child = Command::new(&launch.argv[0])
            .args(&launch.argv[1..])
            .envs(&launch.env)
            .current_dir(&row.cwd)
            .spawn()
            .map_err(|e| {
                CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start pi: {e}.")).with("argv", launch.argv.clone())
            })?;
        let mut recorded = row.file.is_some();
        loop {
            let exited = child.try_wait().map_err(|e| {
                CpdError::new(ErrorCode::SpawnFailed, format!("Cannot wait for pi: {e}.")).with("argv", launch.argv.clone())
            })?;
            if exited.is_some() {
                // A new install: restart before any store write (§6.7 "Reload on install").
                if exe.replaced() {
                    return Err(reload(&exe, &row.id));
                }
                if !recorded {
                    record_file(&mut env, &dirs, &row.id)?;
                }
                break;
            }
            if !recorded {
                recorded = record_file(&mut env, &dirs, &row.id)?;
            }
            if exe.replaced() && reload_due(&mut env, &dirs, &row.id)? {
                print!("  new cpd installed: restarting pi for {}…\n", row.name);
                let _ = std::io::stdout().flush();
                // The adapter reports `stopped` at `session_shutdown`; this process does not.
                stop_pi(&mut child, PI_STOP);
                return Err(reload(&exe, &row.id));
            }
            std::thread::sleep(FILE_POLL);
        }
        // The adapter reports `stopped` at shutdown; report it here when it did not.
        if let Err(e) = state::set(&mut env.store, "session run", &row.id, State::Stopped, None) {
            eprintln!("cpd: cannot report Session {} stopped: {}: {}", row.id, e.code(), e.message());
        }
        let name = env.store.read(|conn| row::load(conn, &args.id))?.name;
        print!("\n  pi exited. Press Enter to resume {name} here.\n");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => return Ok(Output::Owned),
            Ok(_) => {}
        }
        if exe.replaced() {
            return Err(reload(&exe, &row.id));
        }
    }
}

/// Sends pi `SIGTERM` and waits for it to exit; if it has not exited after
/// `grace`, sends `SIGKILL` and waits again (§6.7 "Reload on install").
fn stop_pi(child: &mut std::process::Child, grace: Duration) {
    let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM);
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(STOP_POLL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Replaces this process with the newly installed cpd, which resumes pi on
/// the session file (§10.8 step 1). Returns only the error of failing to.
fn reload(exe: &Exe, id: &SessionId) -> CpdError {
    exe.exec(["session", "run", id.as_str()])
}

/// A new cpd is installed and the Lead is at a safe point (§6.7 "Reload on
/// install"): reads the Session and its file, and decides with [`reload_now`].
fn reload_due(env: &mut Env, dirs: &PiDirs, id: &SessionId) -> Result<bool, CpdError> {
    let row = env.store.read(|conn| row::load(conn, id))?;
    // Cheap first: only an idle Lead can be at a safe point.
    if row.state != State::Idle {
        return Ok(false);
    }
    let Some(file) = existing_file(dirs, &row) else { return Ok(false) };
    let quiet = Duration::from_secs(u64::from(env.config.timeouts.fork_quiet_seconds));
    let since = OffsetDateTime::parse(&row.state_at, &Rfc3339)
        .map_err(|e| bad_row(format!("{} has state_at {:?}: {e}", row.id, row.state_at)))?;
    let idle_for = Duration::try_from(OffsetDateTime::now_utc() - since).unwrap_or_default();
    let finished = std::fs::read_to_string(&file).is_ok_and(|text| turn_finished(&text));
    Ok(reload_now(row.state, &row.place, idle_for, finished, quiet_since(&file, quiet)))
}

/// Whether a Lead whose pi runs reloads it now (hs-d49): it is idle, its last
/// turn is finished and its session file is quiet (the rule of `session fork`),
/// and it is parked, or in the chat slot and has been idle for [`CHAT_IDLE`] (so
/// unsent typing is not lost). A `blocked` or `working` Lead never reloads.
fn reload_now(state: State, place: &str, idle_for: Duration, turn_finished: bool, file_quiet: bool) -> bool {
    state == State::Idle && turn_finished && file_quiet && (place != "chat" || idle_for >= CHAT_IDLE)
}

/// The Plan's plannotator port. When the range is full the pane says so, names the
/// Plans that hold it and waits for Enter to try again; `None` at the end of input.
fn assign_port(env: &mut Env, plan: &PlanId) -> Result<Option<u16>, CpdError> {
    loop {
        match port::assign(&env.home, &mut env.store, &env.config, plan) {
            Ok(port) => return Ok(Some(port)),
            Err(e) if e.code() == ErrorCode::NoPort => {
                print!("\n  {}: {}\n  Press Enter to retry.\n", e.code(), e.message());
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                match std::io::stdin().lock().read_line(&mut line) {
                    Ok(0) | Err(_) => return Ok(None),
                    Ok(_) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// `wrong_pane` unless this process runs in a pane of cpd's tmux server whose
/// `@cpd_session` is `id`. The command that created the pane sets the option
/// just after `new-window`, so an unset option is waited for, briefly.
fn check_pane(tmux: &Tmux, id: &SessionId) -> Result<(), CpdError> {
    let wrong = |why: String| {
        CpdError::new(ErrorCode::WrongPane, format!("cpd session run {id} must run in the pane cpd started for it: {why}."))
            .with("session", id.as_str())
    };
    // `TMUX` is `<socket path>,<pid>,<session>`: the pane must be on cpd's server.
    let server = std::env::var("TMUX").unwrap_or_default();
    let socket = server.split(',').next().and_then(|path| Path::new(path).file_name()).map(|n| n.to_string_lossy().into_owned());
    if socket.as_deref() != Some(tmux.socket()) {
        return Err(wrong(format!("this is not a pane of the tmux server {:?}", tmux.socket())));
    }
    let pane = std::env::var("TMUX_PANE")
        .ok()
        .and_then(|p| PaneId::parse(&p))
        .ok_or_else(|| wrong("TMUX_PANE is not set".into()))?;
    let deadline = SystemTime::now() + PANE_WAIT;
    loop {
        let panes = tmux.list_panes()?;
        let found = panes.into_iter().find(|p| p.pane == pane).ok_or_else(|| wrong(format!("pane {pane} is gone")))?;
        match &found.cpd_session {
            Some(s) if s == id => return Ok(()),
            Some(other) => {
                return Err(wrong(format!("pane {pane} runs Session {other}")).with("pane", pane.as_str()).with("found", other.as_str()));
            }
            None if SystemTime::now() >= deadline => {
                return Err(wrong(format!("pane {pane} has no @cpd_session")).with("pane", pane.as_str()));
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// The Session's pi file: the recorded one if it exists, else the one pi
/// created (contract §6.5).
fn existing_file(dirs: &PiDirs, row: &Row) -> Option<PathBuf> {
    row.file.as_ref().map(PathBuf::from).filter(|p| p.is_file()).or_else(|| pi::session_file(dirs, &row.id))
}

/// Records `sessions.file` once pi has created it: a data write, no Event
/// (commands.md §10.8). Returns whether the file is recorded.
fn record_file(env: &mut Env, dirs: &PiDirs, id: &SessionId) -> Result<bool, CpdError> {
    let Some(file) = pi::session_file(dirs, id) else { return Ok(false) };
    let file = file.display().to_string();
    env.store.write("session run", |tx| {
        tx.execute("UPDATE sessions SET file = ?1 WHERE id = ?2 AND file IS NOT ?1", [file.as_str(), id.as_str()])?;
        Ok(())
    })?;
    Ok(true)
}

/// How pi starts this time (§6.7, commands.md §10.8): resume when the file
/// exists, else fork once the source is idle, else fresh.
fn start(env: &mut Env, dirs: &PiDirs, row: &Row) -> Result<LeadStart, CpdError> {
    if let Some(file) = existing_file(dirs, row) {
        if row.file.as_deref() != Some(&file.display().to_string()) {
            record_file(env, dirs, &row.id)?;
        }
        return Ok(LeadStart::Resume { file });
    }
    let tier = Tier::parse(&row.tier).map_err(|_| bad_row(format!("{} has tier {:?}", row.id, row.tier)))?;
    let models = &env.config.tiers;
    let model = match tier.letter() {
        Letter::O => &models.o,
        Letter::S => &models.s,
        Letter::H => &models.h,
    };
    let model = format!("{model}:{}", tier.level());
    let message = first_message(&env.home, row)?;
    match &row.forked_from {
        Some(source) => {
            let source_file = wait_for_source(env, dirs, source, &row.name)?;
            Ok(LeadStart::Fork { model, name: row.name.clone(), source_file, first_message: message })
        }
        None => {
            let first_message = message.ok_or_else(|| {
                CpdError::new(ErrorCode::FileUnreadable, format!("Session {} has no first message to start with.", row.id))
                    .with("session", row.id.as_str())
                    .with("path", row.plan.as_ref().map(|p| env.home.lead_message(p).display().to_string()))
            })?;
            Ok(LeadStart::Fresh { model, name: row.name.clone(), first_message })
        }
    }
}

/// Waits until the source is idle (§6.7 step 1) and returns its file: the file
/// exists, its state is `idle` or `stopped`, its last `message` entry is an
/// assistant message with `stopReason` `stop`, and it has not been modified
/// for `fork_quiet_seconds`. Checks every 2 s, showing the waiting line.
fn wait_for_source(env: &mut Env, dirs: &PiDirs, source: &SessionId, name: &str) -> Result<PathBuf, CpdError> {
    let quiet = Duration::from_secs(u64::from(env.config.timeouts.fork_quiet_seconds));
    let mut shown = false;
    loop {
        let src = env.store.read(|conn| row::load(conn, source))?;
        if let Some(file) = existing_file(dirs, &src)
            && matches!(src.state, State::Idle | State::Stopped)
            && quiet_since(&file, quiet)
            && std::fs::read_to_string(&file).is_ok_and(|text| turn_finished(&text))
        {
            return Ok(file);
        }
        if !shown {
            print!("\n  Waiting for the source Lead to finish its turn, then forking it as \"{name}\"…\n");
            let _ = std::io::stdout().flush();
            shown = true;
        }
        std::thread::sleep(SOURCE_POLL);
    }
}

/// The file has not been modified for `quiet`.
fn quiet_since(file: &Path, quiet: Duration) -> bool {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .is_ok_and(|modified| SystemTime::now().duration_since(modified).is_ok_and(|age| age >= quiet))
}

/// The last `message` entry of a pi session file is a finished assistant
/// turn (`stopReason` `stop`). A line that is not JSON after it means pi is
/// writing: not finished.
fn turn_finished(text: &str) -> bool {
    for line in text.lines().rev().filter(|l| !l.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return false };
        if entry["type"] == "message" {
            let message = &entry["message"];
            return message["role"] == "assistant" && message["stopReason"] == "stop";
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = r#"{"type":"message","id":"a","message":{"role":"user","content":"hi"}}"#;
    const DONE: &str = r#"{"type":"message","id":"b","message":{"role":"assistant","stopReason":"stop"}}"#;
    const TOOL: &str = r#"{"type":"message","id":"c","message":{"role":"assistant","stopReason":"toolUse"}}"#;
    const MINUTE: Duration = Duration::from_secs(60);
    const NAME: &str = r#"{"type":"session_info","name":"x"}"#;

    #[test]
    fn a_turn_is_finished_when_the_last_message_is_an_assistant_stop() {
        assert!(turn_finished(&[USER, DONE].join("\n")));
        assert!(turn_finished(&[USER, DONE, NAME, ""].join("\n")), "other entries after it do not count");
        assert!(!turn_finished(&[USER, TOOL].join("\n")));
        assert!(!turn_finished(&[DONE, USER].join("\n")));
        assert!(!turn_finished(&[USER, DONE, "{\"type\":\"mess"].join("\n")), "a half-written line is not finished");
        assert!(!turn_finished(NAME));
        assert!(!turn_finished(""));
    }

    #[test]
    fn a_parked_idle_lead_with_a_finished_quiet_turn_reloads_at_once() {
        assert!(reload_now(State::Idle, "parked", Duration::ZERO, true, true));
        assert!(reload_now(State::Idle, "none", Duration::ZERO, true, true));
    }

    #[test]
    fn a_lead_that_is_not_idle_never_reloads() {
        for state in [State::Working, State::Blocked, State::Stopped] {
            for place in ["parked", "chat", "none"] {
                assert!(!reload_now(state, place, 60 * MINUTE, true, true), "{state:?} in {place}");
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
        assert!(!reload_now(State::Idle, "chat", 10 * MINUTE - Duration::from_secs(1), true, true));
        assert!(reload_now(State::Idle, "chat", 10 * MINUTE, true, true));
        assert!(reload_now(State::Idle, "chat", 11 * MINUTE, true, true));
        assert!(!reload_now(State::Idle, "chat", 11 * MINUTE, false, true), "ten minutes do not skip the turn rule");
    }
}
