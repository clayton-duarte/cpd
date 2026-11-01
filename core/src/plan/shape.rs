//! The PR's stored shape, enforced (contract §2.2 "Job kinds", hs-d497; commands.md §4.6, §4.8, §5.3): a `tdd` PR that has not
//! opened keeps a `test` and an `impl` Job, and an `impl` Job needs a `test` Job of its PR.

use serde_json::json;

use super::jobs::Job;
use super::prs::PrRow;
use crate::error::{CpdError, ErrorCode};
use crate::ids::PlanId;

/// A PR of shape `tdd` that has not opened: its pull request does not exist yet, so its Jobs are still being laid out.
fn is_open_for_work(pr: &PrRow) -> bool {
    pr.shape == "tdd" && matches!(pr.status.as_str(), "waiting" | "dispatch")
}

/// The kinds (`test`, `impl`) a PR of shape `tdd` that has not opened lacks, ignoring the Job `without` and every `cancelled` one.
/// Empty for a PR of another shape or one that has opened.
pub(crate) fn missing(pr: &PrRow, jobs: &[Job], without: Option<&str>) -> Vec<&'static str> {
    if !is_open_for_work(pr) {
        return Vec::new();
    }
    ["test", "impl"]
        .into_iter()
        .filter(|kind| {
            !jobs.iter().any(|j| {
                j.pr_str() == pr.key.as_str()
                    && j.kind == *kind
                    && j.status != "cancelled"
                    && Some(j.key.as_str()) != without
            })
        })
        .collect()
}

/// `shape_incomplete`, `details` `{pr, missing}`.
pub(crate) fn incomplete(plan: &PlanId, pr: &PrRow, missing: &[&str], what: &str) -> CpdError {
    CpdError::new(
        ErrorCode::ShapeIncomplete,
        format!(
            "PR {} has the shape tdd, which keeps a test Job and an impl Job that are not cancelled: {what}, so it lacks {}. Add the missing Job first (cpd job add --kind <kind> --pr {}).",
            pr.key,
            missing.join(" and "),
            pr.key
        ),
    )
    .with("plan", plan.as_str())
    .with("pr", pr.key.as_str())
    .with("missing", json!(missing))
}

/// `plan review` and `plan lock` (§4.6, §4.8): every `impl` Job that is a `draft` needs a `test` Job of its PR
/// (`test_job_required`), then every `verify` one an `impl` Job, then every `tdd` PR that has not opened has a `test` and
/// an `impl` Job (`shape_incomplete`). The first refusal.
pub(super) fn check(plan: &PlanId, prs: &[PrRow], jobs: &[Job]) -> Result<(), CpdError> {
    if let Some(e) = test_job_required(plan, jobs) {
        return Err(e);
    }
    if let Some(e) = impl_job_required(plan, jobs) {
        return Err(e);
    }
    shape_incomplete(plan, prs, jobs)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

/// `test_job_required`: a draft `impl` Job needs no `test` Job of its PR.
pub(crate) fn test_job_required(plan: &PlanId, jobs: &[Job]) -> Option<CpdError> {
    let mut lacking: Vec<&Job> = jobs
        .iter()
        .filter(|j| {
            j.is_draft()
                && j.kind == "impl"
                && !j.needs.iter().any(|n| {
                    jobs.iter().any(|t| {
                        t.key == *n && t.pr == j.pr && t.kind == "test" && t.status != "cancelled"
                    })
                })
        })
        .collect();
    if lacking.is_empty() {
        return None;
    }
    lacking.sort_by(|a, b| a.key.cmp(&b.key));
    let keys: Vec<&str> = lacking.iter().map(|j| j.key.as_str()).collect();
    Some(
        CpdError::new(
            ErrorCode::TestJobRequired,
            format!("The impl Jobs {} must each need a test Job of their PR: their tests are the ones those Jobs name. Add the dependency with cpd job edit <key> --needs.", keys.join(", ")),
        )
        .with("plan", plan.as_str())
        .with("jobs", keys),
    )
}

/// `impl_job_required`: a draft `verify` Job needs no `impl` Job of its PR.
pub(crate) fn impl_job_required(plan: &PlanId, jobs: &[Job]) -> Option<CpdError> {
    let mut unneeding: Vec<&Job> = jobs
        .iter()
        .filter(|j| {
            j.is_draft()
                && j.kind == "verify"
                && !j.needs.iter().any(|n| {
                    jobs.iter().any(|t| {
                        t.key == *n && t.pr == j.pr && t.kind == "impl" && t.status != "cancelled"
                    })
                })
        })
        .collect();
    if unneeding.is_empty() {
        return None;
    }
    unneeding.sort_by(|a, b| a.key.cmp(&b.key));
    let keys: Vec<&str> = unneeding.iter().map(|j| j.key.as_str()).collect();
    let first = unneeding[0];
    let mut impls: Vec<&str> = jobs
        .iter()
        .filter(|j| j.pr == first.pr && j.kind == "impl" && j.status != "cancelled")
        .map(|j| j.key.as_str())
        .collect();
    impls.sort();
    Some(
        CpdError::new(
            ErrorCode::ImplJobRequired,
            format!("The verify Jobs {} must each need an impl Job of their PR: they check the code those Jobs build. Add the dependency with cpd job edit <key> --needs.", keys.join(", ")),
        )
        .with("plan", plan.as_str())
        .with("job", first.key.as_str())
        .with("jobs", impls),
    )
}

/// `shape_incomplete`: one refusal per `tdd` PR that has not opened and lacks a `test` or `impl` Job, in PR key order.
pub(crate) fn shape_incomplete(plan: &PlanId, prs: &[PrRow], jobs: &[Job]) -> Vec<CpdError> {
    let mut ordered: Vec<&PrRow> = prs.iter().collect();
    ordered.sort_by(|a, b| a.key.cmp(&b.key));
    ordered
        .into_iter()
        .filter_map(|pr| {
            let lacks = missing(pr, jobs, None);
            (!lacks.is_empty()).then(|| {
                incomplete(plan, pr, &lacks, "the review and the lock need both")
            })
        })
        .collect()
}
