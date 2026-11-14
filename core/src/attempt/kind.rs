//! The checks of the `test` and `impl` kinds at `attempt submit` (contract §7.7, commands.md §6.1): the red-test guard,
//! and the repo's `[test-commands]` (`build`, `list`, `run`) run against the Job's named tests.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::Env;
use crate::config::RepoTests;
use crate::error::{CpdError, ErrorCode};
use crate::git::hooks::{Log, command_variables, render_template, run_command};
use crate::git::{self, HookOutcome, Operation};
use crate::ids::{AttemptId, PlanId};
use crate::job::guard;
use crate::plan::jobs::{self, Job};
use crate::policy::Policy;

/// Whether the Job is a `work` Job of a coverage Workflow: it names no test, and must leave the suite building and passing.
fn coverage_work(env: &mut Env, plan: &PlanId, job: &Job) -> Result<bool, CpdError> {
    let (true, Some(workflow)) = (job.kind == "work", &job.workflow) else {
        return Ok(false);
    };
    env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM workflows WHERE plan = ?1 AND key = ?2 AND shape = 'coverage')",
            [plan.as_str(), workflow.as_str()],
            |r| r.get::<_, bool>(0),
        )?)
    })
}

/// Whether the Job has the checks of this module: a `test` or `impl` Job, or a `work` Job of a coverage Workflow when the policy has `[test-commands]`.
pub(super) fn has_checks(
    env: &mut Env,
    plan: &PlanId,
    job: &Job,
    policy: &Policy,
) -> Result<bool, CpdError> {
    Ok(matches!(job.kind.as_str(), "test" | "impl")
        || (policy.test_commands.is_some() && coverage_work(env, plan, job)?))
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

/// The test-removal guard (b70): of the `written` files of the Attempt, each that held test cases at `base` keeps them
/// at `sha`, with no fewer assertions, unless the Lead approved the case (`job approve-test-removal`, read now so an
/// approval reaches a running Attempt). A file gone at `sha` lost every case. `tests_removed`, `details.path`/`tests`.
pub(super) fn tests_kept(
    env: &mut Env,
    plan: &PlanId,
    key: &str,
    root: &Path,
    base: &str,
    sha: &str,
    written: &[String],
) -> Result<(), CpdError> {
    let job = crate::ids::JobKey::parse(key)?;
    let approved = env
        .store
        .read(|conn| crate::job::approved_test_removals(conn, plan, &job))?;
    for path in written {
        let Some(before) = git::tamper::file_at(root, base, path)? else {
            continue;
        };
        if git::tamper::test_cases(&before).is_empty() {
            continue;
        }
        let after = git::tamper::file_at(root, sha, path)?.unwrap_or_default();
        git::tamper::check_tests_kept(&before, &after, &approved).map_err(|e| {
            let message = format!(
                "{} In {path}: restore them, or ask the Lead (cpd ask), who approves it with cpd job approve-test-removal.",
                e.message()
            );
            CpdError::new(ErrorCode::TestsRemoved, message)
                .with("job", key)
                .with("path", path.as_str())
                .with("tests", e.details().get("tests").cloned().unwrap_or_default())
        })?;
    }
    Ok(())
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
    /// stdout alone of the command.
    stdout: String,
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
        let own = guard::of(&all, &job);
        // A refactor Workflow has no `test` Job: its `impl` Job runs the tests of the coverage Workflow it stands on.
        match (&job.workflow, own.jobs.is_empty()) {
            (Some(workflow), true) => env
                .store
                .read(|conn| crate::workflow::base_coverage_tests(conn, plan, workflow))?
                .unwrap_or(own.tests),
            _ => own.tests,
        }
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
    let refactor_impl = job.kind == "impl"
        && match &job.workflow {
            Some(workflow) => env
                .store
                .read(|conn| crate::workflow::base_coverage_tests(conn, plan, workflow))?
                .is_some(),
            None => false,
        };
    if tests.is_empty() && !refactor_impl && job.kind != "work" {
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
    let changed = if job.kind == "impl" && commands.related.is_some() {
        git::tamper::changed_paths(root, diff_base, sha)?
    } else {
        Vec::new()
    };
    run_tests(env, plan, id, &job, &tests, &changed, &commands, root, lead)
}

#[allow(clippy::too_many_arguments)]
fn run_tests(
    env: &mut Env,
    plan: &PlanId,
    id: &AttemptId,
    job: &Job,
    tests: &[String],
    changed: &[String],
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
                stdout: result.stdout,
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

    // related (an `impl` Job): the tests of the changed files join the named ones, before `list` checks the union; a discovery failure is the Gate's.
    let all = if job.kind == "impl" {
        let context = command_variables(root, &operation)?;
        let mut diagnostics = (None, String::new(), None);
        gate_tests(
            commands.related.as_deref(),
            changed,
            tests,
            &context,
            &mut |command| {
                let raw = format!("{{% raw %}}{command}{{% endraw %}}");
                match go("related", &raw, &[], Log::Truncate) {
                    Ok(ran) => {
                        diagnostics.0 = ran.tail;
                        diagnostics.1 = ran.log;
                        let exit = match ran.outcome {
                            HookOutcome::Passed => 0,
                            HookOutcome::Failed { exit, .. } => exit,
                            HookOutcome::Error(why) => {
                                diagnostics.2 = Some(why.to_string());
                                -1
                            }
                        };
                        RelatedRun {
                            exit,
                            stdout: ran.stdout,
                        }
                    }
                    Err(e) => {
                        diagnostics.2 = Some(e.to_string());
                        RelatedRun {
                            exit: -1,
                            stdout: String::new(),
                        }
                    }
                }
            },
        )
        .map_err(|e| {
            e.with("job", job.key.as_str())
                .with("tail", diagnostics.0)
                .with(
                    "log",
                    if diagnostics.1.is_empty() {
                        Value::Null
                    } else {
                        json!(diagnostics.1)
                    },
                )
                .with("reason", diagnostics.2)
        })?
    } else {
        tests.to_vec()
    };
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
        for name in all.as_slice() {
            let ran = go("list", list, &[("test", json!(name))], Log::Append)?;
            if let Some(e) = error("list", &ran, &all) {
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
            let (named, found) = split_unknown(tests, &unknown);
            if named.is_empty() {
                return Err(refuse(
                    ErrorCode::GateRelatedFailed,
                    format!(
                        "The repo's [test-commands] `related` printed a name that no test has: {}; the repo's `list` said so (details.log).",
                        found.join(", ")
                    ),
                    &ran,
                    &all,
                ));
            }
            return Err(refuse(
                ErrorCode::TestsNotFound,
                format!(
                    "No test has the name: {}. Fix the name or the test, commit, and submit again; the repo's `list` said so (details.log).",
                    named.join(", ")
                ),
                &ran,
                &named,
            ));
        }
    }

    let tests = all.as_slice();

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
        ("impl" | "work", HookOutcome::Failed { exit, .. }) => Err(refuse(
            ErrorCode::TestsFailed,
            format!("The named tests still fail (exit {exit}): fix the code, not the tests, commit, and submit again; the tail is in details.tail."),
            &run,
            tests,
        )),
        _ => Ok(()),
    }
}

/// The names `list` rejected, as (the Job's named tests, the ones `related` found): a named one is `tests_not_found`,
/// one only `related` found is `gate_related_failed`.
pub(crate) fn split_unknown(named: &[String], unknown: &[String]) -> (Vec<String>, Vec<String>) {
    unknown.iter().cloned().partition(|n| named.contains(n))
}

/// What the repo's `related` command ended as.
pub(crate) struct RelatedRun {
    pub exit: i32,
    /// What the command printed to stdout: a JSON array of test names.
    pub stdout: String,
}

/// The tests the Gate runs: the changed tests, unioned with what `related` (the `[test-commands]` key, with `{{ files }}`
/// the changed files, and the hook's other variables from `context`) prints as a JSON array on stdout; a non-zero exit or output that is not that array is `gate_related_failed`. No `related` is the changed tests alone.
pub(crate) fn gate_tests(
    related: Option<&str>,
    changed_files: &[String],
    changed_tests: &[String],
    context: &serde_json::Map<String, Value>,
    run: &mut dyn FnMut(&str) -> RelatedRun,
) -> Result<Vec<String>, CpdError> {
    let mut tests = changed_tests.to_vec();
    let Some(template) = related else {
        return Ok(tests);
    };
    let failed = |message: String| CpdError::new(ErrorCode::GateRelatedFailed, message);
    // The hook's variables, and `files`: the changed files, which a list renders shell-escaped and space-joined.
    let mut vars = context.clone();
    vars.insert("files".to_owned(), json!(changed_files));
    let command = render_template(template, &vars).map_err(|e| {
        failed(format!(
            "The repo's [test-commands] `related` cannot run: {e}"
        ))
    })?;
    let ran = run(&command);
    if ran.exit != 0 {
        return Err(failed(format!(
            "The repo's [test-commands] `related` failed (exit {}): `{command}`.",
            ran.exit
        )));
    }
    let found: Vec<String> = serde_json::from_str(ran.stdout.trim()).map_err(|e| {
        failed(format!(
            "The repo's [test-commands] `related` must print a JSON array of test names, as [\"a\", \"b\"]: {e}."
        ))
    })?;
    for name in found {
        if !tests.contains(&name) {
            tests.push(name);
        }
    }
    Ok(tests)
}
