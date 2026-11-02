//! E4, open Jobs (contract §3.7, hs-d340, hs-d573; it replaces the completion step and its `all-merged` steer, and the escalation to the
//! Manager): the **fallback** for a Workflow whose opener is missing. `workflow add` lays an opener stub out for every shape, and `pr open` adds one to a
//! Workflow that has none; E4 covers the Workflows those did not, each Workflow in status `dispatch`, of a Plan that is not archived, with no `open` Job `draft`, `queued`, `running`, `failed` or `cancelled`, gets one
//! ([`cpd_core::job::open::add`]), a `draft` one while a draft of the Workflow (any kind but `open` and `update`) remains, else `queued`. Each tick
//! it also queues a `draft` `open` Job of such a Workflow when none of the Workflow's other Jobs is `draft` any more, with its fingerprint, in one transaction. A `merged` one on a Workflow still `dispatch` is a merge that died before it opened the pull request, so it
//! counts for nothing and a fresh Job follows; a `cancelled` one is the Lead's and stands (the way on is `pr open --workflow` or `workflow cancel`);
//! a `failed` one waits for the Lead.
//! One transaction for every Workflow missing one; nothing is written when none is, so the step is idempotent.

use super::SOURCE;
use super::engine::{Engine, incident_holds_plan};
use super::report::{About, Step};
use cpd_core::error::CpdError;
use cpd_core::ids::{PlanId, WorkflowKey};
use cpd_core::job::open;
use cpd_core::plan::fingerprint::load_workflows;
use cpd_core::store::WriteTxn;

/// The Workflows E4 is for; `?1` and `?2` are one Workflow's Plan and key, or NULL for every Workflow.
const MISSING: &str = "SELECT r.plan, r.key FROM workflows r JOIN plans p ON p.id = r.plan
  WHERE (?1 IS NULL OR r.plan = ?1) AND (?2 IS NULL OR r.key = ?2)
    AND r.status = 'dispatch' AND p.archived_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.workflow = r.key AND j.kind = 'open' AND j.status IN ('draft', 'queued', 'running', 'failed', 'cancelled'))
  ORDER BY p.created_at, r.plan, r.key";

pub(super) fn run(engine: &mut Engine) {
    let missing = engine.store.read(|conn| {
        let mut stmt = conn.prepare(MISSING)?;
        let rows = stmt
            .query_map([None::<&str>, None::<&str>], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut missing = Vec::new();
        for (plan, key) in rows {
            let plan = PlanId::parse(&plan)?;
            let key = WorkflowKey::parse(&key)?;
            // Incident mode on: a held Plan's Workflow gets no open Job (contract §3.7 E4).
            if !incident_holds_plan(conn, &plan) {
                missing.push((plan, key));
            }
        }
        Ok(missing)
    });
    let missing = match missing {
        Ok(missing) => missing,
        Err(e) => return engine.fail(Step::E4, About::Machine, &e),
    };
    let promotable = match engine.store.read(|conn| {
        let mut found = promotable(conn, None)?;
        // Incident mode on: a held Plan's draft open Job is not queued either (§3.7 E4).
        found.retain(|(plan, _)| match PlanId::parse(plan) {
            Ok(plan) => !incident_holds_plan(conn, &plan),
            Err(_) => true,
        });
        Ok(found)
    }) {
        Ok(p) => p,
        Err(e) => return engine.fail(Step::E4, About::Machine, &e),
    };
    if missing.is_empty() && promotable.is_empty() {
        engine.clear(Step::E4, &About::Machine);
        return;
    }
    // Each open Job runs at `worker_tier` floored to a valid rung of its Workflow's catalog.
    let mut tiers = Vec::new();
    for (plan, key) in &missing {
        match engine
            .catalog_of_workflow(plan, key)
            .and_then(|catalog| open::tier(&engine.config, &catalog))
        {
            Ok(tier) => tiers.push((plan.clone(), key.clone(), tier)),
            Err(e) => return engine.fail(Step::E4, About::Machine, &e),
        }
    }
    match engine.store.write(SOURCE, |tx| {
        Ok((add_all(tx, &missing, &tiers)?, promote_all(tx)?))
    }) {
        Ok((added, promoted)) => {
            for (plan, workflow, key) in added {
                engine.log.line(&format!(
                    "E4: added the open Job {key} to Workflow {workflow} of Plan {plan}"
                ));
            }
            for (plan, key) in promoted {
                engine.log.line(&format!("E4: queued the draft open Job {key} of Plan {plan}: no draft of its Workflow remains"));
            }
            engine.clear(Step::E4, &About::Machine);
        }
        Err(e) => engine.fail(Step::E4, About::Machine, &e),
    }
}

/// The draft `open` Jobs E4 queues: of a `dispatch` Workflow of a Plan that is not archived, with no other Job (as in [`others`]: not `open`
/// or `update`) of the Workflow `draft`. `?1` is a Plan, or NULL for every Plan.
const PROMOTABLE: &str = "SELECT o.plan, o.key FROM jobs o JOIN workflows r ON r.plan = o.plan AND r.key = o.workflow JOIN plans p ON p.id = o.plan
  WHERE (?1 IS NULL OR o.plan = ?1) AND o.kind = 'open' AND o.status = 'draft'
    AND r.status = 'dispatch' AND p.archived_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = o.plan AND j.workflow = o.workflow AND j.status = 'draft' AND j.kind NOT IN ('open', 'update'))
  ORDER BY p.created_at, o.plan, o.seq";

fn promotable(
    conn: &rusqlite::Connection,
    plan: Option<&str>,
) -> Result<Vec<(String, String)>, CpdError> {
    let mut stmt = conn.prepare(PROMOTABLE)?;
    Ok(stmt
        .query_map([plan], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Decides again inside the transaction, then queues each draft `open` Job with its fingerprint as the insert computes it.
fn promote_all(tx: &WriteTxn<'_>) -> Result<Vec<(String, String)>, CpdError> {
    let found = promotable(tx, None)?;
    for (plan, key) in &found {
        let all = cpd_core::plan::jobs::load(tx, &PlanId::parse(plan)?)?;
        let job = all
            .iter()
            .find(|j| j.key == *key)
            .ok_or_else(|| CpdError::bad_row(format!("Job {key} of Plan {plan} vanished")))?;
        tx.execute(
            "UPDATE jobs SET status = 'queued', fingerprint = ?1 WHERE plan = ?2 AND key = ?3 AND status = 'draft'",
            [cpd_core::plan::jobs::locked_fingerprint(job).as_str(), plan.as_str(), key.as_str()],
        )?;
    }
    Ok(found)
}

/// Decides again inside the transaction (a hand `pr open` or `job cancel` may have changed a Workflow), then adds each missing Job.
fn add_all(
    tx: &WriteTxn<'_>,
    missing: &[(PlanId, WorkflowKey)],
    tiers: &[(PlanId, WorkflowKey, cpd_core::config::Tier)],
) -> Result<Vec<(PlanId, WorkflowKey, cpd_core::ids::JobKey)>, CpdError> {
    let mut added = Vec::new();
    for (plan, key) in missing {
        let still: bool = tx.query_row(
            &format!("SELECT EXISTS ({MISSING})"),
            [Some(plan.as_str()), Some(key.as_str())],
            |r| r.get(0),
        )?;
        if !still {
            continue;
        }
        let workflows = load_workflows(tx, plan)?;
        let Some(workflow) = workflows.iter().find(|p| p.key == *key) else {
            continue;
        };
        let draft: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM jobs WHERE plan = ?1 AND workflow = ?2 AND status = 'draft' AND kind NOT IN ('open', 'update'))",
            [plan.as_str(), key.as_str()],
            |r| r.get(0),
        )?;
        let tier = tiers
            .iter()
            .find(|(p, k, _)| p == plan && k == key)
            .map(|(_, _, tier)| *tier)
            .ok_or_else(|| {
                CpdError::bad_row(format!(
                    "no open Tier was read for Workflow {key} of Plan {plan}"
                ))
            })?;
        let job = open::add(tx, plan, workflow, tier, draft, open::BY_ENGINE)?;
        added.push((plan.clone(), key.clone(), job));
    }
    Ok(added)
}
