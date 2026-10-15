//! E3, scheduling (contract §3.2, §3.4, §3.7): each Ready Job, in `jobs.seq`
//! order across all Plans, while a slot is free, is started by the starter
//! `job dispatch` uses: fingerprint, driver lock, Attempt, driver.

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use crate::driver::{self, StartCtx};
use crate::error::{CpdError, ErrorCode};
use crate::ids::{JobKey, PlanId};

/// Ready (§3.2): `queued`, in a Plan in `dispatch` or `pr`, every Dependency `merged`.
const READY: &str = "SELECT j.plan, j.key FROM jobs j JOIN plans p ON p.id = j.plan
  WHERE j.status = 'queued' AND p.phase IN ('dispatch', 'pr')
    AND NOT EXISTS (SELECT 1 FROM dependencies d JOIN jobs n ON n.plan = d.plan AND n.key = d.needs
                     WHERE d.plan = j.plan AND d.job = j.key AND n.status <> 'merged')
  ORDER BY j.seq";

pub(super) fn run(engine: &mut Engine) {
    let ready = engine.store.read(|conn| {
        let mut stmt = conn.prepare(READY)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let active: i64 = conn.query_row("SELECT count(*) FROM attempts WHERE status <> 'ended'", [], |r| r.get(0))?;
        let ready =
            rows.into_iter().map(|(p, k)| Ok((PlanId::parse(&p)?, JobKey::parse(&k)?))).collect::<Result<Vec<_>, CpdError>>()?;
        Ok((ready, active))
    });
    let ready = match ready {
        Ok((ready, active)) => {
            engine.clear(Step::E3, &About::Machine);
            if active >= i64::from(engine.config.concurrency) {
                return;
            }
            ready
        }
        Err(e) => return engine.fail(Step::E3, About::Machine, &e),
    };
    if ready.is_empty() {
        return;
    }
    let catalog = match engine.catalog() {
        Ok(catalog) => catalog,
        Err(e) => return engine.fail(Step::E3, About::Machine, &e),
    };
    for (plan, key) in ready {
        let about = About::Job(plan.clone(), key.clone());
        let ctx = StartCtx { home: &engine.home, config: &engine.config, catalog: &catalog, command: SOURCE };
        match driver::start(&mut engine.store, &ctx, &plan, &key) {
            Ok(started) => {
                engine.log.line(&format!(
                    "E3: started {} of Plan {plan} at Tier {} ({}), driver pid {}",
                    started.attempt,
                    started.tier,
                    started.origin.as_str(),
                    started.pid
                ));
                engine.clear(Step::E3, &about);
            }
            // Every slot is taken: the next tick tries again.
            Err(e) if e.code() == ErrorCode::NoSlot => break,
            // The Job changed since it was read (another starter, a cancel): decided again next tick.
            Err(e) if matches!(e.code(), ErrorCode::CasLost | ErrorCode::JobNotReady) => {
                engine.log.line(&format!("E3: skipped Job {key} of Plan {plan}: {}", e.message()));
            }
            Err(e) => engine.fail(Step::E3, about, &e),
        }
    }
}
