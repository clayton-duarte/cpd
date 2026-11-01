//! `answer` (commands.md §8.2, contract §3.5): `open → answered`.

use rusqlite::OptionalExtension;
use serde_json::json;

use super::{AnswerArgs, Env};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{AttemptId, JobKey};
use crate::output::Output;

const COMMAND: &str = "answer";

pub(super) fn answer(args: AnswerArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let id = args.escalation;
    let attempt = id.attempt().clone();
    let job: JobKey = attempt.job().clone();
    let mut env = Env::open()?;

    let answered_at = env.store.write(COMMAND, |tx| {
        let status: String = tx
            .query_row("SELECT status FROM escalations WHERE plan = ?1 AND id = ?2", [plan.as_str(), &id.to_string()], |r| r.get(0))
            .optional()?
            .ok_or_else(|| {
                CpdError::new(ErrorCode::EscalationNotFound, format!("No Escalation {id} in Plan {plan}."))
                    .with("plan", plan.as_str())
                    .with("escalation", id.to_string())
            })?;
        let refusal = match status.as_str() {
            "open" => None,
            "answered" => Some((ErrorCode::EscalationAnswered, "answered")),
            "expired" => Some((ErrorCode::EscalationExpired, "expired")),
            other => return Err(CpdError::new(ErrorCode::StoreFailed, format!("Escalation {id} has status {other:?}."))),
        };
        if let Some((code, state)) = refusal {
            return Err(CpdError::new(code, format!("Escalation {id} is already {state}: only an open one can be answered."))
                .with("plan", plan.as_str())
                .with("escalation", id.to_string())
                .with("status", state));
        }
        tx.execute(
            "UPDATE escalations SET status = 'answered', answer = ?3, answered_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?1 AND id = ?2 AND status = 'open'",
            [plan.as_str(), &id.to_string(), args.answer.as_str()],
        )?;
        let at: String =
            tx.query_row("SELECT answered_at FROM escalations WHERE plan = ?1 AND id = ?2", [plan.as_str(), &id.to_string()], |r| r.get(0))?;
        let text = format!("Escalation {id} of Attempt {attempt} was answered. Next: cpd attempt show {attempt} --plan {plan}");
        append(
            tx,
            NewEvent::escalation_answered(text, id.to_string())
                .plan(plan.clone())
                .job(job.clone())
                .attempt(AttemptId::clone(&attempt)),
        )?;
        let workflow: Option<String> = tx
            .query_row("SELECT workflow FROM jobs WHERE plan = ?1 AND key = ?2", [plan.as_str(), job.as_str()], |r| r.get(0))
            .optional()?
            .flatten();
        if let Some(workflow) = workflow {
            crate::session::working_pr::name(tx, &plan, &crate::ids::WorkflowKey::parse(&workflow)?)?;
        }
        Ok(at)
    })?;
    Ok(Output::success(
        json!({"escalation": id, "attempt": attempt, "answered_at": answered_at}),
    ))
}
