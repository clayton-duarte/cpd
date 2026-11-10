//! E9, write-time curation tick: starts `cpd context curate` per Context (contract §3.7 E9).

use std::fs;

use time::OffsetDateTime;

use super::engine::{Engine, stopping};
use super::report::{About, Step};
use super::{detached_launch, record_detached};
use cpd_core::config::Config;
use cpd_core::context::curate_run::next_record;
use cpd_core::context::curate_vectors::VectorStore;
use cpd_core::context::curation::not_curable;
use cpd_core::context::distill::{State, log_path, running};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::spawn::Pid;
use cpd_core::ids::ContextId;

pub(super) fn run(engine: &mut Engine, contexts: &[ContextId], now: OffsetDateTime) {
    let config = match Config::load(&engine.home) {
        Ok(config) => config,
        Err(e) => return engine.fail(Step::E9, About::Machine, &e),
    };
    if !config.experimental.write_curation {
        return;
    }
    let floor = config.context_curation_min_records;
    for context in contexts {
        if stopping() {
            return;
        }
        if let Err(e) = tick(engine, context, floor, now) {
            engine.fail(Step::E9, About::Machine, &e);
        }
    }
}

fn tick(
    engine: &mut Engine,
    context: &ContextId,
    floor: u32,
    now: OffsetDateTime,
) -> Result<(), CpdError> {
    if engine
        .store
        .read(|conn| not_curable(conn, context, floor))?
        .is_some()
    {
        return Ok(());
    }
    if let Some(pid) = engine.curating.get(context).copied() {
        let ended = engine.ports.spawn.try_wait(pid);
        let status = match ended {
            Ok(None) => return Ok(()),
            Ok(Some(status)) => Some(status),
            Err(_) => None,
        };
        engine.curating.remove(context);
        let at = engine.ports.clock.now();
        let backoff = engine.curate_backoff.entry(context.clone()).or_default();
        match status {
            Some(status) if status.success() => backoff.succeeded(),
            other => {
                backoff.failed(at);
                let delay = backoff.delay();
                let how =
                    other.map_or_else(|| "its status is unknown".to_owned(), |s| s.to_string());
                engine.log.line(&format!(
                    "curation: curate run of Context {context} failed ({how}); backing off {delay:?}"
                ));
                // A run that ended non-zero is not silent: it raises one `error` Event for E9
                // (contract §3.7 E9), once until it clears, so a judge unreachable or a crash is
                // visible in `cpd events`, not only in `run/watch.log`.
                let failure = CpdError::new(
                    ErrorCode::DistillFailed,
                    format!("The curate run of Context {context} failed ({how})."),
                )
                .with("context", context.as_str());
                engine.fail(Step::E9, About::Machine, &failure);
            }
        }
    }
    let at = engine.ports.clock.now();
    if !engine.curate_backoff.get(context).is_none_or(|b| b.due(at)) {
        return Ok(());
    }
    if running(&engine.home, context)? {
        return Ok(());
    }
    let state = engine.store.read(|conn| State::read(conn, context))?;
    let checked = VectorStore::at(&engine.home)
        .checked(context)
        .unwrap_or_default();
    if next_record(&state, &checked, now).is_none() {
        return Ok(());
    }
    match spawn(engine, context) {
        Ok(pid) => {
            engine.curating.insert(context.clone(), Pid(pid));
            engine.log.line(&format!(
                "curation: started the curate run of Context {context} (pid {pid})"
            ));
        }
        Err(e) => {
            engine
                .curate_backoff
                .entry(context.clone())
                .or_default()
                .failed(at);
            engine.fail(Step::E9, About::Machine, &e);
        }
    }
    Ok(())
}

/// Spawns `cpd context curate --context <context>` as distill's `spawn` does: detached, its output appended to
/// `curate.log`, its pid in `curate.pid`.
fn spawn(engine: &mut Engine, context: &ContextId) -> Result<u32, CpdError> {
    let failed = |why: String| {
        CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot start the curate run of Context {context}: {why}."),
        )
        .with("context", context.as_str())
    };
    let exe = engine.ports.spawn.self_exe()?;
    let dir = log_path(&engine.home, context)
        .parent()
        .expect("a log lives in a directory")
        .to_path_buf();
    fs::create_dir_all(&dir)
        .map_err(|e| failed(format!("cannot create {}: {e}", dir.display())))?;
    let args = ["context", "curate", "--context", context.as_str()];
    let pid = engine
        .ports
        .spawn
        .spawn(&detached_launch(
            &exe,
            &args,
            &engine.home,
            dir.join("curate.log"),
        ))
        .map_err(|e| failed(format!("cannot run {}: {e}", exe.display())))?;
    let pid_path = dir.join("curate.pid");
    record_detached(engine, pid, pid_path.clone()).map_err(|e| {
        failed(format!(
            "it runs, but its pid cannot be written to {}: {e}",
            pid_path.display()
        ))
    })
}
