//! E9, Context curation (contract §3.7; hs-d549, hs-d551): starts `cpd context distill` for each Context with
//! uncurated records in force once the newest of them is quiet, or at once when 20 or more wait, one batch per run.
//! It only decides and spawns, as E3 does: the run takes its own lock,
//! does its own work and appends its own `error` Event.

use std::fs;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::engine::{Engine, incident_holds, stopping};
use super::report::{About, Step};
use super::{detached_launch, record_detached, sweep_detached};
use cpd_core::config::Config;
use cpd_core::context::curation::not_curable;
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
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("The store holds an invalid value: a Context record's at {newest:?}: {e}."),
        )
    })?;
    Ok(now - at >= QUIET)
}

/// The ids of the live (not archived) Contexts.
const LIVE: &str =
    "SELECT id FROM contexts WHERE archived_at IS NULL AND deleted_at IS NULL ORDER BY id";

pub(super) fn run(engine: &mut Engine) {
    sweep_detached(engine);
    if incident_holds(engine, Step::E9) {
        engine.log.line("curation: holding: incident mode is on");
        return;
    }
    let contexts = engine.store.read(|conn| {
        let mut stmt = conn.prepare(LIVE)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter()
            .map(|id| ContextId::parse(id))
            .collect::<Result<Vec<_>, CpdError>>()
    });
    let contexts = match contexts {
        Ok(contexts) => contexts,
        Err(e) => return engine.fail(Step::E9, About::Machine, &e),
    };
    let now = engine.ports.clock.utc();
    let mut failed = false;
    for context in &contexts {
        if stopping() {
            return;
        }
        if let Err(e) = start_if_due(engine, context, now) {
            failed = true;
            engine.fail(Step::E9, About::Machine, &e);
        }
    }
    if !failed {
        engine.clear(Step::E9, &About::Machine);
    }
    super::curate::run(engine, &contexts, now);
}

/// The batch sizes of a state's runs, largest first (FX4): a run that ends without changing its state failed, and
/// the next run of the same state takes the next smaller batch.
const BATCHES: [u8; 5] = [60, 30, 15, 5, 1];

/// Starts the run of `context` when it is due: it has records in force that are not curated, the newest of them is
/// quiet or `BACKLOG` of them are waiting, and no run is alive. The state is the highest `seq` and the count of
/// uncurated records. A run that curates a batch changes the state, so the next tick starts the next batch at 60. A
/// run that has ended with the state unchanged failed: the next tick starts the next run for the same state with
/// the next smaller batch (60, 30, 15, 5, 1) and records the batch with the state; after a failed run of batch 1
/// nothing starts until the state changes or the engine restarts. So a state has at most five runs.
fn start_if_due(
    engine: &mut Engine,
    context: &ContextId,
    now: OffsetDateTime,
) -> Result<(), CpdError> {
    let floor = Config::load(&engine.home)?.context_curation_min_records;
    if engine
        .store
        .read(|conn| not_curable(conn, context, floor))?
        .is_some()
    {
        return Ok(());
    }
    let state = engine.store.read(|conn| State::read(conn, context))?;
    let uncurated = state.uncurated();
    let Some(newest) = uncurated.last() else {
        return Ok(());
    };
    let key = (
        state.highest().expect("an uncurated record is a record"),
        i64::try_from(uncurated.len()).unwrap_or(i64::MAX),
    );
    let previous = engine
        .distill_started
        .get(context)
        .filter(|(started, _)| *started == key)
        .map(|(_, batch)| *batch);
    if previous == Some(BATCHES[BATCHES.len() - 1]) {
        return Ok(());
    }
    if uncurated.len() < BACKLOG && !quiet(&newest.1.at, now)? {
        return Ok(());
    }
    // A run that is alive, or this engine's own run that has not ended yet (its pid file is still there; one an
    // earlier engine left behind says nothing, so only a state this engine started is asked).
    if running(&engine.home, context)?
        || (previous.is_some() && engine.home.distill_pid(context).exists())
    {
        return Ok(());
    }
    let batch = previous.map_or(BATCHES[0], |last| {
        BATCHES
            .iter()
            .copied()
            .find(|b| *b < last)
            .expect("batch 1 stops before this")
    });
    let pid = spawn(engine, context, batch)?;
    engine.distill_started.insert(context.clone(), (key, batch));
    engine.log.line(&format!("curation: started the distill run of Context {context} up to record {} with batch {batch} (pid {pid})", key.0));
    Ok(())
}

/// Spawns `cpd context distill --context <context>` through `engine.ports.spawn`, detached (`setsid`), stdin from
/// `/dev/null`, stdout and stderr appended to the Context's `distill.log`, its pid recorded in `distill.pid`
/// ([`record_detached`]). Returns its pid.
fn spawn(engine: &mut Engine, context: &ContextId, batch: u8) -> Result<u32, CpdError> {
    let failed = |why: String| {
        CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot start the distill run of Context {context}: {why}."),
        )
        .with("context", context.as_str())
    };
    let exe = engine.ports.spawn.self_exe()?;
    let log_path = log_path(&engine.home, context);
    let dir = log_path.parent().expect("a log lives in a directory");
    fs::create_dir_all(dir).map_err(|e| failed(format!("cannot create {}: {e}", dir.display())))?;
    let args = [
        "context",
        "distill",
        "--context",
        context.as_str(),
        "--batch",
        &batch.to_string(),
    ];
    let pid = engine
        .ports
        .spawn
        .spawn(&detached_launch(&exe, &args, &engine.home, log_path))
        .map_err(|e| failed(format!("cannot run {}: {e}", exe.display())))?;
    let pid_path = engine.home.distill_pid(context);
    record_detached(engine, pid, pid_path.clone()).map_err(|e| {
        failed(format!(
            "it runs, but its pid cannot be written to {}: {e}",
            pid_path.display()
        ))
    })
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
        assert_eq!(
            quiet("yesterday", at("2026-10-18T12:00:00Z"))
                .unwrap_err()
                .code(),
            ErrorCode::StoreFailed
        );
    }
}
