//! Red tests of the `## Explore` section of `plan render` (Plan `explore-roles`, R3, R9; commands.md §4.5): after `## Overlaps`
//! and before the PR sections, a table of the Plan's explore Jobs and the summary of each merged one; absent without explore Jobs.

use serde_json::json;

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

#[test]
fn plan_render_omits_the_explore_section_without_explore_jobs() {
    let want = "\
# Plan: T

Context: `ctx` · Phase: `plan`

## PRs

| Key | Repo | Base | Needs | Status |
|---|---|---|---|---|
| `web` | `/r` | `main` | none | dispatch |

## Overlaps

No other active Plan touches these files.

## PR web

Repo: `/r` · Base: `main` · Needs: none

### Jobs

| Key | Title | Kind | Tier | Status | Needs | Touches |
|---|---|---|---|---|---|---|
| `web-docs` | Title web-docs | docs | S2 | draft | none | `a` |

### Dependencies

```mermaid
graph TD
  j1[\"web-docs\"]
```

### `web-docs`: Title web-docs

Status: `draft` · Kind: `docs` · Tier: `S2` · Needs: none

#### Goal

g

#### Context

c

#### Files it touches

- `a`

#### Acceptance

- b

#### Out of scope

None.
";
    let got = render(&web_jobs());
    assert_eq!(got, want, "\n{got}");
    assert!(!got.contains("Explore"), "{got}");
}
