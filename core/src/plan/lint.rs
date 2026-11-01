//! Plan lint (contract §2.7): the rule catalogue and the pure run over a Plan's rows.

use std::collections::{BTreeMap, BTreeSet};

use super::jobs::{self, Job};
use super::outline::Outline;
use super::prs::WorkflowRow;
use super::review::{drafts_incomplete, outline_incomplete, outline_missing};
use super::row::Phase;
use super::shape;
use super::shared::is_shared;
use crate::error::{CpdError, ErrorCode};
use crate::ids::PlanId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
        }
    }
}

/// The catalogue; the declaration order is the catalogue order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    DraftIncomplete,
    OutlineMissing,
    OutlineIncomplete,
    TestJobRequired,
    ImplJobRequired,
    ShapeIncomplete,
    WorkflowEmpty,
    DependencyCycle,
    ParallelWriteOverlap,
}

impl Rule {
    pub const ALL: [Rule; 9] = [
        Rule::DraftIncomplete,
        Rule::OutlineMissing,
        Rule::OutlineIncomplete,
        Rule::TestJobRequired,
        Rule::ImplJobRequired,
        Rule::ShapeIncomplete,
        Rule::WorkflowEmpty,
        Rule::DependencyCycle,
        Rule::ParallelWriteOverlap,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Rule::DraftIncomplete => "draft_incomplete",
            Rule::OutlineMissing => "outline_missing",
            Rule::OutlineIncomplete => "outline_incomplete",
            Rule::TestJobRequired => "test_job_required",
            Rule::ImplJobRequired => "impl_job_required",
            Rule::ShapeIncomplete => "shape_incomplete",
            Rule::WorkflowEmpty => "workflow_empty",
            Rule::DependencyCycle => "dependency_cycle",
            Rule::ParallelWriteOverlap => "parallel_write_overlap",
        }
    }

    pub fn severity(self) -> Severity {
        match self {
            Rule::ParallelWriteOverlap => Severity::Warn,
            _ => Severity::Error,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub rule: Rule,
    pub message: String,
    pub details: serde_json::Map<String, serde_json::Value>,
    pub pr: Option<String>,
    pub job: Option<String>,
    pub other: Option<String>,
    pub paths: Vec<String>,
}

impl Finding {
    pub fn severity(&self) -> Severity {
        self.rule.severity()
    }

    /// The summary row of the finding (after `<rule>: `).
    pub fn row(&self) -> String {
        match self.rule {
            Rule::ParallelWriteOverlap => {
                let mut paths = self
                    .paths
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                if self.paths.len() > 3 {
                    paths.push_str(&format!(" +{}", self.paths.len() - 3));
                }
                format!(
                    "{} and {} both name {paths} (a hint, not a conflict prediction)",
                    self.job.as_deref().unwrap_or(""),
                    self.other.as_deref().unwrap_or("")
                )
            }
            _ => self.message.clone(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "rule": self.rule.as_str(),
            "severity": self.severity().as_str(),
            "message": self.message,
            "pr": self.pr,
            "job": self.job,
            "other": self.other,
            "paths": self.paths,
            "details": self.details,
        })
    }
}

pub struct Input<'a> {
    pub plan: &'a PlanId,
    pub phase: Phase,
    pub prs: &'a [WorkflowRow],
    pub jobs: &'a [Job],
    pub outline: Option<&'a Outline>,
}

impl<'a> Input<'a> {
    pub fn new(
        plan: &'a PlanId,
        phase: Phase,
        prs: &'a [WorkflowRow],
        jobs: &'a [Job],
        outline: Option<&'a Outline>,
    ) -> Self {
        Input {
            plan,
            phase,
            prs,
            jobs,
            outline,
        }
    }
}

type RuleFn = fn(&Input) -> Vec<Finding>;

/// The rules, in catalogue order: another rule is one variant, one function and one line here.
const RULES: [RuleFn; 9] = [
    draft_incomplete_rule,
    outline_missing_rule,
    outline_incomplete_rule,
    test_job_required_rule,
    impl_job_required_rule,
    shape_incomplete_rule,
    pr_empty_rule,
    dependency_cycle_rule,
    parallel_write_overlap_rule,
];

impl Rule {
    /// The code of the refusal an error rule carries.
    fn code(self) -> ErrorCode {
        match self {
            Rule::DraftIncomplete => ErrorCode::DraftIncomplete,
            Rule::OutlineMissing => ErrorCode::OutlineMissing,
            Rule::OutlineIncomplete => ErrorCode::OutlineIncomplete,
            Rule::TestJobRequired => ErrorCode::TestJobRequired,
            Rule::ImplJobRequired => ErrorCode::ImplJobRequired,
            Rule::ShapeIncomplete => ErrorCode::ShapeIncomplete,
            Rule::WorkflowEmpty => ErrorCode::WorkflowEmpty,
            Rule::DependencyCycle => ErrorCode::DependencyCycle,
            Rule::ParallelWriteOverlap => unreachable!("a warning carries no refusal"),
        }
    }
}

/// The finding of an error rule: the refusal's message and details.
fn error_finding(rule: Rule, pr: Option<String>, e: CpdError) -> Finding {
    Finding {
        rule,
        message: e.message().to_owned(),
        details: e.details().clone(),
        pr,
        job: None,
        other: None,
        paths: Vec::new(),
    }
}

fn one(rule: Rule, e: Option<CpdError>) -> Vec<Finding> {
    e.map(|e| error_finding(rule, None, e))
        .into_iter()
        .collect()
}

fn draft_incomplete_rule(i: &Input) -> Vec<Finding> {
    one(Rule::DraftIncomplete, drafts_incomplete(i.plan, i.jobs))
}

fn outline_missing_rule(i: &Input) -> Vec<Finding> {
    one(
        Rule::OutlineMissing,
        i.outline.is_none().then(|| outline_missing(i.plan)),
    )
}

fn outline_incomplete_rule(i: &Input) -> Vec<Finding> {
    let e = i
        .outline
        .and_then(|o| outline_incomplete(i.plan, o, i.prs, i.jobs));
    one(Rule::OutlineIncomplete, e)
}

fn test_job_required_rule(i: &Input) -> Vec<Finding> {
    one(
        Rule::TestJobRequired,
        shape::test_job_required(i.plan, i.jobs),
    )
}

fn impl_job_required_rule(i: &Input) -> Vec<Finding> {
    one(
        Rule::ImplJobRequired,
        shape::impl_job_required(i.plan, i.jobs),
    )
}

fn shape_incomplete_rule(i: &Input) -> Vec<Finding> {
    shape::shape_incomplete(i.plan, i.prs, i.jobs)
        .into_iter()
        .map(|e| {
            let pr = e
                .details()
                .get("pr")
                .and_then(|v| v.as_str())
                .map(str::to_owned);
            error_finding(Rule::ShapeIncomplete, pr, e)
        })
        .collect()
}

fn pr_empty_rule(i: &Input) -> Vec<Finding> {
    if i.phase != Phase::Plan {
        return Vec::new();
    }
    one(Rule::WorkflowEmpty, workflow_empty(i.plan, i.prs, i.jobs))
}

fn dependency_cycle_rule(i: &Input) -> Vec<Finding> {
    one(Rule::DependencyCycle, dependency_cycle(i.plan, i.jobs))
}

/// `workflow_empty` (lock's check): a live Workflow has no draft Job (a draft `open` Job counts), or the Plan has no live Workflow.
pub(super) fn workflow_empty(id: &PlanId, workflows: &[WorkflowRow], jobs: &[Job]) -> Option<CpdError> {
    let live: Vec<&WorkflowRow> = workflows.iter().filter(|p| p.status != "cancelled").collect();
    let empty: Vec<&str> = live
        .iter()
        .filter(|p| {
            !jobs
                .iter()
                .any(|j| j.is_draft() && j.workflow_str() == p.key.as_str())
        })
        .map(|p| p.key.as_str())
        .collect();
    if !live.is_empty() && empty.is_empty() {
        return None;
    }
    let message = if live.is_empty() {
        format!("Plan {id} has no Workflow: add one with cpd workflow add.")
    } else {
        format!(
            "Plan {id} has Workflows without a draft Job: {}.",
            empty.join(", ")
        )
    };
    let mut error = CpdError::new(ErrorCode::WorkflowEmpty, message)
        .with("plan", id.as_str())
        .with("prs", empty);
    if live.is_empty() {
        error = error.with(
            "next",
            format!("cpd workflow add <key> --repo <path> --plan {id}"),
        );
    }
    Some(error)
}

/// `dependency_cycle`: the draft Jobs' `needs` contain a cycle.
pub(super) fn dependency_cycle(id: &PlanId, jobs: &[Job]) -> Option<CpdError> {
    let cycle = jobs::cycle(jobs)?;
    Some(
        CpdError::new(
            ErrorCode::DependencyCycle,
            format!(
                "The draft Jobs of {id} depend on each other in a cycle: {}.",
                cycle.join(" → ")
            ),
        )
        .with("plan", id.as_str())
        .with("cycle", cycle),
    )
}

/// The files a Job's Spec names that the overlap rule compares: no directory, no shared file.
fn files(job: &Job) -> BTreeSet<String> {
    job.spec
        .as_ref()
        .and_then(|s| s.touches.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .filter(|p| !p.ends_with('/') && !is_shared(p))
        .map(str::to_owned)
        .collect()
}

/// Every key reachable through `needs` from `start`, over the given Jobs.
fn reach<'a>(start: &'a Job, by_key: &BTreeMap<&'a str, &'a Job>) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut todo: Vec<&str> = start.needs.iter().map(String::as_str).collect();
    while let Some(key) = todo.pop() {
        if seen.insert(key)
            && let Some(j) = by_key.get(key)
        {
            todo.extend(j.needs.iter().map(String::as_str));
        }
    }
    seen
}

fn parallel_write_overlap_rule(i: &Input) -> Vec<Finding> {
    let mut out = Vec::new();
    let prs: BTreeSet<&str> = i.jobs.iter().map(Job::workflow_str).collect();
    for pr in prs {
        let jobs: Vec<&Job> = i.jobs.iter().filter(|j| j.workflow_str() == pr).collect();
        let by_key: BTreeMap<&str, &Job> = jobs.iter().map(|j| (j.key.as_str(), *j)).collect();
        let closure: BTreeMap<&str, BTreeSet<&str>> = jobs
            .iter()
            .map(|j| (j.key.as_str(), reach(j, &by_key)))
            .collect();
        let comparable = |j: &Job| {
            !matches!(j.status.as_str(), "cancelled" | "merged")
                && !matches!(j.kind.as_str(), "open" | "update")
        };
        let mut drafts: Vec<&Job> = jobs
            .iter()
            .copied()
            .filter(|j| j.is_under_review() && comparable(j))
            .collect();
        drafts.sort_by(|a, b| a.key.cmp(&b.key));
        for d in drafts {
            let mine = files(d);
            for o in jobs
                .iter()
                .copied()
                .filter(|o| o.key != d.key && comparable(o))
            {
                if o.is_under_review() && o.key < d.key {
                    continue;
                }
                if closure[d.key.as_str()].contains(o.key.as_str())
                    || closure[o.key.as_str()].contains(d.key.as_str())
                {
                    continue;
                }
                let paths: Vec<String> = mine.intersection(&files(o)).cloned().collect();
                if paths.is_empty() {
                    continue;
                }
                out.push(Finding {
                    rule: Rule::ParallelWriteOverlap,
                    message: format!(
                        "{} and {} have no dependency path and both name {}. A hint, not a conflict prediction: on the ledger of 2026-10-30, 5 of 209 such pairs (2.4%) hit a merge conflict. Settle it with cpd job edit {} --needs {}, or narrow touches.",
                        d.key, o.key, paths.join(", "), d.key, o.key
                    ),
                    details: serde_json::Map::new(),
                    pr: Some(pr.to_owned()),
                    job: Some(d.key.clone()),
                    other: Some(o.key.clone()),
                    paths,
                });
            }
        }
    }
    out
}

/// Every finding, sorted by catalogue position, then by pr, job, other (None first).
pub fn run(input: &Input) -> Vec<Finding> {
    let mut findings: Vec<Finding> = RULES.iter().flat_map(|rule| rule(input)).collect();
    findings
        .sort_by(|a, b| (a.rule, &a.pr, &a.job, &a.other).cmp(&(b.rule, &b.pr, &b.job, &b.other)));
    findings
}

/// The refusal of the first error finding; None when there is none.
pub fn first_error(findings: &[Finding]) -> Option<CpdError> {
    let f = findings.iter().find(|f| f.severity() == Severity::Error)?;
    Some(f.details.iter().fold(
        CpdError::new(f.rule.code(), f.message.clone()),
        |e, (k, v)| e.with(k, v.clone()),
    ))
}
