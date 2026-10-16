//! `session run` (commands.md §10.8, contract §6.7): the program in every
//! Lead pane. It owns its terminal: it runs pi in the foreground, and when pi
//! exits it offers to resume it.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use super::moves::first_message;
use super::row::{self, Role, Row, State};
use super::{Env, RunArgs, bad_row, not_a_lead, state};
use crate::config::{Letter, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::host::pi::{self, LeadLaunch, LeadStart, PiDirs, Plannotator};
use crate::host::{PaneId, Tmux};
use crate::ids::SessionId;
use crate::output::Output;

/// How often the fork waits for its source to be idle (§6.7 step 1).
const SOURCE_POLL: Duration = Duration::from_secs(2);
/// How often it looks for pi's session file while pi runs.
const FILE_POLL: Duration = Duration::from_millis(500);
/// How long it waits for `new-window`'s caller to set `@cpd_session` on its pane.
const PANE_WAIT: Duration = Duration::from_secs(5);

pub(super) fn run(args: RunArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let row = env.store.read(|conn| row::load(conn, &args.id))?;
    if row.role != Role::Lead {
        return Err(not_a_lead(&row.id, row.role));
    }
    check_pane(&env.tmux(), &row.id)?;
    let dirs = PiDirs::from_env()?;
    // From here on this process owns the terminal (commands.md §1.2).
    loop {
        let row = env.store.read(|conn| row::load(conn, &args.id))?;
        let start = start(&mut env, &dirs, &row)?;
        let candidates = pi::lead_candidates(&env.config.pi_adapter_dir()?, &env.config.lead_pi_packages);
        let extensions = pi::extensions_to_pass(&candidates, Path::new(&row.cwd), &dirs, &mut std::io::stderr());
        let plannotator = Plannotator::from_env();
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
            if !recorded {
                recorded = record_file(&mut env, &dirs, &row.id)?;
            }
            if exited.is_some() {
                break;
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
pub(crate) fn quiet_since(file: &Path, quiet: Duration) -> bool {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .is_ok_and(|modified| SystemTime::now().duration_since(modified).is_ok_and(|age| age >= quiet))
}

/// The last `message` entry of a pi session file is a finished assistant
/// turn (`stopReason` `stop`). A line that is not JSON after it means pi is
/// writing: not finished.
pub(crate) fn turn_finished(text: &str) -> bool {
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
}
