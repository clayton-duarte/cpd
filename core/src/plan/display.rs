//! What the views show of work that is cancelled or cannot start (hs-d518, hs-d521, hs-d523): one derivation for the
//! Plan view, the plan render and `context pending`, so the three never disagree with each other or with scheduling.
//!
//! - **Waiting**: a `queued` Job, or a PR that has not started, that is not Ready (contract §3.2) is *shown* as
//!   `waiting`, with its blockers. A display only: the stored status never changes. The Job rule is E3's
//!   (`engine/src/watch/schedule.rs` `READY`): the Dependencies not `merged`, an `open` or `update` Job's other Jobs
//!   ([`crate::job::open::unfinished`]), a Job of a PR that has not started shows nothing more; a PR's needs not `merged` and its base PR not started (§3.8).
//!   A `cancelled` need counts for nothing: it blocks nothing.
//! - **Superseded**: a `cancelled` PR names the PR that carried its work, from its `pr-cancelled` Event.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use super::jobs::Job;
use super::prs::WorkflowRow;
use crate::error::CpdError;
use crate::ids::PlanId;
use crate::job::open::unfinished;

/// The PR statuses and bases the blockers read, and the PRs that superseded cancelled ones.
#[derive(Clone, Debug, Default)]
pub struct Display {
    /// Every PR's status by PR reference: the bare key of a PR of the Plan, `<plan>/<key>` of another's.
    statuses: BTreeMap<String, String>,
    /// The base PR (a reference) of each stacked PR of the Plan, by PR key.
    bases: BTreeMap<String, String>,
    /// The PR (a reference) that superseded each cancelled PR of the Plan, by PR key.
    superseded: BTreeMap<String, String>,
    /// Each Job's Attempts of the Plan as (number, tier, status), by Job key, in number order.
    attempts: BTreeMap<String, Vec<(u32, String, String)>>,
    /// The Jobs of the Plan with a retry pending ([`crate::job::retries_pending`]).
    retrying: BTreeSet<String>,
}

/// A PR of the Plan as the reference its own Plan uses, or `<plan>/<key>`.
fn reference(own: &str, plan: &str, key: &str) -> String {
    if own == plan { key.to_owned() } else { format!("{plan}/{key}") }
}

impl Display {
    /// Read in the caller's transaction, for `plan`.
    pub fn load(conn: &Connection, plan: &PlanId) -> Result<Display, CpdError> {
        let own = plan.as_str();
        let mut display = Display::default();
        let mut stmt = conn.prepare("SELECT plan, key, status FROM workflows")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (p, key, status) = row?;
            display.statuses.insert(reference(own, &p, &key), status);
        }
        // a stacked PR's base is the PR whose Lead Branch is its `base` (contract §2.2 "The base PR")
        let mut stmt = conn.prepare("SELECT p.key, q.plan, q.key FROM workflows p JOIN workflows q ON q.lead_branch = p.base WHERE p.plan = ?1")?;
        for row in stmt.query_map([own], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (key, base_plan, base) = row?;
            display.bases.insert(key, reference(own, &base_plan, &base));
        }
        let mut stmt = conn.prepare(
            "SELECT json_extract(data, '$.pr'), json_extract(data, '$.superseded_by') FROM events
              WHERE kind = 'pr-cancelled' AND plan = ?1 AND json_extract(data, '$.superseded_by') IS NOT NULL ORDER BY id",
        )?;
        for row in stmt.query_map([own], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (key, by) = row?;
            display.superseded.insert(key, by);
        }
        let mut stmt = conn.prepare("SELECT job, number, tier, status FROM attempts WHERE plan = ?1 ORDER BY job, number")?;
        for row in stmt.query_map([own], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))? {
            let (job, number, tier, status) = row?;
            display.attempts.entry(job).or_default().push((number, tier, status));
        }
        display.retrying = crate::job::retries_pending(conn, plan)?;
        Ok(display)
    }

    /// Without a store: the PRs of the Plan alone (every need is of the Plan), their bases found by Lead Branch.
    #[cfg(test)]
    pub fn of(workflows: &[WorkflowRow]) -> Display {
        let mut display = Display::default();
        for p in workflows {
            display.statuses.insert(p.key.to_string(), p.status.clone());
            if let Some(base) = workflows.iter().find(|q| q.lead_branch == p.base) {
                display.bases.insert(p.key.to_string(), base.key.to_string());
            }
        }
        display
    }

    /// The Tier the Job `job` is shown at (the Plan view pill, `plan render`): the Tier of its active Attempt, else its
    /// `retry_tier` while a retry has no Attempt since, else the latest Attempt's Tier, else its `tier`.
    pub fn job_tier(&self, job: &Job) -> Option<String> {
        let attempts = self.attempts.get(job.key.as_str()).map(Vec::as_slice).unwrap_or_default();
        let active = attempts.iter().find(|(_, _, status)| status != "ended").map(|(_, tier, _)| tier.clone());
        let retry = || self.retrying.contains(job.key.as_str()).then(|| job.retry_tier.clone()).flatten();
        active.or_else(retry).or_else(|| attempts.last().map(|(_, tier, _)| tier.clone())).or_else(|| job.tier.clone())
    }

    /// The PR that superseded the cancelled PR `key`, as a reference.
    pub fn superseded_by(&self, key: &str) -> Option<&str> {
        self.superseded.get(key).map(String::as_str)
    }

    /// What the `queued` Job `job` cannot start until, sorted (references of PRs and keys of Jobs): empty when it is Ready,
    /// and for a Job that is not `queued`. `jobs` are every Job of the Plan.
    pub fn job_blockers(&self, jobs: &[Job], job: &Job) -> Vec<String> {
        if job.status != "queued" {
            return Vec::new();
        }
        let mut blockers: Vec<String> = Vec::new();
        match unfinished(jobs, job) {
            Some(keys) => blockers.extend(keys),
            None => blockers.extend(
                job.needs.iter().filter(|n| jobs.iter().any(|j| &j.key == *n && !matches!(j.status.as_str(), "merged" | "cancelled"))).cloned(),
            ),
        }
        blockers.sort();
        blockers.dedup();
        blockers
    }

    /// The PRs `pr` needs as the views show them: its stored needs, then its base PR (the implied need, derived, never stored).
    pub fn workflow_needs(&self, workflow: &WorkflowRow) -> Vec<String> {
        let mut needs = workflow.needs.clone();
        if let Some(base) = self.bases.get(workflow.key.as_str()) {
            if !needs.contains(base) {
                needs.push(base.clone());
            }
        }
        needs
    }

    /// What the PR that has not started cannot start until, sorted (PR references): the PRs it needs that are not `merged`, and
    /// its base PR when that has not started. Empty when it can start, and for a PR that has started.
    pub fn workflow_blockers(&self, workflow: &WorkflowRow) -> Vec<String> {
        if workflow.status != "waiting" {
            return Vec::new();
        }
        let mut blockers: Vec<String> =
            workflow.needs.iter().filter(|n| self.statuses.get(*n).is_none_or(|s| !matches!(s.as_str(), "merged" | "cancelled"))).cloned().collect();
        // the implied need (contract §2.2): met when the base PR is `open` or `merged`
        if let Some(base) = self.bases.get(workflow.key.as_str()).filter(|b| self.statuses.get(*b).is_some_and(|s| matches!(s.as_str(), "waiting" | "dispatch"))) {
            blockers.push(base.clone());
        }
        blockers.sort();
        blockers.dedup();
        blockers
    }
}
