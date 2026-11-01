//! Plan lint (contract §2.7): the rule catalogue and the pure run over a Plan's rows.

use std::collections::BTreeSet;

use super::jobs::Job;
use super::outline::Outline;
use super::prs::PrRow;
use super::row::Phase;
use crate::error::CpdError;
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
    PrEmpty,
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
        Rule::PrEmpty,
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
            Rule::PrEmpty => "pr_empty",
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

    pub fn to_json(&self) -> serde_json::Value {
        todo!("Finding::to_json")
    }
}

pub struct Input<'a> {
    pub plan: &'a PlanId,
    pub phase: Phase,
    pub prs: &'a [PrRow],
    pub jobs: &'a [Job],
    pub outline: Option<&'a Outline>,
    pub waiting: &'a BTreeSet<String>,
}

impl<'a> Input<'a> {
    pub fn new(
        plan: &'a PlanId,
        phase: Phase,
        prs: &'a [PrRow],
        jobs: &'a [Job],
        outline: Option<&'a Outline>,
        waiting: &'a BTreeSet<String>,
    ) -> Self {
        Input { plan, phase, prs, jobs, outline, waiting }
    }
}

/// Every finding, sorted by catalogue position, then by pr, job, other (None first).
pub fn run(_input: &Input) -> Vec<Finding> {
    Vec::new()
}

/// The refusal of the first error finding; None when there is none.
pub fn first_error(_findings: &[Finding]) -> Option<CpdError> {
    None
}
