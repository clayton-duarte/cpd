//! The Plan summary (commands.md §4.5 "The Plan summary", hs-d502, D4): a short text generated from the Plan and its
//! outline, so the User approves a Plan from a screenful in the Lead's chat. `plan render` writes it to `summary.md`,
//! `plan review` to `summary-<review>.md`. Derived: it holds nothing the Plan lacks. Pure: the same inputs give the
//! same bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::jobs::Job;
use super::overlap::Overlap;
use super::prs::WorkflowRow;
use super::render::{cell, inline};
use super::row::PlanRow;

/// What stands for a missing goal, Workflow title or need.
const NONE: &str = "—";
/// The overlap rows shown before the rest are counted.
const OVERLAPS_SHOWN: usize = 5;

/// Which summary: the Plan's own (`plan render`) or a review's.
pub enum Kind<'a> {
    /// `full_plan` is the absolute path of `plan.md`, which the last line names.
    Plan { full_plan: &'a Path },
    Review { title: &'a str, review: &'a str },
}

/// What the summary reads besides the Plan row.
pub struct Input<'a> {
    pub plan: &'a PlanRow,
    pub workflows: &'a [WorkflowRow],
    pub jobs: &'a [Job],
    pub overlaps: &'a [Overlap],
    /// The base Workflow (`<plan>/<key>`) of each stacked Workflow, by Workflow key (contract §2.2 "The base Workflow").
    pub base_workflows: &'a BTreeMap<String, String>,
}

/// A Job under review: a `draft` with its Tier and its Spec.
fn under_review(job: &Job) -> bool {
    job.is_draft() && job.missing().is_empty()
}

/// A locked Job: neither `draft` nor `cancelled`.
fn locked(job: &Job) -> bool {
    !job.is_draft() && job.status != "cancelled"
}

fn repo_name(workflow: &WorkflowRow) -> String {
    workflow.name.clone().or_else(|| Path::new(&workflow.repo).file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| workflow.repo.clone())
}

/// `Order:` and `Stacked on:` (block 7): empty when no Workflow counts.
fn order(i: &Input, counting: &[&WorkflowRow]) -> Vec<String> {
    if counting.is_empty() {
        return Vec::new();
    }
    let keys: BTreeSet<&str> = counting.iter().map(|p| p.key.as_str()).collect();
    let counts = |need: &str| keys.contains(need);
    let mut items: Vec<String> = Vec::new();
    let mut tied: BTreeSet<&str> = keys.clone();
    for workflow in counting {
        for need in workflow.needs.iter().filter(|n| counts(n)) {
            items.push(format!("{need} → {}", workflow.key));
            tied.remove(workflow.key.as_str());
            tied.remove(need.as_str());
        }
    }
    if !tied.is_empty() {
        items.push(tied.into_iter().collect::<Vec<_>>().join(" · "));
    }
    let stacked: Vec<String> = counting.iter().filter_map(|p| i.base_workflows.get(p.key.as_str()).map(|base| format!("{} on {base}", p.key))).collect();
    vec![
        format!("Order: {}", items.join(", ")),
        format!("Stacked on: {}", if stacked.is_empty() { "none".to_owned() } else { stacked.join(", ") }),
    ]
}

fn overlaps_block(overlaps: &[Overlap]) -> Vec<String> {
    if overlaps.is_empty() {
        return vec!["Overlaps: none".to_owned()];
    }
    let mut out = vec![format!("Overlaps: {}", overlaps.len())];
    for o in overlaps.iter().take(OVERLAPS_SHOWN) {
        let (path, other) = (o.paths.first().map_or("", String::as_str), &o.other_jobs[0]);
        let more = o.paths.len() - 1 + o.other_jobs.len() - 1;
        let more = if more > 0 { format!(" +{more}") } else { String::new() };
        out.push(format!("- {path}: {}/{}/{} ({}){more}", o.plan, o.workflow, other.key, other.status));
    }
    if overlaps.len() > OVERLAPS_SHOWN {
        out.push(format!("- +{} more overlaps", overlaps.len() - OVERLAPS_SHOWN));
    }
    out
}

/// A block of `head` and its rows; no block when there are no rows.
fn list(head: &str, rows: Vec<String>) -> Vec<String> {
    if rows.is_empty() { Vec::new() } else { [vec![head.to_owned()], rows].concat() }
}

/// The summary of `i`, as `kind` says.
pub fn markdown(kind: &Kind, i: &Input) -> String {
    let plan = i.plan;
    let outline = plan.outline.as_ref();
    let mut counting: Vec<&WorkflowRow> = i.workflows.iter().filter(|p| p.status != "cancelled").collect();
    counting.sort_by(|a, b| a.key.cmp(&b.key));
    let mut reviewed: Vec<&Job> = i.jobs.iter().filter(|j| under_review(j)).collect();
    reviewed.sort_by(|a, b| (&a.workflow, &a.key).cmp(&(&b.workflow, &b.key)));
    let has_review = |workflow: &WorkflowRow| reviewed.iter().any(|j| j.workflow_str() == workflow.key.as_str());

    let mut blocks: Vec<Vec<String>> = Vec::new();
    blocks.push(match kind {
        Kind::Plan { .. } => vec![format!("# Plan: {}", inline(&plan.title)), format!("Plan {} · phase {}", plan.id, plan.phase.as_str())],
        Kind::Review { title, review } => {
            vec![format!("# {}", inline(title)), format!("Plan {}: {} · review {review}", plan.id, inline(&plan.title))]
        }
    });
    blocks.push(vec![format!("Goal: {}", outline.map_or(NONE.to_owned(), |o| inline(&o.goal)))]);

    let mut workflows = vec!["## Workflows".to_owned()];
    for workflow in counting.iter().filter(|p| has_review(p)) {
        let title = workflow.title.as_deref().map_or(NONE.to_owned(), inline);
        workflows.push(format!("- {} ({}, base {}): {title}", workflow.key, inline(&repo_name(workflow)), inline(&workflow.base)));
        for done in outline.and_then(|o| o.done_when.get(workflow.key.as_str())).into_iter().flatten() {
            workflows.push(format!("  Done when: {}", inline(done)));
        }
    }
    blocks.push(if workflows.len() > 1 { workflows } else { Vec::new() });

    let decisions = outline.iter().flat_map(|o| &o.decisions).enumerate();
    blocks.push(list("## Decisions", decisions.map(|(n, d)| format!("D{} {} → {}", n + 1, inline(&d.question), inline(&d.answer))).collect()));
    let assumptions = outline.iter().flat_map(|o| &o.assumptions).enumerate();
    blocks.push(list("## Assumptions", assumptions.map(|(n, a)| format!("A{} {}", n + 1, inline(a))).collect()));

    let locked_jobs = i.jobs.iter().filter(|j| locked(j)).count();
    let mut jobs = vec![
        format!("## Jobs ({} under review, {locked_jobs} locked)", reviewed.len()),
        "| Job | Title | Tier | Needs |".to_owned(),
        "|---|---|---|---|".to_owned(),
    ];
    for job in &reviewed {
        let needs = if job.needs.is_empty() { NONE.to_owned() } else { job.needs.join(", ") };
        jobs.push(format!("| {} | {} | {} | {} |", cell(&job.key), cell(&job.title), job.tier.as_deref().unwrap_or(NONE), cell(&needs)));
    }
    blocks.push(jobs);

    blocks.push(order(i, &counting));
    blocks.push(overlaps_block(i.overlaps));
    if let Some(o) = outline.filter(|o| !o.decided.is_empty()) {
        blocks.push(vec![format!("Decided: {}", o.decided.join(", "))]);
    }

    let mut stubs: Vec<&Job> = i.jobs.iter().filter(|j| j.is_draft() && !j.missing().is_empty()).collect();
    stubs.sort_by(|a, b| a.key.cmp(&b.key));
    let later: Vec<String> =
        stubs.iter().map(|j| j.key.clone()).chain(counting.iter().filter(|p| !has_review(p)).map(|p| format!("Workflow {}", p.key))).collect();
    if !later.is_empty() {
        blocks.push(vec![format!("Later: {}", later.join(", "))]);
    }

    let risks = outline.iter().flat_map(|o| &o.risks).enumerate();
    blocks.push(list("## Risks", risks.map(|(n, r)| format!("R{} {}", n + 1, inline(r))).collect()));
    blocks.push(list("## Out of scope", outline.iter().flat_map(|o| &o.out_of_scope).map(|s| format!("- {}", inline(s))).collect()));

    blocks.push(vec![match kind {
        Kind::Plan { full_plan } => format!("Full plan ({})", full_plan.display()),
        Kind::Review { .. } => "Approve | Change".to_owned(),
    }]);

    let blocks: Vec<String> = blocks.into_iter().filter(|b| !b.is_empty()).map(|b| b.join("\n")).collect();
    format!("{}\n", blocks.join("\n\n"))
}
