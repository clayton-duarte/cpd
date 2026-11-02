//! The checks of the `test` and `impl` kinds at `attempt submit` (contract §7.7, commands.md §6.1): the red-test guard,
//! and the repo's `[test-commands]` (`build`, `list`, `run`) run against the Job's named tests.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::Env;
use crate::config::RepoTests;
use crate::error::{CpdError, ErrorCode};
use crate::git::hooks::{Log, run_command};
use crate::git::{self, HookOutcome, Operation};
use crate::ids::{AttemptId, PlanId};
use crate::job::guard;
use crate::plan::jobs::{self, Job};
use crate::policy::Policy;

/// Whether the Job's kind has the checks of this module.
pub(super) fn has_checks(job: &Job) -> bool {
    matches!(job.kind.as_str(), "test" | "impl")
}

/// The Job as stored now, with its siblings: `test_approved` is read when a check runs, so an approval reaches an Attempt that is running.
fn fresh(env: &mut Env, plan: &PlanId, key: &str) -> Result<(Vec<Job>, Job), CpdError> {
    env.store.read(|conn| {
        let all = jobs::load(conn, plan)?;
        let job = all
            .iter()
            .find(|j| j.key == key)
            .cloned()
            .ok_or_else(|| CpdError::bad_row(format!("Plan {plan} has no Job {key}")))?;
        Ok((all, job))
    })
}

/// The red-test guard (`tests_tampered`, §7.5, §7.7) for an `impl` Job: its diff `base...sha` changes no path a `test` Job it needs guards, unless the Lead approved it.
pub(super) fn guard_tests(
    env: &mut Env,
    plan: &PlanId,
    job: &Job,
    root: &Path,
    compare: &str,
    base: &str,
    sha: &str,
) -> Result<(), CpdError> {
    if job.kind != "impl" {
        return Ok(());
    }
    let (all, job) = fresh(env, plan, &job.key)?;
    let guard = guard::of(&all, &job);
    let paths =
        git::tamper::touched_guarded(root, compare, base, sha, &guard.files, &guard.approved)?;
    if paths.is_empty() {
        return Ok(());
    }
    Err(CpdError::new(
        ErrorCode::TestsTampered,
        format!(
            "The diff changes red tests of {}: {}. Revert them, or ask the Lead (cpd ask), who approves a path with cpd job approve-tests.",
            guard.jobs.join(", "),
            paths.join(", ")
        ),
    )
    .with("job", job.key.as_str())
    .with("paths", json!(paths))
    .with("jobs", json!(guard.jobs)))
}

fn unconfigured(job: &str) -> CpdError {
    CpdError::new(
        ErrorCode::TestsUnconfigured,
        "The repo's policy has no [test-commands], so the tests of this Job cannot be checked. The Lead adds the table to the repo's policy file ($CPD_HOME/policy/<owner>/<repo>.toml; the file may not exist yet); a Workflow with no behaviour is made with --shape docs or --shape config.",
    )
    .with("job", job)
}

/// What a `[test-commands]` command ended as, when it did not exit as the check wants.
struct Ran {
    outcome: HookOutcome,
    tail: Option<String>,
    log: String,
}

/// The checks of a `test` or `impl` Job, after the quick check (§6.1): `test_files_missing`, `tests_unconfigured`, then the
/// `build`, the optional `list` and the `run` of the Job's named tests. HEAD is `sha`, the worktree clean.
// Each argument is a distinct input of the check; a signature change is out of scope.
#[allow(clippy::too_many_arguments)]
pub(super) fn check(
    env: &mut Env,
    plan: &PlanId,
    id: &AttemptId,
    job: &Job,
    policy: &Policy,
    root: &Path,
    lead: &str,
    diff_base: &str,
    sha: &str,
) -> Result<(), CpdError> {
    let (all, job) = fresh(env, plan, &job.key)?;
    let tests = if job.kind == "impl" {
        guard::of(&all, &job).tests
    } else {
        guard::own(&job).0
    };
    if job.kind == "test" {
        let (_, files) = guard::own(&job);
        let changed = git::tamper::changed_paths(root, diff_base, sha)?;
        let missing: Vec<&String> = files
            .iter()
            .filter(|f| !changed.iter().any(|c| git::tamper::path_matches(f, c)))
            .collect();
        if !missing.is_empty() {
            return Err(CpdError::new(
                ErrorCode::TestFilesMissing,
                format!(
                    "The test_files of this Job name paths you did not change: {}. Add the red tests there, or ask the Lead (cpd ask).",
                    missing.iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", ")
                ),
            )
            .with("job", job.key.as_str())
            .with("paths", json!(missing)));
        }
    }
    let Some(commands) = policy.test_commands.clone() else {
        return Err(unconfigured(&job.key));
    };
    if tests.is_empty() {
        // An `impl` Job's tests are its `test` Jobs': with none, `run` would be told no test and might run them all (hs-d338).
        return Err(CpdError::new(
            ErrorCode::TestJobRequired,
            format!(
                "The impl Job {} needs no test Job of its Workflow, so it names no test to run.",
                job.key
            ),
        )
        .with("job", job.key.as_str())
        .with("jobs", json!(Vec::<String>::new())));
    }
    run_tests(env, plan, id, &job, &tests, &commands, root, lead)
}

#[allow(clippy::too_many_arguments)]
fn run_tests(
    env: &mut Env,
    plan: &PlanId,
    id: &AttemptId,
    job: &Job,
    tests: &[String],
    commands: &RepoTests,
    root: &Path,
    lead: &str,
) -> Result<(), CpdError> {
    let timeout = Duration::from_secs(u64::from(env.config.timeouts.gate_minutes) * 60);
    let operation = Operation { branch: lead };
    let log_of = |what: &str| env.home.tests_log(plan, id, what);
    let refuse = |code: ErrorCode, message: String, ran: &Ran, tests: &[String]| {
        CpdError::new(code, message)
            .with("job", job.key.as_str())
            .with("tests", json!(tests))
            .with("tail", ran.tail.clone())
            .with("log", ran.log.as_str())
    };
    let go =
        |what: &str, template: &str, extra: &[(&str, Value)], mode: Log| -> Result<Ran, CpdError> {
            let log = log_of(what);
            let result = run_command(template, extra, root, &operation, timeout, &log, mode)?;
            let wrote = result.tail.is_some();
            Ok(Ran {
                outcome: result.outcome,
                tail: result.tail,
                log: if wrote {
                    log.display().to_string()
                } else {
                    String::new()
                },
            })
        };
    // A command that could not run: `tests_error`, exit 1, the Worker asks the Lead.
    let error = |what: &str, ran: &Ran, tests: &[String]| -> Option<CpdError> {
        let HookOutcome::Error(why) = &ran.outcome else {
            return None;
        };
        let mut e = refuse(
            ErrorCode::TestsError,
            format!("The repo's [test-commands] `{what}` command could not be judged: {why}. Ask the Lead: cpd ask {id} \"<question>\"."),
            ran,
            tests,
        )
        .with("reason", why.to_string())
        .with("command", what);
        if ran.log.is_empty() {
            e = e.with("log", Value::Null);
        }
        Some(e)
    };

    // build: non-zero is a build error, and `run` does not run.
    let build = go("build", &commands.build, &[], Log::Truncate)?;
    if let Some(e) = error("build", &build, tests) {
        return Err(e);
    }
    if let HookOutcome::Failed { exit, .. } = build.outcome {
        return Err(refuse(
            ErrorCode::TestsBuildFailed,
            format!(
                "The repo's [test-commands] `build` failed (exit {exit}): the branch does not build. Fix it, commit, and submit again; the tail is in details.tail."
            ),
            &build,
            tests,
        ));
    }

    // list, once per named test: non-zero means no test has that name.
    if let Some(list) = &commands.list {
        let log = log_of("list");
        std::fs::remove_file(&log)
            .or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .map_err(|e| {
                CpdError::new(
                    ErrorCode::TestsError,
                    format!("Cannot reset {}: {e}.", log.display()),
                )
                .with("job", job.key.as_str())
            })?;
        let mut unknown = Vec::new();
        let mut last = None;
        for name in tests {
            let ran = go("list", list, &[("test", json!(name))], Log::Append)?;
            if let Some(e) = error("list", &ran, tests) {
                return Err(e);
            }
            if matches!(ran.outcome, HookOutcome::Failed { .. }) {
                unknown.push(name.clone());
            }
            last = Some(ran);
        }
        if let (false, Some(ran)) = (unknown.is_empty(), last) {
            let tail = std::fs::read_to_string(&log)
                .ok()
                .map(|t| git::tail(&t, git::TAIL_LINES));
            let ran = Ran { tail, ..ran };
            return Err(refuse(
                ErrorCode::TestsNotFound,
                format!(
                    "No test has the name: {}. Fix the name or the test, commit, and submit again; the repo's `list` said so (details.log).",
                    unknown.join(", ")
                ),
                &ran,
                &unknown,
            ));
        }
    }

    // run: a `test` Job's named tests must fail, an `impl` Job's must pass.
    let run = go(
        "run",
        &commands.run,
        &[("tests", json!(tests))],
        Log::Truncate,
    )?;
    if let Some(e) = error("run", &run, tests) {
        return Err(e);
    }
    match (job.kind.as_str(), &run.outcome) {
        ("test", HookOutcome::Passed) => Err(refuse(
            ErrorCode::TestsNotRed,
            "The named tests pass, so they prove nothing yet: make them fail for the behaviour they pin, commit, and submit again.".to_owned(),
            &run,
            tests,
        )),
        ("impl", HookOutcome::Failed { exit, .. }) => Err(refuse(
            ErrorCode::TestsFailed,
            format!("The named tests still fail (exit {exit}): fix the code, not the tests, commit, and submit again; the tail is in details.tail."),
            &run,
            tests,
        )),
        _ => Ok(()),
    }
}
