//! The engine's retry of a failed `open` or `update` Job when its fix merges (contract §3.2; Context decision pro-d89).

use super::retry::requeue;
use super::worked_on::{created_after_failure, queued_or_running_after_failure};
use crate::error::{CpdError, ErrorCode};
use crate::ids::{JobKey, PlanId};
use crate::store::WriteTxn;

/// Requeues each failed `open` or `update` Job of `merged`'s Workflow that `merged` fixes; returns the keys it requeued.
/// Call it after the row of `merged` is `merged`, in the same transaction.
pub(crate) fn retry_after_fix(tx: &WriteTxn<'_>, plan: &PlanId, merged: &JobKey) -> Result<Vec<JobKey>, CpdError> {
    let sql = format!(
        "SELECT j.key, (SELECT a.tier FROM attempts a WHERE a.plan = j.plan AND a.job = j.key ORDER BY a.number DESC LIMIT 1)
           FROM jobs j JOIN jobs m ON m.plan = j.plan AND m.workflow = j.workflow
          WHERE m.plan = ?1 AND m.key = ?2 AND m.kind NOT IN ('open', 'update')
            AND j.kind IN ('open', 'update') AND j.status = 'failed'
            AND {} AND NOT {}
          ORDER BY j.created_txn",
        created_after_failure("m", "j"),
        queued_or_running_after_failure("j")
    );
    let failed: Vec<(String, Option<String>)> = {
        let mut stmt = tx.prepare(&sql)?;
        let rows = stmt.query_map([plan.as_str(), merged.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut out = Vec::new();
    for (key, tier) in failed {
        let store_err = |what: &str| CpdError::new(ErrorCode::StoreFailed, format!("{what} of Job {key} of Plan {plan}.")).with("job", key.clone());
        let tier = tier.ok_or_else(|| store_err("The store holds no Attempt"))?;
        let key = JobKey::parse(&key).map_err(|_| store_err("The store holds an invalid key"))?;
        let text = format!("Job {key} of Plan {plan} is queued again at Tier {tier} after fix {merged} merged. Next: cpd job show {key} --plan {plan}");
        requeue(tx, plan, &key, &tier, text)?;
        out.push(key);
    }
    Ok(out)
}
