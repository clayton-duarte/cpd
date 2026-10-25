//! E14, model discovery (contract §3.7 E14, §1.4 `models-<provider>.json`; Context decisions pro-d13, pro-d17, pro-d20):
//! runs the helper `pi/lib/models-list.ts` when no cached list exists or the newest `polled_at` is
//! `watch.models_discovery_seconds` old, validates its answer and writes one file per provider that has a good list
//! (`cpd_core::models`, shared with `cpd models refresh`). Like E11 the run is detached from the tick: a later tick
//! collects its answer. cpd names no provider and no key here: the helper's answer names them and the helper
//! inherits the engine's environment.
//!
//! The week counts from the files' `polled_at`, never from the engine's start. After a failed run the next attempt is
//! `watch.error_repeat_seconds` after its end (in memory). A failure is one `error` Event per code; the last good file
//! of a failing provider stays.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::json;
use time::OffsetDateTime;

use super::engine::Engine;
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::models::{Answer, Applied, Helper, Outcome, apply, newest_polled_at};

/// What E14 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    /// The run in flight: its answer arrives here.
    running: Option<Receiver<Result<Answer, CpdError>>>,
    /// When the last run ended (or failed to start).
    last_end: Option<Instant>,
}

pub(super) fn run(engine: &mut Engine) {
    if let Some(running) = &engine.models.running {
        let answer = match running.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err(CpdError::new(ErrorCode::SpawnFailed, "The model-discovery helper's reader ended without an answer."))
            }
        };
        engine.models.running = None;
        engine.models.last_end = Some(Instant::now());
        match answer.and_then(|answer| apply(&engine.home, engine.config.timeouts.store_busy_ms, &answer)) {
            Ok(applied) => report(engine, &applied),
            Err(e) => engine.fail(Step::E14, About::Machine, Failure::from(&e)),
        }
        return;
    }
    let repeat = Duration::from_secs(u64::from(engine.config.watch.error_repeat_seconds));
    if engine.models.last_end.is_some_and(|end| end.elapsed() < repeat) {
        return;
    }
    match due(engine).and_then(|due| if due { start(engine).map(Some) } else { Ok(None) }) {
        Ok(Some(running)) => engine.models.running = Some(running),
        Ok(None) => {}
        Err(e) => {
            // The next try is a period away, as after a run that failed.
            engine.models.last_end = Some(Instant::now());
            engine.fail(Step::E14, About::Machine, Failure::from(&e));
        }
    }
}

/// No file, or the newest `polled_at` is at least `watch.models_discovery_seconds` old.
fn due(engine: &Engine) -> Result<bool, CpdError> {
    let period = time::Duration::seconds(i64::from(engine.config.watch.models_discovery_seconds));
    Ok(newest_polled_at(&engine.home)?.is_none_or(|newest| OffsetDateTime::now_utc() - newest >= period))
}

/// Starts the helper on its own thread; returns where its answer will arrive.
fn start(engine: &Engine) -> Result<Receiver<Result<Answer, CpdError>>, CpdError> {
    let helper = Helper::new(&engine.config, &engine.home)?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        // The engine may have stopped: nobody to tell.
        let _ = tx.send(helper.run());
    });
    Ok(rx)
}

/// Reports what the write found: a failing provider and a file that did not parse, each once; nothing failing clears.
fn report(engine: &mut Engine, applied: &Applied) {
    let mut clean = true;
    for outcome in &applied.outcomes {
        match outcome {
            Outcome::Written(w) => {
                let changed = w.changes.as_ref().map_or(0, |c| c.appeared.len() + c.went_away.len());
                engine.log.line(&format!("E14: cached {} id(s) of {} ({changed} changed)", w.count, w.provider));
            }
            Outcome::Failed(failure) => {
                clean = false;
                engine.fail(Step::E14, About::Machine, Failure::from(&failure.error()));
            }
        }
    }
    for (provider, path) in &applied.unparseable {
        clean = false;
        let failure = Failure {
            code: ErrorCode::ModelsUnparseable.as_str().to_owned(),
            message: format!("The cached model list of {provider} is not readable and was treated as missing."),
            details: json!({"provider": provider, "file": path.display().to_string()}),
        };
        engine.fail(Step::E14, About::Machine, failure);
    }
    if clean {
        engine.clear(Step::E14, &About::Machine);
    }
}
