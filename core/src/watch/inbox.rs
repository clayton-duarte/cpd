//! E8, the Inbox (contract §3.7, §2.2 "Inbox item"): after E7, the conditions of the Inbox kinds from the
//! store, and the one transaction that opens and closes the items; then each stuck Job (the clock and
//! `inbox.stuck_job_minutes`) is sent once to its Context's Manager, not opened as an item (hs-d318).
//! A quiet tick writes nothing.

use std::time::Duration;

use time::OffsetDateTime;

use super::engine::Engine;
use crate::clock::timestamp;
use super::report::{About, Step, tell_stuck_job};
use crate::error::CpdError;
use crate::inbox::model::{Changes, reconcile};
use crate::inbox::rules::{conditions, stuck_jobs};

pub(super) fn run(engine: &mut Engine) {
    match reconcile_inbox(engine) {
        Ok((changes, told)) => {
            if !changes.is_empty() {
                engine.log.line(&format!("E8: opened {} Inbox item(s), closed {}", changes.opened.len(), changes.closed.len()));
            }
            if told > 0 {
                engine.log.line(&format!("E8: told the Manager of {told} stuck Job(s)"));
            }
            engine.clear(Step::E8, &About::Machine);
        }
        Err(e) => engine.fail(Step::E8, About::Machine, &e),
    }
}

/// The Inbox's changes, and how many stuck Jobs were sent to their Manager (hs-d318).
fn reconcile_inbox(engine: &mut Engine) -> Result<(Changes, usize), CpdError> {
    let now = OffsetDateTime::now_utc();
    let found = conditions(&mut engine.store, &engine.home)?;
    let changes = reconcile(&mut engine.store, &found, &timestamp(now))?;
    let stuck_after = Duration::from_secs(u64::from(engine.config.inbox.stuck_job_minutes) * 60);
    let mut told = 0;
    for stuck in stuck_jobs(&mut engine.store, now, stuck_after)? {
        told += usize::from(tell_stuck_job(&mut engine.store, &stuck)?);
    }
    Ok((changes, told))
}
