//! What the merge reads from and writes to the Ledger: the Attempt it merges,
//! and the one transaction that ends a `merging` Attempt (contract §3.2,
//! §3.4, §5.2).

use rusqlite::params;
use serde_json::Value;

// Until `end_merging` joins `attempt::ledger::end`, the merge reaches the shared
// Ledger reads through here.
pub use crate::attempt::ledger::{Attempt, cancel_request, has_error, load, merging_on_repo, short};
use crate::config::{Catalog, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::store::WriteTxn;

/// The causes the merge and R end an Attempt with when it fails (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// R3: the driver died before the compare-and-swap.
    Crashed,
    /// Step 5: the rebase conflicted.
    MergeConflict,
    /// Step 8: the Lead Branch kept moving.
    CasLost,
    /// Step 3: the Lead worktree is not clean.
    DirtyTarget,
    /// Step 4: the diff touches protected paths (§7.5).
    Tampered,
}

impl Failure {
    pub fn as_str(self) -> &'static str {
        match self {
            Failure::Crashed => "crashed",
            Failure::MergeConflict => "merge_conflict",
            Failure::CasLost => "cas_lost",
            Failure::DirtyTarget => "dirty_target",
            Failure::Tampered => "tampered",
        }
    }

    /// A ladder cause sends the Job back to `queued` while the ladder allows (§3.4, §9).
    pub fn is_ladder(self) -> bool {
        matches!(self, Failure::Crashed | Failure::MergeConflict)
    }
}

/// How a `merging` Attempt ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// `merged` / `merged`: the Lead Branch is at `sha`.
    Merged { sha: String },
    /// `cancelled` / `cancel_requested`.
    Cancelled,
    /// `failed` / `cause`; `detail` explains it (for `merge_conflict`, the files).
    Failed { cause: Failure, detail: String },
}

impl End {
    pub fn outcome(&self) -> &'static str {
        match self {
            End::Merged { .. } => "merged",
            End::Cancelled => "cancelled",
            End::Failed { .. } => "failed",
        }
    }

    pub fn cause(&self) -> &'static str {
        match self {
            End::Merged { .. } => "merged",
            End::Cancelled => "cancel_requested",
            End::Failed { cause, .. } => cause.as_str(),
        }
    }
}

/// What the ladder needs (§9): pi's catalog and `retry_cap`.
pub struct Ladder<'a> {
    pub catalog: &'a Catalog,
    pub retry_cap: u32,
}

/// Ends a `merging` Attempt and moves its Job, in the caller's transaction
/// (§3.2, §3.4): compare-and-swap on `status = 'merging'` and the Job
/// `running` (`cas_lost` when either changed), with `metrics` (A9), and the
/// Event of §3.4's table: `job-merged`, `job-cancelled`, `attempt-retry` or
/// `job-failed`.
pub fn end_merging(
    tx: &WriteTxn<'_>,
    attempt: &Attempt,
    end: &End,
    metrics: Option<&Value>,
    ladder: &Ladder<'_>,
) -> Result<(), CpdError> {
    let (plan, job, id) = (attempt.plan.as_str(), attempt.job.as_str(), attempt.id.to_string());
    let cas_lost = |what: &str| {
        CpdError::new(ErrorCode::CasLost, format!("{what} changed while the merge ran; nothing was written."))
            .with("plan", plan)
            .with("attempt", id.clone())
    };
    let merged_sha = match end {
        End::Merged { sha } => Some(sha.as_str()),
        _ => None,
    };
    let changed = tx.execute(
        "UPDATE attempts SET status = 'ended', outcome = ?3, cause = ?4, merged_sha = ?5, metrics = ?6,
                ended_at = (SELECT at FROM txns WHERE id = cpd_txn())
         WHERE plan = ?1 AND id = ?2 AND status = 'merging'",
        params![plan, id, end.outcome(), end.cause(), merged_sha, metrics.map(Value::to_string)],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Attempt's status"));
    }
    let cancel = cancel_request(tx, &attempt.plan, &attempt.job)?;
    let tier = attempt.tier.to_string();
    let show = format!("cpd job show {job} --plan {plan}");

    let (status, failed_cause, event) = match end {
        End::Merged { sha } => {
            let mut text = format!("Job {job} merged: Attempt {id} fast-forwarded {} to {}.", attempt.lead_branch, short(sha));
            if cancel.is_some() {
                text.push_str(" Its cancel request came too late: the merge had already happened.");
            }
            text.push_str(&format!(" Next: {show}"));
            ("merged", None, NewEvent::job_merged(text, sha, &tier, cancel.is_some()).attempt(attempt.id.clone()))
        }
        End::Cancelled => {
            let reason = cancel.ok_or_else(|| cas_lost("The Job's cancel request"))?;
            let text = format!("Job {job} cancelled ({reason}): Attempt {id} stopped before its merge. Next: cpd plan show --plan {plan}");
            ("cancelled", None, NewEvent::job_cancelled(text, &reason).attempt(attempt.id.clone()))
        }
        End::Failed { cause, detail } => match next_tier(tx, attempt, *cause, ladder)? {
            Some(next) => {
                let text = format!(
                    "Attempt {id} of Job {job} failed ({}): {detail} The Job is queued again at Tier {next}. Next: {show}",
                    cause.as_str()
                );
                let event = NewEvent::attempt_retry(text, "failed", cause.as_str(), &next.to_string(), detail);
                ("queued", None, event.attempt(attempt.id.clone()))
            }
            None => {
                let attempts: i64 = tx.query_row(
                    "SELECT count(*) FROM attempts WHERE plan = ?1 AND job = ?2",
                    params![plan, job],
                    |r| r.get(0),
                )?;
                let text = format!(
                    "Job {job} failed ({}): {detail} Decide: cpd job retry {job} --tier <T> --plan {plan}, or cpd job cancel {job} --reason <text> --plan {plan}",
                    cause.as_str()
                );
                let event = NewEvent::job_failed(text, cause.as_str(), detail, &tier, attempts);
                ("failed", Some(cause.as_str()), event.attempt(attempt.id.clone()))
            }
        },
    };
    let changed = tx.execute(
        "UPDATE jobs SET status = ?3, merged_sha = ?4, failed_cause = ?5 WHERE plan = ?1 AND key = ?2 AND status = 'running'",
        params![plan, job, status, merged_sha, failed_cause],
    )?;
    if changed == 0 {
        return Err(cas_lost("The Job's status"));
    }
    events::append(tx, event.plan(attempt.plan.clone()).job(attempt.job.clone()))?;
    Ok(())
}

/// The ladder (§3.2, §9): the next Tier when `cause` is a ladder cause, the
/// ladder has a next Tier, and the automatic Attempts since the last
/// `job retry` (that one included) are fewer than `1 + retry_cap`. Attempts
/// whose driver never started are not counted (§3.4).
fn next_tier(tx: &WriteTxn<'_>, attempt: &Attempt, cause: Failure, ladder: &Ladder<'_>) -> Result<Option<Tier>, CpdError> {
    if !cause.is_ladder() {
        return Ok(None);
    }
    let run = crate::attempt::ledger::automatic_attempts(tx, attempt.plan.as_str(), attempt.job.as_str())?;
    if run >= 1 + i64::from(ladder.retry_cap) {
        return Ok(None);
    }
    ladder.catalog.next(attempt.tier)
}

/// Appends an `error` Event about `attempt` (§5.2) for `error`, from `source`.
pub fn append_error(tx: &WriteTxn<'_>, attempt: &Attempt, source: &str, error: &CpdError, text: String) -> Result<(), CpdError> {
    events::append(
        tx,
        NewEvent::error(text, source, error.code().as_str(), error.message(), error.details())
            .plan(attempt.plan.clone())
            .job(attempt.job.clone())
            .attempt(attempt.id.clone()),
    )?;
    Ok(())
}
