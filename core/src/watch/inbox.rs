//! E8, the Inbox (contract §3.7, §2.2 "Inbox item"): after E7, the conditions of the Inbox kinds from the
//! store, the clock, `inbox.stuck_job_minutes` and the engine's own active errors, and the one transaction
//! that opens and closes the items. A quiet tick writes nothing.

use std::time::Duration;

use time::OffsetDateTime;

use super::engine::Engine;
use crate::clock::timestamp;
use super::report::{About, Step};
use crate::error::CpdError;
use crate::inbox::model::{Changes, reconcile};
use crate::inbox::rules::conditions;

pub(super) fn run(engine: &mut Engine) {
    match reconcile_inbox(engine) {
        Ok(changes) => {
            if !changes.is_empty() {
                engine.log.line(&format!("E8: opened {} Inbox item(s), closed {}", changes.opened.len(), changes.closed.len()));
            }
            engine.clear(Step::E8, &About::Machine);
        }
        Err(e) => engine.fail(Step::E8, About::Machine, &e),
    }
}

fn reconcile_inbox(engine: &mut Engine) -> Result<Changes, CpdError> {
    let now = OffsetDateTime::now_utc();
    let stuck_after = Duration::from_secs(u64::from(engine.config.inbox.stuck_job_minutes) * 60);
    let errors = engine.errors();
    let found = conditions(&mut engine.store, &engine.home, now, stuck_after, &errors)?;
    reconcile(&mut engine.store, &found, &timestamp(now))
}
