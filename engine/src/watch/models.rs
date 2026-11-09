//! E15, model discovery (contract §3.7 E15, §1.4 `models-<provider>.json`; Context decisions pro-d13, pro-d17, pro-d20):
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

use super::engine::Engine;
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::models::{Answer, Entry, Helper, Outcome};
use cpd_core::swr::endpoint::{self, Kind};
use cpd_core::swr::source::models::{apply_answer, endpoint_of, kind_of, newest_polled_at};

/// A run in flight: where its answer arrives, and the thread that will send it (joined by tests, [`join`]).
type Run = (
    Receiver<Result<Answer, CpdError>>,
    std::thread::JoinHandle<()>,
);

/// What E15 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    /// The run in flight: its answer arrives here.
    running: Option<Receiver<Result<Answer, CpdError>>>,
    /// The run's own thread, so a test can wait for the answer ([`join`]) instead of racing the scheduler. The
    /// engine itself never blocks on it.
    #[cfg(test)]
    handle: Option<std::thread::JoinHandle<()>>,
    /// When the last run ended (or failed to start).
    last_end: Option<Instant>,
}

pub(super) fn run(engine: &mut Engine) {
    if let Some(running) = &engine.models.running {
        let answer = match running.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(CpdError::new(
                ErrorCode::SpawnFailed,
                "The model-discovery helper's reader ended without an answer.",
            )),
        };
        engine.models.running = None;
        #[cfg(test)]
        {
            // The thread has sent its answer (it sends before it ends); dropping the handle detaches one still finishing.
            engine.models.handle = None;
        }
        engine.models.last_end = Some(engine.ports.clock.now());
        let at = engine.ports.clock.utc();
        match answer.and_then(|mut answer| {
            // air-d143: Bifrost is excluded on air, even when explicitly configured.
            if engine.config.machine.as_str() == "air" {
                answer.entries.retain(|entry| match entry {
                    Entry::Listed(listed) => listed.provider != "bifrost",
                    Entry::Failed(failed) => failed.provider != "bifrost",
                });
            }
            apply_answer(&mut engine.store, &answer, at)
        }) {
            Ok(outcomes) => report(engine, &outcomes, at),
            Err(e) => engine.fail(Step::E15, About::Machine, Failure::from(&e)),
        }
        return;
    }
    let repeat = Duration::from_secs(u64::from(engine.config.watch.error_repeat_seconds));
    if engine
        .models
        .last_end
        .is_some_and(|end| engine.ports.clock.now().saturating_duration_since(end) < repeat)
    {
        return;
    }
    match due(engine).and_then(|due| {
        if due {
            start(engine).map(Some)
        } else {
            Ok(None)
        }
    }) {
        Ok(Some((running, handle))) => {
            engine.models.running = Some(running);
            #[cfg(test)]
            {
                engine.models.handle = Some(handle);
            }
            #[cfg(not(test))]
            let _ = handle;
        }
        Ok(None) => {}
        Err(e) => {
            // The next try is a period away, as after a run that failed.
            engine.models.last_end = Some(engine.ports.clock.now());
            engine.fail(Step::E15, About::Machine, Failure::from(&e));
        }
    }
}

/// No row, or the newest `polled_at` is at least `watch.models_discovery_seconds` old.
fn due(engine: &mut Engine) -> Result<bool, CpdError> {
    let period = time::Duration::seconds(i64::from(engine.config.watch.models_discovery_seconds));
    Ok(newest_polled_at(&mut engine.store)?
        .is_none_or(|newest| engine.ports.clock.utc() - newest >= period))
}

/// Starts the helper on its own thread, through the engine's runner (a test's `FakeRunner` sees the call); returns where
/// its answer will arrive and the thread itself ([`join`] waits on it in tests).
fn start(engine: &Engine) -> Result<Run, CpdError> {
    let helper = Helper::new(&engine.config, &engine.home)?;
    let runner = engine.ports.runner.clone();
    let (tx, rx) = channel();
    let handle = std::thread::spawn(move || {
        // The engine may have stopped: nobody to tell.
        let _ = tx.send(helper.run_with(&*runner));
    });
    Ok((rx, handle))
}

/// Waits for the run in flight to end: by then its answer is on the channel, so the next tick collects it. A test calls
/// this from its wait loop so the answer arrives without a race with the scheduler; the engine never blocks here.
#[cfg(test)]
pub(super) fn join(engine: &mut Engine) {
    if let Some(handle) = engine.models.handle.take() {
        let _ = handle.join();
    }
}

/// Reports what the write found: a failing provider once per outage (a permanent code at once, a transient one after
/// ten minutes of failing); nothing failing clears.
fn report(engine: &mut Engine, outcomes: &[Outcome], now: time::OffsetDateTime) {
    let mut clean = true;
    for outcome in outcomes {
        match outcome {
            Outcome::Written(w) => {
                let changed = w
                    .changes
                    .as_ref()
                    .map_or(0, |c| c.appeared.len() + c.went_away.len());
                engine.log.line(&format!(
                    "model-discovery: cached {} id(s) of {} ({changed} changed)",
                    w.count, w.provider
                ));
            }
            Outcome::Failed(failure) => {
                clean = false;
                let name = match endpoint_of(&mut engine.store, &failure.provider) {
                    Ok(name) => name,
                    Err(e) => {
                        engine.fail(Step::E15, About::Machine, Failure::from(&e));
                        continue;
                    }
                };
                let due = match kind_of(&failure.code) {
                    Kind::Permanent => Ok(true),
                    Kind::Transient => endpoint::due_report(&mut engine.store, &name, now),
                };
                match due {
                    Ok(true) => {
                        let mut f = Failure::from(&failure.error());
                        if let Some(d) = f.details.as_object_mut() {
                            d.insert("endpoint".to_owned(), name.into());
                        }
                        engine.fail(Step::E15, About::Machine, f);
                    }
                    Ok(false) => {}
                    Err(e) => engine.fail(Step::E15, About::Machine, Failure::from(&e)),
                }
            }
        }
    }
    if clean {
        engine.clear(Step::E15, &About::Machine);
    }
}
