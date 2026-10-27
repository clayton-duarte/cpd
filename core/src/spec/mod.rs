//! The Worker Spec (commands.md §16.1): rendered by the driver at A3 to
//! `run/<plan>/specs/<attempt>.md` from `worker.md`, which is compiled into the
//! binary, so nothing in a repo or worktree can change what Workers are told.
//!
//! The driver gathers the facts (the Job, the earlier Attempts, the answered
//! Escalations, the PR policy's `protected.paths`) and calls
//! [`render`] or [`write`]; [`quick_check`] lists the commands of the PR's policy `hooks.pre-commit`.

use std::path::{Path, PathBuf};

use minijinja::syntax::SyntaxConfig;
use minijinja::value::Value;
use minijinja::{Environment, UndefinedBehavior};

use crate::attempt::ledger::{Cause, Stored};
use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::git::Pipeline;
use crate::home::{CpdHome, Hook};
use crate::ids::{AttemptId, EscalationId, PlanId};

#[cfg(test)]
mod resources_tests;

const TEMPLATE: &str = include_str!("worker.md");

/// What no Attempt may touch, whatever the PR's `protected.paths` say (§7.5).
const ALWAYS_PROTECTED: [&str; 1] = [".cpd/"];

/// The cause an `attempts` row holds, read for a Spec. A value outside the table's CHECK is a corrupt row: `store_failed`.
pub fn parse_cause(cause: &str) -> Result<Stored, CpdError> {
    Stored::parse(cause).ok_or_else(|| {
        CpdError::new(ErrorCode::StoreFailed, format!("Unknown Attempt cause {cause:?}.")).with("cause", cause)
    })
}

/// What explains how an earlier Attempt ended, read from its end Event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    None,
    /// `merge_conflict`: the files the rebase conflicted in.
    Files(Vec<String>),
    /// `gate_failed` or `gate_error`: the failed Gate's hook and log, when the detail names them.
    Gate(Option<(Hook, PathBuf)>),
}

/// One sentence saying what happened; it ends with a colon when a list item follows (the files or the Gate's log).
/// Exhaustive over the Ledger's causes: a cause without a sentence does not compile.
fn explanation(cause: Stored, evidence: &Evidence) -> String {
    let gate = match evidence {
        Evidence::Gate(gate) => gate.as_ref(),
        _ => None,
    };
    let files = match evidence {
        Evidence::Files(files) => files.as_slice(),
        _ => &[],
    };
    match cause {
        Stored::Merged => "It was merged.".to_owned(),
        Stored::CancelRequested => "The Lead cancelled the Job while the Attempt ran.".to_owned(),
        Stored::Failed(cause) => match cause {
            Cause::SetupFailed => "The worktree or its `pre-start` hook failed, so the Worker never started.".to_owned(),
            Cause::TimedOut => "The Worker ran out of time before it submitted.".to_owned(),
            Cause::Crashed => "The Worker or its driver died before the work was merged.".to_owned(),
            Cause::NoSubmit => "The Worker went idle twice without running `cpd attempt submit`, even after a nudge.".to_owned(),
            Cause::MergeConflict if files.is_empty() => "Rebasing it onto the newer Lead Branch conflicted.".to_owned(),
            Cause::MergeConflict => "Rebasing it onto the newer Lead Branch conflicted in these files:".to_owned(),
            Cause::CasLost => "The Lead Branch kept moving, so the merge could not be completed.".to_owned(),
            Cause::DirtyTarget => "The Lead worktree was not clean when the merge ran.".to_owned(),
            Cause::Tampered => "Its diff touched a protected path.".to_owned(),
            Cause::GateFailed => match gate {
                Some((hook, _)) => format!(
                    "The Gate (`{}`) failed at its merge. Its whole output is in this log; read it and fix what it reports:",
                    hook.as_str()
                ),
                None => "The Gate failed at its merge.".to_owned(),
            },
            Cause::GateError => {
                const WHY: &str = "it was killed, timed out or could not run, which is a problem of the Gate, not a failing check of your work.";
                match gate {
                    Some((hook, _)) => format!(
                        "The Gate (`{}`) could not be judged at its merge: {WHY} Its output so far is in this log, if the hook started:",
                        hook.as_str()
                    ),
                    None => format!("The Gate could not be judged at its merge: {WHY}"),
                }
            }
        },
    }
}

/// An earlier, ended Attempt of the same Job.
#[derive(Clone, Debug)]
pub struct Previous {
    pub number: u32,
    pub tier: Tier,
    pub cause: Stored,
    pub evidence: Evidence,
}

/// An `answered` Escalation of an earlier Attempt.
#[derive(Clone, Debug)]
pub struct Answer {
    pub id: EscalationId,
    pub question: String,
    pub answer: String,
}

/// What a Job's kind adds to its Spec (§16.1, contract §2.2 "Job kinds"): a section for each kind that has checks. `work` and
/// `docs` have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind<'a> {
    Work,
    Docs,
    /// A `test` Job's own named tests and the files that hold them (its Spec's `tests` and `test_files`).
    Test { tests: &'a [String], test_files: &'a [String] },
    /// An `impl` Job's derived tests, the guarded files and the paths the Lead approved, and the keys of the `test` Jobs it needs.
    Impl { test_jobs: &'a [String], tests: &'a [String], guarded: &'a [String], approved: &'a [String] },
    Verify,
    Open,
    Update,
}

impl JobKind<'_> {
    fn as_str(&self) -> &'static str {
        match self {
            JobKind::Work => "work",
            JobKind::Docs => "docs",
            JobKind::Test { .. } => "test",
            JobKind::Impl { .. } => "impl",
            JobKind::Verify => "verify",
            JobKind::Open => "open",
            JobKind::Update => "update",
        }
    }
}

/// `a`, `b`: Job keys as inline code in a sentence.
fn code_list(items: &[String]) -> String {
    items.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ")
}

/// Everything the Spec says. The lists of the Job's Spec are as `job add` stored them.
#[derive(Clone, Debug)]
pub struct Input<'a> {
    pub plan: &'a PlanId,
    pub attempt: &'a AttemptId,
    pub title: &'a str,
    pub tier: Tier,
    pub worktree: &'a Path,
    /// The Job's kind and what it adds to the Spec.
    pub kind: JobKind<'a>,
    pub goal: &'a str,
    pub context: &'a str,
    pub touches: &'a [String],
    pub acceptance: &'a [String],
    pub out_of_scope: &'a [String],
    /// The names of the exclusive resources the Attempt holds (its Job's Spec `resources`); empty when none.
    pub resources: &'a [String],
    /// The commands of the PR's `hooks.pre-commit`, as [`quick_check`] lists them;
    /// empty when the repo defines none.
    pub quick_check: &'a [String],
    /// The PR policy's `protected.paths` (§7.5); the always-protected paths are added here.
    pub protected: &'a [String],
    /// Earlier ended Attempts of this Job, oldest first.
    pub previous: &'a [Previous],
    /// Answered Escalations of earlier Attempts, oldest first.
    pub answers: &'a [Answer],
}

/// The commands of `pipeline` (the policy's `hooks.pre-commit`), verbatim and in file order; empty when it is not
/// set (R20: no fallback to `pre-merge`).
pub fn quick_check(pipeline: &Pipeline) -> Vec<String> {
    pipeline.commands().map(|c| c.template.clone()).collect()
}

/// Six spaces of indent: a code block under a list item.
fn indented(command: &str) -> String {
    command.lines().map(|l| if l.is_empty() { String::new() } else { format!("      {l}") }).collect::<Vec<_>>().join("\n")
}

/// The Spec's text.
pub fn render(input: &Input<'_>) -> Result<String, CpdError> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_syntax(
        SyntaxConfig::builder()
            .trim_blocks(true)
            .lstrip_blocks(true)
            .keep_trailing_newline(true)
            .build()
            .expect("the syntax is valid"),
    );

    let mut protected: Vec<&str> = ALWAYS_PROTECTED.to_vec();
    for path in input.protected {
        if !protected.contains(&path.as_str()) {
            protected.push(path);
        }
    }
    let quick_check: Vec<String> = input.quick_check.iter().map(|c| indented(c)).collect();
    let previous: Vec<Value> = input
        .previous
        .iter()
        .map(|p| {
            let items: Vec<String> = match &p.evidence {
                Evidence::None | Evidence::Gate(None) => Vec::new(),
                Evidence::Files(files) => files.clone(),
                Evidence::Gate(Some((_, log))) => vec![log.display().to_string()],
            };
            Value::from_pairs([
                ("number", Value::from(p.number)),
                ("tier", Value::from(p.tier.to_string())),
                ("outcome", Value::from(p.cause.outcome())),
                ("cause", Value::from(p.cause.as_str())),
                ("explanation", Value::from(explanation(p.cause, &p.evidence))),
                ("files", Value::from(items)),
            ])
        })
        .collect();
    let answers: Vec<Value> = input
        .answers
        .iter()
        .map(|a| {
            Value::from_pairs([
                ("id", Value::from(a.id.to_string())),
                ("question", Value::from(a.question.as_str())),
                ("answer", Value::from(a.answer.as_str())),
            ])
        })
        .collect();

    let (tests, test_files, test_jobs, approved): (&[String], &[String], String, &[String]) = match input.kind {
        JobKind::Test { tests, test_files } => (tests, test_files, String::new(), &[]),
        JobKind::Impl { test_jobs, tests, guarded, approved } => (tests, guarded, code_list(test_jobs), approved),
        _ => (&[], &[], String::new(), &[]),
    };
    let vars = Value::from_pairs([
        ("kind", Value::from(input.kind.as_str())),
        ("tests", Value::from(tests.to_vec())),
        ("test_files", Value::from(test_files.to_vec())),
        ("test_jobs", Value::from(test_jobs)),
        ("approved", Value::from(approved.to_vec())),
        ("job", Value::from(input.attempt.job().to_string())),
        ("number", Value::from(input.attempt.number().get())),
        ("attempt", Value::from(input.attempt.to_string())),
        ("title", Value::from(input.title)),
        ("plan", Value::from(input.plan.to_string())),
        ("tier", Value::from(input.tier.to_string())),
        ("worktree", Value::from(input.worktree.display().to_string())),
        ("goal", Value::from(input.goal)),
        ("context", Value::from(input.context)),
        ("touches", Value::from(input.touches.to_vec())),
        ("acceptance", Value::from(input.acceptance.to_vec())),
        ("out_of_scope", Value::from(input.out_of_scope.to_vec())),
        ("resources", Value::from(input.resources.to_vec())),
        ("quick_check", Value::from(quick_check)),
        ("protected", Value::from(protected.iter().map(|p| p.to_string()).collect::<Vec<_>>())),
        ("previous", Value::from(previous)),
        ("answers", Value::from(answers)),
    ]);
    env.render_named_str("worker.md", TEMPLATE, vars).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot render the Worker Spec: {e}."))
            .with("attempt", input.attempt.to_string())
    })
}

/// Renders the Spec to `run/<plan>/specs/<attempt>.md` (whole, through
/// [`crate::atomic::write_atomic`], so a reader never sees half of it; rerunning rewrites it) and
/// returns the path, which the driver records as `spec_path`.
pub fn write(home: &CpdHome, input: &Input<'_>) -> Result<PathBuf, CpdError> {
    let text = render(input)?;
    let path = home.worker_spec(input.plan, input.attempt);
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot write {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    };
    crate::atomic::write_atomic(&path, text).map_err(failed)?;
    Ok(path)
}
