//! E23, the PR post bump (contract §3.7, §4.4): every 24 h while the PR of a post is open, not a draft and unreviewed, the
//! helper `pi/lib/slack-post.ts` replies `Bibbidi-bobbidi-bump!` in the post's thread, broadcast to the channel, detached on its
//! own thread, one run at a time; on success the row's `last_posted_at` is set (`posted_at` never changes). A failure leaves the
//! row due and it is tried again `watch.error_repeat_seconds` after the failed run's end; the rows behind it go on meanwhile.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::json;

use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::clock::timestamp;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::deadline::Failed;
use cpd_core::ports::Ports;
use cpd_core::pr::posts::{self, Bump};

/// The helper's deadline (contract §3.7): above its 30 s for the Slack call.
const DEADLINE: Duration = Duration::from_secs(120);

/// The interval between two posts of a thread.
const INTERVAL: time::Duration = time::Duration::seconds(86_400);

/// The text of a bump.
const TEXT: &str = "Bibbidi-bobbidi-bump!";

type Key = (String, String);

/// A run in flight: the post it bumps and where its answer will arrive.
struct Running {
    bump: Bump,
    answer: Receiver<Result<(), Failure>>,
}

/// What E23 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    running: Option<Running>,
    /// When the last run of a post that failed ended (or failed to start), by (Plan, Workflow); in memory only.
    failed_at: HashMap<Key, Instant>,
}

fn key(bump: &Bump) -> Key {
    (bump.plan.clone(), bump.workflow.clone())
}

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E23) {
        engine.log.line("E23: holding: incident mode is on");
        return;
    }
    if let Some(running) = &engine.bump.running {
        let answer = match running.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(Failure::from(&CpdError::new(
                ErrorCode::SpawnFailed,
                "The bump helper's reader ended without an answer.",
            ))),
        };
        let bump = engine
            .bump
            .running
            .take()
            .expect("a run was in flight")
            .bump;
        match answer.and_then(|()| mark(engine, &bump)) {
            Ok(()) => {
                engine.bump.failed_at.remove(&key(&bump));
                engine.log.line(&format!(
                    "E23: bumped {} {} {}",
                    bump.plan, bump.workflow, bump.ts
                ));
                engine.clear(Step::E23, &About::Machine);
            }
            Err(failure) => {
                engine
                    .bump
                    .failed_at
                    .insert(key(&bump), engine.ports.clock.now());
                engine.fail(Step::E23, About::Machine, failure);
            }
        }
        return;
    }
    let repeat = Duration::from_secs(u64::from(engine.config.watch.error_repeat_seconds));
    let now = engine.ports.clock.now();
    engine
        .bump
        .failed_at
        .retain(|_, end| now.saturating_duration_since(*end) < repeat);
    let skip: Vec<Key> = engine.bump.failed_at.keys().cloned().collect();
    let cutoff = timestamp(engine.ports.clock.utc() - INTERVAL);
    let next = match engine
        .store
        .read(|conn| posts::next_bump(conn, &cutoff, &skip))
    {
        Ok(Some(next)) => next,
        Ok(None) => {
            engine.clear(Step::E23, &About::Machine);
            return;
        }
        Err(e) => {
            engine.fail(Step::E23, About::Machine, Failure::from(&e));
            return;
        }
    };
    match start(engine, &next) {
        Ok(answer) => engine.bump.running = Some(Running { bump: next, answer }),
        Err(failure) => {
            engine.bump.failed_at.insert(key(&next), now);
            engine.fail(Step::E23, About::Machine, failure);
        }
    }
}

/// Starts the helper for `bump` on its own thread; returns where its answer will arrive.
fn start(engine: &Engine, bump: &Bump) -> Result<Receiver<Result<(), Failure>>, Failure> {
    let call = cpd_core::slack::call(
        &engine.config,
        "slack-post.ts",
        &[
            ("channel", &bump.channel),
            ("thread-ts", &bump.ts),
            ("reply-broadcast", "true"),
        ],
        Some(TEXT.as_bytes().to_vec()),
        DEADLINE,
    )
    .map_err(|e| Failure::from(&e))?;
    let runner = Ports::current().runner;
    let argv: Vec<String> = std::iter::once(call.program.clone())
        .chain(call.args.iter().cloned())
        .collect();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let answer = match runner.run(&call) {
            Ok(out) => cpd_core::slack::answer(&out, &argv)
                .map(|_| ())
                .map_err(|e| Failure::from(&e)),
            Err(Failed::Io(e)) => Err(Failure::from(
                &CpdError::new(
                    ErrorCode::SpawnFailed,
                    format!("Cannot run {}: {e}.", argv.join(" ")),
                )
                .with("argv", json!(argv)),
            )),
            Err(Failed::Timeout(e)) => Err(Failure::from(&e)),
        };
        // The engine may have stopped: nobody to tell.
        let _ = tx.send(answer);
    });
    Ok(rx)
}

/// Sets the post's `last_posted_at`, in one transaction.
fn mark(engine: &mut Engine, bump: &Bump) -> Result<(), Failure> {
    engine
        .store
        .write("engine E23", |tx| {
            posts::mark_bumped(tx, &bump.plan, &bump.workflow)
        })
        .map(|_| ())
        .map_err(|e| Failure::from(&e))
}

#[cfg(test)]
#[path = "bump_tests.rs"]
mod bump_tests;
