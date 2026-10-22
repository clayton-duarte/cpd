//! E9, Context curation (contract §3.7; hs-d549, hs-d551): starts `cpd context distill` for each Context with
//! uncurated records in force once the newest of them is quiet, or at once when 20 or more wait, one batch per run.
//! It only decides and spawns, as E3 does: the run takes its own lock,
//! does its own work and appends its own `error` Event.

use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::engine::{Engine, stopping};
use super::record_detached;
use super::report::{About, Step};
use cpd_core::context::distill::{State, log_path, running};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ids::ContextId;

/// How old a Context's newest uncurated record must be before a run starts (§3.7 E9).
const QUIET: time::Duration = time::Duration::minutes(10);

/// How many uncurated records start a run without the quiet period (§3.7 E9; a first message carries at most this
/// many uncurated records raw, §4.6 "Consumers").
const BACKLOG: usize = 20;

/// Whether the newest record, written at `newest`, is quiet at `now`.
fn quiet(newest: &str, now: OffsetDateTime) -> Result<bool, CpdError> {
    let at = OffsetDateTime::parse(newest, &Rfc3339).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("The store holds an invalid value: a Context record's at {newest:?}: {e}."))
    })?;
    Ok(now - at >= QUIET)
}

/// The ids of the live (not archived) Contexts.
const LIVE: &str = "SELECT id FROM contexts WHERE archived_at IS NULL ORDER BY id";

pub(super) fn run(engine: &mut Engine) {
    let contexts = engine.store.read(|conn| {
        let mut stmt = conn.prepare(LIVE)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|id| ContextId::parse(id)).collect::<Result<Vec<_>, CpdError>>()
    });
    let contexts = match contexts {
        Ok(contexts) => contexts,
        Err(e) => return engine.fail(Step::E9, About::Machine, &e),
    };
    let now = OffsetDateTime::now_utc();
    let mut failed = false;
    for context in contexts {
        if stopping() {
            return;
        }
        if let Err(e) = start_if_due(engine, &context, now) {
            failed = true;
            engine.fail(Step::E9, About::Machine, &e);
        }
    }
    if !failed {
        engine.clear(Step::E9, &About::Machine);
    }
}

/// Starts the run of `context` when it is due: it has records in force that are not curated, the newest of them is
/// quiet or `BACKLOG` of them are waiting, no run is alive, and no run was started for this state (the highest
/// `seq` and the count of uncurated records) in this engine's life. A run that curates a batch changes the count,
/// so the next tick starts the next batch; a failed one leaves the state as it was and is not tried again.
fn start_if_due(engine: &mut Engine, context: &ContextId, now: OffsetDateTime) -> Result<(), CpdError> {
    let state = engine.store.read(|conn| State::read(conn, context))?;
    let uncurated = state.uncurated();
    let Some(newest) = uncurated.last() else { return Ok(()) };
    let key = (state.highest().expect("an uncurated record is a record"), i64::try_from(uncurated.len()).unwrap_or(i64::MAX));
    if engine.distill_started.get(context) == Some(&key) {
        return Ok(());
    }
    if uncurated.len() < BACKLOG && !quiet(&newest.1.at, now)? {
        return Ok(());
    }
    if running(&engine.home, context)? {
        return Ok(());
    }
    let pid = spawn(engine, context)?;
    engine.distill_started.insert(context.clone(), key);
    engine.log.line(&format!("E9: started the distill run of Context {context} up to record {} (pid {pid})", key.0));
    Ok(())
}

/// Spawns `cpd context distill --context <context>` detached (`setsid`), stdin from `/dev/null`, stdout and
/// stderr appended to the Context's `distill.log`, its pid recorded in `distill.pid` ([`record_detached`]).
/// Returns its pid.
fn spawn(engine: &Engine, context: &ContextId) -> Result<u32, CpdError> {
    let failed = |why: String| CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start the distill run of Context {context}: {why}.")).with("context", context.as_str());
    let exe = cpd_core::host::exe::started_as()?;
    let log_path = log_path(&engine.home, context);
    let dir = log_path.parent().expect("a log lives in a directory");
    fs::create_dir_all(dir).map_err(|e| failed(format!("cannot create {}: {e}", dir.display())))?;
    let log = OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e| failed(format!("cannot open {}: {e}", log_path.display())))?;
    let stdout = log.try_clone().map_err(|e| failed(format!("cannot use {}: {e}", log_path.display())))?;
    let mut command = Command::new(exe);
    command
        .args(["context", "distill", "--context", context.as_str()])
        .current_dir(engine.home.root())
        .env("CPD_HOME", engine.home.root())
        // The caller's own identity is not the run's.
        .env_remove("CPD_PLAN")
        .env_remove("CPD_SESSION")
        .env_remove("CPD_ATTEMPT")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(log);
    // SAFETY: the closure runs in the forked child before exec and only makes an async-signal-safe system call.
    unsafe {
        command.pre_exec(|| nix::unistd::setsid().map(drop).map_err(std::io::Error::from));
    }
    let child = command.spawn().map_err(|e| failed(format!("cannot run {}: {e}", exe.display())))?;
    let pid_path = engine.home.distill_pid(context);
    record_detached(child, pid_path.clone()).map_err(|e| failed(format!("it runs, but its pid cannot be written to {}: {e}", pid_path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    #[test]
    fn a_record_is_quiet_from_ten_minutes_on() {
        let now = at("2026-10-18T12:00:00Z");
        assert!(!quiet("2026-10-18T11:50:00.001Z", now).unwrap());
        assert!(quiet("2026-10-18T11:50:00.000Z", now).unwrap());
        assert!(quiet("2026-10-17T00:00:00.000Z", now).unwrap());
        assert!(!quiet("2026-10-18T12:00:00.000Z", now).unwrap());
    }

    #[test]
    fn an_unreadable_time_is_an_error() {
        assert_eq!(quiet("yesterday", at("2026-10-18T12:00:00Z")).unwrap_err().code(), ErrorCode::StoreFailed);
    }
}
