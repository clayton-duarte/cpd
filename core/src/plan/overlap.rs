//! The overlap warning of `plan render` and the Plan review (commands.md
//! §4.5, B4-A3, hs-d78): the other active Plans whose Jobs touch the files this
//! Plan's Jobs touch. A warning only; nothing is refused.

use rusqlite::Connection;
use serde_json::{Value, json};

use super::prs::PrRow;
use super::jobs::Job;
use crate::error::CpdError;
use crate::ids::PlanId;

/// One Job of another active Plan, in a PR that is not `merged` or `cancelled`
/// and not itself `cancelled`.
#[derive(Clone, Debug, PartialEq)]
pub struct Other {
    pub plan: String,
    pub pr: String,
    /// The PR's repo as stored (`prs.repo`, a realpath).
    pub repo: String,
    pub job: String,
    pub status: String,
    pub touches: Vec<String>,
}

/// One overlap: `job` of this Plan touches `path`, which overlaps `other_path`
/// of `other_job` in `pr` of `plan`.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlap {
    pub job: String,
    pub path: String,
    pub plan: String,
    pub pr: String,
    pub other_job: String,
    pub status: String,
    pub other_path: String,
}

impl Overlap {
    pub fn to_json(&self) -> Value {
        json!({
            "job": self.job, "path": self.path, "plan": self.plan, "pr": self.pr,
            "other_job": self.other_job, "status": self.status, "other_path": self.other_path,
        })
    }
}

/// Equal paths, or one is a directory (ends with `/`) containing the other.
fn overlaps(a: &str, b: &str) -> bool {
    a == b || (a.ends_with('/') && b.starts_with(a)) || (b.ends_with('/') && a.starts_with(b))
}

/// The Jobs of every other active Plan (any Context; not `done`, not
/// archived), in PRs not `merged` and not `cancelled`, that are not
/// `cancelled`: ordered by Plan, PR key and Job creation.
pub fn load_others(conn: &Connection, plan: &PlanId) -> Result<Vec<Other>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT p.plan, p.key, p.repo, j.key, j.status, j.touches
           FROM prs p
           JOIN plans l ON l.id = p.plan
           JOIN jobs j ON j.plan = p.plan AND j.pr = p.key
          WHERE p.plan <> ?1 AND l.phase <> 'done' AND l.archived_at IS NULL
            AND p.status NOT IN ('merged', 'cancelled') AND j.status <> 'cancelled'
          ORDER BY p.plan, p.key, j.seq",
    )?;
    let rows = stmt
        .query_map([plan.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(plan, pr, repo, job, status, touches)| {
            let touches = serde_json::from_str::<Vec<String>>(&touches)
                .map_err(|_| CpdError::bad_row(format!("Job {plan}/{job} has touches {touches:?}")))?;
            Ok(Other { plan, pr, repo, job, status, touches })
        })
        .collect()
}

/// Every overlap of this Plan's PRs (not `cancelled`) and Jobs (not
/// `cancelled`) with `others`, in the order of this Plan's Jobs, each Job's
/// paths, then `others`. Pure.
pub fn find(prs: &[PrRow], jobs: &[Job], others: &[Other]) -> Vec<Overlap> {
    let mut out = Vec::new();
    for job in jobs.iter().filter(|j| j.status != "cancelled") {
        let Some(pr) = prs.iter().find(|p| p.key.as_str() == job.pr && p.status != "cancelled") else { continue };
        let paths = job.touches.as_array().into_iter().flatten().filter_map(Value::as_str);
        for path in paths {
            for other in others.iter().filter(|o| o.repo == pr.repo) {
                for other_path in other.touches.iter().filter(|t| overlaps(path, t)) {
                    out.push(Overlap {
                        job: job.key.clone(),
                        path: path.to_owned(),
                        plan: other.plan.clone(),
                        pr: other.pr.clone(),
                        other_job: other.job.clone(),
                        status: other.status.clone(),
                        other_path: other_path.clone(),
                    });
                }
            }
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
