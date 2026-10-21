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

const COMMAND: &str = "attempt submit";

/// Runs `checks` inside a span of `what` (`tests` or `gate`) and ends the span whatever `checks` returns.
pub(super) fn within<T>(
    env: &mut Env,
    plan: &PlanId,
    id: &AttemptId,
    job: &str,
    what: &str,
    checks: impl FnOnce(&mut Env) -> Result<T, CpdError>,
) -> Result<T, CpdError> {
    let path = env.home.check_lock(plan, id);
    let failed = |e: &dyn std::fmt::Display| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the check lock {}: {e}.", path.display())).with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(|e| failed(&e))?;
    // Held until the span ends: the file closes (and the lock goes) when `file` drops, after the `end` Event.
    lock::blocking(&file).map_err(|e| failed(&e))?;
    let key = JobKey::parse(job).map_err(|_| CpdError::bad_row(format!("job key {job:?}")))?;
    let event = |event: NewEvent| event.plan(plan.clone()).job(key.clone()).attempt(id.clone());
    env.store.write(COMMAND, |tx| {
        let text = format!("Attempt {id} of Job {job} began its {what} check: its clocks stop until it ends.");
        append(tx, event(NewEvent::attempt_check_start(text, what)))?;
        Ok(())
    })?;
    let result = checks(env);
    let ended = env.store.write(COMMAND, |tx| {
        let text = format!("Attempt {id} of Job {job} ended its {what} check.");
        append(tx, event(NewEvent::attempt_check_end(text, what, false)))?;
        Ok(())
    });
    drop(file);
    match (result, ended) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(e), _) | (Ok(_), Err(e)) => Err(e),
    }
}
