//! E8, the Inbox (contract §3.7, §2.2 "Inbox item"): after E7, the conditions of the Inbox kinds from the
//! store, and the one transaction that opens and closes the items; then the stuck Jobs (the clock and
//! `inbox.stuck_job_minutes`), grouped by their Workflow, are sent once per Workflow to the Context's Manager, not opened as an item
//! (hs-d318, hs-d647).
//! A quiet tick writes nothing.

use std::time::Duration;

use super::asks::tell_repeated_asks;
use super::engine::Engine;
use super::report::{About, Step, tell_stuck_jobs};
use cpd_core::clock::timestamp;
use cpd_core::error::CpdError;
use cpd_core::inbox::asks::repeated_asks;
use cpd_core::inbox::model::{Changes, reconcile};
use cpd_core::inbox::rules::{conditions_on, stuck_jobs};

pub(super) fn run(engine: &mut Engine) {
    match reconcile_inbox(engine) {
        Ok((changes, told, asked)) => {
            if !changes.is_empty() {
                engine.log.line(&format!(
                    "E8: opened {} Inbox item(s), closed {}",
                    changes.opened.len(),
                    changes.closed.len()
                ));
            }
            if told > 0 {
                engine.log.line(&format!("E8: told the Manager of {told} stuck Job Workflow(s)"));
            }
            if asked > 0 {
                engine
                    .log
                    .line(&format!("E8: told the Manager of {asked} repeated ask(s)"));
            }
            engine.clear(Step::E8, &About::Machine);
        }
        Err(e) => engine.fail(Step::E8, About::Machine, &e),
    }
}

/// The Inbox's changes, and how many stuck-Job messages were sent to their Manager (hs-d318, hs-d647).
fn reconcile_inbox(engine: &mut Engine) -> Result<(Changes, usize, usize), CpdError> {
    let now = engine.ports.clock.utc();
    let found = conditions_on(&engine.ports, &mut engine.store, &engine.home)?;
    let changes = reconcile(&mut engine.store, &found, &timestamp(now))?;
    let stuck_after = Duration::from_secs(u64::from(engine.config.inbox.stuck_job_minutes) * 60);
    let stuck = stuck_jobs(&mut engine.store, now, stuck_after)?;
    let told = tell_stuck_jobs(&mut engine.store, &stuck)?;
    let asks = repeated_asks(&mut engine.store, now)?;
    let asked = tell_repeated_asks(&mut engine.store, &asks)?;
    Ok((changes, told, asked))
}
