//! `pr cancel` (commands.md §9.6, contract §3.8): `waiting → cancelled` before
//! the lock; after it, a `waiting` or `dispatch` PR whose Jobs are all
//! `cancelled` (or that has none) too; and an `open` PR whose pull request is
//! stored `closed` (hs-d329). The PR keeps its key; before the lock the plan
//! fingerprint changes, after it the Plan may move on. With `--superseded-by <plan>/<key>` a `dispatch` PR whose
//! `merged` Jobs were carried by another PR is cancelled too, the Event naming that PR (hs-d301).

use rusqlite::Connection;
use serde_json::json;

use super::add::{needs_refs, node_of};
use super::open::{merged_prs, opened_prs};
use super::{CancelArgs, Env, find_pr, plan_phase, pulls};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::fingerprint::load_prs;
use crate::plan::PrRow;
use crate::plan::jobs;
use crate::store::WriteTxn;

const COMMAND: &str = "pr cancel";

/// The pull request, Lead worktree and Lead Branch an `open` PR gives up (the output's `released`).
struct Released {
    number: i64,
    url: String,
    lead_worktree: String,
    lead_branch: String,
}

enum Decision {
    /// Cancel it from `from` (`waiting`, `dispatch` or `open`); `released` only for `open`.
    Cancel { from: &'static str, released: Option<Released>, superseded_by: Option<String> },
    /// It was already `cancelled`.
    Already,
}

pub(super) fn cancel(args: CancelArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    // The superseding PR as a reference of the Plan (§2.2): the bare key in the Plan itself, else `<plan>/<key>`.
    let superseded = args.superseded_by.map(|r| Ok::<_, CpdError>(needs_refs(&plan, &[r])?.remove(0))).transpose()?;
    let superseded = superseded.as_deref();
    let mut env = Env::open()?;
    let mut moved_to = None;
    let mut released = None;
    let already = match env.store.read(|conn| decide(conn, &plan, &key, superseded))? {
        Decision::Already => true,
        Decision::Cancel { .. } => env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, superseded)? {
            Decision::Already => Ok(true),
            Decision::Cancel { from, released: gave, superseded_by } => {
                // `prs` ties the Lead worktree, the start time and the pull request to the status: a `cancelled` PR has
                // none (the worktree on disk stays, §3.8). They are null already but for the status's own.
                let moved = tx.execute(
                    "UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL,
                                    pr_number = NULL, pr_url = NULL, pr_opened_at = NULL, pull_request = NULL
                      WHERE plan = ?1 AND key = ?2 AND status = ?3",
                    [plan.as_str(), key.as_str(), from],
                )?;
                if moved == 0 {
                    return Err(CpdError::new(ErrorCode::CasLost, format!("PR {key} changed while it was cancelled: rerun the command."))
                        .with("plan", plan.as_str())
                        .with("pr", key.as_str()));
                }
                let text = match (&gave, &superseded_by) {
                    (None, Some(by)) => format!(
                        "PR {key} of Plan {plan} was cancelled: its work was superseded by PR {by}. cpd removes nothing; remove its Lead worktree \
                         and Lead Branch by hand. Next: cpd plan show --plan {plan}"
                    ),
                    (None, None) => format!("PR {key} of Plan {plan} was cancelled. Next: cpd plan show --plan {plan}"),
                    (Some(r), _) => format!(
                        "PR {key} of Plan {plan} was cancelled: its pull request #{} ({}) was closed without a merge. cpd removes nothing; \
                         remove the Lead worktree {} and the Lead Branch {} by hand. Next: cpd plan show --plan {plan}",
                        r.number, r.url, r.lead_worktree, r.lead_branch
                    ),
                };
                let event = match &superseded_by {
                    Some(by) => NewEvent::pr_cancelled_superseded(text, &key, by),
                    None => NewEvent::pr_cancelled(text, &key),
                };
                append(tx, event.plan(plan.clone()))?;
                moved_to = if gave.is_some() { finish(tx, &plan, &key)? } else { advance(tx, &plan, &key)? };
                released = gave;
                Ok(false)
            }
        })?,
    };
    let mut out = json!({"pr": {"key": key, "status": "cancelled"}});
    if already {
        out["already"] = json!(true);
    }
    if let Some(phase) = moved_to {
        out["phase"] = json!(phase.as_str());
    }
    if let Some(r) = released {
        out["released"] = json!({"number": r.number, "url": r.url, "lead_worktree": r.lead_worktree, "lead_branch": r.lead_branch});
    }
    Ok(Output::success(out))
}

/// After the lock, the cancelled PR may have been the last one `waiting` or `dispatch`: the Plan then moves on as
/// `pr open` and `pr poll` move it (§3.1): `dispatch → deploy`, and `deploy → done` when every PR left is `merged`.
/// Never when no PR is left that is not `cancelled`. The Plan's new phase, if it moved.
fn advance(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey) -> Result<Option<Phase>, CpdError> {
    if plan_phase(tx, plan)? != Phase::Dispatch {
        return Ok(None);
    }
    let count = |statuses: &str| -> Result<i64, CpdError> {
        Ok(tx.query_row(&format!("SELECT count(*) FROM prs WHERE plan = ?1 AND status {statuses}"), [plan.as_str()], |r| r.get(0))?)
    };
    if count("IN ('waiting', 'dispatch')")? > 0 || count("<> 'cancelled'")? == 0 {
        return Ok(None);
    }
    let lost = || {
        CpdError::new(ErrorCode::CasLost, format!("Plan {plan} left phase dispatch while PR {key} was cancelled: rerun the command."))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
    };
    if tx.execute("UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'", [plan.as_str()])? == 0 {
        return Err(lost());
    }
    let text = format!("Every PR of Plan {plan} that is not cancelled is open or merged. The engine polls them; to follow them by hand: cpd pr poll --plan {plan}");
    append(tx, NewEvent::plan_phase_deployed(text, opened_prs(tx, plan)?).plan(plan.clone()))?;
    Ok(Some(finish(tx, plan, key)?.unwrap_or(Phase::Deploy)))
}

/// In phase `deploy`, when every PR that is not `cancelled` is `merged` and one is: `deploy → done` with `plan-done`
/// (§3.1). The Plan's new phase, if it moved. Never in another phase: in `dispatch` PRs remain to start or open.
fn finish(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey) -> Result<Option<Phase>, CpdError> {
    if plan_phase(tx, plan)? != Phase::Deploy {
        return Ok(None);
    }
    let count = |statuses: &str| -> Result<i64, CpdError> {
        Ok(tx.query_row(&format!("SELECT count(*) FROM prs WHERE plan = ?1 AND status {statuses}"), [plan.as_str()], |r| r.get(0))?)
    };
    if count("NOT IN ('merged', 'cancelled')")? > 0 || count("= 'merged'")? == 0 {
        return Ok(None);
    }
    let done = tx.execute(
        "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND phase = 'deploy'",
        [plan.as_str()],
    )?;
    if done == 0 {
        return Err(CpdError::new(ErrorCode::CasLost, format!("Plan {plan} left phase deploy while PR {key} was cancelled: rerun the command."))
            .with("plan", plan.as_str())
            .with("pr", key.as_str()));
    }
    let text = format!("Every PR of Plan {plan} is merged: the Plan is done. See cpd plan show --plan {plan}");
    append(tx, NewEvent::plan_done(text, merged_prs(tx, plan)?).plan(plan.clone()))?;
    Ok(Some(Phase::Done))
}

fn decide(conn: &Connection, plan: &PlanId, key: &PrKey, superseded: Option<&str>) -> Result<Decision, CpdError> {
    let prs = load_prs(conn, plan)?;
    let pr = find_pr(&prs, plan, key)?;
    let phase = plan_phase(conn, plan)?;
    let locked = !matches!(phase, Phase::Grounding | Phase::Plan);
    let mut needing: Vec<String> =
        prs.iter().filter(|p| p.status != "cancelled" && p.needs.iter().any(|n| n == key.as_str())).map(|p| p.key.to_string()).collect();
    // A base PR cannot be cancelled while a PR is stacked on it (contract §3.8 "Stacked PRs" step 9).
    needing.extend(stacked_on(conn, plan, pr)?);
    let in_use = |jobs: Vec<String>| {
        CpdError::new(
            ErrorCode::PrInUse,
            format!("PR {key} is in use: cancel its Jobs (cpd job cancel) and edit away the needs and the PRs stacked on it (cpd pr edit) first."),
        )
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
        .with("jobs", jobs)
        .with("prs", needing.clone())
    };
    let from = match (pr.status.as_str(), locked) {
        ("cancelled", _) => return Ok(Decision::Already),
        (status, _) if superseded.is_some() && !(status == "dispatch" && locked) => {
            return Err(CpdError::new(
                ErrorCode::IllegalTransition,
                format!("PR {key} is {status}: --superseded-by cancels only a dispatch PR, after the lock."),
            )
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("from", status)
            .with("to", "cancelled"));
        }
        ("waiting", _) => "waiting",
        ("dispatch", true) => "dispatch",
        ("open", true) if matches!(phase, Phase::Dispatch | Phase::Deploy) => return decide_open(conn, plan, key, pr, &needing),
        (status, _) => {
            let only = match (locked, status) {
                (false, _) => "only a waiting PR is cancelled",
                (true, "open") => "an open PR is cancelled only in phase dispatch or deploy",
                (true, _) => "only a waiting, dispatch or open PR is cancelled",
            };
            return Err(CpdError::new(ErrorCode::IllegalTransition, format!("PR {key} is {status}: {only}."))
                .with("plan", plan.as_str())
                .with("pr", key.as_str())
                .with("from", status)
                .with("to", "cancelled"));
        }
    };
    // A superseded PR's `merged` Jobs are work another PR carried: only a live Job holds it.
    let holds = |status: &str| status != "cancelled" && !(superseded.is_some() && status == "merged");
    let live_jobs: Vec<String> =
        jobs::load(conn, plan)?.into_iter().filter(|j| j.pr == key.as_str() && holds(&j.status)).map(|j| j.key).collect();
    if !live_jobs.is_empty() || !needing.is_empty() {
        return Err(in_use(live_jobs));
    }
    if let Some(by) = superseded {
        check_superseding(conn, plan, key, by)?;
    }
    Ok(Decision::Cancel { from, released: None, superseded_by: superseded.map(str::to_owned) })
}

/// The superseding PR `by` (a reference of `plan`, §2.2): a PR of a Plan of the same Context (`pr_not_found`,
/// `pr_other_context`), not `key` itself (`usage`), not `cancelled` (`illegal_transition`). Any other status will do:
/// cpd cannot know where the work went (contract §3.8).
fn check_superseding(conn: &Connection, plan: &PlanId, key: &PrKey, by: &str) -> Result<(), CpdError> {
    let (by_plan, by_key) = node_of(plan, by);
    if by_plan == plan.as_str() && by_key == key.as_str() {
        return Err(CpdError::new(ErrorCode::Usage, format!("--superseded-by names PR {key} itself.")).with("pr", key.as_str()));
    }
    let found: Option<(String, String)> = conn
        .query_row(
            "SELECT r.status, p.context FROM prs r JOIN plans p ON p.id = r.plan WHERE r.plan = ?1 AND r.key = ?2",
            [by_plan.as_str(), by_key.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;
    let Some((status, context)) = found else {
        return Err(CpdError::new(ErrorCode::PrNotFound, format!("No PR {by_key} in Plan {by_plan} to be superseded by."))
            .with("plan", by_plan.as_str())
            .with("pr", by_key.as_str()));
    };
    let own: String = conn.query_row("SELECT context FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?;
    if context != own {
        return Err(CpdError::new(
            ErrorCode::PrOtherContext,
            format!("PR {by_plan}/{by_key} is in Context {context}, not in {own}: PRs reference only PRs of their Context."),
        )
        .with("pr", format!("{by_plan}/{by_key}"))
        .with("context", own.as_str())
        .with("other_context", context.as_str()));
    }
    if status == "cancelled" {
        return Err(CpdError::new(ErrorCode::IllegalTransition, format!("PR {by_plan}/{by_key} is cancelled: its work cannot supersede PR {key}'s."))
            .with("plan", by_plan.as_str())
            .with("pr", by_key.as_str())
            .with("from", "cancelled")
            .with("to", "superseding"));
    }
    Ok(())
}

/// The PRs that are not `cancelled` and stacked on `pr`, of any Plan: those of its repo whose `base` is its Lead Branch
/// (§2.2 "The base PR"), as a bare key in its own Plan and `<plan>/<key>` in another.
fn stacked_on(conn: &Connection, plan: &PlanId, pr: &PrRow) -> Result<Vec<String>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT plan, key FROM prs WHERE repo = ?1 AND base = ?2 AND status <> 'cancelled' AND NOT (plan = ?3 AND key = ?4) ORDER BY plan, key",
    )?;
    let found = stmt
        .query_map([pr.repo.as_str(), pr.lead_branch.as_str(), plan.as_str(), pr.key.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(found.into_iter().map(|(p, k)| if p == plan.as_str() { k } else { format!("{p}/{k}") }).collect())
}

/// An `open` PR: its pull request must be stored `closed` (`pr_not_closed`, no `gh` call: the row is what the last poll
/// wrote) and no PR that is not `cancelled` may need it (`pr_in_use`). Its Jobs are not checked.
fn decide_open(conn: &Connection, plan: &PlanId, key: &PrKey, pr: &PrRow, needing: &[String]) -> Result<Decision, CpdError> {
    let (Some(id), Some(pull), Some(worktree)) = (pr.pull_request, &pr.pull, &pr.lead_worktree) else {
        return Err(CpdError::bad_row(format!("PR {key} of Plan {plan} is open without a pull request or a Lead worktree")));
    };
    let state = pulls::load(conn, id)?.state;
    if state != "closed" {
        return Err(CpdError::new(
            ErrorCode::PrNotClosed,
            format!("PR {key}'s pull request #{} is {state} as the last poll stored it: only a closed one is given up (run cpd pr poll first).", pull.number),
        )
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
        .with("number", pull.number)
        .with("url", pull.url.as_str())
        .with("state", state));
    }
    if !needing.is_empty() {
        return Err(CpdError::new(
            ErrorCode::PrInUse,
            format!("PR {key} is in use: PRs that need it, or are stacked on it, would wait for a merge that cannot come; edit away the needs and the stacked PRs on it (cpd pr edit) first."),
        )
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
        .with("jobs", Vec::<String>::new())
        .with("prs", needing.to_vec()));
    }
    let released = Released {
        number: pull.number,
        url: pull.url.clone(),
        lead_worktree: worktree.display().to_string(),
        lead_branch: pr.lead_branch.clone(),
    };
    Ok(Decision::Cancel { from: "open", released: Some(released), superseded_by: None })
}
