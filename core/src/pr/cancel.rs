//! `pr cancel` (commands.md §9.6, contract §3.8): `waiting → cancelled` before
//! the lock; after it, a `waiting` or `dispatch` PR with no `running` Job (and no `merged` one, unless superseded) too,
//! its `draft`, `queued` and `failed` Jobs cancelled with it in the same transaction, the shape check never applying; and an `open` PR whose pull request is
//! stored `closed` (hs-d329). The PR keeps its key; before the lock the plan
//! fingerprint changes, after it the Plan may move on. With `--superseded-by <plan>/<key>` a `dispatch` PR whose
//! `merged` Jobs were carried by another PR is cancelled too, the Event naming that PR (hs-d301). A `dispatch` PR whose merged Jobs are all review
//! Jobs (which commit nothing) is cancelled without it (pro-d77).

use rusqlite::Connection;
use serde_json::json;

use super::add::needs_refs;
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
                cancel_jobs(tx, &plan, &key)?;
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

/// The PR's Jobs go with it (contract §3.8): in the transaction that cancels a PR, every Job of it that is `draft`, `queued` or `failed`, of every kind,
/// is `cancelled` (`reason` `pr cancelled`, one `job-cancelled` each, by `seq`). One already `cancelled` is untouched.
fn cancel_jobs(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey) -> Result<(), CpdError> {
    cancel_jobs_with(tx, plan, key, &["draft", "queued", "failed"], "pr cancelled", "was cancelled")
}

/// The Jobs of PR `key` whose status is one of `statuses`, `cancelled` in the caller's transaction with `reason`, one `job-cancelled` Event each,
/// by `seq`. `what` is what happened to the PR, for the Event's text. `pr cancel` cancels `draft`, `queued` and `failed` ones; the merge of a PR its `draft` ones
/// (hs-d735, `pr merged`).
pub(super) fn cancel_jobs_with(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey, statuses: &[&str], reason: &str, what: &str) -> Result<(), CpdError> {
    let list = statuses.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ");
    let own: Vec<String> = {
        let mut stmt = tx.prepare(&format!("SELECT key FROM jobs WHERE plan = ?1 AND pr = ?2 AND status IN ({list}) ORDER BY seq"))?;
        stmt.query_map([plan.as_str(), key.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for job in own {
        tx.execute(
            &format!(
                "UPDATE jobs SET status = 'cancelled', failed_cause = NULL, cancel_reason = ?3,
                        cancel_requested_at = (SELECT at FROM txns WHERE id = cpd_txn())
                  WHERE plan = ?1 AND key = ?2 AND status IN ({list})"
            ),
            [plan.as_str(), job.as_str(), reason],
        )?;
        let text = format!("Job {job} of Plan {plan} was cancelled: {reason}: PR {key} {what}. Next: cpd plan show --plan {plan}");
        append(tx, NewEvent::job_cancelled(text, reason).plan(plan.clone()).job(crate::ids::JobKey::parse(&job)?))?;
    }
    Ok(())
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
    crate::plan::close_reviews(tx, plan, &crate::plan::Closing::Done)?;
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
            format!(
                "PR {key} is in use: a running Job holds it (cancel it with cpd job cancel and wait for it to stop), a merged Job holds it unless --superseded-by \
                 names the PR that carried its work (a merged review Job, which commits nothing, never holds it), and PRs that need it or are stacked on it hold it (edit them away with cpd pr edit) first."
            ),
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
    // Only a `running` Job holds the PR, and a `merged` one unless superseded (its work another PR carried) or a review Job (commits nothing): `draft`, `queued` and `failed` Jobs
    // go with the PR (`cancel_jobs`).
    let holds = |j: &jobs::Job| j.status == "running" || (j.status == "merged" && superseded.is_none() && !j.review);
    let live_jobs: Vec<String> = jobs::load(conn, plan)?.into_iter().filter(|j| j.pr == key.as_str() && holds(j)).map(|j| j.key).collect();
    if !live_jobs.is_empty() || !needing.is_empty() {
        return Err(in_use(live_jobs));
    }
    if let Some(by) = superseded {
        check_superseding(conn, plan, key, by)?;
    }
    Ok(Decision::Cancel { from, released: None, superseded_by: superseded.map(str::to_owned) })
}

/// The superseding PR `by` (a reference of `plan`, §2.2: a bare key, or `<plan>/<key>`): a PR of a Plan of the same
/// Context (`pr_not_found`), not `key` itself (`usage`), not `cancelled` (`illegal_transition`). Any other status will
/// do: cpd cannot know where the work went (contract §3.8). It is where the work went, not a dependency: it may be
/// another Plan's.
fn check_superseding(conn: &Connection, plan: &PlanId, key: &PrKey, by: &str) -> Result<(), CpdError> {
    let (by_plan, by_key) = by.split_once('/').unwrap_or((plan.as_str(), by));
    if by_plan == plan.as_str() && by_key == key.as_str() {
        return Err(CpdError::new(ErrorCode::Usage, format!("--superseded-by names PR {key} itself.")).with("pr", key.as_str()));
    }
    let found: Option<String> = conn
        .query_row(
            "SELECT r.status FROM prs r JOIN plans p ON p.id = r.plan
              WHERE r.plan = ?1 AND r.key = ?2 AND p.context = (SELECT context FROM plans WHERE id = ?3)",
            [by_plan, by_key, plan.as_str()],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;
    let Some(status) = found else {
        return Err(CpdError::new(ErrorCode::PrNotFound, format!("No PR {by_key} in Plan {by_plan} to be superseded by."))
            .with("plan", by_plan)
            .with("pr", by_key));
    };
    if status == "cancelled" {
        return Err(CpdError::new(ErrorCode::IllegalTransition, format!("PR {by_plan}/{by_key} is cancelled: its work cannot supersede PR {key}'s."))
            .with("plan", by_plan)
            .with("pr", by_key)
            .with("from", "cancelled")
            .with("to", "superseding"));
    }
    Ok(())
}

/// The PRs of `plan` that are not `cancelled` and stacked on `pr`: those of its repo whose `base` is its Lead Branch
/// (§2.2 "The base PR"), by key.
fn stacked_on(conn: &Connection, plan: &PlanId, pr: &PrRow) -> Result<Vec<String>, CpdError> {
    let mut stmt = conn.prepare("SELECT key FROM prs WHERE plan = ?1 AND repo = ?2 AND base = ?3 AND status <> 'cancelled' AND key <> ?4 ORDER BY key")?;
    let found = stmt
        .query_map([plan.as_str(), pr.repo.as_str(), pr.lead_branch.as_str(), pr.key.as_str()], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(found)
}

/// An `open` PR: its pull request must be stored `closed` (`pr_not_closed`, no `gh` call: the row is what the last poll
/// wrote) and no PR that is not `cancelled` may need it (`pr_in_use`). Its Jobs are not checked: its `draft`, `queued` and `failed` ones go with it.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    fn fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                       VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                     INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                       VALUES ('pay', 'api', '/r/api', 'main', 'dispatch', 'cpd/pay/api', '/w/api', cpd_txn(), 't', 't');
                     INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind,
                                       fingerprint, created_txn, created_at)
                       VALUES ('pay', 'api', 'open-api', 'Open', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 1, 'open', hex(zeroblob(32)), cpd_txn(), 't');",
                )?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    /// A `draft`, `failed` or `queued` Job of any kind goes with its PR (contract §3.8): it does not hold the cancel, and the cancel's transaction cancels it with
    /// `job-cancelled`. One that is `running` is a live Job, which holds the cancel as any does.
    #[test]
    fn a_draft_queued_and_failed_job_go_with_its_pr_and_a_running_one_holds_it() {
        let (_dir, mut store) = fixture();
        let (plan, key) = (PlanId::parse("pay").unwrap(), PrKey::parse("api").unwrap());
        assert!(matches!(store.read(|conn| decide(conn, &plan, &key, None)).unwrap(), Decision::Cancel { from: "dispatch", .. }));

        store
            .write("fixture", |tx| {
                for (k, status, kind) in [("t-api", "draft", "test"), ("i-api", "failed", "impl")] {
                    tx.execute(
                        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind,
                                           fingerprint, created_txn, created_at, failed_cause)
                         VALUES ('pay', 'api', ?1, 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', ?2, 1, ?3, CASE WHEN ?2 = 'draft' THEN NULL ELSE hex(zeroblob(32)) END, cpd_txn(), 't', CASE WHEN ?2 = 'failed' THEN 'x' END)",
                        [k, status, kind],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        store.write("fixture", |tx| cancel_jobs(tx, &plan, &key)).unwrap();
        let others: i64 = store
            .read(|conn| Ok(conn.query_row("SELECT count(*) FROM jobs WHERE status = 'cancelled' AND cancel_reason = 'pr cancelled'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(others, 3);
        let (status, reason, events): (String, String, i64) = store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT status, cancel_reason, (SELECT count(*) FROM events WHERE kind = 'job-cancelled' AND job = 'open-api') FROM jobs WHERE key = 'open-api'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!((status.as_str(), reason.as_str(), events), ("cancelled", "pr cancelled", 1));

        let (_dir, mut store) = fixture();
        store.write("fixture", |tx| Ok(tx.execute_batch("UPDATE jobs SET status = 'running' WHERE key = 'open-api';")?)).unwrap();
        let held = store.read(|conn| decide(conn, &plan, &key, None)).map(|_| ()).unwrap_err();
        assert_eq!(held.code(), ErrorCode::PrInUse);
        assert_eq!(held.details()["jobs"], json!(["open-api"]));
    }
}
