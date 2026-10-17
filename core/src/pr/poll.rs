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
use super::gh::{self, PolledPr};
use super::pulls::{self, Change, Reason};
use super::row::PlanRow;
use super::status::PrState;
use super::{Env, PollArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git;
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
    let present: Vec<(String, i64)> = search.prs.iter().map(|p| (p.repo.clone(), p.number)).collect();
    let owned = env.store.read(pulls::owned_open)?;

    // The owned pull requests that left the open search: merged, closed, or the index lags (D3).
    let mut absent: Vec<(String, i64)> = Vec::new();
    for (_, _, row) in &owned {
        let key = (row.repo.clone(), row.number);
        if !present.contains(&key) && !absent.contains(&key) {
            absent.push(key);
        }
    }
    let looked_up = gh::lookup(&root, &absent)?;
    let mut seen = search.prs.clone();
    // `OPEN` is "not decided yet", `None` a pull request GitHub cannot find: nothing changes.
    seen.extend(looked_up.into_iter().flatten().filter(|p| p.state != PrState::Open));

    let dirty = env.store.read(|conn| {
        for polled in &seen {
            match pulls::find(conn, &polled.repo, polled.number)? {
                Some(row) if row.polled()? == *polled => {}
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

    // After the transaction: the clean-up of each Plan that became `done`, and of the selected one when it already was.
    for id in &outcome.done {
        let plan = env.plan(id)?;
        clean_up(&mut env, &plan)?;
    }
    let mut printed_prs = None;
    if let Some(id) = &selected {
        let plan = env.plan(id)?;
        if plan.phase == crate::plan::Phase::Done && !outcome.done.contains(id) {
            // Rerun the clean-up, in case the run that ended the Plan died before finishing it.
            clean_up(&mut env, &plan)?;
        }
        printed_prs = Some(plan_prs(&mut env, &plan)?);
    }

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
fn apply(tx: &WriteTxn<'_>, seen: &[PolledPr], present: &[(String, i64)], owned: &[Owned]) -> Result<Outcome, CpdError> {
    let mut out = Outcome::default();
    for polled in seen {
        // A merged row needs a merge time: GitHub's, else this poll's.
        let mut polled = polled.clone();
        if polled.state == PrState::Merged && polled.merged_at.is_none() {
            polled.merged_at = Some(pulls::txn_time(tx)?);
        }
        let Some(change) = pulls::upsert(tx, &polled)? else { continue };
        let after = &change.after;
        out.changed.push(json!({
            "repo": after.repo, "number": after.number, "state": after.state, "draft": after.draft,
            "checks": after.checks, "review_decision": after.review_decision,
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

/// After `done` (§3.4 A9): the Plan's remaining Attempt worktrees and branches,
/// in every PR's repo, are removed, kept so far for debugging, and its Attempt
/// windows killed. The Lead worktrees and the Lead Branches stay. Idempotent.
fn clean_up(env: &mut Env, plan: &PlanRow) -> Result<(), CpdError> {
    // Each Attempt with its PR's Lead worktree, from which git addresses that repo.
    let attempts = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.id, a.branch, a.worktree, r.lead_worktree
               FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job JOIN prs r ON r.plan = j.plan AND r.key = j.pr
              WHERE a.plan = ?1 ORDER BY a.id",
        )?;
        let rows = stmt
            .query_map([plan.id.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    let tmux = Tmux::new(env.config.tmux.socket.as_str());
    let work = env.config.tmux.work_session.as_str();
    let panes = tmux.list_panes()?;
    for (id, branch, worktree, lead) in attempts {
        let attempt = AttemptId::parse(&id)?;
        let lead = lead.ok_or_else(|| {
            CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: the PR of Attempt {id} has no Lead worktree."))
        })?;
        let name = attempt_window(&plan.id, &attempt);
        for pane in panes.iter().filter(|p| p.session_name == work && p.window_name == name) {
            tmux.kill_window(&pane.window)?;
        }
        git::discard_worktree(Path::new(&lead), Path::new(&worktree), &branch)?;
    }
    Ok(())
}
