//! The plan fingerprint (contract §2.3): what a Plan review approves, so that
//! it covers the `draft` Jobs and the PRs.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::jobs::{self, Job, sha256_of};
use super::outline::Outline;
use super::prs::PrRow;
use crate::error::CpdError;
use crate::ids::PlanId;

pub use super::prs::load_prs;

/// The plan fingerprint of `plan` as it is now: `sha256` of the canonical JSON
/// `{jobs, prs}` of [`of`], plus `outline` when the Plan's outline is set (hs-d502, D2). In a phase after `plan` the review covers the
/// drafts only, so `prs` is the empty array there: a PR appended after the
/// lock must not void a review of the drafts (§2.3).
pub fn plan_fingerprint(conn: &Connection, plan: &PlanId) -> Result<String, CpdError> {
    let phase: Option<String> = conn.query_row("SELECT phase FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0)).optional()?;
    let prs = if reviews_prs(phase.as_deref()) { load_prs(conn, plan)? } else { Vec::new() };
    Ok(of_with(&jobs::load(conn, plan)?, &prs, super::outline::load(conn, plan)?.as_ref()))
}

/// Whether the review of a Plan in `phase` (none: not found) covers its PRs: before the lock only.
pub(super) fn reviews_prs(phase: Option<&str>) -> bool {
    matches!(phase, None | Some("grounding" | "plan"))
}

/// `sha256` of `{jobs, prs}` (§2.3): `jobs` is the Locked fields of every
/// `draft` Job (title, Tier, Spec, needs and PR, with the key, as cpd 1 §11
/// `plan render`), ordered by key, a stub with its missing fields as JSON
/// `null` (hs-d241), with `kind` only when it is not `work` and `tests` and `test_files` only when set (hs-d341);
/// `prs` is `{key, repo, base, needs}` of every PR in `prs` that is not `cancelled`, with `shape` only when it is
/// not `free` (hs-d497), ordered by key, `needs` sorted.
pub fn of(jobs: &[Job], prs: &[PrRow]) -> String {
    of_with(jobs, prs, None)
}

/// [`of`] with the Plan's outline: when it is set, the object is `{jobs, outline, prs}`, `outline` being its object
/// (contract §2.3, hs-d502); when it is not, `{jobs, prs}` exactly, so a review made before the outline existed stays current.
pub fn of_with(jobs: &[Job], prs: &[PrRow], outline: Option<&Outline>) -> String {
    let mut drafts: Vec<&Job> = jobs.iter().filter(|j| j.is_under_review()).collect();
    drafts.sort_by(|a, b| a.key.cmp(&b.key));
    let jobs: Vec<Value> = drafts
        .into_iter()
        .map(|j| {
            let mut needs = j.needs.clone();
            needs.sort();
            let mut fields = j.locked_fields();
            fields.extend([("key".into(), json!(j.key)), ("pr".into(), json!(j.pr)), ("title".into(), json!(j.title)), ("needs".into(), json!(needs))]);
            fields.extend(j.kind_fields());
            Value::Object(fields)
        })
        .collect();
    let mut prs: Vec<&PrRow> = prs.iter().filter(|p| p.status != "cancelled").collect();
    prs.sort_by(|a, b| a.key.cmp(&b.key));
    let prs: Vec<Value> = prs
        .into_iter()
        .map(|p| {
            let mut needs = p.needs.clone();
            needs.sort();
            let mut fields = json!({"key": p.key, "repo": p.repo, "base": p.base, "needs": needs});
            if p.shape != "free" {
                fields["shape"] = json!(p.shape);
            }
            fields
        })
        .collect();
    let mut plan = json!({"jobs": jobs, "prs": prs});
    if let Some(outline) = outline {
        plan["outline"] = outline.to_value();
    }
    sha256_of(&plan)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::super::jobs::JobSpec;
    use super::*;

    fn job(key: &str, status: &str, needs: &[&str]) -> Job {
        Job {
            seq: 1,
            key: key.into(),
            pr: Some("web".into()),
            title: format!("Title {key}"),
            tier: Some("S2".into()),
            spec: Some(JobSpec { goal: "g".into(), context: "c".into(), touches: json!(["a"]), acceptance: json!(["b"]), out_of_scope: json!([]) }),
            status: status.into(),
            appended: false,
            kind: "work".into(),
            tests: None,
            test_files: None,
            test_approved: None,
            resources: None,
            urgent: false,
            fingerprint: None,
            retry_tier: None,
            cancel_reason: None,
            cancel_requested_at: None,
            failed_cause: None,
            merged_sha: None,
            summary: None,
            created_at: "t".into(),
            needs: needs.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    fn pr(key: &str, needs: &[&str]) -> PrRow {
        PrRow::fixture(key, &format!("/r/{key}"), "main", needs, "waiting")
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
        retiered.tier = Some("O3".into());
        assert_ne!(of(&[a.clone(), retiered], &prs), base);
        let mut moved = b;
        moved.pr = Some("api".into());
        assert_ne!(of(&[a, moved], &prs), base, "a Job's PR counts");
        assert_eq!(of(&[], &[]), format!("{:x}", Sha256::digest(br#"{"jobs":[],"prs":[]}"#)));
    }

    #[test]
    fn the_plan_fingerprint_covers_a_stub_as_it_is_with_null_for_what_it_lacks() {
        let mut stub = job("a", "draft", &[]);
        (stub.tier, stub.spec) = (None, None);
        let expected = format!(
            "{:x}",
            Sha256::digest(
                br#"{"jobs":[{"acceptance":null,"context":null,"goal":null,"key":"a","needs":[],"out_of_scope":null,"pr":"web","tier":null,"title":"Title a","touches":null}],"prs":[]}"#
            )
        );
        assert_eq!(of(&[stub.clone()], &[]), expected);
        let mut tiered = stub.clone();
        tiered.tier = Some("S2".into());
        assert_ne!(of(&[tiered.clone()], &[]), expected, "a Tier changes it");
        assert_ne!(of(&[job("a", "draft", &[])], &[]), of(&[tiered], &[]), "so does the Spec");
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

    #[test]
    fn kind_and_tests_are_in_the_plan_fingerprint_only_when_not_default_and_shape_only_when_not_free() {
        // A `work` Job with no tests and a `free` PR hash as they did before migration `job-kinds` (no stored review is voided).
        let before = format!(
            "{:x}",
            Sha256::digest(
                br#"{"jobs":[{"acceptance":["b"],"context":"c","goal":"g","key":"a","needs":[],"out_of_scope":[],"pr":"web","tier":"S2","title":"Title a","touches":["a"]}],"prs":[{"base":"main","key":"web","needs":[],"repo":"/r/web"}]}"#
            )
        );
        assert_eq!(of(&[job("a", "draft", &[])], &[pr("web", &[])]), before);

        let mut docs = job("a", "draft", &[]);
        docs.kind = "docs".into();
        let docs_fp = of(&[docs.clone()], &[pr("web", &[])]);
        assert_ne!(docs_fp, before, "a kind that is not work counts");
        let expected = format!(
            "{:x}",
            Sha256::digest(
                br#"{"jobs":[{"acceptance":["b"],"context":"c","goal":"g","key":"a","kind":"docs","needs":[],"out_of_scope":[],"pr":"web","tier":"S2","title":"Title a","touches":["a"]}],"prs":[{"base":"main","key":"web","needs":[],"repo":"/r/web"}]}"#
            )
        );
        assert_eq!(docs_fp, expected);

        let mut test = job("a", "draft", &[]);
        test.kind = "test".into();
        test.tests = Some(json!(["t::one"]));
        test.test_files = Some(json!(["tests/t.rs"]));
        let with_tests = of(&[test.clone()], &[pr("web", &[])]);
        test.tests = Some(json!(["t::two"]));
        assert_ne!(of(&[test.clone()], &[pr("web", &[])]), with_tests, "the named tests count");
        test.tests = Some(json!(["t::one"]));
        test.test_files = Some(json!(["tests/u.rs"]));
        assert_ne!(of(&[test], &[pr("web", &[])]), with_tests, "so do the files that hold them");
        let mut approved = job("a", "draft", &[]);
        approved.kind = "impl".into();
        let without = of(&[approved.clone()], &[]);
        approved.test_approved = Some(json!(["tests/t.rs"]));
        assert_eq!(of(&[approved], &[]), without, "test_approved is in no fingerprint");

        let mut tdd = pr("web", &[]);
        tdd.shape = "tdd".into();
        let expected = format!(
            "{:x}",
            Sha256::digest(br#"{"jobs":[],"prs":[{"base":"main","key":"web","needs":[],"repo":"/r/web","shape":"tdd"}]}"#)
        );
        assert_eq!(of(&[], &[tdd.clone()]), expected);
        assert_ne!(of(&[], &[tdd.clone()]), of(&[], &[pr("web", &[])]));
        let mut docs_pr = tdd;
        docs_pr.shape = "docs".into();
        assert_ne!(of(&[], &[docs_pr]), expected, "the shape counts");
    }

    #[test]
    fn the_locked_job_fingerprint_has_no_kind_tests_or_test_files() {
        let mut plain = job("a", "queued", &["x"]);
        let base = jobs::locked_fingerprint(&plain);
        plain.kind = "test".into();
        plain.tests = Some(json!(["t"]));
        plain.test_files = Some(json!(["f"]));
        assert_eq!(jobs::locked_fingerprint(&plain), base, "they are guarded by jobs_locked, not hashed (contract §2.3)");
    }
}
