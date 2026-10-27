//! E17, the Slack reaction queue (Plan pr-post, pro-d64 step 7; pro-d107, pro-d110; commands.md §21.9): the helper
//! `pi/lib/slack-react.ts` adds the queued emoji to the post of the OLDEST `slack_reactions` row with `reacted_at` NULL,
//! detached on its own thread, one run at a time, and the row's `reacted_at` is set. A failure leaves `reacted_at` NULL,
//! is reported once per code and that row is tried again `watch.error_repeat_seconds` after the failed run's end; the
//! rows behind it go on meanwhile. (The Spec calls the step E16, a name the janitor already holds.)

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::json;

use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::deadline::Failed;
use cpd_core::ports::Ports;
use cpd_core::pr::reactions::{self, Due};

/// The helper's deadline (contract §3.7): above its 30 s for the Slack call.
const DEADLINE: Duration = Duration::from_secs(120);

/// A run in flight: the queue row it reacts to and where its answer will arrive.
struct Running {
    due: Due,
    answer: Receiver<Result<(), Failure>>,
}

/// What E17 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    running: Option<Running>,
    /// When the last run of a queue row that failed ended (or failed to start), by row id; in memory only, so a restart
    /// retries at once and a row that keeps failing never blocks the rows behind it.
    failed_at: HashMap<i64, Instant>,
}

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E17) {
        engine.log.line("E17: holding: incident mode is on");
        return;
    }
    if let Some(running) = &engine.react.running {
        let answer = match running.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Err(Failure::from(&CpdError::new(ErrorCode::SpawnFailed, "The reaction helper's reader ended without an answer.")))
            }
        };
        let due = engine.react.running.take().expect("a run was in flight").due;
        match answer.and_then(|()| mark(engine, &due)) {
            Ok(()) => {
                engine.react.failed_at.remove(&due.id);
                engine.log.line(&format!("E17: reacted :{}: to {} {}", due.emoji, due.channel, due.ts));
                engine.clear(Step::E17, &About::Machine);
            }
            Err(failure) => {
                engine.react.failed_at.insert(due.id, engine.ports.clock.now());
                engine.fail(Step::E17, About::Machine, failure);
            }
        }
        return;
    }
    let repeat = Duration::from_secs(u64::from(engine.config.watch.error_repeat_seconds));
    let now = engine.ports.clock.now();
    engine.react.failed_at.retain(|_, end| now.saturating_duration_since(*end) < repeat);
    let skip: Vec<i64> = engine.react.failed_at.keys().copied().collect();
    let next = match engine.store.read(|conn| reactions::next_due(conn, &skip)) {
        Ok(Some(next)) => next,
        Ok(None) => {
            engine.clear(Step::E17, &About::Machine);
            return;
        }
        Err(e) => {
            engine.fail(Step::E17, About::Machine, Failure::from(&e));
            return;
        }
    };
    match start(engine, &next) {
        Ok(answer) => engine.react.running = Some(Running { due: next, answer }),
        Err(failure) => {
            engine.react.failed_at.insert(next.id, now);
            engine.fail(Step::E17, About::Machine, failure);
        }
    }
}

/// Starts the helper for `post` on its own thread; returns where its answer will arrive.
fn start(engine: &Engine, due: &Due) -> Result<Receiver<Result<(), Failure>>, Failure> {
    let call = cpd_core::slack::call(
        &engine.config,
        "slack-react.ts",
        &[("channel", &due.channel), ("ts", &due.ts), ("emoji", &due.emoji)],
        None,
        DEADLINE,
    )
    .map_err(|e| Failure::from(&e))?;
    let runner = Ports::current().runner;
    let argv: Vec<String> = std::iter::once(call.program.clone()).chain(call.args.iter().cloned()).collect();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let answer = match runner.run(&call) {
            Ok(out) => cpd_core::slack::answer(&out, &argv).map(|_| ()).map_err(|e| Failure::from(&e)),
            Err(Failed::Io(e)) => Err(Failure::from(
                &CpdError::new(ErrorCode::SpawnFailed, format!("Cannot run {}: {e}.", argv.join(" "))).with("argv", json!(argv)),
            )),
            Err(Failed::Timeout(e)) => Err(Failure::from(&e)),
        };
        // The engine may have stopped: nobody to tell.
        let _ = tx.send(answer);
    });
    Ok(rx)
}

/// Sets the row's `reacted_at`, in one transaction.
fn mark(engine: &mut Engine, due: &Due) -> Result<(), Failure> {
    engine.store.write("engine E17", |tx| reactions::mark_reacted(tx, due.id)).map(|_| ()).map_err(|e| Failure::from(&e))
}
