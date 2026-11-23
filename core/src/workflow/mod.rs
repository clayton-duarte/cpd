//! The Plan's Workflows (contract §3.8, §7.6; commands.md §9.4–§9.7): the object before and after it starts.
//!
//! - `add`, `edit`, `describe`, `cancel`, `start`: `workflow add|edit|describe|cancel|start`;
//! - `row`: the Plan as the Workflow commands read it.
//!
//! The pull request side (`open`, `poll`, `view`, `reply`, `rerun`, `rebase`, `push`, `merge`, `gh`, `status`) stays in [`crate::pr`].

use std::path::PathBuf;

use crate::args::{PlanArg, WorkflowTitle};
use crate::error::CpdError;
use crate::ids::WorkflowKey;
use crate::output::Output;

pub(crate) mod add;
pub(crate) mod cancel;
pub(crate) mod describe;
pub(crate) mod edit;
pub mod jira_title;
pub mod prereq;
pub(crate) mod reopen;
pub(crate) mod row;
pub mod start;

#[cfg(test)]
mod jira_title_tests;
#[cfg(test)]
pub(crate) mod reopen_tests;

/// The `workflow` group (commands.md §9): the Plan's Workflows, before and after they start.
#[derive(clap::Subcommand, Debug)]
pub enum WorkflowCommand {
    /// Add a Workflow to the Plan (§9.4)
    Add(AddArgs),
    /// Change a waiting Workflow before the lock (§9.5)
    Edit(EditArgs),
    /// Store the Workflow's description, which becomes the pull request body (§9.5a)
    Describe(DescribeArgs),
    /// Remove a Workflow from the Plan before the lock; after it, end a Workflow with nothing to open, or whose work another Workflow carried (`--superseded-by`) (§9.6)
    Cancel(CancelArgs),
    /// Start a waiting Workflow whose needs are merged, after a failed start (§9.7)
    Start(StartArgs),
    /// External prerequisites of a Workflow (§9.8)
    #[command(subcommand)]
    Prereq(prereq::PrereqCommand),
}

#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// The Workflow key, chosen by the Lead
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// The Workflow's repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: PathBuf,
    /// The Workflow's own title: names its change; 1-72 characters on one line
    #[arg(long, value_name = "text")]
    title: WorkflowTitle,
    /// The Workflow's own Jira ticket key, e.g. ABC-123 (optional)
    #[arg(long, value_name = "KEY")]
    jira: Option<crate::context::Jira>,
    /// The branch the pull request targets [default: origin's HEAD branch, else main], or <key> (or <plan>/<key>) to stack on that Workflow
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// Workflows that must be merged before this one starts, comma-separated: a key of the Plan, or <plan>/<key> of another Plan of the Context
    #[arg(
        long,
        value_name = "r1,r2",
        value_delimiter = ',',
        conflicts_with = "no_needs"
    )]
    needs: Vec<add::WorkflowRef>,
    /// No needs, even on a reopened Plan (default there: every merged Workflow of the Plan)
    #[arg(long = "no-needs")]
    no_needs: bool,
    /// The Workflow's starting Job template: use free for one cohesive work Job or tdd for separate red-test and implementation Jobs (§9.4)
    #[arg(long, value_name = "shape", value_parser = clap::builder::PossibleValuesParser::new(add::SHAPES), default_value = "tdd")]
    shape: String,
    /// After the lock: the template's Jobs as decided work, a JSON object keyed by the template suffix, each `{"tier", "spec"}` (§9.4)
    #[arg(long, value_name = "file.json")]
    jobs: Option<PathBuf>,
    /// Adopt the existing pull request <number> of the repo: its head branch is pushed to, its base is the pull request's (§9.4)
    #[arg(long, value_name = "number", conflicts_with = "base", value_parser = clap::value_parser!(i64).range(1..))]
    adopt: Option<i64>,
    /// With --adopt: the login of the pull request's author when the User is not the author (confirmed by the User first)
    #[arg(long = "confirm-author", value_name = "login", requires = "adopt")]
    confirm_author: Option<String>,
    /// The bug this Workflow fixes, `b<k>`: it must have an issue; the pull request body ends with `Fixes #<N>`
    #[arg(long, value_name = "id")]
    bug: Option<String>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct EditArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// The new repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: Option<PathBuf>,
    /// The new base branch, or <key> (or <plan>/<key>) to stack on that Workflow
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// The new needs, replacing the list, comma-separated: keys of the Plan or <plan>/<key>
    #[arg(
        long,
        value_name = "r1,r2",
        value_delimiter = ',',
        conflicts_with = "no_needs"
    )]
    needs: Option<Vec<add::WorkflowRef>>,
    /// Remove every need
    #[arg(long = "no-needs")]
    no_needs: bool,
    /// The new title, in any phase while the Workflow is not merged or cancelled; 1-72 characters on one line
    #[arg(long, value_name = "text")]
    title: Option<WorkflowTitle>,
    /// The Workflow's Jira ticket key, e.g. ABC-123: sets or replaces it, in any phase while the Workflow is not merged or cancelled
    #[arg(long, value_name = "KEY")]
    jira: Option<crate::context::Jira>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct DescribeArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// The description: a path to a UTF-8 file, or - for stdin
    #[arg(long, value_name = "path")]
    file: PathBuf,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct CancelArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    /// Cancel a dispatch Workflow whose merged work another Workflow carried: that Workflow, as <plan>/<key> (a bare <key> is of the Plan)
    #[arg(long = "superseded-by", value_name = "ref")]
    superseded_by: Option<add::WorkflowRef>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// The Workflow key
    #[arg(value_name = "key")]
    key: WorkflowKey,
    #[command(flatten)]
    plan: PlanArg,
}

/// Runs `command` of the `workflow` group (commands.md §9.4–§9.7).
pub fn run_workflow(command: WorkflowCommand) -> Result<Output, CpdError> {
    match command {
        WorkflowCommand::Add(args) => add::add(args),
        WorkflowCommand::Edit(args) => edit::edit(args),
        WorkflowCommand::Describe(args) => describe::describe(args),
        WorkflowCommand::Cancel(args) => cancel::cancel(args),
        WorkflowCommand::Start(args) => start::run(args),
        WorkflowCommand::Prereq(cmd) => prereq::run(cmd),
    }
}

/// The named tests an `impl` Job of a refactor Workflow runs when its Workflow has no `test` Job: those of the `test`
/// Jobs of the coverage Workflow it stands on (the one whose Lead Branch is its base, or a merged one of the repo).
/// `None` when the Job's Workflow is not a refactor; `Some` (possibly empty: the repo's whole suite) when it is.
pub(crate) fn base_coverage_tests(
    conn: &rusqlite::Connection,
    plan: &crate::ids::PlanId,
    workflow: &str,
) -> Result<Option<Vec<String>>, CpdError> {
    use rusqlite::OptionalExtension;
    let Some((repo, base)): Option<(String, String)> = conn
        .query_row(
            "SELECT repo, base FROM workflows WHERE plan = ?1 AND key = ?2 AND shape = 'refactor'",
            [plan.as_str(), workflow],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut keys = conn.prepare(
        "SELECT key FROM workflows WHERE plan = ?1 AND shape = 'coverage' AND repo = ?2 AND (lead_branch = ?3 OR status = 'merged')",
    )?;
    let coverage: Vec<String> = keys
        .query_map(rusqlite::params![plan.as_str(), repo, base], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut tests: Vec<String> = Vec::new();
    for job in crate::plan::jobs::load(conn, plan)? {
        if job.kind != "test"
            || job.status == "cancelled"
            || !job.workflow.as_ref().is_some_and(|w| coverage.contains(w))
        {
            continue;
        }
        for name in crate::job::guard::own(&job).0 {
            if !tests.contains(&name) {
                tests.push(name);
            }
        }
    }
    Ok(Some(tests))
}

/// The `paths` that do not exist in the tree of `head`: deleted, or renamed away.
pub(crate) fn absent_paths(
    root: &std::path::Path,
    head: &str,
    paths: &[String],
) -> Result<Vec<String>, CpdError> {
    let out = crate::git::run::run(root, &["ls-tree", "-r", "--name-only", "-z", head])?;
    if out.code != 0 {
        return Err(CpdError::new(
            crate::error::ErrorCode::GitFailed,
            format!("Could not list the files of {head}."),
        ));
    }
    let present: std::collections::HashSet<&str> = out.stdout.split('\0').collect();
    Ok(paths
        .iter()
        .filter(|p| !present.contains(p.as_str()))
        .cloned()
        .collect())
}

/// The `(old, new)` path pairs `base...head` renames (`git diff -M --diff-filter=R --name-status -z`).
pub(crate) fn renamed_paths(
    root: &std::path::Path,
    base: &str,
    head: &str,
) -> Result<Vec<(String, String)>, CpdError> {
    let range = format!("{base}...{head}");
    let out = crate::git::run::run(
        root,
        &[
            "diff",
            "-M",
            "--diff-filter=R",
            "--name-status",
            "-z",
            &range,
        ],
    )?;
    if out.code != 0 {
        return Err(CpdError::new(
            crate::error::ErrorCode::GitFailed,
            format!("Could not list the renames of {range}: {}", out.stderr),
        ));
    }
    // Entries are `R<score>\0old\0new\0`.
    let mut fields = out.stdout.split('\0').filter(|f| !f.is_empty());
    let mut pairs = Vec::new();
    while let Some(status) = fields.next() {
        if let (true, Some(old), Some(new)) =
            (status.starts_with('R'), fields.next(), fields.next())
        {
            pairs.push((old.to_owned(), new.to_owned()));
        }
    }
    Ok(pairs)
}

/// Enforce test immutability on refactor diffs, including declared test-file globs.
pub(crate) fn guard_refactor_tests(
    conn: &rusqlite::Connection,
    plan: &crate::ids::PlanId,
    workflow: &str,
    root: &std::path::Path,
    base: &str,
    head: &str,
) -> Result<(), CpdError> {
    guard_refactor_tests_allowing(conn, plan, workflow, root, base, head, &[])
}

/// [`guard_refactor_tests`], except the test changes `head` inherits unchanged from a ref of `allowed` (the Workflow's base,
/// whose amended coverage tests a base merge or a rebase brings in): those are not the refactor's own.
pub(crate) fn guard_refactor_tests_allowing(
    conn: &rusqlite::Connection,
    plan: &crate::ids::PlanId,
    workflow: &str,
    root: &std::path::Path,
    base: &str,
    head: &str,
    allowed: &[&str],
) -> Result<(), CpdError> {
    let shape: String = conn.query_row(
        "SELECT shape FROM workflows WHERE plan = ?1 AND key = ?2",
        [plan.as_str(), workflow],
        |row| row.get(0),
    )?;
    if shape != "refactor" {
        return Ok(());
    }
    let repo: String = conn.query_row(
        "SELECT repo FROM workflows WHERE plan = ?1 AND key = ?2",
        [plan.as_str(), workflow],
        |row| row.get(0),
    )?;
    // Declared test paths bind this repository only: another repo's Workflow may name the same glob for its own files.
    let mut in_repo = conn.prepare("SELECT key FROM workflows WHERE plan = ?1 AND repo = ?2")?;
    let in_repo: Vec<String> = in_repo
        .query_map(rusqlite::params![plan.as_str(), repo], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let jobs = crate::plan::jobs::load(conn, plan)?;
    let patterns: Vec<&str> = jobs
        .iter()
        .filter(|j| j.workflow.as_ref().is_some_and(|w| in_repo.contains(w)))
        .filter_map(|j| j.test_files.as_ref())
        .filter_map(|v| v.as_array())
        .flatten()
        .filter_map(|v| v.as_str())
        .collect();
    let paths: Vec<_> = crate::git::tamper::written_paths(root, base, head, allowed)?
        .into_iter()
        .filter(|p| {
            p.ends_with("_test.rs")
                || p.ends_with(".test.ts")
                || p.split('/').any(|part| part == "tests")
                || patterns
                    .iter()
                    .any(|pattern| crate::git::tamper::touches_matches(pattern, p))
        })
        .collect();
    if !paths.is_empty() {
        return Err(crate::error::CpdError::new(
            crate::error::ErrorCode::TestChangeForbidden,
            format!("Refactor workflows cannot change test files: {}. Amend the coverage workflow instead.", paths.join(", ")),
        ).with("paths", serde_json::json!(paths)));
    }
    Ok(())
}

/// Run a configured coverage command at an immutable commit in a temporary worktree.
pub(crate) fn measure_coverage(
    repo: &std::path::Path,
    plan: &crate::ids::PlanId,
    key: &WorkflowKey,
    commit: &str,
    command: &str,
) -> Result<CoverageReport, CpdError> {
    use crate::error::ErrorCode;
    // Do not collide with an enclosing `pr rebase` worktree for this workflow.
    let scratch_key = WorkflowKey::parse(if key.as_str() == "cpd-coverage" {
        "cpd-coverage-2"
    } else {
        "cpd-coverage"
    })
    .map_err(|e| CpdError::bad_row(e.to_string()))?;
    let worktree = crate::git::RebaseWorktree::add(repo, plan, &scratch_key, commit)?;
    let call = crate::host::runner::Call {
        tool: "coverage".into(),
        program: "sh".into(),
        args: vec!["-c".into(), command.into()],
        envs: vec![],
        cwd: Some(worktree.path().to_path_buf()),
        deadline: std::time::Duration::from_secs(3600),
        stdin: None,
        stderr_to: None,
    };
    let output = crate::ports::Ports::current().runner.run(&call);
    worktree.remove()?;
    let output = output.map_err(|e| {
        CpdError::new(
            ErrorCode::CoverageBelowBaseline,
            format!("Coverage measurement failed: {e:?}"),
        )
    })?;
    if !output.status.success() {
        return Err(CpdError::new(
            ErrorCode::CoverageBelowBaseline,
            "Coverage measurement failed.",
        ));
    }
    let report: CoverageReport = serde_json::from_slice(&output.stdout).map_err(|e| {
        CpdError::new(
            ErrorCode::CoverageBelowBaseline,
            format!("Invalid coverage report: {e}"),
        )
    })?;
    report.validate()?;
    Ok(report)
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct CoverageReport {
    line: f64,
    branch: f64,
    files: Option<std::collections::BTreeMap<String, CoverageMetrics>>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct CoverageMetrics {
    line: f64,
    branch: f64,
}

impl CoverageReport {
    fn validate(&self) -> Result<(), CpdError> {
        let valid = |line: f64, branch: f64| {
            line.is_finite() && branch.is_finite() && line >= 0.0 && branch >= 0.0
        };
        if !valid(self.line, self.branch)
            || self
                .files
                .as_ref()
                .is_some_and(|files| files.values().any(|v| !valid(v.line, v.branch)))
        {
            return Err(CpdError::new(
                crate::error::ErrorCode::CoverageBelowBaseline,
                "Invalid coverage values.",
            ));
        }
        Ok(())
    }
    /// `renamed`: `(old, new)` pairs; the new path must keep the old path's baseline, and the aggregate must hold.
    /// `removed`: the paths `touched` that no longer exist (deleted or renamed away): their coverage left with them, it did not go missing.
    pub(crate) fn check(
        &self,
        current: &Self,
        touched: &[String],
        removed: &[String],
        renamed: &[(String, String)],
    ) -> Result<(), CpdError> {
        let below = if let Some(base_files) = &self.files {
            let current_files = current.files.as_ref().ok_or_else(|| {
                CpdError::new(
                    crate::error::ErrorCode::CoverageBelowBaseline,
                    "Coverage report omitted per-file data.",
                )
            })?;
            let lowered = |base: &CoverageMetrics, now: &CoverageMetrics| {
                now.line < base.line || now.branch < base.branch
            };
            let renamed_below = renamed.iter().any(|(old, new)| {
                base_files
                    .get(old)
                    .is_some_and(|base| current_files.get(new).is_none_or(|now| lowered(base, now)))
            }) || (!renamed.is_empty()
                && (current.line < self.line || current.branch < self.branch));
            renamed_below
                || touched.iter().any(|path| {
                    match (base_files.get(path), current_files.get(path)) {
                        (Some(base), Some(now)) => lowered(base, now),
                        (Some(_), None) => !removed.contains(path),
                        _ => false,
                    }
                })
        } else {
            current.line < self.line || current.branch < self.branch
        };
        if below {
            return Err(CpdError::new(
                crate::error::ErrorCode::CoverageBelowBaseline,
                "Refactor coverage is below the baseline.",
            ));
        }
        Ok(())
    }
}
