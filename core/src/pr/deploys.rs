//! The deploy read (swr-github.md §8, contract §7.6 step 8): the workflow runs that wait for the User's approval, of every
//! repo whose policy declares `[deploys] workflows`, kept in `swr_deploys` for E8's `deploy` item.

use std::path::Path;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::gh::run_raw_with;
use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
use crate::store::Store;
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::host::runner::{Call, Runner};
use crate::policy::{self, load_scope_file};

/// How long one `gh api` call of the read may take.
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
struct Runs {
    workflow_runs: Vec<Listed>,
}

#[derive(Deserialize)]
struct Listed {
    id: u64,
    #[serde(default)]
    updated_at: String,
    triggering_actor: Option<Actor>,
    #[serde(flatten)]
    rest: serde_json::Map<String, Value>,
}

#[derive(Deserialize)]
struct Actor {
    login: String,
}

/// The delta read of `swr_deploys` (swr-github.md §8): the waiting runs of every declared workflow, filtered by
/// `swr_viewer.login`, `pending_deployments` only for a run whose `(repo, run_id, updated_at)` is not stored; rows of runs that
/// are no longer waiting, or whose pull request reads `closed` or `merged`, get `inactive_at` = `now`. A workflow whose read
/// fails keeps its rows; the first failure is returned after the others are written. Unchanged data opens no write.
pub fn read_into_store(
    runner: &dyn Runner,
    store: &mut Store,
    home: &CpdHome,
    now: OffsetDateTime,
) -> Result<(), CpdError> {
    let declared = declared(home)?;
    let me: Option<String> = store.read(|c| {
        Ok(c.query_row("SELECT login FROM swr_viewer WHERE id = 1", [], |r| r.get(0))
            .optional()?)
    })?;
    // No viewer yet: the pass writes it; there is nobody to filter by before that.
    let Some(me) = me else { return Ok(()) };
    let at = timestamp(now);
    let mut failure: Option<CpdError> = None;
    let mut writes: Vec<Write> = Vec::new();
    // A repo whose policy no longer declares a workflow: its waiting runs are nobody's to approve.
    let undeclared: Vec<(String, i64)> = store.read(|c| {
        let mut stmt = c.prepare("SELECT repo, run_id FROM swr_deploys WHERE inactive_at IS NULL")?;
        Ok(stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    writes.extend(
        undeclared
            .into_iter()
            .filter(|(repo, _)| !declared.iter().any(|(r, _)| r == repo))
            .map(|(repo, id)| Write::Inactive(repo, id)),
    );
    for (repo, workflow) in &declared {
        let alone = declared.iter().filter(|(r, _)| r == repo).count() == 1;
        let stored: Vec<(i64, String, bool, Option<String>)> = store.read(|c| {
            let mut stmt = c.prepare(
                "SELECT run_id, updated_at, inactive_at IS NULL, json_extract(run_json, '$._workflow')
                   FROM swr_deploys WHERE repo = ?1",
            )?;
            Ok(stmt
                .query_map(params![repo], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        match changed_runs(runner, home.root(), &me, repo, workflow, &stored) {
            Ok((listed, changed)) => {
                for (id, _, active, of) in &stored {
                    let ours = match of {
                        Some(of) => of == workflow,
                        None => alone,
                    };
                    if *active && ours && !listed.contains(&(*id as u64)) {
                        writes.push(Write::Inactive(repo.clone(), *id));
                    }
                }
                writes.extend(changed.into_iter().map(|c| Write::Row(repo.clone(), c)));
            }
            Err(e) => {
                failure.get_or_insert(e);
            }
        }
    }
    let closed = "UPDATE swr_deploys SET inactive_at = ?1 WHERE inactive_at IS NULL AND EXISTS (
                    SELECT 1 FROM pull_requests p WHERE p.repo = swr_deploys.repo
                      AND p.number = json_extract(swr_deploys.run_json, '$.pull_requests[0].number')
                      AND p.state IN ('closed', 'merged'))";
    let closing: bool = store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM swr_deploys WHERE inactive_at IS NULL AND EXISTS (
                        SELECT 1 FROM pull_requests p WHERE p.repo = swr_deploys.repo
                          AND p.number = json_extract(swr_deploys.run_json, '$.pull_requests[0].number')
                          AND p.state IN ('closed', 'merged')))",
            [],
            |r| r.get(0),
        )?)
    })?;
    if !writes.is_empty() || closing {
        store.write("swr deploys", |tx| {
            for w in &writes {
                match w {
                    Write::Row(repo, c) => {
                        tx.execute(
                            "INSERT INTO swr_deploys (repo, run_id, updated_at, run_json, pending_json, inactive_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, NULL)
                             ON CONFLICT (repo, run_id) DO UPDATE SET updated_at = excluded.updated_at,
                               run_json = excluded.run_json, pending_json = excluded.pending_json, inactive_at = NULL",
                            params![repo, c.run_id as i64, c.updated_at, c.run_json, c.pending_json],
                        )?;
                    }
                    Write::Inactive(repo, id) => {
                        tx.execute(
                            "UPDATE swr_deploys SET inactive_at = ?3 WHERE repo = ?1 AND run_id = ?2 AND inactive_at IS NULL",
                            params![repo, id, at],
                        )?;
                    }
                }
            }
            tx.execute(closed, [&at])?;
            Ok::<(), CpdError>(())
        })?;
    }
    failure.map_or(Ok(()), Err)
}

enum Write {
    Row(String, Changed),
    Inactive(String, i64),
}

/// A run to write: new or moved since the stored row.
struct Changed {
    run_id: u64,
    updated_at: String,
    run_json: String,
    pending_json: String,
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

/// The waiting runs of `workflow` of `repo` that are the User's (`me`): their ids, and the ones whose `(run_id, updated_at)` is
/// not in `stored` with the `pending_deployments` read for them. A run with nothing the User can approve is not kept.
fn changed_runs(
    runner: &dyn Runner,
    cwd: &Path,
    me: &str,
    repo: &str,
    workflow: &str,
    stored: &[(i64, String, bool, Option<String>)],
) -> Result<(Vec<u64>, Vec<Changed>), CpdError> {
    let path =
        format!("repos/{repo}/actions/workflows/{workflow}/runs?status=waiting&per_page=100");
    let args = ["api", path.as_str()];
    let listed: Runs = parsed(&args, &api(runner, cwd, &args)?)?;
    let mut ids = Vec::new();
    let mut out = Vec::new();
    for run in listed.workflow_runs {
        if !run
            .triggering_actor
            .as_ref()
            .is_some_and(|a| a.login.eq_ignore_ascii_case(me))
        {
            continue;
        }
        let known = stored
            .iter()
            .find(|(id, _, _, _)| *id == run.id as i64)
            .is_some_and(|(_, updated, active, _)| *active && *updated == run.updated_at);
        if known {
            ids.push(run.id);
            continue;
        }
        let path = format!("repos/{repo}/actions/runs/{}/pending_deployments", run.id);
        let args = ["api", path.as_str()];
        let pending: Vec<Value> = parsed(&args, &api(runner, cwd, &args)?)?;
        let approvable: Vec<Value> = pending
            .into_iter()
            .filter(|p| p["current_user_can_approve"].as_bool() == Some(true))
            .collect();
        if approvable.is_empty() {
            continue;
        }
        ids.push(run.id);
        let mut json_run = run.rest;
        json_run.insert("id".into(), json!(run.id));
        json_run.insert("updated_at".into(), json!(run.updated_at));
        json_run.insert(
            "triggering_actor".into(),
            json!({"login": run.triggering_actor.map(|a| a.login)}),
        );
        json_run.insert("_workflow".into(), json!(workflow));
        out.push(Changed {
            run_id: run.id,
            updated_at: run.updated_at,
            run_json: Value::Object(json_run).to_string(),
            pending_json: Value::Array(approvable).to_string(),
        });
    }
    Ok((ids, out))
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
