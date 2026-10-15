//! The Worker Spec (commands.md §16.1): rendered by the driver at A3 to
//! `run/<plan>/specs/<attempt>.md` from `worker.md`, which is compiled into the
//! binary, so nothing in a repo or worktree can change what Workers are told.
//!
//! The driver gathers the facts (the Job, the earlier Attempts, the answered
//! Escalations, the protected list of [`crate::git::protected_list`]) and calls
//! [`render`] or [`write`]; [`quick_check`] reads the one fact that lives in
//! the worktree.

use std::path::{Path, PathBuf};

use minijinja::syntax::SyntaxConfig;
use minijinja::value::Value;
use minijinja::{Environment, UndefinedBehavior};

use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::git::HookError;
use crate::git::hooks::load;
use crate::home::{CpdHome, Hook};
use crate::ids::{AttemptId, EscalationId, PlanId};

const TEMPLATE: &str = include_str!("worker.md");

/// What no Attempt may touch, whatever the repo's own list says (§7.5).
const ALWAYS_PROTECTED: [&str; 3] = [".config/wt.toml", ".config/cpd.toml", ".cpd/"];

/// How an earlier Attempt ended (§3.4): its cause, which fixes its outcome.
/// Only `merge_conflict` carries what explains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    Merged,
    CancelRequested,
    SetupFailed,
    TimedOut,
    Crashed,
    NoSubmit,
    /// The files the rebase conflicted in.
    MergeConflict { files: Vec<String> },
    CasLost,
    DirtyTarget,
    Tampered,
}

impl Cause {
    /// The cause as the `attempts` table spells it. `conflicts` is read only for
    /// `merge_conflict`. An unknown cause is an error: the table's CHECK should
    /// have kept it out.
    pub fn parse(cause: &str, conflicts: Vec<String>) -> Result<Cause, CpdError> {
        Ok(match cause {
            "merged" => Cause::Merged,
            "cancel_requested" => Cause::CancelRequested,
            "setup_failed" => Cause::SetupFailed,
            "timed_out" => Cause::TimedOut,
            "crashed" => Cause::Crashed,
            "no_submit" => Cause::NoSubmit,
            "merge_conflict" => Cause::MergeConflict { files: conflicts },
            "cas_lost" => Cause::CasLost,
            "dirty_target" => Cause::DirtyTarget,
            "tampered" => Cause::Tampered,
            other => {
                return Err(CpdError::new(ErrorCode::StoreFailed, format!("Unknown Attempt cause {other:?}."))
                    .with("cause", other));
            }
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Cause::Merged => "merged",
            Cause::CancelRequested => "cancel_requested",
            Cause::SetupFailed => "setup_failed",
            Cause::TimedOut => "timed_out",
            Cause::Crashed => "crashed",
            Cause::NoSubmit => "no_submit",
            Cause::MergeConflict { .. } => "merge_conflict",
            Cause::CasLost => "cas_lost",
            Cause::DirtyTarget => "dirty_target",
            Cause::Tampered => "tampered",
        }
    }

    /// `merged`, `cancelled` or `failed` (the `attempts` table's CHECKs).
    pub fn outcome(&self) -> &'static str {
        match self {
            Cause::Merged => "merged",
            Cause::CancelRequested => "cancelled",
            _ => "failed",
        }
    }

    /// One sentence saying what happened; for `merge_conflict` it ends with a
    /// colon when the files follow as a list.
    fn explanation(&self) -> &'static str {
        match self {
            Cause::Merged => "It was merged.",
            Cause::CancelRequested => "The Lead cancelled the Job while the Attempt ran.",
            Cause::SetupFailed => "The worktree or its `pre-start` hook failed, so the Worker never started.",
            Cause::TimedOut => "The Worker ran out of time before it submitted.",
            Cause::Crashed => "The Worker or its driver died before the work was merged.",
            Cause::NoSubmit => "The Worker went idle twice without running `cpd attempt submit`, even after a nudge.",
            Cause::MergeConflict { files } if files.is_empty() => {
                "Rebasing it onto the newer Lead Branch conflicted."
            }
            Cause::MergeConflict { .. } => "Rebasing it onto the newer Lead Branch conflicted in these files:",
            Cause::CasLost => "The Lead Branch kept moving, so the merge could not be completed.",
            Cause::DirtyTarget => "The Lead worktree was not clean when the merge ran.",
            Cause::Tampered => "Its diff touched a protected path.",
        }
    }
}

/// An earlier, ended Attempt of the same Job.
#[derive(Clone, Debug)]
pub struct Previous {
    pub number: u32,
    pub tier: Tier,
    pub cause: Cause,
}

/// An `answered` Escalation of an earlier Attempt.
#[derive(Clone, Debug)]
pub struct Answer {
    pub id: EscalationId,
    pub question: String,
    pub answer: String,
}

/// Everything the Spec says. The lists of the Job's Spec are as `job add` stored them.
#[derive(Clone, Debug)]
pub struct Input<'a> {
    pub plan: &'a PlanId,
    pub attempt: &'a AttemptId,
    pub title: &'a str,
    pub tier: Tier,
    pub worktree: &'a Path,
    pub goal: &'a str,
    pub context: &'a str,
    pub touches: &'a [String],
    pub acceptance: &'a [String],
    pub out_of_scope: &'a [String],
    /// The commands of the worktree's `pre-commit`, as [`quick_check`] reads them;
    /// empty when the repo defines none.
    pub quick_check: &'a [String],
    /// The repo's own protected list (§7.5); the always-protected paths are added here.
    pub protected: &'a [String],
    /// Earlier ended Attempts of this Job, oldest first.
    pub previous: &'a [Previous],
    /// Answered Escalations of earlier Attempts, oldest first.
    pub answers: &'a [Answer],
}

/// The commands of `pre-commit` in `<worktree>/.config/wt.toml`, verbatim and in
/// file order; empty when the file or the table is missing (R20: no fallback to
/// `pre-merge`).
pub fn quick_check(worktree: &Path) -> Result<Vec<String>, CpdError> {
    let pipeline = load(worktree, Hook::PreCommit).map_err(|e| {
        let code = match e {
            HookError::Invalid(_) => ErrorCode::HookInvalid,
            _ => ErrorCode::StoreFailed,
        };
        CpdError::new(code, format!("Cannot read the quick check of {}: {e}", worktree.display()))
            .with("worktree", worktree.display().to_string())
    })?;
    Ok(pipeline.commands().map(|c| c.template.clone()).collect())
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
            let files: &[String] = match &p.cause {
                Cause::MergeConflict { files } => files,
                _ => &[],
            };
            Value::from_pairs([
                ("number", Value::from(p.number)),
                ("tier", Value::from(p.tier.to_string())),
                ("outcome", Value::from(p.cause.outcome())),
                ("cause", Value::from(p.cause.as_str())),
                ("explanation", Value::from(p.cause.explanation())),
                ("files", Value::from(files.to_vec())),
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

    let vars = Value::from_pairs([
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

/// Renders the Spec to `run/<plan>/specs/<attempt>.md` (whole, through a
/// temporary file, so a reader never sees half of it; rerunning rewrites it) and
/// returns the path, which the driver records as `spec_path`.
pub fn write(home: &CpdHome, input: &Input<'_>) -> Result<PathBuf, CpdError> {
    let text = render(input)?;
    let path = home.worker_spec(input.plan, input.attempt);
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot write {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    };
    let dir = path.parent().expect("a Spec lives in a directory");
    std::fs::create_dir_all(dir).map_err(failed)?;
    let tmp = dir.join(format!(".{}.{}.tmp", input.attempt, std::process::id()));
    std::fs::write(&tmp, text).map_err(failed)?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        failed(e)
    })?;
    Ok(path)
}
