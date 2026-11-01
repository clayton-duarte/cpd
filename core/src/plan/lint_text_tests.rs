//! Red tests of the Lint block of the summary and the `## Lint` section of plan.md (commands.md §4.5): pure calls.

use std::collections::BTreeMap;

use serde_json::json;

use super::display::Display;
use super::jobs::{Job, JobSpec};
use super::lint::{Finding, Rule};
use super::outline::Outline;
use super::prs::WorkflowRow;
use super::render::{Heading, markdown as plan_markdown};
use super::row::{Phase, PlanRow};
use super::summary::{Input, Kind, markdown as summary_markdown};
use crate::ids::{ContextId, PlanId, SessionId};

const HINT: &str = "(a hint, not a conflict prediction)";

fn job(
    key: &str,
    workflow: &str,
    kind: &str,
    status: &str,
    touches: &[&str],
    needs: &[&str],
) -> Job {
    Job {
        seq: 1,
        key: key.into(),
        workflow: Some(workflow.into()),
        title: format!("Title {key}"),
        tier: Some("S2".into()),
        spec: Some(JobSpec {
            goal: "g".into(),
            context: "c".into(),
            touches: json!(touches),
            acceptance: json!(["b"]),
            out_of_scope: json!([]),
        }),
        status: status.into(),
        appended: false,
        kind: kind.into(),
        tests: None,
        test_files: None,
        resources: None,
        test_approved: None,
        touches_approved: None,
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

/// A draft `work` Job of Workflow `web`.
fn draft(key: &str, touches: &[&str], needs: &[&str]) -> Job {
    job(key, "web", "work", "draft", touches, needs)
}

fn workflow(key: &str) -> WorkflowRow {
    WorkflowRow::fixture(key, "/repo", "main", &[], "waiting")
}

fn plan_row(outline: Option<Outline>) -> PlanRow {
    PlanRow {
        id: PlanId::parse("pl").unwrap(),
        context: ContextId::parse("ctx").unwrap(),
        title: "Title".into(),
        jira: None,
        phase: Phase::Plan,
        lead_session: SessionId::parse("s1").unwrap(),
        created_at: "t".into(),
        locked_at: None,
        done_at: None,
        archived_at: None,
        outline,
    }
}

fn outline(decided: &[&str]) -> Outline {
    Outline {
        assumptions: vec![],
        decided: decided.iter().map(|d| (*d).to_owned()).collect(),
        decisions: vec![],
        done_when: BTreeMap::new(),
        goal: "g".into(),
        out_of_scope: vec![],
        risks: vec![],
    }
}

fn finding(job: Option<&str>, other: Option<&str>, paths: &[&str], message: &str) -> Finding {
    Finding {
        rule: Rule::ParallelWriteOverlap,
        message: message.into(),
        details: serde_json::Map::new(),
        pr: Some("web".into()),
        job: job.map(str::to_owned),
        other: other.map(str::to_owned),
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
    }
}

fn overlap(job: &str, other: &str, paths: &[&str]) -> Finding {
    finding(Some(job), Some(other), paths, "message")
}

/// The summary of a review of a Plan with one Workflow and one draft Job, showing `lint`.
fn summary(outline: Option<Outline>, lint: &[Finding]) -> String {
    let workflows = [workflow("web")];
    let jobs = [draft("a", &["src/a.rs"], &[])];
    let base_workflows = BTreeMap::new();
    let plan = plan_row(outline);
    summary_markdown(
        &Kind::Review {
            title: "T",
            review: "r1",
        },
        &Input {
            plan: &plan,
            workflows: &workflows,
            jobs: &jobs,
            overlaps: &[],
            lint,
            base_workflows: &base_workflows,
        },
    )
}

fn plan_md(workflows: &[WorkflowRow], lint: &[Finding]) -> String {
    let jobs = [
        draft("a", &["src/a.rs"], &[]),
        draft("b", &["src/a.rs"], &[]),
    ];
    let jobs: &[Job] = if workflows.is_empty() { &[] } else { &jobs };
    plan_markdown(
        &Heading::Plan {
            title: "T",
            plan: "pl",
        },
        "ctx",
        "plan",
        workflows,
        jobs,
        &[],
        "S2",
        &Display::of(workflows),
        lint,
    )
}

#[test]
fn finding_row_of_an_overlap_names_both_jobs_the_paths_and_the_limit() {
    let f = overlap("a", "b", &["src/a.rs", "src/b.rs"]);
    assert_eq!(
        f.row(),
        format!("a and b both name src/a.rs, src/b.rs {HINT}")
    );
}

#[test]
fn finding_row_of_an_overlap_shows_three_paths_and_counts_the_rest() {
    let f = overlap("a", "b", &["p1", "p2", "p3", "p4", "p5"]);
    assert_eq!(f.row(), format!("a and b both name p1, p2, p3 +2 {HINT}"));
    let f = overlap("a", "b", &["p1", "p2", "p3"]);
    assert_eq!(f.row(), format!("a and b both name p1, p2, p3 {HINT}"));
}

#[test]
fn summary_lint_block_is_none_without_warnings() {
    let text = summary(None, &[]);
    assert!(text.lines().any(|l| l == "Lint: none"), "{text}");
    assert!(!text.contains("Lint: 0"), "{text}");
}

#[test]
fn summary_lint_block_lists_five_rows_then_counts_the_rest() {
    let findings: Vec<Finding> = ["b", "c", "d", "e", "f", "g", "h"]
        .iter()
        .map(|o| overlap("a", o, &["src/a.rs"]))
        .collect();
    let text = summary(None, &findings);
    let block: Vec<&str> = text
        .split("\n\n")
        .find(|b| b.starts_with("Lint:"))
        .unwrap_or_else(|| panic!("{text}"))
        .lines()
        .collect();
    let mut want = vec!["Lint: 7".to_owned()];
    for f in &findings[..5] {
        want.push(format!("- parallel_write_overlap: {}", f.row()));
    }
    want.push("- +2 more warnings".to_owned());
    assert_eq!(block, want, "{text}");
    assert!(
        block[1].ends_with(HINT) && block[1].contains("a and b both name src/a.rs"),
        "{text}"
    );
}

#[test]
fn summary_lint_block_comes_right_after_overlaps() {
    let text = summary(
        Some(outline(&["t-d1"])),
        &[overlap("a", "b", &["src/a.rs"])],
    );
    let blocks: Vec<&str> = text.trim_end_matches('\n').split("\n\n").collect();
    let at = blocks
        .iter()
        .position(|b| b.starts_with("Lint:"))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(blocks[at - 1].starts_with("Overlaps: none"), "{text}");
    assert!(
        blocks[at].starts_with("Lint: 1\n- parallel_write_overlap: a and b both name src/a.rs"),
        "{text}"
    );
    assert_eq!(blocks[at + 1], "Decided: t-d1", "{text}");
    assert_eq!(*blocks.last().unwrap(), "Approve | Change", "{text}");
}

#[test]
fn plan_md_lint_section_is_a_table_of_the_warnings() {
    let many = ["p1", "p2", "p3", "p4", "p5", "p6", "p7"];
    let findings = [
        finding(Some("a"), Some("b"), &many, "x | y"),
        finding(None, None, &[], "plain"),
    ];
    let text = plan_md(&[workflow("web")], &findings);
    let section = text
        .split("## Lint\n\n")
        .nth(1)
        .unwrap_or_else(|| panic!("{text}"));
    let section = section.split("\n\n").next().unwrap();
    assert_eq!(
        section,
        "| Rule | Workflow | Job | Other | Paths | Message |\n|---|---|---|---|---|---|\n\
| `parallel_write_overlap` | `web` | `a` | `b` | `p1`, `p2`, `p3`, `p4`, `p5`, +2 | x \\| y |\n\
| `parallel_write_overlap` | `web` | — | — |  | plain |",
        "{text}"
    );
}

#[test]
fn plan_md_lint_section_says_none_without_warnings() {
    let text = plan_md(&[workflow("web")], &[]);
    let overlaps = "## Overlaps\n\nNo other active Plan touches these files.\n\n";
    let lint = "## Lint\n\nNo lint warnings.\n\n";
    assert!(text.contains(&format!("{overlaps}{lint}")), "{text}");
    assert!(
        text.find("## Lint").unwrap() < text.find("## Workflow web").unwrap(),
        "{text}"
    );
}

#[test]
fn plan_md_has_no_lint_section_for_a_plan_without_workflows() {
    // Control: a Plan with a Workflow has the section.
    assert!(plan_md(&[workflow("web")], &[]).contains("## Lint\n"));
    let text = plan_md(&[], &[overlap("a", "b", &["src/a.rs"])]);
    assert!(text.contains("No Workflows yet."), "{text}");
    assert!(!text.contains("## Lint"), "{text}");
    assert!(!text.contains("No lint warnings."), "{text}");
}
