//! E4, completion (contract §3.7), per PR: a PR in status `dispatch` with no
//! Job `draft`, `queued` or `running`, at least one `merged`, and
//! `all_merged_at` null gets `all-merged` `{pr, merged, cancelled}` and that
//! `all_merged_at`, once. A `draft` is an unapproved batch: the PR waits for it.


use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use crate::error::CpdError;
use crate::events::{self, NewEvent};
use crate::ids::{PlanId, PrKey};
use crate::store::WriteTxn;

/// The PRs E4 is for; `?1` and `?2` are one PR's Plan and key, or NULL for every PR.
const DONE: &str = "SELECT r.plan, r.key FROM prs r
  WHERE (?1 IS NULL OR r.plan = ?1) AND (?2 IS NULL OR r.key = ?2)
    AND r.status = 'dispatch' AND r.all_merged_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status IN ('draft', 'queued', 'running'))
    AND EXISTS (SELECT 1 FROM jobs j WHERE j.plan = r.plan AND j.pr = r.key AND j.status = 'merged')
  ORDER BY r.plan, r.key";

pub(super) fn run(engine: &mut Engine) {
    let prs = engine.store.read(|conn| {
        let mut stmt = conn.prepare(DONE)?;
        let rows = stmt
            .query_map([None::<&str>, None::<&str>], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|(plan, key)| Ok((PlanId::parse(plan)?, PrKey::parse(key)?))).collect::<Result<Vec<_>, CpdError>>()
    });
    let prs = match prs {
        Ok(prs) => {
            engine.clear(Step::E4, &About::Machine);
            prs
        }
        Err(e) => return engine.fail(Step::E4, About::Machine, &e),
    };
    for (plan, key) in prs {
        let about = About::Plan(plan.clone());
        match engine.store.write(SOURCE, |tx| complete(tx, &plan, &key)) {
            Ok(done) => {
                if done {
                    engine.log.line(&format!("E4: every Job of PR {key} of Plan {plan} is done: all-merged"));
                }
                engine.clear(Step::E4, &about);
            }
            Err(e) => engine.fail(Step::E4, about, &e),
        }
    }
}

/// The keys of the Jobs of PR `pr` of `plan` in `status`, in key order.
fn keys(tx: &WriteTxn<'_>, plan: &PlanId, pr: &PrKey, status: &str) -> Result<Vec<String>, CpdError> {
    let mut stmt = tx.prepare("SELECT key FROM jobs WHERE plan = ?1 AND pr = ?2 AND status = ?3 ORDER BY key")?;
    let keys = stmt.query_map([plan.as_str(), pr.as_str(), status], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(keys)
}

/// Checks the condition again inside the transaction, then sets
/// `all_merged_at` and appends `all-merged`. False when it no longer holds.
fn complete(tx: &WriteTxn<'_>, plan: &PlanId, pr: &PrKey) -> Result<bool, CpdError> {
    let still: bool = tx.query_row(&format!("SELECT EXISTS ({DONE})"), [plan.as_str(), pr.as_str()], |r| r.get(0))?;
    if !still {
        return Ok(false);
    }
    tx.execute(
        "UPDATE prs SET all_merged_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND key = ?2 AND all_merged_at IS NULL",
        [plan.as_str(), pr.as_str()],
    )?;
    let merged = keys(tx, plan, pr, "merged")?;
    let cancelled = keys(tx, plan, pr, "cancelled")?;
    let failed = keys(tx, plan, pr, "failed")?;
    let mut text = format!("No Job of PR {pr} of Plan {plan} is draft, queued or running. Merged: {}.", merged.join(", "));
    if !cancelled.is_empty() {
        text += &format!(" Cancelled: {}.", cancelled.join(", "));
    }
    if failed.is_empty() {
        text += &format!(" Next: cpd pr open --pr {pr} --plan {plan}");
    } else {
        text += &format!(
            " Failed: {}; decide each with cpd job retry <job> --tier <T> --plan {plan} or cpd job cancel <job> --reason <text> --plan {plan}, then: cpd pr open --pr {pr} --plan {plan}",
            failed.join(", ")
        );
    }
    events::append(tx, NewEvent::all_merged(text, pr, &merged, &cancelled).plan(plan.clone()))?;
    Ok(true)
}
