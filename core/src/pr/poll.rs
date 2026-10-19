//! `pr poll` (commands.md §9.2, contract §7.6): one poll of every open pull
//! request of the User, stored in `pull_requests`, and the merge of the PRs
//! the Plans own.

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};

use super::feedback;
use super::gh::{self, Listed};
use super::pulls::{self, Change, Reason};
use super::row::PlanRow;
use super::status::PrState;
use super::{Env, PollArgs};
use crate::driver::lock;
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git;
use crate::home::CpdHome;
use crate::host::layout::attempt_window;
use crate::host::tmux::Tmux;
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::output::Output;
use crate::store::WriteTxn;

const COMMAND: &str = "pr poll";

/// How long `pr poll` waits for `locks/pr-poll.lock` (`lock_timeout`).
const LOCK_WAIT: Duration = Duration::from_secs(60);

/// How often the wait tries the lock again.
const RETRY: Duration = Duration::from_millis(100);

/// A pull request an `open` PR of a Plan points to.
type Owned = (PlanId, PrKey, pulls::PullRequest);

/// What the transaction did.
#[derive(Default)]
struct Outcome {
    changed: Vec<Value>,
    removed: usize,
    feedback: Vec<(PlanId, PrKey)>,
    merged: Vec<(PlanId, PrKey)>,
    done: Vec<PlanId>,
}

pub(super) fn poll(args: PollArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let selected = args.plan.plan;
    if let Some(id) = &selected {
        env.plan(id)?;
    }
    let _lock = take_lock(&env.home.pr_poll_lock())?;
    let root = env.home.root().to_owned();

    let search = gh::search_open(&root)?;
    let present: Vec<(String, i64)> = search.prs.iter().map(|l| (l.pr.repo.clone(), l.pr.number)).collect();
    let owned = env.store.read(pulls::owned_open)?;

    // The owned pull requests that left the open search: merged, closed, or the index lags (D3).
    // One stored as decided (`closed`, A11) is already right: it is not looked up again until the search lists it.
    let mut absent: Vec<(String, i64)> = Vec::new();
    for (_, _, row) in owned.iter().filter(|(_, _, row)| row.state == "open") {
        let key = (row.repo.clone(), row.number);
        if !present.contains(&key) && !absent.contains(&key) {
            absent.push(key);
        }
    }
    let looked_up = gh::lookup(&root, &absent)?;
    let mut seen = search.prs.clone();
    // `OPEN` is "not decided yet", `None` a pull request GitHub cannot find: nothing changes.
    seen.extend(looked_up.into_iter().flatten().filter(|p| p.state != PrState::Open).map(|pr| Listed { pr, membership: Default::default() }));

    let dirty = env.store.read(|conn| {
        for listed in &seen {
            match pulls::find(conn, &listed.pr.repo, listed.pr.number)? {
                Some(row) if row.holds(listed)? => {}
                _ => return Ok(true),
            }
        }
        let mut stmt = conn
            .prepare("SELECT repo, number FROM pull_requests WHERE NOT EXISTS (SELECT 1 FROM prs WHERE prs.pull_request = pull_requests.id)")?;
        let unowned = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(unowned.iter().any(|row| !present.contains(row)))
    })?;
    // A poll that finds nothing new opens no write transaction (D5).
    let outcome = if dirty { env.store.write(COMMAND, |tx| apply(tx, &seen, &present, &owned))? } else { Outcome::default() };

    // After the transaction: the clean-up of each PR that merged, then the silent retry of the earlier ones.
    clean_up(&mut env, &outcome.merged)?;
    let printed_prs = match &selected {
        Some(id) => {
            let plan = env.plan(id)?;
            Some(plan_prs(&mut env, &plan)?)
        }
        None => None,
    };

    let pair = |(plan, pr): &(PlanId, PrKey)| json!({"plan": plan, "pr": pr});
    let mut out = json!({
        "polled": search.prs.len(),
        "changed": outcome.changed,
        "removed": outcome.removed,
        "feedback": outcome.feedback.iter().map(pair).collect::<Vec<_>>(),
        "merged": outcome.merged.iter().map(pair).collect::<Vec<_>>(),
        "done": outcome.done,
        "rate_limit": {"cost": search.rate.cost, "remaining": search.rate.remaining, "reset_at": search.rate.reset_at},
    });
    if let Some(prs) = printed_prs {
        out["prs"] = Value::Array(prs);
    }
    Ok(Output::success(out))
}

/// `flock` on `path`, waiting at most [`LOCK_WAIT`]: `lock_timeout` (exit 5). The
/// descriptor stays open while waiting.
fn take_lock(path: &Path) -> Result<Flock<fs::File>, CpdError> {
    let failed = |what: &dyn std::fmt::Display| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the lock {}: {what}.", path.display())).with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(|e| failed(&e))?;
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(lock) => return Ok(lock),
            Err((back, Errno::EWOULDBLOCK | Errno::EINTR)) => file = back,
            Err((_, errno)) => return Err(failed(&format!("flock failed: {errno}"))),
        }
        if Instant::now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::LockTimeout,
                format!("Another pr poll held {} for longer than {}s.", path.display(), LOCK_WAIT.as_secs()),
            )
            .with("lock", path.display().to_string())
            .with("waited_seconds", LOCK_WAIT.as_secs()));
        }
        std::thread::sleep(RETRY);
    }
}

/// The one write transaction (D5, D7): every changed row, the Events of the
/// owning Plans, the rows that left the search.
fn apply(tx: &WriteTxn<'_>, seen: &[Listed], present: &[(String, i64)], owned: &[Owned]) -> Result<Outcome, CpdError> {
    let mut out = Outcome::default();
    for listed in seen {
        // A merged row needs a merge time: GitHub's, else this poll's.
        let mut listed = listed.clone();
        if listed.pr.state == PrState::Merged && listed.pr.merged_at.is_none() {
            listed.pr.merged_at = Some(pulls::txn_time(tx)?);
        }
        let polled = &listed.pr;
        let Some(change) = pulls::upsert(tx, &listed)? else { continue };
        let after = &change.after;
        out.changed.push(json!({
            "repo": after.repo, "number": after.number, "state": after.state, "draft": after.draft,
            "checks": after.checks, "review_decision": after.review_decision,
            "mergeable": after.mergeable, "merge_state": after.merge_state,
        }));
        for (plan, key, _) in owned.iter().filter(|(_, _, row)| row.id == after.id) {
            if polled.state == PrState::Merged {
                out.merged.push((plan.clone(), key.clone()));
                if merge(tx, plan, key, after)? {
                    out.done.push(plan.clone());
                }
            } else if tell(tx, plan, key, &change)? {
                out.feedback.push((plan.clone(), key.clone()));
            }
        }
    }
    out.removed = pulls::delete_unowned_absent(tx, present)?;
    Ok(out)
}

/// D7: `pr-feedback` to the Plan, when the change needs its attention.
fn tell(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey, change: &Change) -> Result<bool, CpdError> {
    let reasons: Vec<Reason> = pulls::attention(change.before.as_ref(), &change.after);
    if reasons.is_empty() {
        return Ok(false);
    }
    let text = feedback::text(plan.as_str(), key.as_str(), change.after.number, &reasons);
    let data = feedback::data(key.as_str(), &change.after, &reasons);
    append(tx, NewEvent::new(Kind::PrFeedback, text, data).plan(plan.clone()))?;
    Ok(true)
}

/// The PR `key` of the Plan is merged on GitHub (§7.6): `prs` `merged`,
/// `pr-merged`, and, when it was the last PR, the Plan `done` with `plan-done`
/// (§3.1). Whether the Plan became `done`.
fn merge(tx: &WriteTxn<'_>, id: &PlanId, key: &PrKey, pull: &pulls::PullRequest) -> Result<bool, CpdError> {
    let moved = tx.execute(
        "UPDATE prs SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE plan = ?1 AND key = ?2 AND status = 'open'",
        [id.as_str(), key.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(id, key));
    }
    let text = format!("PR {key} (#{}) of Plan {id} is merged.", pull.number);
    let data = json!({"pr": key, "number": pull.number, "url": pull.url, "merged_at": pull.merged_at});
    append(tx, NewEvent::new(Kind::PrMerged, text, data).plan(id.clone()))?;
    // The last PR to merge ends the Plan (§3.1).
    let unmerged: i64 =
        tx.query_row("SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled')", [id.as_str()], |r| r.get(0))?;
    if unmerged > 0 {
        return Ok(false);
    }
    let moved = tx.execute(
        "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE id = ?1 AND phase = 'deploy'",
        [id.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(id, key));
    }
    let merged = merged_prs(tx, id, key, pull.merged_at.as_deref())?;
    let text = format!("Every PR of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}");
    append(tx, NewEvent::new(Kind::PlanDone, text, json!({"prs": merged})).plan(id.clone()))?;
    Ok(true)
}

/// `[{key, status, pull_request}]` of the Plan's PRs that are not `cancelled`, in key order.
fn plan_prs(env: &mut Env, plan: &PlanRow) -> Result<Vec<Value>, CpdError> {
    env.store.read(|conn| {
        plan.live()
            .map(|pr| {
                let row = pr.pull_request.map(|id| pulls::load(conn, id)).transpose()?;
                Ok(json!({"key": pr.key, "status": pr.status, "pull_request": row.map(|r| r.to_json())}))
            })
            .collect()
    })
}

/// `[{key, number, url, merged_at}]` of every PR that is not `cancelled`, in key
/// order: `merged_at` is the moment the PR was seen merged, and for `just`, the
/// one this poll merged, the time `gh` reports when it does.
fn merged_prs(tx: &WriteTxn<'_>, id: &PlanId, just: &PrKey, reported: Option<&str>) -> Result<Vec<serde_json::Value>, CpdError> {
    let mut stmt = tx.prepare("SELECT key, pr_number, pr_url, merged_at FROM prs WHERE plan = ?1 AND status = 'merged' ORDER BY key")?;
    let rows = stmt
        .query_map([id.as_str()], |r| {
            let key: String = r.get(0)?;
            let merged_at: String = r.get(3)?;
            let merged_at = if key == just.as_str() { reported.map_or(merged_at, str::to_owned) } else { merged_at };
            Ok(json!({"key": key, "number": r.get::<_, i64>(1)?, "url": r.get::<_, String>(2)?, "merged_at": merged_at}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn cas_lost(id: &PlanId, key: &PrKey) -> CpdError {
    CpdError::new(ErrorCode::CasLost, format!("PR {key} of Plan {id} changed while it was being polled: rerun the command."))
        .with("plan", id.as_str())
        .with("pr", key.as_str())
}

/// A merged PR with what its clean-up removes.
struct Merged {
    plan: PlanId,
    pr: PrKey,
    repo: String,
    lead_branch: String,
    lead_worktree: Option<String>,
    attempts: Vec<Attempt>,
}

/// An Attempt of one of the PR's Jobs.
struct Attempt {
    id: AttemptId,
    branch: String,
    worktree: String,
    ended: bool,
}

impl Merged {
    /// Whether a path this clean-up removes is still on disk: an Attempt or Lead
    /// worktree, or a gate worktree of the PR (contract §7.1).
    fn on_disk(&self) -> bool {
        self.lead_worktree.iter().chain(self.attempts.iter().map(|a| &a.worktree)).any(|p| Path::new(p).exists()) || self.has_gate_worktree()
    }

    /// Whether `<repo-parent>` holds an entry named as a gate worktree of this PR.
    fn has_gate_worktree(&self) -> bool {
        // The path of pid 0 ends in `gate-0`; without the `0` it is the name every gate worktree of the PR starts with.
        let Ok(path) = git::gate_worktree_path(Path::new(&self.repo), &self.plan, &self.pr, 0) else { return false };
        let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else { return false };
        let Some(prefix) = name.strip_suffix('0') else { return false };
        let Ok(entries) = fs::read_dir(dir) else { return false };
        entries.flatten().any(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(prefix)))
    }
}

/// A removal that failed or a Lead worktree that was kept: `(path, reason)`.
type Failure = (String, String);

/// §7.2 (hs-d138, hs-d142): the clean-up of every PR that `just` merged in this poll, which
/// kills its Attempt windows and appends one `error` Event `cleanup_failed` for each failure
/// or kept Lead worktree; then, with no Event, the retry of every other `merged` PR that
/// still has a path on disk. Best effort: only a failure to read or write the store fails it.
fn clean_up(env: &mut Env, just: &[(PlanId, PrKey)]) -> Result<(), CpdError> {
    let all = env.store.read(merged_prs_to_clean)?;
    let (fresh, earlier): (Vec<Merged>, Vec<Merged>) = all.into_iter().partition(|m| just.contains(&(m.plan.clone(), m.pr.clone())));
    for merged in earlier.iter().filter(|m| m.on_disk()) {
        clean_when_idle(&env.home, merged);
    }
    if fresh.is_empty() {
        return Ok(());
    }
    let tmux = Tmux::new(env.config.tmux.socket.as_str());
    let work = env.config.tmux.work_session.as_str();
    let panes = tmux.list_panes()?;
    let mut events = Vec::new();
    for merged in &fresh {
        let mut failures = Vec::new();
        for attempt in &merged.attempts {
            let name = attempt_window(&merged.plan, &attempt.id);
            for pane in panes.iter().filter(|p| p.session_name == work && p.window_name == name) {
                if let Err(e) = tmux.kill_window(&pane.window) {
                    failures.push((attempt.worktree.clone(), reason(&e)));
                }
            }
        }
        failures.extend(clean_when_idle(&env.home, merged));
        events.extend(failures.into_iter().map(|(path, reason)| (merged.plan.clone(), merged.pr.clone(), path, reason)));
    }
    if events.is_empty() {
        return Ok(());
    }
    env.store.write(COMMAND, |tx| {
        for (plan, pr, path, reason) in &events {
            let message = format!("The clean-up of the merged PR {pr} of Plan {plan} did not remove {path}: {reason}");
            let text = format!("{message} It is retried by every later poll, except a dirty or unpushed Lead worktree, which stays until you deal with it.");
            let details = json!({"plan": plan, "pr": pr, "path": path, "reason": reason});
            let data = json!({"source": COMMAND, "code": ErrorCode::CleanupFailed.as_str(), "message": message, "details": details});
            append(tx, NewEvent::new(Kind::Error, text, data).plan(plan.clone()))?;
        }
        Ok(())
    })
}

/// Every PR in status `merged`, with its Attempts.
fn merged_prs_to_clean(conn: &rusqlite::Connection) -> Result<Vec<Merged>, CpdError> {
    let mut prs = conn.prepare("SELECT plan, key, repo, lead_branch, lead_worktree FROM prs WHERE status = 'merged' ORDER BY plan, key")?;
    let rows = prs
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut attempts = conn.prepare(
        "SELECT a.id, a.branch, a.worktree, a.status = 'ended' FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE j.plan = ?1 AND j.pr = ?2 ORDER BY a.id",
    )?;
    let mut found = Vec::new();
    for (plan, pr, repo, lead_branch, lead_worktree) in rows {
        let list = attempts
            .query_map([&plan, &pr], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(id, branch, worktree, ended)| Ok(Attempt { id: AttemptId::parse(&id)?, branch, worktree, ended }))
            .collect::<Result<Vec<_>, CpdError>>()?;
        found.push(Merged { plan: PlanId::parse(&plan)?, pr: PrKey::parse(&pr)?, repo, lead_branch, lead_worktree, attempts: list });
    }
    Ok(found)
}

/// [`clean`] once the PR is idle (contract §7.2, hs-d243, hs-n124): every Attempt of its
/// Jobs is `ended` and its driver lock is free, probed without creating the file, since
/// a driver that just ended its Attempt may still be in A9, removing its worktree. While
/// it is not, nothing is removed and there is no failure: the PR is still on disk, so a
/// later poll cleans it up. A driver lock that cannot be probed is a failed removal.
fn clean_when_idle(home: &CpdHome, merged: &Merged) -> Vec<Failure> {
    for attempt in &merged.attempts {
        if !attempt.ended {
            return Vec::new();
        }
        match lock::probe(home, &merged.plan, &attempt.id) {
            Ok(Some(_free)) => {}
            Ok(None) => return Vec::new(),
            Err(e) => return vec![(attempt.worktree.clone(), reason(&e))],
        }
    }
    clean(merged)
}

/// Removes the Attempt worktrees of the PR that are on disk, then its gate worktrees whose
/// pid is dead, then its Lead worktree, first brought up to date by rule S. Each removal
/// runs in the PR's repo, so the order does not matter; a failure does not stop the others.
/// What it did not remove.
fn clean(merged: &Merged) -> Vec<Failure> {
    let repo = Path::new(&merged.repo);
    let mut failures = Vec::new();
    for attempt in &merged.attempts {
        if Path::new(&attempt.worktree).exists()
            && let Err(e) = git::discard_worktree(repo, Path::new(&attempt.worktree), &attempt.branch)
        {
            failures.push((attempt.worktree.clone(), reason(&e)));
        }
    }
    // A gate worktree whose pid lives belongs to a Gate still running: it stays (§7.2).
    if merged.has_gate_worktree()
        && let Err(e) = git::sweep_gate_worktrees(repo, &merged.plan, &merged.pr)
    {
        failures.push((merged.repo.clone(), reason(&e)));
    }
    if let Some(lead) = &merged.lead_worktree {
        // Rule S first, so a tree a merge left stale is not kept as dirty. Every stop of it is
        // `lead_worktree_stale` and leaves the worktree as it was: the check below keeps such a
        // worktree and reports it as `cleanup_failed`, never as `lead_worktree_stale` (§7.2).
        if Path::new(lead).exists() {
            match git::sync_lead(Path::new(lead), &merged.lead_branch) {
                Ok(_) => {}
                Err(e) if e.code() == ErrorCode::LeadWorktreeStale => {}
                Err(e) => {
                    failures.push((lead.clone(), reason(&e)));
                    return failures;
                }
            }
        }
        match git::remove_merged_lead(repo, Path::new(lead), &merged.lead_branch) {
            Ok(git::LeadRemoval::Removed) => {}
            Ok(git::LeadRemoval::Kept(why)) => failures.push((lead.clone(), why.as_str().to_owned())),
            Err(e) => failures.push((lead.clone(), reason(&e))),
        }
    }
    failures
}

/// git's stderr tail when the error has one, else its message.
fn reason(error: &CpdError) -> String {
    match error.details().get("stderr").and_then(Value::as_str) {
        Some(stderr) if !stderr.trim().is_empty() => stderr.trim().to_owned(),
        _ => error.message().to_owned(),
    }
}
