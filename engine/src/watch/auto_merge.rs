//! E18, the pending auto-merge requests (commands.md §9.8b; pro-d115, pro-d117): a posted PR whose request is `pending`
//! (deferred because it was stacked, or its `gh` call failed) is retried through the core function `pr post` runs, detached
//! on its own thread, its answer collected at a later tick. `enabled` sets the row `enabled`; `not_allowed` (the repo lacks
//! auto-merge) sets it `none` with no Event; `refused` (GitHub's refusal, or no merge method) sets it `refused` with one
//! `error` Event for the Plan (`auto_merge_refused`); a failure leaves it `pending`, reported
//! once per code and tried again `watch.error_repeat_seconds` after the failed run's end.

use std::path::Path;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use super::SOURCE;
use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, NewEvent};
use cpd_core::ids::PlanId;
use cpd_core::ports::Ports;
use cpd_core::pr::auto_merge::{self, Outcome};
use cpd_core::pr::posts::{self, Pending};

/// A run in flight: the request and where its outcome will arrive.
struct Running {
    post: Pending,
    answer: Receiver<Outcome>,
}

/// What E18 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    running: Option<Running>,
    /// When the last run that failed ended; a success leaves it unset.
    last_end: Option<Instant>,
}

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E18) {
        engine.log.line("E18: holding: incident mode is on");
        return;
    }
    if let Some(running) = &engine.auto_merge.running {
        let outcome = match running.answer.try_recv() {
            Ok(outcome) => outcome,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                Outcome::Failed(CpdError::new(ErrorCode::SpawnFailed, "The auto-merge request's reader ended without an answer."))
            }
        };
        let post = engine.auto_merge.running.take().expect("a run was in flight").post;
        settle(engine, &post, outcome);
        return;
    }
    let repeat = Duration::from_secs(u64::from(engine.config.watch.error_repeat_seconds));
    if engine.auto_merge.last_end.is_some_and(|end| engine.ports.clock.now().saturating_duration_since(end) < repeat) {
        return;
    }
    let next = match engine.store.read(posts::next_pending) {
        Ok(Some(next)) => next,
        Ok(None) => {
            engine.clear(Step::E18, &About::Machine);
            return;
        }
        Err(e) => {
            engine.fail(Step::E18, About::Machine, Failure::from(&e));
            return;
        }
    };
    let runner = Ports::current().runner;
    let (repo, number) = (next.repo.clone(), next.number);
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        // The engine may have stopped: nobody to tell.
        let _ = tx.send(auto_merge::attempt(&*runner, Path::new("."), &repo, number));
    });
    engine.auto_merge.running = Some(Running { post: next, answer: rx });
}

/// Records `outcome` of `post`: the row, and for a refusal the Event, in one transaction.
fn settle(engine: &mut Engine, post: &Pending, outcome: Outcome) {
    let about = About::Plan(PlanId::parse(&post.plan).expect("a stored Plan id"));
    let not_allowed = matches!(outcome, Outcome::NotAllowed);
    let refused = match outcome {
        Outcome::Enabled | Outcome::NotAllowed => None,
        Outcome::Refused(why) => Some(why.split_whitespace().collect::<Vec<_>>().join(" ")),
        Outcome::Failed(e) => {
            engine.auto_merge.last_end = Some(engine.ports.clock.now());
            let mut failure = Failure::from(&e);
            if let Some(stderr) = failure.details["stderr"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                failure.message = format!("{} {stderr}", failure.message);
            }
            engine.fail(Step::E18, about, failure);
            return;
        }
    };
    let written = engine.store.write("engine E18", |tx| {
        posts::set_auto_merge(tx, &post.plan, &post.pr, match (&refused, &not_allowed) {
            (Some(_), _) => "refused",
            (None, true) => "none",
            (None, false) => "enabled",
        })?;
        if let Some(why) = &refused {
            let failure = Failure::from(&CpdError::new(
                ErrorCode::AutoMergeRefused,
                format!("Auto-merge for PR {}/{} (#{} of {}) was refused: {why}.", post.plan, post.pr, post.number, post.repo),
            ));
            let text = format!("{} See: cpd plan show --plan {}", failure.message, post.plan);
            let plan = PlanId::parse(&post.plan).expect("a stored Plan id");
            events::append(tx, NewEvent::error(text, SOURCE, &failure.code, &failure.message, &failure.details).plan(plan))?;
        }
        Ok(())
    });
    match written {
        Ok(()) => {
            engine.auto_merge.last_end = None;
            engine.clear(Step::E18, &about);
            if not_allowed {
                engine.log.line(&format!("E18: the repo of PR {}/{} does not allow auto-merge: row set to none", post.plan, post.pr));
            } else if refused.is_none() {
                engine.log.line(&format!("E18: enabled auto-merge for PR {}/{}", post.plan, post.pr));
            }
        }
        Err(e) => {
            engine.auto_merge.last_end = Some(engine.ports.clock.now());
            engine.fail(Step::E18, about, Failure::from(&e));
        }
    }
}
