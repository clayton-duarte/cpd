//! Red tests of the `## Explore` section of `plan render` (Plan `explore-roles`, R3, R9; commands.md §4.5): after `## Overlaps`
//! and before the PR sections, a table of the Plan's explore Jobs and the summary of each merged one; absent without explore Jobs.

use serde_json::json;

use super::render_all_jobs_tests::{PLAN, store_with};
use crate::ids::PlanId;
use super::jobs;

use super::display::Display;
use super::jobs::{Job, JobSpec};
use super::prs::PrRow;
use super::render::{Heading, markdown};

fn job(key: &str, pr: Option<&str>, kind: &str, status: &str, tier: &str, goal: &str, needs: &[&str]) -> Job {
    Job {
        seq: 1,
        key: key.into(),
        pr: pr.map(Into::into),
        title: format!("Title {key}"),
        tier: Some(tier.into()),
        spec: Some(JobSpec {
            goal: goal.into(),
            context: "c".into(),
            touches: if pr.is_some() { json!(["a"]) } else { json!([format!("run/p1/explore/{key}.md")]) },
            acceptance: json!(["b"]),
            out_of_scope: json!([]),
        }),
        status: status.into(),
        appended: false,
        kind: kind.into(),
        tests: None,
        test_files: None,
        test_approved: None,
        touches_approved: None,
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

/// The merged `scout` `a-scout` (with its summary) and the queued `research` `b-research`, which needs it.
fn explore_jobs() -> Vec<Job> {
    let mut scout = job("a-scout", None, "scout", "merged", "H2", "Where is the cache key built?\nAnd who reads it?", &[]);
    scout.merged_sha = Some("0".repeat(64));
    scout.summary = Some("The key is built in `core/src/cache.rs` from the repo path and the schema version.".into());
    let research = job("b-research", None, "research", "queued", "S1", &format!("{}{}", "q".repeat(100), "TAIL"), &["a-scout"]);
    vec![scout, research]
}

fn web_jobs() -> Vec<Job> {
    vec![job("web-docs", Some("web"), "docs", "draft", "S2", "g", &[])]
}

fn render(jobs: &[Job]) -> String {
    let prs = [PrRow::fixture("web", "/r", "main", &[], "dispatch")];
    markdown(&Heading::Plan { title: "T", plan: "p1" }, "ctx", "plan", &prs, jobs, &[], "S2", &Display::of(&prs))
}

#[test]
fn plan_render_has_an_explore_section_with_artefact_and_summary() {
    let mut jobs = web_jobs();
    jobs.extend(explore_jobs());
    let md = render(&jobs);
    let section = format!(
        "## Explore

| Job | Kind | Tier | Status | Needs | Question | Artefact |
|---|---|---|---|---|---|---|
| `a-scout` | `scout` | H2 | merged | — | Where is the cache key built? | `run/p1/explore/a-scout.md` |
| `b-research` | `research` | S1 | queued | `a-scout` | {} | `run/p1/explore/b-research.md` |

### a-scout
The key is built in `core/src/cache.rs` from the repo path and the schema version.

## PR web
",
        "q".repeat(100)
    );
    assert!(md.contains(&section), "{md}");
    let at = |needle: &str| md.find(needle).unwrap_or_else(|| panic!("no {needle:?} in\n{md}"));
    assert!(at("## Overlaps") < at("## Explore") && at("## Explore") < at("## PR web"), "{md}");
    // explore Jobs are in no other block: not the PR's table, not its graph
    assert!(!md.contains("TAIL") && !md.contains("[\"a-scout\"]") && !md.contains("### `b-research`"), "{md}");
    assert_eq!(md.matches("| `web-docs` |").count(), 1, "{md}");
}

fn render_no_prs(jobs: &[Job], phase: &str) -> String {
    markdown(&Heading::Plan { title: "T", plan: "p1" }, "ctx", phase, &[], jobs, &[], "S2", &Display::default())
}

#[test]
fn plan_render_of_a_plan_with_no_prs_has_the_explore_section_after_no_prs_yet() {
    let scout = explore_jobs().remove(0);
    let want = "\
# Plan: T

Context: `ctx` · Phase: `grounding`

No PRs yet.

## Explore

| Job | Kind | Tier | Status | Needs | Question | Artefact |
|---|---|---|---|---|---|---|
| `a-scout` | `scout` | H2 | merged | — | Where is the cache key built? | `run/p1/explore/a-scout.md` |

### a-scout
The key is built in `core/src/cache.rs` from the repo path and the schema version.
";
    let got = render_no_prs(&[scout], "grounding");
    assert_eq!(got, want, "\n{got}");
}

#[test]
fn plan_render_marks_a_cancelled_explore_job_and_gives_it_no_summary() {
    let mut cancelled = job("x-scout", None, "scout", "cancelled", "H1", "Never asked?", &[]);
    cancelled.cancel_reason = Some("not needed".into());
    cancelled.cancel_requested_at = Some("t".into());
    let got = render_no_prs(&[explore_jobs().remove(0), cancelled], "grounding");
    let row = got.lines().find(|l| l.starts_with("| `x-scout` |")).unwrap_or_else(|| panic!("no row of x-scout in\n{got}"));
    assert!(row.contains("| cancelled: not needed |"), "row: {row}\n{got}");
    assert!(got.contains("### a-scout") && !got.contains("### x-scout"), "{got}");
}

#[test]
fn job_load_reads_the_summary_of_a_merged_explore_job() {
    let spec = "'Goal.', 'c', '[\"a\"]', '[\"b\"]', '[]'";
    let sql = format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('{PLAN}', 'r', '/r', 'main', 'dispatch', 'cpd/{PLAN}/r', '/w', cpd_txn(), 't', 't');
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, merged_sha, summary, kind, created_txn, created_at)
           VALUES ('{PLAN}', NULL, 'scout-m', 'T', 'H1', {spec}, 'merged', 0, '{f}', '{f}', 'S', 'scout', cpd_txn(), 't'),
                  ('{PLAN}', NULL, 'scout-q', 'T', 'H1', {spec}, 'queued', 0, '{f}', NULL, NULL, 'scout', cpd_txn(), 't'),
                  ('{PLAN}', 'r', 'work-m', 'T', 'S2', {spec}, 'merged', 0, '{f}', '{f}', NULL, 'work', cpd_txn(), 't');",
        f = "f".repeat(64)
    );
    let (_dir, mut store) = store_with(&sql);
    let plan = PlanId::parse(PLAN).unwrap();
    let loaded = store.read(|conn| jobs::load(conn, &plan)).unwrap();
    let summary = |key: &str| loaded.iter().find(|j| j.key == key).unwrap_or_else(|| panic!("no Job {key}")).summary.clone();
    assert_eq!(
        (summary("scout-m"), summary("scout-q"), summary("work-m")),
        (Some("S".to_owned()), None, None)
    );
}
