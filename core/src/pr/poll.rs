//! `pr poll` (commands.md §9.2, contract §7.6): each open PR's state, stored in
//! its `pull_requests` row, and its merge.

use std::path::Path;

use serde_json::json;

use super::feedback;
use super::gh::{self, PolledPr};
use super::pulls::{self, Reason};
use super::row::{PlanRow, PrRow};
use super::status::{ChecksSummary, PrState, ReviewDecision};
use super::{Args, Env, illegal_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git;
use crate::host::layout::attempt_window;
use crate::host::tmux::Tmux;
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
use crate::store::WriteTxn;

const COMMAND: &str = "pr poll";

pub(super) fn poll(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let plan = env.plan(&id)?;
    match plan.phase {
        Phase::Dispatch | Phase::Pr => {}
        Phase::Done => {
            // Rerun the clean-up, in case the run that ended the Plan died before finishing it.
            clean_up(&mut env, &plan)?;
            return Ok(Output::success(json!({"plan": id, "phase": "done", "prs": [], "already": true})));
        }
        _ => return Err(illegal_phase(&plan, COMMAND, "dispatch or pr")),
    }
    // With --pr, that one; without it, every `open` PR in key order (§9.2).
    let keys: Vec<PrKey> = match &args.pr.pr {
        Some(_) => {
            let pr = plan.select(&args.pr)?;
            match pr.status.as_str() {
                "open" => vec![pr.key.clone()],
                "merged" => return Ok(Output::success(json!({"plan": id, "phase": plan.phase.as_str(), "prs": [], "already": true}))),
                _ => return Err(pr.refuse(&id, COMMAND, "open")),
            }
        }
        None => plan.prs.iter().filter(|p| p.status == "open").map(|p| p.key.clone()).collect(),
    };
    let mut polled = Vec::new();
    for key in &keys {
        let pr = plan.prs.iter().find(|p| p.key == *key).expect("a key taken from the Plan's own PRs");
        polled.push(poll_pr(&mut env, &plan, pr)?);
    }
    let after = env.plan(&id)?;
    if after.phase == Phase::Done {
        clean_up(&mut env, &after)?;
    }
    Ok(Output::success(json!({"plan": id, "phase": after.phase.as_str(), "prs": polled})))
}

/// Polls one `open` PR (§7.6): what `gh` says, stored in its `pull_requests`
/// row when a value changed (and only then: a poll that finds nothing new opens
/// no write transaction), and `pr-feedback` when D7's rules say so.
fn poll_pr(env: &mut Env, plan: &PlanRow, pr: &PrRow) -> Result<serde_json::Value, CpdError> {
    let id = &plan.id;
    let key = &pr.key;
    let pull = pr.pull.clone().expect("an open PR has its pull request (prs CHECK)");
    let row_id = pr.pull_request.expect("an open PR has its pull_requests row (prs CHECK)");
    let lead = pr.lead(id)?.clone();

    let view = gh::view(&lead, pull.number)?;
    let state = PrState::of_graphql(&view.state)?;
    let decision = ReviewDecision::of(view.review_decision.as_deref());
    let checks = ChecksSummary::of(&feedback::current_checks(&view));
    let stored = env.store.read(|conn| pulls::load(conn, row_id))?;
    // What the row would hold: `gh pr view` has no `isDraft` or `createdAt`, so those stay as stored.
    let polled = |merged_at: Option<String>| -> Result<PolledPr, CpdError> {
        Ok(PolledPr { title: view.title.clone(), state, review_decision: decision, checks, merged_at, ..stored.polled()? })
    };
    let printed = |reasons: &[Reason], merged: bool| {
        json!({
            "pr": key, "number": pull.number, "state": view.state, "merged": merged, "title": view.title,
            "review_decision": decision.as_str(), "checks": checks.as_str(),
            "reasons": reasons.iter().map(|r| r.as_str()).collect::<Vec<_>>(),
        })
    };
    if state == PrState::Merged {
        env.store.write(COMMAND, |tx| {
            let moved = tx.execute(
                "UPDATE prs SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn())
                  WHERE plan = ?1 AND key = ?2 AND status = 'open'",
                [id.as_str(), key.as_str()],
            )?;
            if moved == 0 {
                return Err(cas_lost(id, key));
            }
            // A merged row needs a merge time: GitHub's, else this poll's.
            let merged_at = match &view.merged_at {
                Some(at) => at.clone(),
                None => tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get(0))?,
            };
            pulls::upsert(tx, &polled(Some(merged_at))?)?;
            let text = format!("PR {key} (#{}) of Plan {id} is merged.", pull.number);
            let data = json!({"pr": key, "number": pull.number, "url": pull.url, "merged_at": view.merged_at});
            append(tx, NewEvent::new(Kind::PrMerged, text, data).plan(id.clone()))?;
            // The last PR to merge ends the Plan (§3.1).
            let unmerged: i64 =
                tx.query_row("SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled')", [id.as_str()], |r| r.get(0))?;
            if unmerged > 0 {
                return Ok(());
            }
            let moved = tx.execute(
                "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn())
                  WHERE id = ?1 AND phase = 'pr'",
                [id.as_str()],
            )?;
            if moved == 0 {
                return Err(cas_lost(id, key));
            }
            let merged = merged_prs(tx, id, key, view.merged_at.as_deref())?;
            let text = format!("Every PR of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}");
            append(tx, NewEvent::new(Kind::PlanDone, text, json!({"prs": merged})).plan(id.clone()))?;
            Ok(())
        })?;
        return Ok(printed(&[], true));
    }

    let polled = polled(None)?;
    if stored.polled()? == polled {
        return Ok(printed(&[], false));
    }
    let reasons = env.store.write(COMMAND, |tx| {
        let Some(change) = pulls::upsert(tx, &polled)? else { return Ok(Vec::new()) };
        let reasons = pulls::attention(change.before.as_ref(), &change.after);
        if !reasons.is_empty() {
            let text = feedback::text(id.as_str(), key.as_str(), pull.number, &reasons);
            let data = feedback::data(key.as_str(), &change.after, &reasons);
            append(tx, NewEvent::new(Kind::PrFeedback, text, data).plan(id.clone()))?;
        }
        Ok(reasons)
    })?;
    Ok(printed(&reasons, false))
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
