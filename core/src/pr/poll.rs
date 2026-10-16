//! `pr poll` (commands.md §9.2, contract §7.6): the PR's feedback and merge,
//! compared with `pr_seen`.

use std::path::Path;

use serde_json::json;

use super::feedback::{self, Changes, ChecksSummary, ReviewDecision, Seen};
use super::row::PlanRow;
use super::{Args, Env, gh, illegal_phase};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git;
use crate::host::layout::attempt_window;
use crate::host::tmux::Tmux;
use crate::ids::{AttemptId, PlanId};
use crate::output::Output;
use crate::plan::Phase;
use crate::store::WriteTxn;

const COMMAND: &str = "pr poll";

pub(super) fn poll(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;
    let plan = env.plan(&id)?;
    match plan.phase {
        Phase::Pr => {}
        Phase::Done => {
            // Rerun the clean-up, in case the run that ended the Plan died before finishing it.
            clean_up(&mut env, &plan)?;
            return Ok(Output::success(json!({"plan": id, "phase": "done", "merged": true, "already": true})));
        }
        _ => return Err(illegal_phase(&plan, COMMAND, "pr")),
    }
    let pr = plan.pr.clone().expect("a Plan in phase pr has an open PR (prs CHECK)");
    let lead = plan.lead()?.clone();
    let seen = Seen::parse(&plan.pr_seen)?;

    let view = gh::view(&lead, pr.number)?;
    // Stored on every poll, together, with `pr_polled_at`, and printed (contract §7.6).
    let decision = ReviewDecision::of(view.review_decision.as_deref());
    let checks = ChecksSummary::of(&feedback::current_checks(&view));
    let stored = |tx: &WriteTxn<'_>| -> rusqlite::Result<()> {
        tx.execute(
            "UPDATE prs SET pr_title = ?1, pr_review_decision = ?2, pr_checks = ?3 WHERE plan = ?4 AND key = ?5",
            rusqlite::params![view.title, decision.as_str(), checks.as_str(), id.as_str(), plan.key.as_str()],
        )?;
        Ok(())
    };
    let printed = |mut out: serde_json::Value| {
        out["title"] = json!(view.title);
        out["review_decision"] = json!(decision.as_str());
        out["checks"] = json!(checks.as_str());
        out
    };
    if view.state == "MERGED" {
        env.store.write(COMMAND, |tx| {
            let moved = tx.execute(
                "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn())
                  WHERE id = ?1 AND phase = 'pr'",
                [id.as_str()],
            )?;
            if moved == 0 {
                return Err(cas_lost(&id));
            }
            let moved = tx.execute(
                "UPDATE prs SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn()),
                        pr_polled_at = (SELECT at FROM txns WHERE id = cpd_txn())
                  WHERE plan = ?1 AND key = ?2 AND status = 'open'",
                [id.as_str(), plan.key.as_str()],
            )?;
            if moved == 0 {
                return Err(cas_lost(&id));
            }
            stored(tx)?;
            let text = format!("PR #{} of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}", pr.number);
            let data = json!({"pr": pr.short(), "merged_at": view.merged_at});
            append(tx, NewEvent::new(Kind::PlanDone, text, data).plan(id.clone()))?;
            Ok(())
        })?;
        clean_up(&mut env, &plan)?;
        return Ok(Output::success(printed(json!({
            "plan": id, "state": view.state, "merged": true, "changes": Changes::default().to_json(),
        }))));
    }

    let inline = gh::inline_comments(&lead, pr.number)?;
    let (changes, mut next) = feedback::diff(&seen, &view, &inline);
    let closed = view.state == "CLOSED";
    // The closure is reported once; the mark goes when the PR is open again.
    let closure = closed && seen.closed != Some(true);
    next.closed = closed.then_some(true);

    env.store.write(COMMAND, |tx| {
        let moved = tx.execute(
            "UPDATE prs SET pr_seen = ?1, pr_polled_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE plan = ?2 AND key = ?3 AND status = 'open' AND pr_seen = ?4
                AND (SELECT phase FROM plans WHERE id = ?2) = 'pr'",
            rusqlite::params![next.to_text(), id.as_str(), plan.key.as_str(), plan.pr_seen],
        )?;
        if moved == 0 {
            return Err(cas_lost(&id));
        }
        stored(tx)?;
        if !changes.is_empty() || closure {
            let text = feedback::text(id.as_str(), pr.number, &changes, closure);
            let data = feedback::data(&changes, &view.state);
            append(tx, NewEvent::new(Kind::PrFeedback, text, data).plan(id.clone()))?;
        }
        Ok(())
    })?;
    Ok(Output::success(printed(json!({"plan": id, "state": view.state, "merged": false, "changes": changes.to_json()}))))
}

fn cas_lost(id: &PlanId) -> CpdError {
    CpdError::new(ErrorCode::CasLost, format!("Plan {id} changed while it was being polled: rerun the command."))
        .with("plan", id.as_str())
}

/// After `done` (§3.4 A9): the Plan's remaining Attempt worktrees and branches
/// are removed, kept so far for debugging, and its Attempt windows killed. The
/// Lead worktree and the Lead Branch stay. Idempotent.
fn clean_up(env: &mut Env, plan: &PlanRow) -> Result<(), CpdError> {
    let attempts = env.store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id, branch, worktree FROM attempts WHERE plan = ?1 ORDER BY id")?;
        let rows = stmt
            .query_map([plan.id.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    let tmux = Tmux::new(env.config.tmux.socket.as_str());
    let work = env.config.tmux.work_session.as_str();
    let panes = tmux.list_panes()?;
    let lead = plan.lead()?;
    for (id, branch, worktree) in attempts {
        let attempt = AttemptId::parse(&id)?;
        let name = attempt_window(&plan.id, &attempt);
        for pane in panes.iter().filter(|p| p.session_name == work && p.window_name == name) {
            tmux.kill_window(&pane.window)?;
        }
        git::discard_worktree(lead, Path::new(&worktree), &branch)?;
    }
    Ok(())
}
