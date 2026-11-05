//! A parked draft (contract §2.2 "A parked draft", hs-d822): a `draft` Job of a live Plan that is neither incomplete
//! (a stub, no Tier or no Spec) nor covered by a current Plan review (pending, or approved in phase `plan`). Derived from
//! the store, never stored; this module is the one definition: the door of `job add` and `job edit` asks [`door`], the
//! engine asks [`parked_drafts`].

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension};

use super::fingerprint::{load_workflows, plan_fingerprint};
use super::jobs::{self, Job};
use crate::error::{CpdError, ErrorCode};
use crate::ids::{PlanId, WorkflowKey};
use crate::pr::start::not_ready;

/// What decides whether a draft waits: the Plan's Jobs and the Workflows that wait for another Workflow (only after the lock).
struct Waits<'a> {
    all: &'a [Job],
    workflow_waits: BTreeSet<String>,
}

impl Waits<'_> {
    /// `job` needs, directly or through drafts that wait themselves, a Job that is `queued`, `running` or `failed`;
    /// or its Workflow waits for another Workflow.
    fn waiting(&self, job: &Job, seen: &mut BTreeSet<String>) -> bool {
        if self.workflow_waits.contains(job.workflow_str()) {
            return true;
        }
        job.needs.iter().any(|need| {
            let Some(need) = self.all.iter().find(|j| &j.key == need) else {
                return false;
            };
            match need.status.as_str() {
                "queued" | "running" | "failed" => true,
                "draft" => seen.insert(need.key.clone()) && self.waiting(need, seen),
                _ => false,
            }
        })
    }
}

fn locked(phase: &str) -> bool {
    matches!(phase, "dispatch" | "deploy" | "done")
}

/// The Workflows of `plan` that wait for another Workflow, when the Plan is locked: the condition engine E7 starts a Workflow by.
fn waiting_prs(
    conn: &Connection,
    plan: &PlanId,
    phase: &str,
) -> Result<BTreeSet<String>, CpdError> {
    let mut waits = BTreeSet::new();
    if locked(phase) {
        for workflow in load_workflows(conn, plan)? {
            let key = WorkflowKey::parse(workflow.key.as_str())?;
            if !not_ready(conn, plan, &key)?.is_empty() {
                waits.insert(workflow.key.to_string());
            }
        }
    }
    Ok(waits)
}

/// The Plan's latest Plan review is current (its recorded fingerprint is the Plan's, as `plan review` tests it) and is `pending`,
/// or `approved` in phase `plan` (the drafts stay drafts until `plan lock`; later phases queued them on approval).
fn covered(conn: &Connection, plan: &PlanId, phase: &str) -> Result<bool, CpdError> {
    let latest: Option<(String, String)> = conn
        .query_row("SELECT status, fingerprint FROM plan_reviews WHERE plan = ?1 ORDER BY number DESC LIMIT 1", [plan.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    match latest {
        Some((status, fingerprint))
            if status == "pending" || (status == "approved" && phase == "plan") =>
        {
            Ok(fingerprint == plan_fingerprint(conn, plan)?)
        }
        _ => Ok(false),
    }
}

/// The keys of the parked drafts of `plan`, in Job order. An archived Plan has none.
pub fn parked_drafts(conn: &Connection, plan: &PlanId) -> Result<Vec<String>, CpdError> {
    let (phase, archived): (String, bool) = conn
        .query_row(
            "SELECT phase, archived_at IS NOT NULL FROM plans WHERE id = ?1",
            [plan.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| CpdError::plan_not_found(plan))?;
    if archived {
        return Ok(Vec::new());
    }
    let all = jobs::load(conn, plan)?;
    if !all.iter().any(|j| j.is_draft() && j.kind != "open") || covered(conn, plan, &phase)? {
        return Ok(Vec::new());
    }
    Ok(all
        .iter()
        .filter(|j| j.is_draft() && j.kind != "open")
        .map(|j| j.key.clone())
        .collect())
}

/// The door of `job add --draft` and `job edit` for ONE draft row, before it is written: after the lock, a draft
/// that waits is refused `draft_parked`: it waits on a queued, running or failed Job (directly or
/// through other drafts) or on its Workflow waiting for another Workflow. `others` are the Plan's Jobs as stored, `job` the row as it
/// would be (never in `others`). Never refused: the `open` Job's draft, a draft that does not wait, any
/// draft in phase `plan`.
pub fn door(
    conn: &Connection,
    plan: &PlanId,
    phase: &str,
    others: &[Job],
    job: &Job,
) -> Result<(), CpdError> {
    if !locked(phase) || !job.is_draft() || job.kind == "open" {
        return Ok(());
    }
    let all: Vec<Job> = others
        .iter()
        .filter(|j| j.key != job.key)
        .cloned()
        .collect();
    let waits = Waits {
        all: &all,
        workflow_waits: waiting_prs(conn, plan, phase)?,
    };
    if !waits.waiting(job, &mut BTreeSet::from([job.key.clone()])) {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::DraftParked,
        format!(
            "Job {} would be a parked draft: a draft that waits for a queued, running or failed Job (directly or \
             through other drafts) or for another Workflow, and no current review covers it. The legal forms: add the Job it waits \
             for queued with --needs and no --draft; or put this complete draft under a Plan review (cpd_plan_review) in the \
             same turn.",
            job.key
        ),
    )
    .with("plan", plan.as_str())
    .with("job", job.key.as_str())
    .with("pr", job.workflow_str())
    .with("needs", job.needs.clone()))
}
