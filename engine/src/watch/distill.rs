//! E9, Context digests (contract §3.7; hs-d194 D4): starts `cpd context distill`
//! for each Context whose digest is behind its records once its newest record is
//! quiet, or at once when 20 or more records are newer than the digest (hs-d252).
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
use cpd_core::context::digest;
use cpd_core::context::distill::{log_path, running};
use cpd_core::context::{Record, inforce};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ids::ContextId;

/// How old a Context's newest record must be before its digest is rebuilt (§3.7 E9).
const QUIET: time::Duration = time::Duration::minutes(10);

/// How many records newer than the latest digest start a rebuild without the quiet period (§3.7 E9; a first
/// message carries the digest plus at most this many newer records, §4.6 "Consumers").
const BACKLOG: i64 = 20;

/// Whether the newest record, written at `newest`, is quiet at `now`.
fn quiet(newest: &str, now: OffsetDateTime) -> Result<bool, CpdError> {
    let at = OffsetDateTime::parse(newest, &Rfc3339).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("The store holds an invalid value: a Context record's at {newest:?}: {e}."))
    })?;
    Ok(now - at >= QUIET)
}

/// Each live Context with its highest record `seq` and that record's `at`.
const NEWEST: &str = "SELECT c.id, r.seq, r.at FROM contexts c
  JOIN context_records r ON r.context = c.id AND r.seq = (SELECT max(seq) FROM context_records WHERE context = c.id)
  WHERE c.archived_at IS NULL ORDER BY c.id";

pub(super) fn run(engine: &mut Engine) {
    let newest = engine.store.read(|conn| {
        let mut stmt = conn.prepare(NEWEST)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|(id, seq, at)| Ok((ContextId::parse(&id)?, seq, at))).collect::<Result<Vec<_>, CpdError>>()
    });
    let newest = match newest {
        Ok(newest) => newest,
        Err(e) => return engine.fail(Step::E9, About::Machine, &e),
    };
    let now = OffsetDateTime::now_utc();
    let mut failed = false;
    for (context, seq, at) in newest {
        if stopping() {
            return;
        }
        if let Err(e) = start_if_due(engine, &context, seq, &at, now) {
            failed = true;
            engine.fail(Step::E9, About::Machine, &e);
        }
    }
    if !failed {
        engine.clear(Step::E9, &About::Machine);
    }
}

/// Starts the run of `context` when it is due: its digest is behind `seq`, its newest record is quiet or
/// `BACKLOG` records are newer than the digest, a record is in force, no run is alive, and no run was started for this `seq` in this engine's life.
fn start_if_due(engine: &mut Engine, context: &ContextId, seq: i64, at: &str, now: OffsetDateTime) -> Result<(), CpdError> {
    if engine.distill_started.get(context).is_some_and(|started| *started >= seq) {
        return Ok(());
    }
    let upto = match digest::latest(&engine.home, context)? {
        Some(d) => i64::try_from(d.upto_seq).unwrap_or(i64::MAX),
        None => 0,
    };
    if upto >= seq {
        return Ok(());
    }
    if !quiet(at, now)? {
        let newer = engine.store.read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM context_records WHERE context = ?1 AND seq > ?2",
                rusqlite::params![context.as_str(), upto],
                |r| r.get::<_, i64>(0),
            )?)
        })?;
        if newer < BACKLOG {
            return Ok(());
        }
    }
    let (records, retired) = engine.store.read(|conn| {
        let mut stmt =
            conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
        let records = stmt.query_map([context.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((records, inforce::retired(conn, context)?))
    })?;
    if inforce::in_force_without(&records, seq, &retired).is_empty() || running(&engine.home, context)? {
        return Ok(());
    }
    let pid = spawn(engine, context)?;
    engine.distill_started.insert(context.clone(), seq);
    engine.log.line(&format!("E9: started the distill run of Context {context} up to record {seq} (pid {pid})"));
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
