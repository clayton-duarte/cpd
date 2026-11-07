//! The Manager's waking message for a Lead-less shaping Plan's Job Events (contract §5.3 "The Events of a Lead-less
//! Plan", §5.3 "The Manager of a Lead-less Plan"; commands.md §4.1, §5.1, §8.1, §8.2; air-d19, air-d22).
//!
//! When cpd appends a `job-merged`, a `job-failed` or an `escalation` Event (the open of a `cpd ask`) for a Plan whose
//! `lead_session` is NULL, it appends in the same transaction one `session-message` Event to the live Manager of the
//! Plan's Context, as engine E8's `tell_manager_of` does: `from` null, `wake` true. The message's `data.text` is the
//! triggering Event's `text` for `job-merged` and `job-failed`; for an Escalation it names the Worker's agent key, the
//! question and the answer command. With no live Manager nothing is written, and a Plan with a Lead is unchanged.

use rusqlite::OptionalExtension;

use super::kind::Kind;
use crate::agent_key::AgentKey;
use crate::error::CpdError;
use crate::ids::{ContextId, EscalationId, JobKey, PlanId, SessionId};
use crate::store::WriteTxn;

/// How much of the question the Escalation message quotes (contract §5.3).
const QUESTION_CHARS: usize = 300;

/// The Manager's message for the Job Event `event` (its id in `events`), appended in the caller's transaction: the
/// Event's own `text` for `job-merged` and `job-failed`, nothing for every other kind. Nothing when the Plan has a Lead
/// or its Context has no live Manager.
pub fn job_event(tx: &WriteTxn<'_>, event: i64) -> Result<(), CpdError> {
    let row: Option<(String, String, Option<String>)> = tx
        .query_row(
            "SELECT kind, text, plan FROM events WHERE id = ?1",
            [event],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((kind, text, plan)) = row else {
        return Ok(());
    };
    let kind: Kind = kind.parse()?;
    if !matches!(kind, Kind::JobMerged | Kind::JobFailed) {
        return Ok(());
    }
    let Some(plan) = plan else {
        return Ok(());
    };
    shaping_manager(tx, &PlanId::parse(&plan)?, &text)
}

/// The Manager's message for the Escalation `id` a Worker of Job `job` opened on `plan`, in the caller's transaction:
/// `Worker <agent-key> of Plan <plan> asks: "<question>" (escalation <id>). Answer with: cpd answer <id> "<text>"
/// --plan <plan>`, the question cut to [`QUESTION_CHARS`] (contract §5.3). Nothing when the Plan has a Lead or its
/// Context has no live Manager.
pub fn escalation(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    job: &JobKey,
    question: &str,
    id: &EscalationId,
) -> Result<(), CpdError> {
    let Some((lead, context)) = plan_lead_and_context(tx, plan)? else {
        return Ok(());
    };
    if lead.is_some() {
        return Ok(());
    }
    let key = AgentKey::worker(context.clone(), plan.clone(), job.clone());
    let question: String = question.chars().take(QUESTION_CHARS).collect();
    let text = format!(
        "Worker {key} of Plan {plan} asks: \"{question}\" (escalation {id}). Answer with: cpd answer {id} \"<text>\" --plan {plan}"
    );
    tell(tx, &context, &text)
}

/// The `session-message` to the live Manager of a Lead-less Plan's Context: `text` verbatim, as E8's `tell_managers`
/// writes it (contract §5.3 "The Manager of a Lead-less Plan"). Nothing when `plan` has a Lead or no Manager is live.
fn shaping_manager(tx: &WriteTxn<'_>, plan: &PlanId, text: &str) -> Result<(), CpdError> {
    let Some((lead, context)) = plan_lead_and_context(tx, plan)? else {
        return Ok(());
    };
    if lead.is_some() {
        return Ok(());
    }
    tell(tx, &context, text)
}

/// The `plan`'s `lead_session` and Context, or `None` when no such Plan is stored.
fn plan_lead_and_context(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
) -> Result<Option<(Option<SessionId>, ContextId)>, CpdError> {
    let row: Option<(Option<String>, String)> = tx
        .query_row(
            "SELECT lead_session, context FROM plans WHERE id = ?1",
            [plan.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((lead, context)) = row else {
        return Ok(None);
    };
    let lead = lead.map(|s| SessionId::parse(&s)).transpose()?;
    Ok(Some((lead, ContextId::parse(&context)?)))
}

/// The message to one live Manager of `context`, from null, waking (E8's `tell_manager_of`). Returns 0 or 1.
fn tell(tx: &WriteTxn<'_>, context: &ContextId, text: &str) -> Result<(), CpdError> {
    crate::session::tell_manager_of(tx, Some(context.as_str()), text)?;
    Ok(())
}
