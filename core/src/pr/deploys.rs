//! The deploy read of `pr poll` (contract §7.6 step 8): the workflow runs that wait for the User's approval, of every
//! repo whose policy declares `[deploys] workflows`, cached in `run/deploy-runs.json` (§1.4) for E8's `deploy` item.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::gh::run_raw_with;
use crate::atomic::write_atomic;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::host::runner::{Call, Runner};
use crate::policy::{self, load_scope_file};

/// How long one `gh api` call of the read may take.
const TIMEOUT: Duration = Duration::from_secs(15);

/// A run waiting for the User's approval, as `run/deploy-runs.json` holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Run {
    repo: String,
    workflow: String,
    run_id: u64,
    environment: String,
    title: String,
    url: String,
    created_at: String,
}

/// `run/deploy-runs.json`.
#[derive(Serialize, Deserialize)]
struct Cache {
    version: u32,
    polled_at: String,
    runs: Vec<Run>,
}

#[derive(Deserialize)]
struct Runs {
    workflow_runs: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    id: u64,
    display_title: String,
    html_url: String,
    created_at: String,
    triggering_actor: Option<Actor>,
}

#[derive(Deserialize)]
struct Actor {
    login: String,
}

#[derive(Deserialize)]
struct Pending {
    environment: Environment,
    current_user_can_approve: bool,
}

#[derive(Deserialize)]
struct Environment {
    name: String,
}

/// Reads the deploys that wait for the User's approval and rewrites `run/deploy-runs.json`, `polled_at` its time.
/// A workflow whose read fails keeps the runs the file had for it; the first failure is returned after the file is written.
pub(super) fn read(runner: &dyn Runner, home: &CpdHome, polled_at: &str) -> Result<(), CpdError> {
    let path = home.run_dir().join("deploy-runs.json");
    let declared = declared(home)?;
    let previous = previous(&path);
    let mut me: Option<Result<String, CpdError>> = None;
    let mut runs = Vec::new();
    let mut failure: Option<CpdError> = None;
    for (repo, workflow) in &declared {
        let me = me
            .get_or_insert_with(|| login(runner, home.root()))
            .as_ref()
            .map_err(Clone::clone);
        let read = me.and_then(|me| workflow_runs(runner, home.root(), me, repo, workflow));
        match read {
            Ok(found) => runs.extend(found),
            Err(e) => {
                runs.extend(
                    previous
                        .iter()
                        .filter(|r| r.repo == *repo && r.workflow == *workflow)
                        .cloned(),
                );
                failure.get_or_insert(e);
            }
        }
    }
    runs.sort_by(|a, b| (&a.created_at, a.run_id).cmp(&(&b.created_at, b.run_id)));
    let cache = Cache {
        version: 1,
        polled_at: polled_at.to_owned(),
        runs,
    };
    let text = serde_json::to_string(&cache).expect("the cache serializes");
    write_atomic(&path, text).map_err(|e| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot write {}: {e}.", path.display()),
        )
        .with("path", path.display().to_string())
    })?;
    failure.map_or(Ok(()), Err)
}

/// The delta read of `swr_deploys` (swr-github.md §8): the waiting runs of every declared workflow, filtered by
/// `swr_viewer.login`, `pending_deployments` only for a run whose `(repo, run_id, updated_at)` is not stored; rows of runs that
/// are no longer waiting, or whose pull request reads `closed` or `merged`, get `inactive_at` = `now`. A stub until the implementing Job.
pub fn read_into_store(
    _runner: &dyn Runner,
    _store: &mut crate::store::Store,
    _home: &CpdHome,
    _now: time::OffsetDateTime,
) -> Result<(), CpdError> {
    todo!("pr::deploys::read_into_store")
}

/// The runs the file holds now; none when it is missing or is not that JSON (the next write replaces it).
fn previous(path: &Path) -> Vec<Run> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Cache>(&text) {
        Ok(cache) if cache.version == 1 => cache.runs,
        _ => Vec::new(),
    }
}

/// Every `(owner/repo, workflow)` the repo policy files declare, sorted.
fn declared(home: &CpdHome) -> Result<Vec<(String, String)>, CpdError> {
    let dir = policy::policy_dir(home);
    let mut out = Vec::new();
    let Ok(owners) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for owner in owners.filter_map(Result::ok).filter(|e| e.path().is_dir()) {
        let owner_name = owner.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(owner.path()) else {
            continue;
        };
        for file in files.filter_map(Result::ok) {
            let path = file.path();
            let name = file.file_name().to_string_lossy().into_owned();
            let Some(repo) = name.strip_suffix(".toml").filter(|r| *r != "owner") else {
                continue;
            };
            let Some((_, scope)) = load_scope_file(&path, policy::ScopeKind::Repo)? else {
                continue;
            };
            let workflows = scope.deploys.and_then(|d| d.workflows).unwrap_or_default();
            out.extend(
                workflows
                    .into_iter()
                    .map(|w| (format!("{owner_name}/{repo}"), w)),
            );
        }
    }
    out.sort();
    Ok(out)
}

/// One `gh api <path>`: its stdout. A failed call is `gh_failed`, transient on an HTTP 5xx or a timeout, as the poll's lookups.
fn api(runner: &dyn Runner, cwd: &Path, args: &[&str]) -> Result<String, CpdError> {
    let fail = |message: String, exit: Option<i32>, stderr: &str, reason: Option<&str>| {
        gh_failed(args, message, exit, stderr, reason)
    };
    let out = run_raw_with(runner, cwd, args, TIMEOUT).map_err(|e| {
        e.with("call", "deploys")
            .with("transient", true)
            .with("reason", "timeout")
    })?;
    match out.exit {
        Some(0) => Ok(out.stdout),
        Some(code) => {
            let status = out.stderr.match_indices("HTTP 5").find_map(|(at, _)| {
                let s = out.stderr.get(at..at + 8)?;
                s[6..]
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| s.to_owned())
            });
            Err(fail(
                format!("gh {} exited {code}.", args[0]),
                Some(code),
                &out.stderr,
                status.as_deref(),
            ))
        }
        None => Err(fail(
            format!("gh {} was killed by a signal.", args[0]),
            None,
            &out.stderr,
            None,
        )),
    }
}

/// The `gh_failed` of `gh <args>`: transient on an HTTP 5xx or a timeout (`reason`).
fn gh_failed(
    args: &[&str],
    message: String,
    exit: Option<i32>,
    stderr: &str,
    reason: Option<&str>,
) -> CpdError {
    let mut argv = vec!["gh"];
    argv.extend_from_slice(args);
    CpdError::new(ErrorCode::GhFailed, message)
        .with("argv", json!(argv))
        .with("exit", json!(exit))
        .with("stderr", crate::git::tail(stderr, crate::git::TAIL_LINES))
        .with("call", "deploys")
        .with("transient", reason.is_some())
        .with(
            "reason",
            reason.map_or(serde_json::Value::Null, |r| json!(r)),
        )
}

fn parsed<T: for<'de> Deserialize<'de>>(args: &[&str], stdout: &str) -> Result<T, CpdError> {
    serde_json::from_str(stdout).map_err(|e| {
        let mut argv = vec!["gh"];
        argv.extend_from_slice(args);
        CpdError::new(
            ErrorCode::GhFailed,
            format!("gh {} printed unexpected JSON: {e}.", args[0]),
        )
        .with("argv", json!(argv))
        .with("exit", 0)
        .with("call", "deploys")
        .with("transient", false)
        .with("reason", serde_json::Value::Null)
    })
}

/// The login of the User (`gh api user --jq .login`).
fn login(runner: &dyn Runner, cwd: &Path) -> Result<String, CpdError> {
    let args = ["api", "user", "--jq", ".login"];
    let me = api(runner, cwd, &args)?.trim().to_owned();
    if me.is_empty() {
        return Err(
            CpdError::new(ErrorCode::GhFailed, "gh api user printed no login.")
                .with("argv", json!(["gh", "api", "user", "--jq", ".login"])),
        );
    }
    Ok(me)
}

/// The runs of `workflow` of `repo` that wait for `me` to approve.
fn workflow_runs(
    runner: &dyn Runner,
    cwd: &Path,
    me: &str,
    repo: &str,
    workflow: &str,
) -> Result<Vec<Run>, CpdError> {
    let path =
        format!("repos/{repo}/actions/workflows/{workflow}/runs?status=waiting&per_page=100");
    let args = ["api", path.as_str()];
    let listed: Runs = parsed(&args, &api(runner, cwd, &args)?)?;
    let mut out = Vec::new();
    for run in listed.workflow_runs {
        if !run
            .triggering_actor
            .is_some_and(|a| a.login.eq_ignore_ascii_case(me))
        {
            continue;
        }
        let path = format!("repos/{repo}/actions/runs/{}/pending_deployments", run.id);
        let args = ["api", path.as_str()];
        let pending: Vec<Pending> = parsed(&args, &api(runner, cwd, &args)?)?;
        let mut names: Vec<String> = pending
            .into_iter()
            .filter(|p| p.current_user_can_approve)
            .map(|p| p.environment.name)
            .collect();
        if names.is_empty() {
            continue;
        }
        names.sort();
        out.push(Run {
            repo: repo.to_owned(),
            workflow: workflow.to_owned(),
            run_id: run.id,
            environment: names.join(", "),
            title: run.display_title,
            url: run.html_url,
            created_at: run.created_at,
        });
    }
    Ok(out)
}

/// An entry of `pending_deployments` the User can approve, as `approve` reads it.
#[derive(Deserialize)]
struct Waiting {
    environment: Target,
    current_user_can_approve: bool,
}

#[derive(Deserialize)]
struct Target {
    id: u64,
    name: String,
}

/// A deployment environment the User can approve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approvable {
    pub id: u64,
    pub name: String,
}

/// `gh api repos/<repo>/actions/runs/<run_id>/pending_deployments`: the environments the User can approve, sorted by
/// (name, id). A failed call, a timeout or another shape of JSON is `gh_failed`.
pub fn approvable(
    runner: &dyn Runner,
    home: &CpdHome,
    repo: &str,
    run_id: u64,
) -> Result<Vec<Approvable>, CpdError> {
    let path = format!("repos/{repo}/actions/runs/{run_id}/pending_deployments");
    let args = ["api", path.as_str()];
    let waiting: Vec<Waiting> = parsed(&args, &api(runner, home.root(), &args)?)?;
    let mut out: Vec<Approvable> = waiting
        .into_iter()
        .filter(|w| w.current_user_can_approve)
        .map(|w| Approvable {
            id: w.environment.id,
            name: w.environment.name,
        })
        .collect();
    out.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
    Ok(out)
}

/// `gh api --method POST repos/<repo>/actions/runs/<run_id>/pending_deployments --input -`: approves `environments`.
pub fn approve(
    runner: &dyn Runner,
    home: &CpdHome,
    repo: &str,
    run_id: u64,
    environments: &[Approvable],
) -> Result<(), CpdError> {
    let path = format!("repos/{repo}/actions/runs/{run_id}/pending_deployments");
    let args = ["api", "--method", "POST", path.as_str(), "--input", "-"];
    let ids: Vec<u64> = environments.iter().map(|e| e.id).collect();
    let body = json!({"environment_ids": ids, "state": "approved", "comment": ""})
        .to_string()
        .into_bytes();
    let call = Call {
        tool: "gh".to_owned(),
        program: "gh".to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: vec![
            ("GH_PROMPT_DISABLED".to_owned(), Some("1".to_owned())),
            ("NO_COLOR".to_owned(), Some("1".to_owned())),
        ],
        cwd: Some(home.root().to_path_buf()),
        deadline: TIMEOUT,
        stdin: Some(body),
        stderr_to: None,
    };
    let out = match runner.run(&call) {
        Ok(out) => out,
        Err(Failed::Timeout(_)) => {
            return Err(gh_failed(
                &args,
                format!(
                    "gh api took longer than {}s and was killed.",
                    TIMEOUT.as_secs()
                ),
                None,
                "",
                Some("timeout"),
            ));
        }
        Err(Failed::Io(e)) => {
            return Err(gh_failed(
                &args,
                format!("Cannot run gh: {e}."),
                None,
                &e.to_string(),
                None,
            ));
        }
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    match out.status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(gh_failed(
            &args,
            format!("gh api exited {code}."),
            Some(code),
            &stderr,
            None,
        )),
        None => Err(gh_failed(
            &args,
            "gh api was killed by a signal.".to_owned(),
            None,
            &stderr,
            None,
        )),
    }
}

/// Removes the entries of run `run_id` of `repo` from `run/deploy-runs.json`, rewriting it only when one was removed.
/// A missing, unreadable, invalid or not-version-1 file is left as it is.
pub fn forget_run(home: &CpdHome, repo: &str, run_id: u64) -> Result<(), CpdError> {
    let path = home.run_dir().join("deploy-runs.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(mut cache) = serde_json::from_str::<Cache>(&text) else {
        return Ok(());
    };
    if cache.version != 1 {
        return Ok(());
    }
    let before = cache.runs.len();
    cache
        .runs
        .retain(|r| !(r.repo == repo && r.run_id == run_id));
    if cache.runs.len() == before {
        return Ok(());
    }
    let text = serde_json::to_string(&cache).expect("the cache serializes");
    write_atomic(&path, text).map_err(|e| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot write {}: {e}.", path.display()),
        )
        .with("path", path.display().to_string())
    })
}
