//! Reopen of a cancelled Workflow whose pull request GitHub reopened (pro-d362): `cancelled -> open`.

use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, WorkflowKey};

const COMMAND: &str = "workflow reopen";

/// What [`reopen`] did.
#[derive(Debug, PartialEq, Eq)]
pub struct Reopened {
    /// `false` when the Workflow was already `open` and nothing was written (A5).
    pub changed: bool,
}

/// A cancelled Workflow that may have its pull request back (A7).
#[derive(Debug, PartialEq, Eq)]
pub struct Candidate {
    pub plan: PlanId,
    pub key: WorkflowKey,
    /// The repo path of the Workflow.
    pub repo: String,
    pub lead_branch: String,
    /// The kept `pull_requests.id` whose state is `open`; `None` for a legacy row, which only `gh` can confirm (by its Lead
    /// Branch, [`reopen`] does it).
    pub pull_request: Option<i64>,
}

/// The cancelled Workflows to try [`reopen`] on: those that kept a `pull_request` whose stored state is `open`, and the legacy
/// ones (`pull_request` NULL) that are the only cancelled Workflow of their (repo, lead branch): the store holds no head ref
/// of a pull request, so [`reopen`] confirms those with `gh`.
pub fn reopenable(conn: &rusqlite::Connection) -> Result<Vec<Candidate>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT w.plan, w.key, w.repo, w.lead_branch, w.pull_request FROM workflows w
           LEFT JOIN pull_requests q ON q.id = w.pull_request
          WHERE w.status = 'cancelled'
            AND (q.state = 'open'
                 OR w.pull_request IS NULL
                    AND (SELECT count(*) FROM workflows t
                          WHERE t.status = 'cancelled' AND t.repo = w.repo AND t.lead_branch = w.lead_branch) = 1)
          ORDER BY w.plan, w.key",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(plan, key, repo, lead_branch, pull_request)| {
            Ok(Candidate {
                plan: PlanId::parse(&plan)?,
                key: WorkflowKey::parse(&key)?,
                repo,
                lead_branch,
                pull_request,
            })
        })
        .collect()
}

fn refused(plan: &PlanId, key: &WorkflowKey, why: &str) -> CpdError {
    CpdError::new(
        ErrorCode::IllegalTransition,
        format!("Workflow {key} of Plan {plan} cannot be reopened: {why}."),
    )
    .with("plan", plan.as_str())
    .with("pr", key.as_str())
    .with("needs", "cancelled with an open pull request")
}

struct Row {
    status: String,
    repo: String,
    lead_branch: String,
    pull_request: Option<i64>,
    /// Other cancelled Workflows with the same (repo, lead branch).
    twins: i64,
}

/// Rebinds the cancelled Workflow `key` of `plan` to its pull request when GitHub says that pull request is `OPEN` again.
///
/// The one `gh` call is `gh pr list --head <lead branch> --state all --json number,url,state,createdAt` (`gh::head_pr`, with no
/// start bound: a cancelled row has no `started_at`). A cancelled row is matched by its kept `pull_request`; a legacy row with
/// `pull_request` NULL by (repo, lead_branch) only when exactly one cancelled Workflow has that pair (A7).
/// In one write transaction: the Workflow `cancelled -> open` with `pr_number`, `pr_url`, `pr_opened_at`, `pull_request` and
/// `lead_worktree` set, the Event `workflow-reopened`, the Lead Session's `asleep_at` NULL (A4) and, only when the Plan is
/// `done`, the Plan `done -> deploy` with `done_at` NULL and a `plan-phase` Event (A3). The Lead worktree is recreated from
/// `origin/<lead_branch>` when its path and local branch are gone; a registered worktree of that branch is kept as it is.
/// The `pull_requests` row of the pull request is already stored (the poll writes it), matched by its url. A Workflow already `open` changes nothing (A5). A pull request that is `CLOSED` or absent is `illegal_transition`,
/// with nothing written.
pub fn reopen(env: &mut Env, plan: &PlanId, key: &WorkflowKey) -> Result<Reopened, CpdError> {
    let row = env.store.read(|conn| {
        conn.query_row(
            "SELECT w.status, w.repo, w.lead_branch, w.pull_request,
                    (SELECT count(*) FROM workflows t WHERE t.status = 'cancelled' AND t.repo = w.repo AND t.lead_branch = w.lead_branch)
               FROM workflows w WHERE w.plan = ?1 AND w.key = ?2",
            [plan.as_str(), key.as_str()],
            |r| {
                Ok(Row {
                    status: r.get(0)?,
                    repo: r.get(1)?,
                    lead_branch: r.get(2)?,
                    pull_request: r.get(3)?,
                    twins: r.get(4)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| CpdError::bad_row(format!("no Workflow {key} in Plan {plan}")))
    })?;
    match row.status.as_str() {
        "open" => return Ok(Reopened { changed: false }),
        "cancelled" => {}
        other => return Err(refused(plan, key, &format!("it is {other}"))),
    }
    if row.pull_request.is_none() && row.twins != 1 {
        return Err(refused(
            plan,
            key,
            "another cancelled Workflow has the same Lead Branch, so its pull request is not told apart",
        ));
    }
    let repo = Path::new(&row.repo);
    let (number, url) = crate::pr::open::open_head_pr(repo, &row.lead_branch)?
        .ok_or_else(|| refused(plan, key, "its pull request is not open on GitHub"))?;
    let pull_request: i64 = env
        .store
        .read(|conn| {
            conn.query_row(
                "SELECT id FROM pull_requests WHERE url = ?1",
                [url.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(CpdError::from)
        })?
        .ok_or_else(|| {
            refused(
                plan,
                key,
                &format!("pull request {} is not stored yet: the poll writes it", url),
            )
        })?;
    let path: PathBuf = crate::git::workflow_lead_worktree_path(repo, plan, key, &row.lead_branch)?;
    crate::git::worktree::ensure_reopened_lead_worktree(repo, &path, &row.lead_branch)?;
    let worktree = path.to_string_lossy().into_owned();

    env.store.write(COMMAND, |tx| {
        let reopened = tx.execute(
            "UPDATE workflows SET status = 'open', lead_worktree = ?1, started_at = (SELECT at FROM txns WHERE id = cpd_txn()), pr_number = ?2, pr_url = ?3,
                    pr_opened_at = coalesce(pr_opened_at, (SELECT at FROM txns WHERE id = cpd_txn())), pull_request = ?4
              WHERE plan = ?5 AND key = ?6 AND status = 'cancelled'",
            rusqlite::params![worktree, number, url, pull_request, plan.as_str(), key.as_str()],
        )?;
        if reopened == 0 {
            return Ok(Reopened { changed: false });
        }
        let text = format!("Workflow {key} of Plan {plan} is open again as #{number}: {url}.");
        append(
            tx,
            NewEvent::workflow_reopened(text, key).plan(plan.clone()),
        )?;
        let moved = tx.execute(
            "UPDATE plans SET phase = 'deploy', done_at = NULL WHERE id = ?1 AND phase = 'done'",
            [plan.as_str()],
        )?;
        if moved == 1 {
            let opened = crate::pr::open::opened_workflows(tx, plan)?;
            append(
                tx,
                NewEvent::plan_phase_deployed(
                    format!("Workflow {key} of Plan {plan} is open again: the Plan is in deploy."),
                    opened,
                )
                .plan(plan.clone()),
            )?;
        }
        tx.execute(
            "UPDATE sessions SET asleep_at = NULL WHERE id = (SELECT lead_session FROM plans WHERE id = ?1)",
            [plan.as_str()],
        )?;
        Ok(Reopened { changed: true })
    })
}
