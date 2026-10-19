//! The plan fingerprint (contract §2.3): what a Plan review approves, so that
//! it covers the `draft` Jobs and the PRs.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::jobs::{self, Job, sha256_of};
use crate::error::CpdError;
use crate::ids::PlanId;

/// A PR as the fingerprint sees it: `{key, repo, base, needs}`, and its status
/// (`cancelled` PRs are left out of the fingerprint).
#[derive(Clone, Debug, PartialEq)]
pub struct PrSpec {
    pub key: String,
    pub repo: String,
    pub base: String,
    /// Keys of the PRs it needs, sorted.
    pub needs: Vec<String>,
    pub status: String,
}

/// Every PR of `plan`, `cancelled` ones included, ordered by key.
pub fn load_prs(conn: &Connection, plan: &PlanId) -> Result<Vec<PrSpec>, CpdError> {
    let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT pr, needs FROM pr_needs WHERE plan = ?1 ORDER BY pr, needs")?;
    for row in stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (pr, need) = row?;
        needs.entry(pr).or_default().push(need);
    }
    let mut stmt = conn.prepare("SELECT key, repo, base, status FROM prs WHERE plan = ?1 ORDER BY key")?;
    let rows = stmt
        .query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().map(|(key, repo, base, status)| PrSpec { needs: needs.remove(&key).unwrap_or_default(), key, repo, base, status }).collect())
}

/// The plan fingerprint of `plan` as it is now: `sha256` of the canonical JSON
/// `{jobs, prs}` of [`of`]. In a phase after `plan` the review covers the
/// drafts only, so `prs` is the empty array there: a PR appended after the
/// lock must not void a review of the drafts (§2.3).
pub fn plan_fingerprint(conn: &Connection, plan: &PlanId) -> Result<String, CpdError> {
    let phase: Option<String> = conn.query_row("SELECT phase FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0)).optional()?;
    let prs = if reviews_prs(phase.as_deref()) { load_prs(conn, plan)? } else { Vec::new() };
    Ok(of(&jobs::load(conn, plan)?, &prs))
}

/// Whether the review of a Plan in `phase` (none: not found) covers its PRs: before the lock only.
pub(super) fn reviews_prs(phase: Option<&str>) -> bool {
    matches!(phase, None | Some("grounding" | "plan"))
}

/// `sha256` of `{jobs, prs}` (§2.3): `jobs` is the Locked fields of every
/// `draft` Job (title, Tier, Spec, needs and PR, with the key, as cpd 1 §11
/// `plan render`), ordered by key; `prs` is `{key, repo, base, needs}` of
/// every PR in `prs` that is not `cancelled`, ordered by key, `needs` sorted.
pub fn of(jobs: &[Job], prs: &[PrSpec]) -> String {
    let mut drafts: Vec<&Job> = jobs.iter().filter(|j| j.is_draft()).collect();
    drafts.sort_by(|a, b| a.key.cmp(&b.key));
    let jobs: Vec<Value> = drafts
        .into_iter()
        .map(|j| {
            let mut needs = j.needs.clone();
            needs.sort();
            json!({
                "key": j.key, "pr": j.pr, "title": j.title, "tier": j.tier, "goal": j.goal, "context": j.context,
                "touches": j.touches, "acceptance": j.acceptance, "out_of_scope": j.out_of_scope, "needs": needs,
            })
        })
        .collect();
    let mut prs: Vec<&PrSpec> = prs.iter().filter(|p| p.status != "cancelled").collect();
    prs.sort_by(|a, b| a.key.cmp(&b.key));
    let prs: Vec<Value> = prs
        .into_iter()
        .map(|p| {
            let mut needs = p.needs.clone();
            needs.sort();
            json!({"key": p.key, "repo": p.repo, "base": p.base, "needs": needs})
        })
        .collect();
    sha256_of(&json!({"jobs": jobs, "prs": prs}))
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    fn job(key: &str, status: &str, needs: &[&str]) -> Job {
        Job {
            seq: 1,
            key: key.into(),
            pr: "web".into(),
            title: format!("Title {key}"),
            tier: "S2".into(),
            goal: "g".into(),
            context: "c".into(),
            touches: json!(["a"]),
            acceptance: json!(["b"]),
            out_of_scope: json!([]),
            status: status.into(),
            appended: false,
            review: false,
            fingerprint: None,
            retry_tier: None,
            cancel_reason: None,
            cancel_requested_at: None,
            failed_cause: None,
            merged_sha: None,
            created_at: "t".into(),
            needs: needs.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    fn pr(key: &str, needs: &[&str]) -> PrSpec {
        PrSpec { key: key.into(), repo: format!("/r/{key}"), base: "main".into(), needs: needs.iter().map(|n| (*n).to_owned()).collect(), status: "waiting".into() }
    }

    #[test]
    fn the_plan_fingerprint_covers_the_drafts_by_key() {
        let (a, b, gone) = (job("a", "draft", &[]), job("b", "draft", &["a"]), job("c", "cancelled", &[]));
        let prs = [pr("web", &[])];
        let base = of(&[a.clone(), b.clone()], &prs);
        assert_eq!(of(&[b.clone(), gone, a.clone()], &prs), base, "order and non-drafts do not count");
        let mut retitled = b.clone();
        retitled.title = "New".into();
        assert_ne!(of(&[a.clone(), retitled], &prs), base);
        let mut retiered = b.clone();
        retiered.tier = "O3".into();
        assert_ne!(of(&[a.clone(), retiered], &prs), base);
        let mut moved = b;
        moved.pr = "api".into();
        assert_ne!(of(&[a, moved], &prs), base, "a Job's PR counts");
        assert_eq!(of(&[], &[]), format!("{:x}", Sha256::digest(br#"{"jobs":[],"prs":[]}"#)));
    }

    #[test]
    fn the_plan_fingerprint_covers_the_prs_by_key_with_needs_sorted() {
        let jobs = [job("a", "draft", &[])];
        let expected = format!(
            "{:x}",
            Sha256::digest(
                br#"{"jobs":[],"prs":[{"base":"main","key":"api","needs":[],"repo":"/r/api"},{"base":"main","key":"web","needs":["api","db"],"repo":"/r/web"}]}"#
            )
        );
        assert_eq!(of(&[], &[pr("web", &["db", "api"]), pr("api", &[])]), expected);
        let base = of(&jobs, &[pr("api", &[]), pr("web", &["api"])]);
        let mut other_repo = pr("web", &["api"]);
        other_repo.repo = "/elsewhere".into();
        let mut other_base = pr("web", &["api"]);
        other_base.base = "develop".into();
        let mut cancelled = pr("extra", &[]);
        cancelled.status = "cancelled".into();
        let mut with_cancelled = vec![pr("api", &[]), pr("web", &["api"])];
        with_cancelled.push(cancelled);
        assert_eq!(of(&jobs, &with_cancelled), base, "a cancelled PR does not count");
        for changed in [vec![pr("api", &[]), other_repo], vec![pr("api", &[]), other_base], vec![pr("api", &[]), pr("web", &[])], vec![pr("api", &[])]] {
            assert_ne!(of(&jobs, &changed), base);
        }
    }
}
