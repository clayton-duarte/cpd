//! Reopen of a cancelled Workflow whose pull request GitHub reopened (pro-d362): `cancelled -> open`.

use crate::env::Env;
use crate::error::CpdError;
use crate::ids::{PlanId, WorkflowKey};

/// What [`reopen`] did.
#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)] // the caller (`pr open`) comes with the implementation
pub struct Reopened {
    /// `false` when the Workflow was already `open` and nothing was written (A5).
    pub changed: bool,
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
#[allow(dead_code)] // the caller (`pr open`) comes with the implementation
pub fn reopen(env: &mut Env, plan: &PlanId, key: &WorkflowKey) -> Result<Reopened, CpdError> {
    let _ = (env, plan, key);
    Err(CpdError::bad_row("workflow reopen is not implemented"))
}
