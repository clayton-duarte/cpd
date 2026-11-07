//! `ask` (commands.md §8.1, contract §3.3, §3.5): `(none) → open`, then the
//! wait, then `open → answered` (by `answer`) or `open → expired` (here).

use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{AskArgs, Env};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{AttemptId, EscalationId, JobKey, PlanId, SessionId};
use crate::output::Output;
use crate::store::WriteTxn;

const COMMAND: &str = "ask";

/// Why the Worker Session is `waiting` while it waits (commands.md §8.1): for its Lead, not for the User (hs-d303).
pub(crate) const LABEL: &str = "φ: question to the Lead";

/// Why a Lead Session is `waiting` on a Worker's question (hs-d303).
pub(crate) const WORKER_QUESTION_LABEL: &str = "φ: Worker question";

/// How often `ask` reads its row (§8.1).
const POLL: Duration = Duration::from_millis(500);

/// The Attempt that asks, as `ask` needs it.
struct Asker {
    plan: PlanId,
    attempt: AttemptId,
    job: JobKey,
    session: SessionId,
}

/// How the wait ended.
enum Settled {
    Answered(String),
    TimedOut,
}

/// [`ask`] for Attempt `attempt` of `plan` on the ports of `env`: the deadline and the 500 ms polls are read on
/// `env.ports.clock`. `timeout_minutes` is `--timeout-minutes`.
pub(crate) fn ask_in(
    env: &mut Env,
    plan: &PlanId,
    attempt: &AttemptId,
    question: &str,
    timeout_minutes: Option<u32>,
) -> Result<Output, CpdError> {
    let minutes = timeout_minutes.unwrap_or(env.config.timeouts.ask_minutes);
    let clock = env.ports.clock.clone();
    let deadline = clock.now() + Duration::from_secs(u64::from(minutes) * 60);

    let (asker, id) = env
        .store
        .write(COMMAND, |tx| open(tx, plan, attempt, question))?;

    let settled = loop {
        let (status, _) = env.store.read(|conn| row(conn, &asker.plan, &id))?;
        let open = status == "open";
        if !open || clock.now() >= deadline {
            break settle(env, &asker, &id, open)?;
        }
        clock.sleep(POLL);
    };
    match settled {
        Settled::Answered(answer) => {
            Ok(Output::success(json!({"escalation": id, "answer": answer})))
        }
        Settled::TimedOut => Err(CpdError::new(
            ErrorCode::AskTimeout,
            format!("Escalation {id} was not answered within {minutes} minutes."),
        )
        .with("plan", asker.plan.as_str())
        .with("escalation", id.to_string())
        .with("timeout_minutes", minutes)),
    }
}

pub(super) fn ask(args: AskArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let mut env = Env::open()?;
    ask_in(
        &mut env,
        &plan,
        &args.attempt,
        args.question.as_str(),
        args.timeout_minutes,
    )
}

/// Transaction 1: the Escalation, the Worker Session `waiting`, their Events.
fn open(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    attempt: &AttemptId,
    question: &str,
) -> Result<(Asker, EscalationId), CpdError> {
    let found: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT status, worker_session FROM attempts WHERE plan = ?1 AND id = ?2",
            [plan.as_str(), &attempt.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((status, session)) = found else {
        let plan_exists: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM plans WHERE id = ?1)",
            [plan.as_str()],
            |r| r.get(0),
        )?;
        return Err(if plan_exists {
            CpdError::new(
                ErrorCode::AttemptNotFound,
                format!("No Attempt {attempt} in Plan {plan}."),
            )
            .with("plan", plan.as_str())
            .with("attempt", attempt.to_string())
        } else {
            CpdError::plan_not_found(plan)
        });
    };
    if status != "running" {
        return Err(CpdError::new(
            ErrorCode::AttemptNotRunning,
            format!("Attempt {attempt} is {status}: only a running Attempt can ask."),
        )
        .with("plan", plan.as_str())
        .with("attempt", attempt.to_string())
        .with("status", status));
    }
    let session = session
        .and_then(|s| SessionId::parse(&s).ok())
        .ok_or_else(|| {
            CpdError::bad_row(format!(
                "Attempt {attempt} is running with no valid Worker Session"
            ))
        })?;
    let asker = Asker {
        plan: plan.clone(),
        attempt: attempt.clone(),
        job: attempt.job().clone(),
        session,
    };

    let number: u32 = tx.query_row(
        "SELECT coalesce(max(number), 0) + 1 FROM escalations WHERE plan = ?1 AND attempt = ?2",
        [plan.as_str(), &attempt.to_string()],
        |r| r.get(0),
    )?;
    let number = std::num::NonZeroU32::new(number).expect("max + 1 is at least 1");
    let id = EscalationId::new(attempt.clone(), number);
    tx.execute(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6, (SELECT at FROM txns WHERE id = cpd_txn()))",
        rusqlite::params![
            plan.as_str(),
            id.to_string(),
            attempt.to_string(),
            number.get(),
            question,
            std::process::id()
        ],
    )?;
    let text = format!(
        "Worker of Job {job} (Attempt {attempt}) asks: {question} Next: cpd answer {id} \"<answer>\" --plan {plan}",
        job = asker.job
    );
    append(
        tx,
        NewEvent::escalation(text, id.to_string(), question)
            .plan(plan.clone())
            .job(asker.job.clone())
            .attempt(attempt.clone()),
    )?;
    crate::events::escalation(tx, plan, &asker.job, question, &id)?;
    wait(tx, &asker)?;
    Ok((asker, id))
}

/// The Worker Session `waiting` with [`LABEL`] (§3.6: from `working` or `idle`;
/// a changed label on a `waiting` one). A `stopped` Session cannot ask.
fn wait(tx: &WriteTxn<'_>, asker: &Asker) -> Result<(), CpdError> {
    let (state, label, name) = session_row(tx, &asker.session)?;
    if state == "stopped" {
        return Err(CpdError::new(
            ErrorCode::IllegalTransition,
            format!(
                "Session {} is stopped: it cannot go to waiting (contract §3.6).",
                asker.session
            ),
        )
        .with("session", asker.session.as_str())
        .with("from", "stopped")
        .with("to", "waiting"));
    }
    if state == "waiting" && label.as_deref() == Some(LABEL) {
        return Ok(());
    }
    set_state(tx, asker, &state, "waiting", Some(LABEL), &name)
}

/// The Worker Session `working` again, when it is `waiting`.
fn resume(tx: &WriteTxn<'_>, asker: &Asker) -> Result<(), CpdError> {
    let (state, _, name) = session_row(tx, &asker.session)?;
    if state != "waiting" {
        return Ok(());
    }
    set_state(tx, asker, &state, "working", None, &name)
}

/// The Worker Session of an Attempt `working` again, once the Escalation that held it `waiting` is
/// settled (engine E1, contract §3.7): nothing when the Attempt has no Worker Session or another of its
/// Escalations is still `open`; a Session that is not `waiting` is left as it is.
pub fn resume_attempt(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    attempt: &AttemptId,
) -> Result<(), CpdError> {
    let session: Option<String> = tx
        .query_row(
            "SELECT worker_session FROM attempts WHERE plan = ?1 AND id = ?2",
            [plan.as_str(), &attempt.to_string()],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let Some(session) = session else {
        return Ok(());
    };
    let session = SessionId::parse(&session).map_err(|_| {
        CpdError::bad_row(format!("Attempt {attempt} has an invalid Worker Session"))
    })?;
    let still_open: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM escalations WHERE plan = ?1 AND attempt = ?2 AND status = 'open')",
        [plan.as_str(), &attempt.to_string()],
        |r| r.get(0),
    )?;
    if still_open {
        return Ok(());
    }
    resume(
        tx,
        &Asker {
            plan: plan.clone(),
            attempt: attempt.clone(),
            job: attempt.job().clone(),
            session,
        },
    )
}

fn session_row(
    conn: &Connection,
    id: &SessionId,
) -> Result<(String, Option<String>, String), CpdError> {
    conn.query_row(
        "SELECT state, state_label, name FROM sessions WHERE id = ?1",
        [id.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()?
    .ok_or_else(|| {
        CpdError::new(ErrorCode::SessionNotFound, format!("No Session {id}."))
            .with("session", id.as_str())
    })
}

fn set_state(
    tx: &WriteTxn<'_>,
    asker: &Asker,
    from: &str,
    to: &str,
    label: Option<&str>,
    name: &str,
) -> Result<(), CpdError> {
    tx.execute(
        "UPDATE sessions SET state = ?1, state_label = ?2, state_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?3",
        rusqlite::params![to, label, asker.session.as_str()],
    )?;
    let text = match label {
        Some(label) => format!("Session {name} is {to} ({label})."),
        None => format!("Session {name} is {to}."),
    };
    append(
        tx,
        NewEvent::session_state(text, from, to, label, false)
            .session(asker.session.clone())
            .plan(asker.plan.clone()),
    )?;
    Ok(())
}

/// The Escalation's `status` and `answer`.
fn row(
    conn: &Connection,
    plan: &PlanId,
    id: &EscalationId,
) -> Result<(String, Option<String>), CpdError> {
    Ok(conn.query_row(
        "SELECT status, answer FROM escalations WHERE plan = ?1 AND id = ?2",
        [plan.as_str(), &id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// Transaction 2: the end of the wait. When the Escalation is still `open` and
/// `timed_out`, it expires; otherwise it is whatever another process made it.
/// The Worker Session goes back to `working` either way.
fn settle(
    env: &mut Env,
    asker: &Asker,
    id: &EscalationId,
    timed_out: bool,
) -> Result<Settled, CpdError> {
    env.store.write(COMMAND, |tx| {
        let expired = timed_out
            && tx.execute(
                "UPDATE escalations SET status = 'expired', expired_at = (SELECT at FROM txns WHERE id = cpd_txn())
                  WHERE plan = ?1 AND id = ?2 AND status = 'open'",
                [asker.plan.as_str(), &id.to_string()],
            )? == 1;
        if expired {
            let text = format!("Escalation {id} of Attempt {} expired: no answer in time.", asker.attempt);
            append(
                tx,
                NewEvent::escalation_expired(text, id.to_string(), "timeout")
                    .plan(asker.plan.clone())
                    .job(asker.job.clone())
                    .attempt(asker.attempt.clone()),
            )?;
        }
        let (status, answer) = row(tx, &asker.plan, id)?;
        resume(tx, asker)?;
        match (status.as_str(), answer) {
            ("answered", Some(answer)) => Ok(Settled::Answered(answer)),
            ("expired", _) => Ok(Settled::TimedOut),
            _ => Err(CpdError::bad_row(format!("Escalation {id} is {status} after the wait"))),
        }
    })
}
