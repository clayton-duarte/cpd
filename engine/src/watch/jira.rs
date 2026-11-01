//! E19, the Jira transition queue (Plan jira-sync; pro-d126, pro-d128; contract §3.7 E19; commands.md §21.11, §21.12): the helper
//! `pi/lib/jira-transition.ts` moves the ticket of the OLDEST PENDING `jira_transitions` row whose PR has a ticket and is not
//! cancelled, detached on its own thread, one run at a time, 120 s deadline, as E17 does. The ticket, the target and the three
//! status names are read when the row runs. A target whose name is empty settles the row `skipped` without a call. An `ok`
//! answer settles it (`moved`, `unchanged`); a `jira_refused` answer parks it with one `error` Event, and each tick a parked row
//! whose ticket or status name now differs from what was refused is reopened. Any other failure leaves the row PENDING and is
//! skipped for `watch.error_repeat_seconds` after the failed run's end; the rows behind it go on. A row failure is one `error`
//! Event scoped to the row's Plan (`About::Move`), once per code until the row ends (settles, is skipped or parked), naming the
//! ticket, the status and Jira's message; `jira_auth` and `jira_not_configured` stay one machine-wide Event each.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::SOURCE;
use super::engine::{Engine, incident_holds};
use super::report::{About, Failure, Step};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::deadline::Failed;
use cpd_core::ids::{PlanId, WorkflowKey};
use cpd_core::jira::{Done, Names, Request};
use cpd_core::policy::Policy;
use cpd_core::ports::Ports;
use cpd_core::pr::jira_sync::{self, Due, Outcome, Refusal, Target};

/// The helper's deadline (contract §3.7): above its 30 s per Jira call.
const DEADLINE: Duration = Duration::from_secs(120);

/// A run in flight: the queue row, the status name it uses and where its answer will arrive.
struct Running {
    due: Due,
    status: String,
    answer: Receiver<Result<Done, Failure>>,
}

/// What E19 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    running: Option<Running>,
    /// When the last run of a row that failed ended (or failed to start), by row id; in memory only.
    failed_at: HashMap<i64, Instant>,
}

fn name_of(names: &Names, target: Target) -> &str {
    match target {
        Target::InProgress => &names.in_progress,
        Target::InReview => &names.in_review,
        Target::Done => &names.done,
    }
}

fn names_of(policy: &Policy) -> Names {
    let jira = &policy.workflow.jira;
    Names { in_progress: jira.in_progress.clone(), in_review: jira.in_review.clone(), done: jira.done.clone() }
}

fn about(plan: &str) -> About {
    About::Plan(PlanId::parse(plan).expect("a stored Plan id"))
}

/// The policy of the PR of `plan`/`pr`, resolved now.
fn policy_of(engine: &mut Engine, plan: &str, workflow: &str) -> Result<Policy, CpdError> {
    let home = engine.home.clone();
    let plan = PlanId::parse(plan)?;
    let key = WorkflowKey::parse(workflow)?;
    engine.store.read(|conn| cpd_core::pr::policy_of_workflow(conn, &home, &plan, &key))
}

fn move_about(due: &Due) -> About {
    About::Move(PlanId::parse(&due.plan).expect("a stored Plan id"), WorkflowKey::parse(&due.workflow).expect("a stored PR key"), due.target)
}

/// Reports a failure of the row `due`: machine-wide for a missing sign-in or server, else one Event of the row's Plan.
fn fail_row(engine: &mut Engine, due: &Due, status: &str, failure: Failure) {
    if failure.code == "jira_auth" || failure.code == "jira_not_configured" {
        engine.fail(Step::E19, About::Machine, failure);
        return;
    }
    let mut details = match failure.details {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    details.insert("plan".into(), json!(due.plan));
    details.insert("pr".into(), json!(due.workflow));
    details.insert("ticket".into(), json!(due.ticket));
    details.insert("target".into(), json!(due.target.as_str()));
    details.insert("status".into(), json!(status));
    let message = format!("{} could not be moved to \"{status}\": {}", due.ticket, failure.message);
    engine.fail(Step::E19, move_about(due), Failure { code: failure.code, message, details: Value::Object(details) });
}

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E19) {
        engine.log.line("E19: holding: incident mode is on");
        return;
    }
    if engine.jira.running.is_some() {
        collect(engine);
        return;
    }
    reopen_parked(engine);
    let repeat = Duration::from_secs(u64::from(engine.config.watch.error_repeat_seconds));
    let now = engine.ports.clock.now();
    engine.jira.failed_at.retain(|_, end| now.saturating_duration_since(*end) < repeat);
    loop {
        let skip: Vec<i64> = engine.jira.failed_at.keys().copied().collect();
        let due = match engine.store.read(|conn| jira_sync::next_due(conn, &skip)) {
            Ok(Some(due)) => due,
            Ok(None) => {
                engine.clear(Step::E19, &About::Machine);
                return;
            }
            Err(e) => {
                engine.fail(Step::E19, About::Machine, Failure::from(&e));
                return;
            }
        };
        let policy = match policy_of(engine, &due.plan, &due.workflow) {
            Ok(policy) => policy,
            Err(e) => {
                engine.jira.failed_at.insert(due.id, now);
                engine.fail(Step::E19, about(&due.plan), Failure::from(&e));
                continue;
            }
        };
        engine.clear(Step::E19, &about(&due.plan));
        let names = names_of(&policy);
        let status = name_of(&names, due.target).to_owned();
        if status.is_empty() {
            match engine.store.write("engine E19", |tx| jira_sync::mark_settled(tx, due.id, Outcome::Skipped)) {
                Ok(_) => engine.clear(Step::E19, &move_about(&due)),
                Err(e) => {
                    engine.jira.failed_at.insert(due.id, now);
                    engine.fail(Step::E19, about(&due.plan), Failure::from(&e));
                }
            }
            continue;
        }
        match start(engine, &due, names, policy.workflow.jira.estimate_fields.clone()) {
            Ok(answer) => engine.jira.running = Some(Running { due, status, answer }),
            Err(failure) => {
                engine.jira.failed_at.insert(due.id, now);
                fail_row(engine, &due, &status, failure);
            }
        }
        return;
    }
}

/// Reads the answer of the run in flight, if it has come.
fn collect(engine: &mut Engine) {
    let running = engine.jira.running.as_ref().expect("a run was in flight");
    let answer = match running.answer.try_recv() {
        Ok(answer) => answer,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => {
            Err(Failure::from(&CpdError::new(ErrorCode::SpawnFailed, "The Jira helper's reader ended without an answer.")))
        }
    };
    let Running { due, status, .. } = engine.jira.running.take().expect("a run was in flight");
    let ended = engine.ports.clock.now();
    match answer {
        Ok(done) => {
            let outcome = if done == Done::Moved { Outcome::Moved } else { Outcome::Unchanged };
            match engine.store.write("engine E19", |tx| jira_sync::mark_settled(tx, due.id, outcome)) {
                Ok(_) => {
                    engine.jira.failed_at.remove(&due.id);
                    engine.log.line(&format!("E19: {} {} {} ({}): {}", outcome.as_str(), due.ticket, due.target.as_str(), due.workflow, status));
                    engine.clear(Step::E19, &About::Machine);
                    engine.clear(Step::E19, &move_about(&due));
                }
                Err(e) => {
                    engine.jira.failed_at.insert(due.id, ended);
                    fail_row(engine, &due, &status, Failure::from(&e));
                }
            }
        }
        Err(failure) if failure.code == "jira_refused" => {
            let reason = failure.details["reason"].as_str().unwrap_or("unknown").to_owned();
            let refusal = Refusal { reason: reason.clone(), message: failure.message.clone(), ticket: due.ticket.clone(), status: status.clone() };
            match engine.store.write("engine E19", |tx| jira_sync::refuse(tx, SOURCE, due.id, &refusal)) {
                Ok(_) => {
                    engine.jira.failed_at.remove(&due.id);
                    engine.log.line(&format!("E19: parked {} {} on {} of {}: {reason}", due.ticket, due.target.as_str(), due.workflow, due.plan));
                    engine.clear(Step::E19, &About::Machine);
                    engine.clear(Step::E19, &move_about(&due));
                }
                Err(e) => {
                    engine.jira.failed_at.insert(due.id, ended);
                    fail_row(engine, &due, &status, Failure::from(&e));
                }
            }
        }
        Err(failure) => {
            engine.jira.failed_at.insert(due.id, ended);
            fail_row(engine, &due, &status, failure);
        }
    }
}

/// Reopens each parked row whose ticket or target's status name now differs from what was refused.
fn reopen_parked(engine: &mut Engine) {
    let parked = match engine.store.read(jira_sync::parked) {
        Ok(parked) => parked,
        Err(e) => {
            engine.fail(Step::E19, About::Machine, Failure::from(&e));
            return;
        }
    };
    for row in parked {
        let due = row.due;
        match policy_of(engine, &due.plan, &due.workflow) {
            Err(e) => engine.fail(Step::E19, about(&due.plan), Failure::from(&e)),
            Ok(policy) => {
                engine.clear(Step::E19, &about(&due.plan));
                let name = name_of(&names_of(&policy), due.target).to_owned();
                if !name.is_empty() && format!("{}|{name}", due.ticket) != row.tried {
                    match engine.store.write("engine E19", |tx| jira_sync::reopen(tx, due.id)) {
                        Ok(_) => engine.log.line(&format!("E19: reopened {} {} on {} of {}", due.ticket, due.target.as_str(), due.workflow, due.plan)),
                        Err(e) => engine.fail(Step::E19, about(&due.plan), Failure::from(&e)),
                    }
                }
            }
        }
    }
}

/// Starts the helper for `due` on its own thread; returns where its answer will arrive.
fn start(engine: &Engine, due: &Due, names: Names, estimate_fields: Vec<String>) -> Result<Receiver<Result<Done, Failure>>, Failure> {
    let request = Request { ticket: due.ticket.clone(), target: due.target, names, estimate_fields };
    let call = cpd_core::jira::call(&engine.config, &engine.home, &request, DEADLINE).map_err(|e| Failure::from(&e))?;
    let runner = Ports::current().runner;
    let argv: Vec<String> = std::iter::once(call.program.clone()).chain(call.args.iter().cloned()).collect();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let answer = match runner.run(&call) {
            Ok(out) => cpd_core::jira::answer(&out, &argv).map_err(|e| Failure::from(&e)),
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
