//! `attempt abort` (commands.md §6.5, contract §3.4): the Lead ends a running
//! Attempt `failed` / `aborted` in one write transaction, which also fails its
//! Job. A Worker is refused (`not_a_lead`), a non-running Attempt is refused
//! `attempt_not_running`, and the transaction re-reads the status first
//! (ADR-0014, as `job edit`): an end that happened meanwhile stands, never both.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use super::ledger::{self, Attempt, Cause, End, Ladder, Stage};
use super::{AbortArgs, Env};
use crate::config::{Catalog, TierModel};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::output::Output;

const COMMAND: &str = "attempt abort";

/// The Tiers of a no-ladder cause: `next_rung` returns `None` for `Cause::Aborted`
/// before it reads the catalog, so no ladder decision is ever made here.
fn no_catalog() -> Catalog {
    let empty = || TierModel { model: String::new(), efforts: Vec::new() };
    Catalog { pi_version: String::new(), o: empty(), s: empty(), h: empty(), g: empty() }
}

pub(super) fn abort(args: AbortArgs) -> Result<Output, CpdError> {
    // The Lead only, through `CPD_ATTEMPT` (hs-d497), the same check as `job approve-tests` (commands.md §5.9):
    // cpd sets it in every Worker's pane. Nothing else authenticates the caller.
    if std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead ends an Attempt: ask it with cpd ask, and it runs cpd attempt abort.",
        ));
    }
    let plan = args.plan.require()?.clone();
    let id = args.attempt;
    let mut env = Env::open()?;
    let by = caller(&mut env, &plan)?;
    // The expensive refusals (no Plan, no Attempt, not running) before the write; the write re-reads (ADR-0014).
    let attempt = env.store.read(|conn| decide(conn, &plan, &id))?;
    let retry_cap = env.config.retry_cap;
    let catalog = no_catalog();
    let job = attempt.job.clone();
    env.store.write(COMMAND, |tx| {
        let attempt = decide(tx, &plan, &id)?;
        // `stop_request = aborted` first: the `attempts_ended` trigger refuses any update of the row once it ends.
        tx.execute(
            "UPDATE attempts SET stop_request = 'aborted' WHERE plan = ?1 AND id = ?2 AND status = 'running'",
            params![plan.as_str(), id.to_string()],
        )?;
        let detail = format!("The Lead aborted Attempt {id} of Job {job}: stop, its work is not merged.");
        let ladder = Ladder { catalog: &catalog, retry_cap };
        // `Cause::Aborted` is no-ladder: `next_rung` returns `None`, so the Job goes `failed` with a `job-failed`
        // Event that already names `cpd job retry <key> --tier <T>` (contract §3.4).
        ledger::end(tx, &attempt, &End::Failed { cause: Cause::Aborted, detail }, None, &ladder, Stage::Driver)?;
        let text = format!("Attempt {id} of Job {job} aborted by the Lead.");
        append(
            tx,
            NewEvent::attempt_aborted(text, &attempt.tier.to_string(), &by)
                .plan(plan.clone())
                .job(attempt.job.clone())
                .attempt(id.clone()),
        )?;
        Ok(())
    })?;
    Ok(Output::success(json!({"attempt": id, "outcome": "failed", "cause": "aborted"})))
}

/// The calling Lead Session's id (contract §5.2): `CPD_SESSION`, else the Plan's Lead Session.
fn caller(env: &mut Env, plan: &PlanId) -> Result<String, CpdError> {
    if let Ok(id) = std::env::var("CPD_SESSION") {
        if !id.is_empty() {
            return Ok(SessionId::parse(&id)?.to_string());
        }
    }
    env.store
        .read(|conn| {
            Ok(conn
                .query_row("SELECT lead_session FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get::<_, String>(0))
                .optional()?)
        })?
        .ok_or_else(|| {
            CpdError::new(ErrorCode::SessionRequired, "No Session selected: pass CPD_SESSION or run as the Plan's Lead.")
        })
}

/// The Attempt to end, refused `attempt_not_running` unless it is `running`; its cause is named in the refusal.
fn decide(conn: &Connection, plan: &PlanId, id: &AttemptId) -> Result<Attempt, CpdError> {
    let attempt = ledger::load(conn, plan, id)?;
    if attempt.status != "running" {
        let cause: Option<String> = conn.query_row(
            "SELECT cause FROM attempts WHERE plan = ?1 AND id = ?2",
            params![plan.as_str(), id.to_string()],
            |r| r.get(0),
        )?;
        return Err(not_running(&attempt, cause.as_deref()));
    }
    Ok(attempt)
}

fn not_running(attempt: &Attempt, cause: Option<&str>) -> CpdError {
    let mut err = CpdError::new(
        ErrorCode::AttemptNotRunning,
        format!("Attempt {} is {}, not running: only a running Attempt is aborted.", attempt.id, attempt.status),
    )
    .with("plan", attempt.plan.as_str())
    .with("attempt", attempt.id.to_string())
    .with("status", attempt.status.as_str());
    if let Some(cause) = cause {
        err = err.with("cause", cause);
    }
    err
}
