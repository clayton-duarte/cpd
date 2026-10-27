//! E17, the Slack reaction queue (Plan pr-post, pro-d64 step 7; pro-d107, pro-d110, pro-d124, pro-d125; commands.md §21.9): the
//! helper `pi/lib/slack-react.ts` adds the queued emoji to the post of the OLDEST `slack_reactions` row that is neither reacted nor
//! refused, detached on its own thread, one run at a time, and the row's `reacted_at` is set. The emoji of a merge reaction (a row
//! with its Plan and PR) is resolved from the PR's policy (`pr.post.merged_reaction`) each time it is about to be placed. A
//! transient failure leaves the row open, is reported once per code and the row is tried again `watch.error_repeat_seconds`
//! after the failed run's end; the rows behind it go on meanwhile. A permanent refusal (`slack_refused`), or a policy that names
//! no emoji (`policy_empty`), parks the row (`refused_at`) with one `error` Event, in one transaction; a parked row is never
//! picked again, and each tick a parked merge row whose policy now names another emoji is reopened. (The docs call the step E16
//! in places, a name the janitor already holds; its number is E17.)

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::json;

use super::SOURCE;
use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::deadline::Failed;
use cpd_core::ids::PlanId;
use cpd_core::ports::Ports;
use cpd_core::pr::reactions::{self, Due, MergeOf};

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
        if let Err(failure) = &answer {
            if failure.code == "slack_refused" {
                let reason = failure.details["reason"].as_str().unwrap_or("unknown").to_owned();
                match park(engine, &due, &reason) {
                    Ok(()) => {
                        engine.react.failed_at.remove(&due.id);
                        engine.log.line(&format!("E17: parked :{}: on {} {}: {reason}", due.emoji, due.channel, due.ts));
                        engine.clear(Step::E17, &About::Machine);
                    }
                    Err(failure) => {
                        engine.react.failed_at.insert(due.id, engine.ports.clock.now());
                        engine.fail(Step::E17, About::Machine, failure);
                    }
                }
                return;
            }
        }
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
    reopen_parked(engine);
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
    let mut next = next;
    if let Some(merge) = next.merge.clone() {
        let about = About::Plan(PlanId::parse(&merge.plan).expect("a stored Plan id"));
        let home = engine.home.clone();
        let resolved = engine.store.read(|conn| reactions::policy_emoji(conn, &home, &merge.plan, &merge.pr));
        let resolved = match resolved {
            Ok(resolved) => resolved,
            Err(e) => {
                engine.react.failed_at.insert(next.id, now);
                engine.fail(Step::E17, about, Failure::from(&e));
                return;
            }
        };
        engine.clear(Step::E17, &about);
        if resolved.emoji.is_empty() {
            if let Err(failure) = refuse(engine, next.id, "policy_empty", Some(&resolved.file)) {
                engine.react.failed_at.insert(next.id, now);
                engine.fail(Step::E17, about, failure);
            }
            return;
        }
        if resolved.emoji != next.emoji {
            let (id, emoji) = (next.id, resolved.emoji.clone());
            if let Err(e) = engine.store.write("engine E17", |tx| reactions::set_emoji(tx, id, &emoji)) {
                engine.react.failed_at.insert(next.id, now);
                engine.fail(Step::E17, about, Failure::from(&e));
                return;
            }
            next.emoji = resolved.emoji;
        }
    }
    match start(engine, &next) {
        Ok(answer) => engine.react.running = Some(Running { due: next, answer }),
        Err(failure) => {
            engine.react.failed_at.insert(next.id, now);
            engine.fail(Step::E17, About::Machine, failure);
        }
    }
}

/// Reopens each parked merge row whose PR's policy now names another, non-empty emoji.
fn reopen_parked(engine: &mut Engine) {
    let parked = match engine.store.read(reactions::parked) {
        Ok(parked) => parked,
        Err(e) => {
            engine.fail(Step::E17, About::Machine, Failure::from(&e));
            return;
        }
    };
    for row in parked {
        let Some(MergeOf { plan, pr }) = row.merge.clone() else { continue };
        let about = About::Plan(PlanId::parse(&plan).expect("a stored Plan id"));
        let home = engine.home.clone();
        match engine.store.read(|conn| reactions::policy_emoji(conn, &home, &plan, &pr)) {
            Err(e) => engine.fail(Step::E17, about, Failure::from(&e)),
            Ok(resolved) => {
                engine.clear(Step::E17, &about);
                if !resolved.emoji.is_empty() && resolved.emoji != row.emoji {
                    let (id, emoji) = (row.id, resolved.emoji.clone());
                    match engine.store.write("engine E17", |tx| reactions::reopen(tx, id, &emoji)) {
                        Ok(_) => engine.log.line(&format!("E17: reopened {} {} with :{}:", row.channel, row.ts, resolved.emoji)),
                        Err(e) => engine.fail(Step::E17, About::Plan(PlanId::parse(&plan).expect("a stored Plan id")), Failure::from(&e)),
                    }
                }
            }
        }
    }
}

/// Parks the row of a refused run; the policy file is named for a merge reaction.
fn park(engine: &mut Engine, due: &Due, reason: &str) -> Result<(), Failure> {
    let file = match &due.merge {
        Some(merge) => {
            let home = engine.home.clone();
            let resolved = engine.store.read(|conn| reactions::policy_emoji(conn, &home, &merge.plan, &merge.pr)).map_err(|e| Failure::from(&e))?;
            Some(resolved.file)
        }
        None => None,
    };
    refuse(engine, due.id, reason, file.as_deref())
}

/// Parks row `id` with its one Event, in one transaction.
fn refuse(engine: &mut Engine, id: i64, reason: &str, file: Option<&std::path::Path>) -> Result<(), Failure> {
    engine.store.write("engine E17", |tx| reactions::refuse(tx, SOURCE, id, reason, file)).map(|_| ()).map_err(|e| Failure::from(&e))
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
