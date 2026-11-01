//! The chain of a Job added to an `open` Workflow whose `open` Job is merged (commands.md §5.1): it needs the chain leaves.

use std::collections::BTreeSet;

use crate::error::{CpdError, ErrorCode};
use crate::ids::PlanId;
use crate::plan::jobs::Job;

/// The leaves of the Workflow's chain, or `None` when the Workflow is not chained (not `open`, or its `open` Job is not merged).
/// The chain is the non-cancelled Jobs after the open Job (the open Job and every Job that transitively needs it),
/// minus `key` and whoever needs `key` (a rerun of the same add); drafts and `update` Jobs are never members. A leaf is a member
/// no other member needs; a `failed` leaf refuses `needs_unavailable`.
pub(super) fn leaves(all: &[Job], plan: &PlanId, workflow: &str, workflow_status: &str, key: &str) -> Result<Option<Vec<String>>, CpdError> {
    if workflow_status != "open" {
        return Ok(None);
    }
    let Some(open) = all.iter().find(|j| j.workflow_str() == workflow && j.kind == "open" && j.status == "merged") else { return Ok(None) };
    let descendants = |root: &str| {
        let mut seen: BTreeSet<String> = BTreeSet::from([root.to_owned()]);
        loop {
            let before = seen.len();
            let next: Vec<String> = all.iter().filter(|j| j.workflow_str() == workflow && j.needs.iter().any(|n| seen.contains(n))).map(|j| j.key.clone()).collect();
            seen.extend(next);
            if seen.len() == before {
                return seen;
            }
        }
    };
    let after_open = descendants(&open.key);
    let after_key = descendants(key);
    let members: Vec<&Job> = all
        .iter()
        .filter(|j| after_open.contains(&j.key) && !after_key.contains(&j.key))
        .filter(|j| j.status != "cancelled" && !j.is_draft() && j.kind != "update")
        .collect();
    let leaves: Vec<&Job> = members.iter().copied().filter(|j| !members.iter().any(|m| m.needs.contains(&j.key))).collect();
    let failed: Vec<&str> = leaves.iter().filter(|j| j.status == "failed").map(|j| j.key.as_str()).collect();
    if !failed.is_empty() {
        return Err(CpdError::new(
            ErrorCode::NeedsUnavailable,
            format!("A Job added to Workflow {workflow} chains after {}, which failed: retry or cancel it first.", failed.join(", ")),
        )
        .with("plan", plan.as_str())
        .with("pr", workflow)
        .with("jobs", failed));
    }
    let mut keys: Vec<String> = leaves.iter().map(|j| j.key.clone()).collect();
    keys.sort();
    Ok(Some(keys))
}
