//! `cpd janitor` (commands.md §29, contract §7.2): one pass over the merged and cancelled PRs that still have a
//! worktree on disk, and the ended Attempts of merged Jobs on live PRs (contract §7.2), then exit. The poll cleans nothing: a PR in status `merged` or `cancelled` is the queue.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git;
use crate::home::CpdHome;
use crate::host::layout::attempt_window;
use crate::host::tmux::Tmux;
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::lock;
use crate::output::Output;

/// The `source` of the Events and the `by` of the store writes of a pass.
const SOURCE: &str = "janitor";

fn lock_failed(path: &Path, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the janitor lock {}: {e}.", path.display())).with("lock", path.display().to_string())
}

fn lock_path(home: &CpdHome) -> PathBuf {
    home.locks_dir().join("janitor.lock")
}

/// `locks/janitor.lock`, taken without waiting; `None` when another pass holds it.
fn take(home: &CpdHome) -> Result<Option<File>, CpdError> {
    let path = lock_path(home);
    std::fs::create_dir_all(home.locks_dir()).map_err(|e| lock_failed(&path, e))?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(|e| lock_failed(&path, e))?;
    Ok(lock::try_lock(&file).map_err(|e| lock_failed(&path, e))?.then_some(file))
}

/// One pass: takes `locks/janitor.lock` without waiting (held by another pass: `{"ok":true,"skipped":"running"}`,
/// nothing touched) and cleans every pending PR. Output `{"ok":true,"outcomes":[{plan, pr, status, path, outcome,
/// reason?}]}` (commands.md §29). Appends the `cleanup_failed` Events (source `janitor`).
pub fn pass(env: &mut Env) -> Result<Output, CpdError> {
    let Some(_lock) = take(&env.home)? else {
        log(env, "locked");
        return Ok(Output::success(json!({"skipped": "running"})));
    };
    let result = run(env);
    match &result {
        Ok(_) => log(env, "ok"),
        Err(e) => log(env, e.code().as_str()),
    }
    result
}

/// Whether a pass holds `locks/janitor.lock` (engine E16's probe): opens the file without creating it, lets go at once.
pub fn running(home: &CpdHome) -> Result<bool, CpdError> {
    let path = lock_path(home);
    let file = match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(lock_failed(&path, e)),
    };
    // The probe's lock is let go when `file` drops.
    Ok(!lock::try_lock(&file).map_err(|e| lock_failed(&path, e))?)
}

/// The log of the passes, appended (as `context::distill::log_path`).
pub fn log_path(home: &CpdHome) -> PathBuf {
    home.root().join("run").join("janitor.log")
}

/// One line per pass: `<timestamp> <pass id> <status>`. A write failure fails nothing.
fn log(env: &Env, status: &str) {
    let line = format!("{} {} {status}", crate::clock::timestamp(env.ports.clock.utc()), std::process::id());
    let path = log_path(&env.home);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

/// What a pass did to one path.
struct Entry {
    plan: PlanId,
    pr: PrKey,
    status: String,
    path: String,
    outcome: &'static str,
    reason: Option<String>,
    /// The path was renamed aside before the failure: its Event says what was removed.
    renamed: bool,
}

fn run(env: &mut Env) -> Result<Output, CpdError> {
    let pending = env.store.read(merged_prs_to_clean)?;
    let tmux = Tmux::new(env.config.tmux.socket.as_str());
    let work = env.config.tmux.work_session.clone();
    let mut panes = None;
    let mut entries = Vec::new();
    let mut events = Vec::new();
    for merged in pending.iter().filter(|m| m.on_disk()) {
        if panes.is_none() && merged.idle(&env.home).is_ok_and(|idle| idle) {
            panes = Some(tmux.list_panes());
        }
        let found = clean_when_idle(&env.home, merged, &tmux, &work, panes.as_ref());
        if merged.fresh {
            events.extend(found.iter().filter(|e| matches!(e.outcome, "kept" | "failed")).map(event));
        }
        entries.extend(found);
    }
    if !events.is_empty() {
        env.store.write(SOURCE, |tx| {
            for (plan, event) in events {
                append(tx, event.plan(plan))?;
            }
            Ok(())
        })?;
    }
    let outcomes: Vec<Value> = entries
        .iter()
        .map(|e| {
            let mut v = json!({"plan": e.plan, "pr": e.pr, "status": e.status, "path": e.path, "outcome": e.outcome});
            if let Some(reason) = &e.reason {
                v["reason"] = json!(reason);
            }
            v
        })
        .collect();
    Ok(Output::success(json!({"outcomes": outcomes})))
}

/// The `cleanup_failed` Event of a `kept` or `failed` entry (DESIGN 5).
fn event(e: &Entry) -> (PlanId, NewEvent) {
    let reason = e.reason.clone().unwrap_or_default();
    let (status, pr, plan, path) = (&e.status, &e.pr, &e.plan, &e.path);
    let (message, text) = if e.outcome == "kept" {
        let m = format!("The clean-up of the {status} PR {pr} of Plan {plan} kept {path} untouched: {reason}");
        (m.clone(), m)
    } else if e.renamed {
        let m = format!("The clean-up of the {status} PR {pr} of Plan {plan} removed {path}, but not everything: {reason}");
        (m.clone(), format!("{m}. The next janitor pass retries."))
    } else {
        let m = format!("The clean-up of the {status} PR {pr} of Plan {plan} did not remove {path}: {reason}");
        (m.clone(), format!("{m}. The next janitor pass retries."))
    };
    let details = json!({"plan": plan, "pr": pr, "status": status, "outcome": e.outcome, "path": path, "reason": reason});
    (plan.clone(), NewEvent::error(text, SOURCE, ErrorCode::CleanupFailed.as_str(), &message, details))
}

/// A merged or cancelled PR with what its clean-up removes; also a live (`dispatch` or `open`) PR with only its ended
/// Attempts of merged Jobs, never its Lead worktree (contract §7.2 "Ended Attempts of merged Jobs on a live PR").
pub(crate) struct Merged {
    pub(crate) plan: PlanId,
    pub(crate) pr: PrKey,
    /// The PR's status in the store: `merged`, `cancelled`, or (for the swept Attempts only) `dispatch` or `open`.
    pub(crate) status: String,
    pub(crate) repo: String,
    pub(crate) lead_branch: String,
    pub(crate) lead_worktree: Option<String>,
    /// The PR's pull request, `(owner/name, number)`, from its `pull_requests` row (contract §7.2 "The recorded head").
    pub(crate) pull_request: Option<(String, i64)>,
    pub(crate) attempts: Vec<Attempt>,
    /// Whether no `cleanup_failed` Event of the PR exists yet: this pass reports its failures.
    pub(crate) fresh: bool,
}

/// An Attempt of one of the PR's Jobs.
pub(crate) struct Attempt {
    pub(crate) id: AttemptId,
    pub(crate) branch: String,
    pub(crate) worktree: String,
    pub(crate) ended: bool,
}

/// `<path>.removing`, the aside leftover of a failed removal (contract §7.2).
fn aside(path: &str) -> PathBuf {
    let mut name = PathBuf::from(path).into_os_string();
    name.push(".removing");
    PathBuf::from(name)
}

impl Merged {
    /// The parallel Attempts' worktrees on disk (a linear Attempt runs in the Lead worktree).
    fn attempt_paths(&self) -> impl Iterator<Item = &Attempt> {
        self.attempts.iter().filter(|a| !git::worktree::is_linear(&a.branch, &self.lead_branch) && (Path::new(&a.worktree).exists() || aside(&a.worktree).exists()))
    }

    /// The Lead worktree or its aside leftover, when on disk.
    fn lead_path(&self) -> Option<&String> {
        self.lead_worktree.as_ref().filter(|p| Path::new(p).exists() || aside(p).exists())
    }

    /// Whether a path this clean-up removes is still on disk: an Attempt or Lead worktree, or the aside leftover.
    fn on_disk(&self) -> bool {
        self.attempt_paths().next().is_some() || self.lead_path().is_some()
    }

    /// Contract §7.2 (hs-d243, hs-n124): every Attempt of the PR's Jobs is `ended` and its driver lock is free, probed
    /// without creating the file. An unprobeable lock is the error.
    fn idle(&self, home: &CpdHome) -> Result<bool, CpdError> {
        for attempt in &self.attempts {
            if !attempt.ended || crate::driver::lock::probe(home, &self.plan, &attempt.id)?.is_none() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// Every PR in status `merged` or `cancelled`, with its Attempts, and every live PR (`dispatch` or `open`) with only its
/// ended Attempts that merged or whose Job merged (contract §7.2 "Ended Attempts of merged Jobs on a live PR"). A cancelled PR's row has no `lead_worktree`
/// (`pr cancel` clears it), so its Lead worktree is the one contract §7.1 names, when that is a worktree of the repo
/// on the PR's Lead Branch (or its aside leftover is on disk).
pub(crate) fn merged_prs_to_clean(conn: &rusqlite::Connection) -> Result<Vec<Merged>, CpdError> {
    let mut prs = conn.prepare(
        "SELECT p.plan, p.key, p.repo, p.lead_branch, p.lead_worktree, q.repo, q.number, p.status
           FROM workflows p LEFT JOIN pull_requests q ON q.id = p.pull_request WHERE p.status IN ('dispatch', 'open', 'merged', 'cancelled') ORDER BY p.plan, p.key",
    )?;
    let rows = prs
        .query_map([], |r| {
            let pull_request = r.get::<_, Option<String>>(5)?.zip(r.get::<_, Option<i64>>(6)?);
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, pull_request, r.get::<_, String>(7)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut attempts = conn.prepare(
        "SELECT a.id, a.branch, a.worktree, a.status = 'ended', coalesce(a.outcome = 'merged' OR j.status = 'merged', 0) FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE j.plan = ?1 AND j.workflow = ?2 ORDER BY a.id",
    )?;
    let mut reported = conn.prepare(
        "SELECT count(*) FROM events WHERE plan = ?1 AND kind = 'error' AND json_extract(data, '$.code') = 'cleanup_failed'
            AND json_extract(data, '$.details.pr') = ?2",
    )?;
    let mut found = Vec::new();
    for (plan, pr, repo, lead_branch, lead_worktree, pull_request, status) in rows {
        let live = status == "dispatch" || status == "open";
        let list = attempts
            .query_map([&plan, &pr], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?, r.get::<_, bool>(4)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, _, _, ended, sweepable)| !live || *ended && *sweepable)
            .map(|(id, branch, worktree, ended, _)| Ok(Attempt { id: AttemptId::parse(&id)?, branch, worktree, ended }))
            .collect::<Result<Vec<_>, CpdError>>()?;
        let plan = PlanId::parse(&plan)?;
        let pr = PrKey::parse(&pr)?;
        let lead_worktree = if status == "cancelled" {
            git::pr_lead_worktree_path(Path::new(&repo), &plan, &pr, &lead_branch)
                .ok()
                .filter(|path| {
                    aside(&path.to_string_lossy()).exists()
                        || path.exists()
                            && git::worktree::list(Path::new(&repo)).is_ok_and(|all| {
                                all.iter().any(|w| w.branch.as_deref() == Some(lead_branch.as_str()) && w.path.canonicalize().ok() == path.canonicalize().ok())
                            })
                })
                .map(|path| path.to_string_lossy().into_owned())
        } else if live {
            None
        } else {
            lead_worktree
        };
        let seen: i64 = reported.query_row([plan.as_str(), pr.as_str()], |r| r.get(0))?;
        found.push(Merged { plan, pr, status, repo, lead_branch, lead_worktree, pull_request, attempts: list, fresh: seen == 0 });
    }
    Ok(found)
}

/// [`clean`] once the PR is idle; while it is not, every path on disk is `pending` and nothing is touched. A driver
/// lock that cannot be probed is a failed removal.
fn clean_when_idle(home: &CpdHome, merged: &Merged, tmux: &Tmux, work: &str, panes: Option<&Result<Vec<crate::host::tmux::Pane>, CpdError>>) -> Vec<Entry> {
    let mut paths: Vec<String> = merged.attempt_paths().map(|a| a.worktree.clone()).collect();
    paths.extend(merged.lead_path().cloned());
    let entry = |path: &str, outcome: &'static str, reason: Option<String>, renamed: bool| Entry {
        plan: merged.plan.clone(),
        pr: merged.pr.clone(),
        status: merged.status.clone(),
        path: path.to_owned(),
        outcome,
        reason,
        renamed,
    };
    match merged.idle(home) {
        Ok(true) => {}
        Ok(false) => return paths.iter().map(|p| entry(p, "pending", None, false)).collect(),
        Err(e) => return paths.iter().map(|p| entry(p, "failed", Some(reason(&e)), false)).collect(),
    }
    // The windows first: a live window still has the worktree as its working directory.
    let mut killed = Vec::new();
    match panes {
        Some(Ok(panes)) => {
            for attempt in &merged.attempts {
                let name = attempt_window(&merged.plan, &attempt.id);
                for pane in panes.iter().filter(|p| p.session_name == work && p.window_name == name) {
                    if let Err(e) = tmux.kill_window(&pane.window) {
                        killed.push((attempt.worktree.clone(), reason(&e)));
                    }
                }
            }
        }
        Some(Err(e)) => return paths.iter().map(|p| entry(p, "failed", Some(reason(e)), false)).collect(),
        None => {}
    }
    let mut entries: Vec<Entry> = killed.into_iter().map(|(path, why)| entry(&path, "failed", Some(why), false)).collect();
    entries.extend(clean(merged).into_iter().map(|(path, outcome, why, renamed)| entry(&path, outcome, why, renamed)));
    entries
}

/// `(path, outcome, reason, renamed)`.
type Cleaned = (String, &'static str, Option<String>, bool);

/// Removes the Attempt worktrees of the PR that are on disk, then its Lead worktree, first brought up to date by
/// rule S. A failure does not stop the others.
fn clean(merged: &Merged) -> Vec<Cleaned> {
    let repo = Path::new(&merged.repo);
    let mut out = Vec::new();
    for attempt in merged.attempt_paths() {
        match git::discard_worktree(repo, Path::new(&attempt.worktree), &attempt.branch) {
            Ok(_) => out.push((attempt.worktree.clone(), "removed", None, false)),
            Err(e) => out.push((attempt.worktree.clone(), "failed", Some(reason(&e)), false)),
        }
    }
    let Some(lead) = merged.lead_path() else { return out };
    // Rule S first, so a tree a merge left stale is not kept as dirty. A stop of it as `lead_worktree_stale` leaves
    // the worktree as it was; the check below keeps it if it is dirty.
    if Path::new(lead).exists() {
        match git::sync_lead(Path::new(lead), &merged.lead_branch) {
            Ok(_) => {}
            Err(e) if e.code() == ErrorCode::LeadWorktreeStale => {}
            Err(e) => {
                out.push((lead.clone(), "kept", Some(reason(&e)), false));
                return out;
            }
        }
    }
    // The pull request's head commit, asked of GitHub only when `HEAD` has commits outside every `origin/*` ref:
    // a squash merge deletes the head branch, so the PR's own commits look unpushed (contract §7.2).
    let recorded_head = || {
        let (gh_repo, number) = merged.pull_request.as_ref()?;
        crate::pr::head_ref_oid(repo, gh_repo, *number).ok()
    };
    let after_rename = !Path::new(lead).exists();
    match git::remove_merged_lead(repo, Path::new(lead), &merged.lead_branch, recorded_head) {
        Ok(git::LeadRemoval::Removed) => out.push((lead.clone(), "removed", None, false)),
        Ok(git::LeadRemoval::Kept(why)) => out.push((lead.clone(), "kept", Some(why.as_str().to_owned()), false)),
        Err(e) => out.push((lead.clone(), "failed", Some(reason(&e)), after_rename || aside(lead).exists())),
    }
    out
}

/// git's stderr tail when the error has one, else its message.
fn reason(error: &CpdError) -> String {
    match error.details().get("stderr").and_then(Value::as_str) {
        Some(stderr) if !stderr.trim().is_empty() => stderr.trim().to_owned(),
        _ => error.message().to_owned(),
    }
}
