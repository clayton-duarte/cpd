//! E4, open Jobs (contract §3.7, hs-d340; it replaces the completion step and its `all-merged` steer, and the escalation to the
//! Manager): each PR in status `dispatch`, of a Plan that is not archived, with no `open` Job `queued`, `running` or `failed`, gets one
//! ([`cpd_core::job::open::add`]). A `merged` one on a PR still `dispatch` is a merge that died before it opened the pull request, so it
//! counts for nothing and a fresh Job follows; a `cancelled` one is how the Lead starts over; a `failed` one waits for the Lead.
//! One transaction for every PR missing one; nothing is written when none is, so the step is idempotent.

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use cpd_core::error::CpdError;
use cpd_core::ids::{PlanId, PrKey};
use cpd_core::job::open;
use cpd_core::plan::fingerprint::load_prs;
use cpd_core::store::WriteTxn;

/// The PRs E4 is for; `?1` and `?2` are one PR's Plan and key, or NULL for every PR.
const MISSING: &str = "SELECT r.plan, r.key FROM prs r JOIN plans p ON p.id = r.plan
  WHERE (?1 IS NULL OR r.plan = ?1) AND (?2 IS NULL OR r.key = ?2)
    AND r.status = 'dispatch' AND p.archived_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.kind = 'open' AND j.status IN ('queued', 'running', 'failed'))
  ORDER BY p.created_at, r.plan, r.key";

pub(super) fn run(engine: &mut Engine) {
    let missing = engine.store.read(|conn| {
        let mut stmt = conn.prepare(MISSING)?;
        let rows = stmt
            .query_map([None::<&str>, None::<&str>], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|(plan, key)| Ok((PlanId::parse(plan)?, PrKey::parse(key)?))).collect::<Result<Vec<_>, CpdError>>()
    });
    let missing = match missing {
        Ok(missing) => missing,
        Err(e) => return engine.fail(Step::E4, About::Machine, &e),
    };
    if missing.is_empty() {
        engine.clear(Step::E4, &About::Machine);
        return;
    }
    let tier = engine.config.open_tier;
    match engine.store.write(SOURCE, |tx| add_all(tx, &missing, tier)) {
        Ok(added) => {
            for (plan, pr, key) in added {
                engine.log.line(&format!("E4: added the open Job {key} to PR {pr} of Plan {plan}"));
            }
            engine.clear(Step::E4, &About::Machine);
        }
        Err(e) => engine.fail(Step::E4, About::Machine, &e),
    }
}

/// Decides again inside the transaction (a hand `pr open` or `job cancel` may have changed a PR), then adds each missing Job.
fn add_all(tx: &WriteTxn<'_>, missing: &[(PlanId, PrKey)], tier: cpd_core::config::Tier) -> Result<Vec<(PlanId, PrKey, cpd_core::ids::JobKey)>, CpdError> {
    let mut added = Vec::new();
    for (plan, key) in missing {
        let still: bool = tx.query_row(&format!("SELECT EXISTS ({MISSING})"), [Some(plan.as_str()), Some(key.as_str())], |r| r.get(0))?;
        if !still {
            continue;
        }
        let prs = load_prs(tx, plan)?;
        let Some(pr) = prs.iter().find(|p| p.key == *key) else { continue };
        let job = open::add(tx, plan, pr, tier)?;
        added.push((plan.clone(), key.clone(), job));
    }
    Ok(added)
}
