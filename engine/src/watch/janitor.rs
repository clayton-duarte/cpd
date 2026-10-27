//! E16, the janitor (contract §3.7): starts `cpd janitor` detached on the first tick of an engine's life and then every
//! `watch.janitor_minutes` by the engine's clock (kept in memory, as E9 keeps `distill_started`). The engine never waits
//! for the pass or kills it.

use std::fs;
use std::time::Duration;

use super::engine::{Engine, incident_holds};
use super::report::{About, Step};
use super::{detached_launch, record_detached, sweep_detached};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::janitor::{log_path, running};

pub(super) fn run(engine: &mut Engine) {
    sweep_detached(engine);
    if incident_holds(engine, Step::E16) {
        engine.log.line("E16: holding: incident mode is on");
        return;
    }
    match due(engine).and_then(|due| if due { start(engine) } else { Ok(()) }) {
        Ok(()) => engine.clear(Step::E16, &About::Machine),
        Err(e) => engine.fail(Step::E16, About::Machine, &e),
    }
}

/// A pass holds the lock: not due. Else none was started in this engine's life, or the last start is old enough.
fn due(engine: &Engine) -> Result<bool, CpdError> {
    if running(&engine.home)? {
        return Ok(false);
    }
    let period = Duration::from_secs(u64::from(engine.config.watch.janitor_minutes) * 60);
    Ok(engine.janitor_started.is_none_or(|at| engine.ports.clock.now().saturating_duration_since(at) >= period))
}

/// Spawns `cpd janitor` detached, its output in the janitor log, its pid recorded in `run/janitor.pid`.
fn start(engine: &mut Engine) -> Result<(), CpdError> {
    let failed = |why: String| CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start the janitor pass: {why}."));
    let exe = engine.ports.spawn.self_exe()?;
    let log = log_path(&engine.home);
    let dir = log.parent().expect("a log lives in a directory");
    fs::create_dir_all(dir).map_err(|e| failed(format!("cannot create {}: {e}", dir.display())))?;
    let pid = engine.ports.spawn.spawn(&detached_launch(&exe, &["janitor"], &engine.home, log)).map_err(|e| failed(format!("cannot run {}: {e}", exe.display())))?;
    engine.janitor_started = Some(engine.ports.clock.now());
    let pid_path = engine.home.root().join("run").join("janitor.pid");
    record_detached(engine, pid, pid_path.clone()).map_err(|e| failed(format!("it runs, but its pid cannot be written to {}: {e}", pid_path.display())))?;
    engine.log.line(&format!("E16: started the janitor pass (pid {})", pid.0));
    Ok(())
}
