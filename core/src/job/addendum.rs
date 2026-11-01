//! `job addendum` (commands.md §5.10, contract §3.4 "Addenda"): the Lead adds information to a locked Job.
//! One `job_addenda` row and one `job-addendum` Event, in one transaction; nothing else changes.

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{AddendumArgs, Env, find, plan_phase};
use crate::args::Text;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{JobKey, PlanId};
use crate::output::Output;
use crate::plan::jobs::{self, Job};

const COMMAND: &str = "job addendum";

/// Where the addendum goes: a running Attempt and its Worker Session, or nobody.
type Address = Option<(String, String)>;

pub(super) fn addendum(args: AddendumArgs) -> Result<Output, CpdError> {
    if std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()) {
        return Err(CpdError::new(ErrorCode::NotALead, "Only the Lead adds an addendum to a Job: ask it with cpd ask."));
    }
    let body = match (args.text, &args.file) {
        (Some(text), None) => text,
        (None, Some(path)) => std::fs::read_to_string(path).map_err(|e| CpdError::file_unreadable(path, e))?.parse::<Text>()?,
        _ => unreachable!("clap requires exactly one of <text> and --file"),
    };
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;

    env.store.read(|conn| decide(conn, &plan, &key).map(|_| ()))?;
    let (job, number, address) = env.store.write(COMMAND, |tx| {
        let (job, address) = decide(tx, &plan, &key)?;
        let number: u32 = tx.query_row(
            "SELECT coalesce(max(number), 0) + 1 FROM job_addenda WHERE plan = ?1 AND job = ?2",
            [plan.as_str(), key.as_str()],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO job_addenda (plan, job, number, attempt, session, text, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT at FROM txns WHERE id = cpd_txn()))",
            rusqlite::params![
                plan.as_str(),
                key.as_str(),
                number,
                address.as_ref().map(|a| a.0.as_str()),
                address.as_ref().map(|a| a.1.as_str()),
                body.as_str()
            ],
        )?;
        let text = match &address {
            Some((attempt, _)) => format!(
                "Addendum {number} from the Lead on Job {key} (Attempt {attempt}). It adds to your Spec and replaces nothing in it: your Goal, Files you may touch, Acceptance and Out of scope stand as written.\n\n{}",
                body.as_str()
            ),
            None => format!(
                "Addendum {number} added to Job {key}: no Attempt is running, so it reaches the next Attempt's Spec. Next: cpd job show {key} --plan {plan}\n\n{}",
                body.as_str()
            ),
        };
        let mut event = NewEvent::job_addendum(text, body.as_str()).plan(plan.clone()).job(key.clone());
        if let Some((attempt, session)) = &address {
            event = event.attempt(crate::ids::AttemptId::parse(attempt)?).session(crate::ids::SessionId::parse(session)?);
        }
        append(tx, event)?;
        if let Some(pr) = &job.pr {
            crate::session::working_pr::name(tx, &plan, &crate::ids::PrKey::parse(pr)?)?;
        }
        Ok((job, number, address))
    })?;
    Ok(Output::success(json!({
        "job": {"key": job.key, "status": job.status, "kind": job.kind},
        "addendum": {"number": number, "attempt": address.as_ref().map(|a| &a.0), "delivered": address.is_some()},
    })))
}

/// The preconditions of §5.10 after `usage`, in order; returns the Job and where the addendum goes.
fn decide(conn: &Connection, plan: &PlanId, key: &JobKey) -> Result<(Job, Address), CpdError> {
    plan_phase(conn, plan)?;
    let all = jobs::load(conn, plan)?;
    let job = find(&all, plan, key)?.clone();
    let refuse = |code: ErrorCode, message: String| {
        CpdError::new(code, message).with("plan", plan.as_str()).with("job", key.as_str()).with("status", job.status.as_str())
    };
    match job.status.as_str() {
        "queued" | "failed" => Ok((job, None)),
        "running" => {
            let latest: Option<(String, String, Option<String>)> = conn
                .query_row(
                    "SELECT id, status, worker_session FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number DESC LIMIT 1",
                    [plan.as_str(), key.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match latest {
                Some((id, status, session)) if status == "running" => {
                    let session = session.ok_or_else(|| CpdError::bad_row(format!("running Attempt {id} of Job {key} has no Worker Session")))?;
                    Ok((job, Some((id, session))))
                }
                _ => Err(refuse(
                    ErrorCode::AttemptNotRunning,
                    format!("Job {key} has no running Attempt right now (one is starting, merging or ended): run the command again in a moment."),
                )),
            }
        }
        other => Err(refuse(
            ErrorCode::IllegalTransition,
            format!("Job {key} is {other}: a draft is edited with job edit --spec, and an ended Job takes no new information."),
        )),
    }
}
