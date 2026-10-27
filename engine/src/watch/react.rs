//! E17, the merge reaction (Plan pr-post, decision pro-d64 step 7; commands.md §21.8, §21.9): after a PR that was posted
//! in Slack has merged, the helper `pi/lib/slack-react.ts` adds the owner's emoji to the post, detached on its own
//! thread, and the row's `reacted_at` is set. A failure leaves `reacted_at` NULL, is reported once per code and the same
//! post is tried again `watch.error_repeat_seconds` after the failed run's end. (The Spec calls the step E16, a name the
//! janitor already holds.)

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::json;

use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::deadline::Failed;
use cpd_core::ports::Ports;
use cpd_core::pr::posts::{self, Unreacted};

/// The helper's deadline (contract §3.7): above its 30 s for the Slack call.
const DEADLINE: Duration = Duration::from_secs(120);

/// A run in flight: the post it reacts to and where its answer will arrive.
struct Running {
    post: Unreacted,
    answer: Receiver<Result<(), Failure>>,
}

/// What E17 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    running: Option<Running>,
    /// When the last run that failed ended (or failed to start); a success leaves it unset.
    last_end: Option<Instant>,
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
        let post = engine.react.running.take().expect("a run was in flight").post;
        match answer.and_then(|()| mark(engine, &post)) {
            Ok(()) => {
                engine.react.last_end = None;
                engine.log.line(&format!("E17: reacted to the post of PR {}/{}", post.plan, post.pr));
                engine.clear(Step::E17, &About::Machine);
            }
            Err(failure) => {
                engine.react.last_end = Some(engine.ports.clock.now());
                engine.fail(Step::E17, About::Machine, failure);
            }
        }
        return;
    }
    let repeat = Duration::from_secs(u64::from(engine.config.watch.error_repeat_seconds));
    if engine.react.last_end.is_some_and(|end| engine.ports.clock.now().saturating_duration_since(end) < repeat) {
        return;
    }
    let next = match engine.store.read(posts::next_unreacted) {
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
        Ok(answer) => engine.react.running = Some(Running { post: next, answer }),
        Err(failure) => {
            engine.react.last_end = Some(engine.ports.clock.now());
            engine.fail(Step::E17, About::Machine, failure);
        }
    }
}

/// Starts the helper for `post` on its own thread; returns where its answer will arrive.
fn start(engine: &Engine, post: &Unreacted) -> Result<Receiver<Result<(), Failure>>, Failure> {
    let call = cpd_core::slack::call(
        &engine.config,
        "slack-react.ts",
        &[("channel", &post.channel), ("ts", &post.ts), ("emoji", &post.reaction)],
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

/// Sets the post's `reacted_at`, in one transaction.
fn mark(engine: &mut Engine, post: &Unreacted) -> Result<(), Failure> {
    engine
        .store
        .write("engine E17", |tx| posts::mark_reacted(tx, &post.plan, &post.pr))
        .map(|_| ())
        .map_err(|e| Failure::from(&e))
}
