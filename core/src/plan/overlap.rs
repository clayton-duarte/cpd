//! The overlap warning of `plan render` and the Plan review (commands.md
//! §4.5, B4-A3, hs-d78, hs-n201): the other active Plans whose Jobs touch the
//! files this Plan's open Jobs touch. A warning only; nothing is refused.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde_json::{Value, json};

use super::prs::WorkflowRow;
use super::jobs::Job;
use crate::error::CpdError;
use crate::ids::PlanId;

/// One Job of another active Plan, in a Workflow that is not `merged` or `cancelled`
/// and not itself `cancelled`.
#[derive(Clone, Debug, PartialEq)]
pub struct Other {
    pub plan: String,
    pub workflow: String,
    /// The Workflow's repo as stored (`prs.repo`, a realpath).
    pub repo: String,
    pub job: String,
    pub status: String,
    pub touches: Vec<String>,
}

/// A Job of another Plan's Workflow that overlaps a Job of this Plan.
#[derive(Clone, Debug, PartialEq)]
pub struct OtherJob {
    pub key: String,
    pub status: String,
}

/// One overlap: `job` of this Plan overlaps `other_jobs`, all of `pr` of `plan`.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlap {
    pub job: String,
    pub plan: String,
    pub workflow: String,
    /// The Jobs of `pr` that overlap `job`, in the order of the others given to [`find`].
    pub other_jobs: Vec<OtherJob>,
    /// The paths of `job` that overlap a path of one of `other_jobs`, each once, in the order of its `touches`.
    pub paths: Vec<String>,
}

impl Overlap {
    pub fn to_json(&self) -> Value {
        let other_jobs: Vec<Value> = self.other_jobs.iter().map(|j| json!({"key": j.key, "status": j.status})).collect();
        json!({"job": self.job, "plan": self.plan, "pr": self.workflow, "other_jobs": other_jobs, "paths": self.paths})
    }
}

/// Equal paths, or one is a directory (ends with `/`) containing the other.
fn overlaps(a: &str, b: &str) -> bool {
    a == b || (a.ends_with('/') && b.starts_with(a)) || (b.ends_with('/') && a.starts_with(b))
}

/// The Jobs of every other active Plan (any Context; not `done`, not
/// archived), in Workflows not `merged` and not `cancelled`, that are not
/// `cancelled`: ordered by Plan, Workflow key and Job creation.
pub fn load_others(conn: &Connection, plan: &PlanId) -> Result<Vec<Other>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.plan, p.key, p.repo, j.key, j.status, j.touches
           FROM workflows p
           JOIN plans l ON l.id = p.plan
           JOIN jobs j ON j.plan = p.plan AND j.workflow = p.key
          WHERE p.plan <> ?1 AND l.phase <> 'done' AND l.archived_at IS NULL
            AND p.status NOT IN ('merged', 'cancelled') AND j.status <> 'cancelled'
          ORDER BY p.plan, p.key, j.seq",
    )?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<String>>(5)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(plan, workflow, repo, job, status, touches)| {
            // A stub draft has no Spec yet: no files to compare.
            let touches = touches
                .map(|text| {
                    serde_json::from_str::<Vec<String>>(&text)
                        .map_err(|_| CpdError::bad_row(format!("Job {plan}/{job} has touches {text:?}")))
                })
                .transpose()?
                .unwrap_or_default();
            Ok(Other { plan, workflow, repo, job, status, touches })
        })
        .collect()
}

/// A Workflow of this Plan whose Jobs are not on their base yet: not `merged`, not
/// `cancelled`.
fn is_open(workflow: &WorkflowRow) -> bool {
    !matches!(workflow.status.as_str(), "merged" | "cancelled")
}

/// Every overlap of this Plan's open work with `others`: the Jobs not
/// `cancelled` of the Workflows that are not `merged` and not `cancelled` (a Job of a
/// `merged` Workflow is on its base already). One overlap per (Job, other Plan,
/// other Workflow) that has a Job overlapping it, ordered by this Job's key, then the
/// other Plan, then the Workflow. Pure.
pub fn find(workflows: &[WorkflowRow], jobs: &[Job], others: &[Other]) -> Vec<Overlap> {
    let mut open: Vec<&Job> = jobs.iter().filter(|j| j.status != "cancelled").collect();
    open.sort_by(|a, b| a.key.cmp(&b.key));
    let mut out = Vec::new();
    for job in open {
        let Some(workflow) = workflows.iter().find(|p| p.key.as_str() == job.workflow_str() && is_open(p)) else { continue };
        let mut paths: Vec<&str> = Vec::new();
        // A stub draft has no Spec yet, so no files to compare.
        let touches = job.spec.as_ref().and_then(|s| s.touches.as_array());
        for path in touches.into_iter().flatten().filter_map(Value::as_str) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        // By (other Plan, other Workflow): the Jobs there that overlap this one, and which of its paths they overlap.
        let mut groups: BTreeMap<(&str, &str), (Vec<OtherJob>, BTreeSet<usize>)> = BTreeMap::new();
        for other in others.iter().filter(|o| o.repo == workflow.repo) {
            let hit: Vec<usize> = (0..paths.len()).filter(|&i| other.touches.iter().any(|t| overlaps(paths[i], t))).collect();
            if hit.is_empty() {
                continue;
            }
            let (other_jobs, overlapped) = groups.entry((&other.plan, &other.workflow)).or_default();
            other_jobs.push(OtherJob { key: other.job.clone(), status: other.status.clone() });
            overlapped.extend(hit);
        }
        for ((other_plan, other_workflow), (other_jobs, overlapped)) in groups {
            out.push(Overlap {
                job: job.key.clone(),
                plan: other_plan.to_owned(),
                workflow: other_workflow.to_owned(),
                other_jobs,
                paths: overlapped.into_iter().map(|i| paths[i].to_owned()).collect(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_overlap_when_equal_or_one_directory_contains_the_other() {
        assert!(overlaps("core/src/x.rs", "core/src/x.rs"));
        assert!(overlaps("core/src/x.rs", "core/src/"));
        assert!(overlaps("core/src/", "core/src/x.rs"));
        assert!(overlaps("core/", "core/src/"));
        assert!(!overlaps("core/src/x.rs", "core/src/y.rs"));
        assert!(!overlaps("core/src", "core/src/x.rs"), "a path without a trailing slash is a file");
        assert!(!overlaps("core/src/", "core/srcs/x.rs"));
    }
}
