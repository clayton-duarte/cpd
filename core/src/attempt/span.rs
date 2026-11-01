//! A check span of `attempt submit` (contract §7.7 "Check spans and the Worker's clocks"): the checks that can run for
//! `gate_minutes` and wait for a test slot hold `locks/check-<plan>--<attempt>.lock` and are bracketed by two
//! `attempt-status` Events, `check: "start"` and `check: "end"`. The Attempt's active time and the stall kill skip the
//! span, and E2 closes one whose submit died (`died: true`).

use std::fs::{self, OpenOptions};

use super::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::lock;
use crate::store::WriteTxn;

const COMMAND: &str = "attempt submit";
const WHAT: &str = "checks";

/// An open `checks` span: holds the lock until it drops. It ends exactly once: `end_in` (the pass path, in the transaction
/// that makes the Attempt `merging`, which the caller then commits) or `end` (every other way out, a transaction of its own).
pub(super) struct Span {
    _lock: std::fs::File,
    plan: PlanId,
    id: AttemptId,
    job: String,
    key: JobKey,
}

/// Opens the span: takes the lock, then appends the `start` Event in a transaction of its own.
pub(super) fn open(
    env: &mut Env,
    plan: &PlanId,
    id: &AttemptId,
    job: &str,
) -> Result<Span, CpdError> {
    let path = env.home.check_lock(plan, id);
    let failed = |e: &dyn std::fmt::Display| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot use the check lock {}: {e}.", path.display()),
        )
        .with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| failed(&e))?;
    lock::blocking(&file).map_err(|e| failed(&e))?;
    let key = JobKey::parse(job).map_err(|_| CpdError::bad_row(format!("job key {job:?}")))?;
    let span = Span {
        _lock: file,
        plan: plan.clone(),
        id: id.clone(),
        job: job.to_owned(),
        key,
    };
    env.store.write(COMMAND, |tx| {
        let text = format!(
            "Attempt {id} of Job {job} began its {WHAT} check: its clocks stop until it ends."
        );
        append(tx, span.event(NewEvent::attempt_check_start(text, WHAT)))?;
        Ok(())
    })?;
    Ok(span)
}

impl Span {
    fn event(&self, event: NewEvent) -> NewEvent {
        event
            .plan(self.plan.clone())
            .job(self.key.clone())
            .attempt(self.id.clone())
    }

    /// The `end` Event, appended in `tx`; the lock goes when the span drops, after the commit.
    pub(super) fn end_in(&self, tx: &WriteTxn<'_>) -> Result<(), CpdError> {
        let text = format!(
            "Attempt {} of Job {} ended its {WHAT} check.",
            self.id, self.job
        );
        append(
            tx,
            self.event(NewEvent::attempt_check_end(text, WHAT, false)),
        )?;
        Ok(())
    }

    /// Ends the span in a transaction of its own (a refusal, or a final transaction that rolled back).
    pub(super) fn end(self, env: &mut Env) -> Result<(), CpdError> {
        env.store.write(COMMAND, |tx| self.end_in(tx))
    }
}
