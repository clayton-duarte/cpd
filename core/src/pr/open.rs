//! `pr open` (commands.md §9.1, contract §7.6): the Workflow's `open` Job, whose merge runs
//! the Gate in its Attempt worktree and opens the pull request, for one Workflow of the Plan.
//! No lock and no R (hs-d243).

use std::path::Path;

use serde_json::json;

use super::gh::{self, HeadState, Listed, Membership, PolledPr, PrRef};
use super::pulls;
use super::row::PlanWorkflows;
use super::start::base_workflow;
use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use super::{Args, Env, illegal_phase, short, workflow_policy};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git;
use crate::ids::{AttemptId, PlanId, WorkflowKey};
use crate::job;
use crate::output::Output;
use crate::plan::WorkflowRow;
use crate::plan::fingerprint::load_workflows;
use crate::plan::{Phase, jobs};
use crate::ports::Ports;
use crate::store::WriteTxn;

const COMMAND: &str = "pr open";

/// Whether the caller is an agent (hs-d643): `CPD_SESSION` set and non-empty.
pub(super) fn agent() -> bool {
    std::env::var_os("CPD_SESSION").is_some_and(|v| !v.is_empty())
}

/// `incident_hold` (exit 3, contract §7.6, commands.md §9.1/§9.3/§9.9, hs-d643): `plan` is held
/// by incident mode and the caller is an agent. `pr` names the Workflow the caller resolved, for
/// `details.pr`; `details.incident` is the incident as `cpd incident status` prints it.
pub fn hold_refused(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    workflow: &WorkflowKey,
    agent: bool,
) -> Result<(), CpdError> {
    if !agent || !crate::incident::is_held(conn, plan) {
        return Ok(());
    }
    let incident = crate::incident::row(conn)?.unwrap_or(serde_json::Value::Null);
    Err(CpdError::new(
        ErrorCode::IncidentHold,
        format!("Plan {plan} is held by incident mode: an agent cannot open, push or rebase Workflow {workflow}. The Manager ends the incident (cpd incident end) to release it."),
    )
    .with("pr", workflow.as_str())
    .with("plan", plan.as_str())
    .with("incident", incident))
}

pub(super) fn open(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let key = match require_dispatch(&plan, &args)? {
        Step::Already(done) => return Ok(done),
        Step::Reopen(key) => {
            let done = crate::workflow::reopen::reopen(&mut env, &id, &key)?;
            let plan = env.plan(&id)?;
            return Ok(Output::success(json!({
                "plan": id, "phase": plan.plan.phase.as_str(),
                "pr": plan.select(&args.workflow)?.to_json(), "reopened": done.changed,
            })));
        }
        Step::Go(key) => key,
    };
    env.store
        .read(|conn| hold_refused(conn, &id, &key, agent()))?;
    require_no_open_job_live(&mut env, &id, &key)?;
    require_jobs_done(&mut env, &plan, &key)?;
    stored_title(&mut env, &id, &key)?;
    stored_description(&mut env, &id, &key)?;
    env.store
        .read(|conn| jira_required_of(conn, &env.home, &id, &key))?;

    // `pr open` no longer opens the pull request itself: it adds and queues the Workflow's opener, whose submit runs the
    // Gate, pushes and opens the pull request (contract §7.6, §7.7). The write transaction decides again so two
    // openers can never race a concurrent `pr open` or engine E4 (hs-d573).
    let tier = env.config.worker_tier;
    let job = env.store.write(COMMAND, |tx| {
        if let Some(live) = live_open_job(tx, &id, &key)? {
            return Err(open_job_live(&id, &key, &live));
        }
        let workflow = load_workflows(tx, &id)?
            .into_iter()
            .find(|p| p.key == key)
            .ok_or_else(|| {
                CpdError::bad_row(format!(
                    "Workflow {key} of Plan {id} vanished while it was being queued"
                ))
            })?;
        job::open::add(tx, &id, &workflow, tier, false, COMMAND)
    })?;
    Ok(Output::success(
        json!({"plan": id, "pr": key, "job": job, "status": "queued"}),
    ))
}

/// A stacked Workflow's base branch, its base Workflow's Lead Branch, is on the remote (contract §7.6 `pr open` step 1), else
/// `base_not_found` (exit 4): the pull request targets it, and the base Workflow's Lead pushes it. A Workflow that is not stacked
/// has its base fetched next, and a repo with no `origin` has no pull request to target a branch of.
/// A stacked Workflow's base is on the remote: the Workflow's base Workflow, read from `store` by [`base_check`].
fn require_base_on_remote_pr(
    repo: &Path,
    workflow: &WorkflowRow,
    base_workflow: Option<&str>,
    then: &str,
) -> Result<(), CpdError> {
    let Some(base_workflow) = base_workflow else {
        return Ok(());
    };
    if !git::run::has_origin(repo)? {
        return Ok(());
    }
    let listed = git::run::run(
        repo,
        &[
            "ls-remote",
            "--exit-code",
            "--heads",
            "origin",
            &workflow.base,
        ],
    )?;
    match listed.code {
        0 => Ok(()),
        2 => Err(CpdError::new(
            ErrorCode::BaseNotFound,
            format!(
                "Workflow {}'s base {}, the Lead Branch of {base_workflow}, is not on origin: the pull request cannot target it. {then}",
                workflow.key, workflow.base
            ),
        )
        .with("pr", workflow.key.as_str())
        .with("base", workflow.base.as_str())
        .with("base_workflow", base_workflow)),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git ls-remote origin {} exited {code}.", workflow.base))
            .with("exit", code)
            .with("stderr", listed.stderr)),
    }
}

/// The `open` or `update` Job's base check at its merge (contract §7.7; hs-d679): a stacked Workflow's base is on the remote, and
/// `sha` does not conflict with the freshly fetched base. It runs before the Gates, so nothing is pushed or opened when it refuses.
pub fn base_check(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    repo: &Path,
    worktree: &Path,
    workflow: &WorkflowRow,
    sha: &str,
    again: &str,
) -> Result<(), CpdError> {
    let base_workflow = base_workflow(conn, plan, workflow)?;
    require_base_on_remote_pr(
        repo,
        workflow,
        base_workflow.as_deref(),
        &format!(
            "Its Lead pushes it (cpd pr push): ask the Lead (cpd ask), then run {again} again."
        ),
    )?;
    let base_sha = fresh_base(repo, &workflow.base)?;
    require_no_conflict(
        repo,
        worktree,
        workflow,
        &base_sha,
        sha,
        again,
        "your branch",
    )
}

/// B (contract §7.6 `pr open` step 1): `origin/<base>` after `git fetch origin <base>`, read once; a repo with no
/// `origin` skips the fetch and reads `refs/heads/<base>`.
fn fresh_base(repo: &Path, base: &str) -> Result<String, CpdError> {
    if git::run::has_origin(repo)? {
        git::run::fetch(repo, &["origin", base])?;
        git::run::rev_parse(repo, &format!("refs/remotes/origin/{base}"))
    } else {
        git::run::rev_parse(repo, &format!("refs/heads/{base}"))
    }
}

/// `base_conflict` (exit 3) when `sha` conflicts with `base_sha` (`git merge-tree`, hs-d490): a pull request is opened
/// unless its head conflicts with its base; a base that merely moved ahead is no refusal. `details.files` are the
/// conflicted paths, `details.next` what the caller runs, as written: the merge in `worktree` (the Lead worktree, or a Worker's
/// Attempt worktree), then `again`, the command that tries the check again. `into` names the branch the base is merged into.
fn require_no_conflict(
    repo: &Path,
    worktree: &Path,
    workflow: &WorkflowRow,
    base_sha: &str,
    sha: &str,
    again: &str,
    into: &str,
) -> Result<(), CpdError> {
    let Some(files) = git::merge_conflicts(repo, base_sha, sha)? else {
        return Ok(());
    };
    let remote = if git::run::has_origin(repo)? {
        format!("origin/{}", workflow.base)
    } else {
        workflow.base.clone()
    };
    Err(CpdError::new(
        ErrorCode::BaseConflict,
        format!(
            "{} of Workflow {} conflicts with {remote} ({}) in {}: merge it into {into} and resolve the conflict, then run {again} again. Nothing was gated, pushed or opened.",
            short(sha),
            workflow.key,
            short(base_sha),
            files.join(", ")
        ),
    )
    .with("pr", workflow.key.as_str())
    .with("base", workflow.base.as_str())
    .with("base_sha", base_sha)
    .with("sha", sha)
    .with("files", files)
    .with("next", json!([format!("git -C {} merge --no-edit {remote}", worktree.display()), again])))
}

/// The open pull request step 0 found, else a new one; when `gh pr create` fails, the head's pull request created at or after the
/// Workflow's start (an adopted Workflow: any) is looked up again and adopted if it is open now (another `pr open` of the Workflow created it). This runs at
/// the merge of an `open` Job (§7.4 step 8b), not at submit.
fn find_or_create(
    lead: &Path,
    workflow: &WorkflowRow,
    existing: Option<PrRef>,
    title: &str,
    description: Option<&str>,
) -> Result<PrRef, CpdError> {
    if let Some(found) = existing {
        return Ok(found);
    }
    match gh::create_pr(
        lead,
        &workflow.base,
        workflow.remote_branch(),
        title,
        description.unwrap_or_default(),
    ) {
        Ok(created) => {
            super::request_poll_now();
            Ok(created)
        }
        Err(failed) => workflow
            .bound_since()
            .and_then(|since| gh::head_pr(lead, workflow.remote_branch(), since))
            .ok()
            .flatten()
            .filter(|h| h.state == HeadState::Open)
            .map(|h| PrRef {
                number: h.number,
                url: h.url,
            })
            .ok_or(failed),
    }
}

/// A pull request of the Workflow's Lead Branch, as the rows of `dispatch → open` record it (contract §7.6 `pr open` steps 0 and 4, §7.4 step 8b).
#[derive(Clone, Debug)]
pub struct Pulled {
    row: PolledPr,
    /// Already `merged` on GitHub and never recorded (step 0): the Workflow goes `dispatch → open → merged`, with no Gate and no push. Else an
    /// open pull request, created or adopted (step 3): the Workflow goes `dispatch → open`.
    merged: bool,
}

impl Pulled {
    pub fn number(&self) -> i64 {
        self.row.number
    }

    pub fn url(&self) -> &str {
        &self.row.url
    }

    /// Whether it is merged already.
    pub fn is_merged(&self) -> bool {
        self.merged
    }
}

/// The pull request of Workflow `pr`'s Lead Branch for an `open` Job's merge (§7.4 step 8b), `pr open`'s steps 0 and 3 (§7.6): the newest pull
/// request of the branch created at or after the Workflow's start (`prs.started_at`; an adopted Workflow has no such bound) decides, in any state, `gh pr list --head <lead branch> --state all`; an older one is not the Workflow's and is ignored. `MERGED` is adopted as it is; `CLOSED` is
/// `pr_closed`; an `OPEN` one is adopted, and with none one is created, never a draft (hs-d329), through the one seam from the stored
/// title to GitHub's. `lead` is the Workflow's Lead worktree, so `gh` addresses its repo; `description` the Workflow's stored description (§9.5a), the body verbatim.
pub fn pull_request(
    lead: &Path,
    workflow: &WorkflowRow,
    stored_title: &str,
    description: Option<&str>,
    bots: &[String],
) -> Result<Pulled, CpdError> {
    match gh::head_pr(lead, workflow.remote_branch(), workflow.bound_since()?)? {
        Some(head) if head.state == HeadState::Merged => Ok(Pulled {
            row: merged_row(lead, &head, bots)?,
            merged: true,
        }),
        Some(head) if head.state == HeadState::Closed => {
            Err(closed(&workflow.key, workflow.remote_branch(), &head))
        }
        Some(head) => Ok(Pulled {
            row: opened_row(
                lead,
                workflow,
                Some(PrRef {
                    number: head.number,
                    url: head.url,
                }),
                stored_title,
                description,
                bots,
            )?,
            merged: false,
        }),
        None => Ok(Pulled {
            row: opened_row(lead, workflow, None, stored_title, description, bots)?,
            merged: false,
        }),
    }
}

/// The `(number, url)` of the newest pull request of `branch`, when it is `OPEN` (a reopened pull request, pro-d362); `closed`,
/// `merged` or none is `None`. `gh` runs in `repo`.
pub(crate) fn open_head_pr(repo: &Path, branch: &str) -> Result<Option<(i64, String)>, CpdError> {
    Ok(gh::head_pr(repo, branch, None)?
        .filter(|h| h.state == HeadState::Open)
        .map(|h| (h.number, h.url)))
}

/// Whether the newest pull request of the Lead Branch is already `merged` on GitHub and never recorded (§7.6 `pr open` step 0): the merge of an
/// `open` Job adopts it (§7.4 step 8b), with no Gate and no push.
pub fn is_merged_on_github(lead: &Path, workflow: &WorkflowRow) -> Result<bool, CpdError> {
    Ok(
        gh::head_pr(lead, workflow.remote_branch(), workflow.bound_since()?)?
            .is_some_and(|head| head.state == HeadState::Merged),
    )
}

/// `pr_closed` (exit 3): the newest pull request of the Lead Branch is closed without a merge (§7.6 `pr open` step 0): it is found at the merge of an `open` Job (§7.4 step 8b), not at submit.
pub fn closed(key: &WorkflowKey, branch: &str, head: &gh::HeadPr) -> CpdError {
    CpdError::new(
        ErrorCode::PrClosed,
        format!(
            "The pull request {} of Workflow {key}'s Lead Branch {branch} is closed without a merge: reopen it on GitHub, or move the work to a new branch.",
            head.url
        ),
    )
    .with("workflow", key.as_str())
    .with("number", head.number)
    .with("url", head.url.as_str())
}

/// The row of the open pull request `existing` (an earlier run died before its transaction), else of a new one: `gh pr create`
/// (never `--draft`), and when that fails the Lead Branch's pull request is looked up again ([`find_or_create`]). GitHub's creation time is
/// read by the poller's own lookup, which a pull request it cannot find fails as `gh_failed`; a rerun adopts the one created.
fn opened_row(
    lead: &Path,
    workflow: &WorkflowRow,
    existing: Option<PrRef>,
    stored_title: &str,
    description: Option<&str>,
    bots: &[String],
) -> Result<PolledPr, CpdError> {
    let title = github_title(stored_title);
    let found = find_or_create(lead, workflow, existing, &title, description)?;
    let (repo, _) = pulls::parse_url(&found.url).ok_or_else(|| {
        CpdError::new(ErrorCode::GhFailed, format!("gh printed the pull request URL {:?}, which is not https://github.com/<owner>/<name>/pull/<n>.", found.url))
            .with("url", found.url.as_str())
    })?;
    let (created_at, updated_at, base_ref, default_branch) = match gh::lookup(
        &*Ports::current().runner,
        lead,
        &[(repo.clone(), found.number)],
        bots,
    )?
    .into_iter()
    .next()
    .flatten()
    {
        Some(looked_up) => (
            looked_up.created_at,
            looked_up.updated_at,
            looked_up.base_ref,
            looked_up.default_branch,
        ),
        None => {
            return Err(CpdError::new(
                ErrorCode::GhFailed,
                format!(
                    "gh cannot find pull request {repo}#{}, which it just opened.",
                    found.number
                ),
            )
            .with("url", found.url.as_str()));
        }
    };
    Ok(PolledPr {
        repo,
        number: found.number,
        url: found.url,
        title,
        draft: false,
        state: PrState::Open,
        review_decision: ReviewDecision::None,
        checks: ChecksSummary::None,
        mergeable: Mergeable::Unknown,
        merge_state: MergeState::Unknown,
        created_at,
        updated_at,
        merged_at: None,
        base_ref,
        default_branch,
        open_threads: vec![],
        archived: false,
    })
}

/// The lookup of a pull request of the Lead Branch that `gh pr list` says is merged (§7.6 `pr open` step 0): one lookup of the poll's.
fn merged_row(lead: &Path, head: &gh::HeadPr, bots: &[String]) -> Result<PolledPr, CpdError> {
    let (repo, _) = pulls::parse_url(&head.url).ok_or_else(|| {
        CpdError::new(ErrorCode::GhFailed, format!("gh printed the pull request URL {:?}, which is not https://github.com/<owner>/<name>/pull/<n>.", head.url))
            .with("url", head.url.as_str())
    })?;
    match gh::lookup(
        &*Ports::current().runner,
        lead,
        &[(repo.clone(), head.number)],
        bots,
    )?
    .into_iter()
    .next()
    .flatten()
    {
        Some(looked_up) if looked_up.state == PrState::Merged => Ok(looked_up),
        Some(other) => Err(CpdError::new(
            ErrorCode::GhFailed,
            format!(
                "gh lists pull request {repo}#{} as merged but reads it {}.",
                head.number,
                other.state.as_str()
            ),
        )
        .with("url", head.url.as_str())),
        None => Err(CpdError::new(
            ErrorCode::GhFailed,
            format!(
                "gh cannot find pull request {repo}#{}, which it lists.",
                head.number
            ),
        )
        .with("url", head.url.as_str())),
    }
}

/// The rows of `dispatch → open` (§7.6 `pr open` step 4, and step 0 for a pull request already merged), in the caller's transaction: the
/// `pull_requests` row (inserted, or kept as the poll wrote it), the Workflow `open` with its pull request, `pr-opened`, and the Plan
/// `dispatch → deploy` when this was the last Workflow not yet open; for a merged pull request `pr-merged`, and `plan-done` when it completes
/// the Plan. `None`, with nothing written, when the Workflow is no longer `dispatch` (another opener, or the poll, got there first): `pr open`
/// refuses `cas_lost`, an `open` Job's merge ends `merged` all the same (§7.7). Else the Plan's phase after.
pub fn record(
    tx: &WriteTxn<'_>,
    id: &PlanId,
    key: &WorkflowKey,
    pulled: &Pulled,
) -> Result<Option<Phase>, CpdError> {
    let status: Option<String> = {
        use rusqlite::OptionalExtension;
        tx.query_row(
            "SELECT status FROM workflows WHERE plan = ?1 AND key = ?2",
            [id.as_str(), key.as_str()],
            |r| r.get(0),
        )
        .optional()?
    };
    if status.as_deref() != Some("dispatch") {
        return Ok(None);
    }
    let (number, url) = (pulled.number(), pulled.url());
    let mut merged_at = None;
    let pull_request = if pulled.merged {
        // A merge time: GitHub's, else this transaction's (as the poll).
        let mut polled = pulled.row.clone();
        if polled.merged_at.is_none() {
            polled.merged_at = Some(pulls::txn_time(tx)?);
        }
        // The row as this lookup reads it, with the searches that listed it as the poll left them (the User's own when new).
        let membership = pulls::find(tx, &polled.repo, polled.number)?.map_or(
            Membership {
                authored: true,
                review_requested: false,
            },
            |row| row.membership(),
        );
        merged_at = polled.merged_at.clone();
        pulls::upsert(
            tx,
            &Listed {
                pr: polled,
                membership,
            },
            None,
        )?;
        crate::inbox::model::close_pr_ready(tx, &pulled.row.repo, pulled.row.number)?;
        pulls::find(tx, &pulled.row.repo, pulled.row.number)?
            .expect("the row was just written")
            .id
    } else {
        pulls::ensure(tx, &pulled.row)?
    };
    // `prs_status` allows no `dispatch → merged`: through `open`, in this one transaction.
    let opened = tx.execute(
        "UPDATE workflows SET status = 'open', pr_number = ?1, pr_url = ?2, pr_opened_at = (SELECT at FROM txns WHERE id = cpd_txn()),
                pull_request = ?3
          WHERE plan = ?4 AND key = ?5 AND status = 'dispatch'",
        rusqlite::params![number, url, pull_request, id.as_str(), key.as_str()],
    )?;
    if opened == 0 {
        return Ok(None);
    }
    if pulled.merged {
        let merged = tx.execute(
            "UPDATE workflows SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND key = ?2 AND status = 'open'",
            [id.as_str(), key.as_str()],
        )?;
        if merged == 0 {
            return Err(cas_lost(id, key));
        }
        super::cancel::cancel_jobs_with(tx, id, key, &["draft"], "pr merged", "was merged")?;
        super::reactions::queue_merge(tx, id.as_str(), key.as_str())?;
        super::jira_sync::queue(
            tx,
            id.as_str(),
            key.as_str(),
            super::jira_sync::Target::Done,
        )?;
        let text = format!("Workflow {key} (#{number}) of Plan {id} is merged.");
        append(
            tx,
            NewEvent::pr_merged(text, key, number, url, merged_at.as_deref()).plan(id.clone()),
        )?;
    } else {
        let text = format!(
            "Workflow {key} of Plan {id} opened as #{number}: {url}. The engine polls it; to follow it by hand: cpd pr poll --plan {id}"
        );
        append(
            tx,
            NewEvent::pr_opened(text, key, number, url).plan(id.clone()),
        )?;
    }

    // The last Workflow to open moves the Plan on (§3.1, A10).
    let waiting: i64 =
        tx.query_row("SELECT count(*) FROM workflows WHERE plan = ?1 AND status NOT IN ('open', 'merged', 'cancelled')", [id.as_str()], |r| r.get(0))?;
    if waiting > 0 {
        return Ok(Some(Phase::Dispatch));
    }
    let moved = tx.execute(
        "UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'",
        [id.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(id, key));
    }
    let opened = opened_workflows(tx, id)?;
    let text = if pulled.is_merged() {
        format!(
            "Every Workflow of Plan {id} is open or merged. The engine polls them; to follow them by hand: cpd pr poll --plan {id}"
        )
    } else {
        format!(
            "Every Workflow of Plan {id} is open. The engine polls them; to follow them by hand: cpd pr poll --plan {id}"
        )
    };
    append(
        tx,
        NewEvent::plan_phase_deployed(text, opened).plan(id.clone()),
    )?;
    if !pulled.is_merged() {
        return Ok(Some(Phase::Deploy));
    }
    // The last Workflow to merge ends the Plan (§3.1).
    let unmerged: i64 = tx.query_row(
        "SELECT count(*) FROM workflows WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled')",
        [id.as_str()],
        |r| r.get(0),
    )?;
    if unmerged > 0 {
        return Ok(Some(Phase::Deploy));
    }
    crate::plan::close_reviews(tx, id, &crate::plan::Closing::Done)?;
    let moved = tx.execute(
        "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND phase = 'deploy'",
        [id.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(id, key));
    }
    let text = format!(
        "Every Workflow of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}"
    );
    append(
        tx,
        NewEvent::plan_done(text, merged_workflows(tx, id)?).plan(id.clone()),
    )?;
    Ok(Some(Phase::Done))
}

/// What an `open` or `update` Job's `attempt submit` once checked (contract §7.7, hs-d340). It has had no caller since commit 84c274f
/// (#425): submit now checks only the Workflow's policy (§7.7 "An `open` or `update` Job at submit"), and the checks listed here run at the
/// merge (§7.4), where a refusal ends the Attempt, not the submit. They were, in this
/// order, each refusal changing nothing: for an `open` Job the stored title (`workflow_title_required`) and the newest pull request of the Lead
/// Branch not closed without a merge (`pr_closed`; a merged one is adopted by the Job's merge, so nothing below it runs, as `pr open` runs no
/// Gate for it); for an `update` Job the Workflow is `open`; the Workflow's policy resolves; and the Lead Branch's tip is an ancestor of HEAD
/// (`lead_branch_moved`: a Job merged into it since this Attempt began, and A8 never rebases these kinds). The base check and the Gates run
/// at the merge (§7.4 step 6, hs-d679). `root` is the Attempt worktree, `sha` its HEAD.
pub fn submit_checks(
    env: &mut Env,
    plan: &PlanId,
    attempt: &AttemptId,
    job: &jobs::Job,
    root: &Path,
    sha: &str,
) -> Result<(), CpdError> {
    let key = WorkflowKey::parse(job.workflow_str()).map_err(|_| {
        CpdError::bad_row(format!(
            "Job {} has the Workflow key {:?}",
            job.key,
            job.workflow_str()
        ))
    })?;
    let workflows = env.plan(plan)?;
    let workflow = workflow_of(&workflows, &key);
    let lead = workflow.lead(plan)?.clone();
    let again = format!("cpd attempt submit {attempt}");
    // 1. The pull request of the Lead Branch (open); the Workflow's status (update).
    if job.kind == "update" && workflow.status != "open" {
        return Err(workflow.refuse(plan, "an update Job's submit", "open"));
    }
    let mut adopting = false;
    if job.kind == "open" {
        stored_title(env, plan, &key)?;
        stored_description(env, plan, &key)?;
        env.store
            .read(|conn| jira_required_of(conn, &env.home, plan, &key))?;
        if let Some(head) = gh::head_pr(&lead, workflow.remote_branch(), workflow.bound_since()?)? {
            match head.state {
                HeadState::Closed => {
                    return Err(
                        closed(&key, workflow.remote_branch(), &head).with("plan", plan.as_str())
                    );
                }
                HeadState::Merged => adopting = true,
                HeadState::Open => {}
            }
        }
    }
    // 2. The policy resolves, as for `pr open`.
    let _policy = workflow_policy(env, plan, &key)?;
    if adopting {
        return Ok(());
    }
    // 3. The Lead Branch's tip is in HEAD.
    let repo = Path::new(&workflow.repo);
    let tip = git::run::rev_parse(repo, &format!("refs/heads/{}", workflow.lead_branch))?;
    let contained = git::run::run(root, &["merge-base", "--is-ancestor", &tip, sha])?;
    match contained.code {
        0 => {}
        1 => {
            return Err(CpdError::new(
                ErrorCode::LeadBranchMoved,
                format!(
                    "A Job merged into {} since your Attempt began ({} is not in HEAD), and an open or update Job is never rebased: merge it, then run {again} again.",
                    workflow.lead_branch,
                    short(&tip)
                ),
            )
            .with("workflow", key.as_str())
            .with("lead_branch", workflow.lead_branch.as_str())
            .with("lead_sha", tip.as_str())
            .with("sha", sha)
            .with("next", format!("git -C {} merge --no-edit {}", root.display(), workflow.lead_branch)));
        }
        code => {
            return Err(CpdError::new(
                ErrorCode::GitFailed,
                "git merge-base --is-ancestor failed.",
            )
            .with("exit", code)
            .with("stderr", git::tail(&contained.stderr, git::TAIL_LINES)));
        }
    }
    // The base check and the Gate run at the merge (§7.4 step 6, hs-d679), not at submit.
    Ok(())
}

/// `[{key, number, url, merged_at}]` of every `merged` Workflow, in key order (`plan-done`'s `prs`).
pub(crate) fn merged_workflows(
    tx: &WriteTxn<'_>,
    id: &PlanId,
) -> Result<Vec<serde_json::Value>, CpdError> {
    let mut stmt = tx.prepare("SELECT key, pr_number, pr_url, merged_at FROM workflows WHERE plan = ?1 AND status = 'merged' ORDER BY key")?;
    let rows = stmt
        .query_map([id.as_str()], |r| {
            Ok(json!({"key": r.get::<_, String>(0)?, "number": r.get::<_, i64>(1)?, "url": r.get::<_, String>(2)?, "merged_at": r.get::<_, String>(3)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn cas_lost(id: &PlanId, key: &WorkflowKey) -> CpdError {
    CpdError::new(
        ErrorCode::CasLost,
        format!("Workflow {key} of Plan {id} left status dispatch meanwhile: rerun the command."),
    )
    .with("plan", id.as_str())
    .with("workflow", key.as_str())
}

/// `[{key, number, url}]` of every Workflow that is not `cancelled`, in key order.
pub(crate) fn opened_workflows(
    tx: &WriteTxn<'_>,
    id: &PlanId,
) -> Result<Vec<serde_json::Value>, CpdError> {
    let mut stmt = tx.prepare("SELECT key, pr_number, pr_url FROM workflows WHERE plan = ?1 AND status <> 'cancelled' ORDER BY key")?;
    let rows = stmt
        .query_map([id.as_str()], |r| Ok(json!({"key": r.get::<_, String>(0)?, "number": r.get::<_, i64>(1)?, "url": r.get::<_, String>(2)?})))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The Workflow `key`, which the Plan has.
fn workflow_of<'a>(plan: &'a PlanWorkflows, key: &WorkflowKey) -> &'a WorkflowRow {
    plan.workflows
        .iter()
        .find(|p| p.key == *key)
        .expect("the selected Workflow is still in the Plan")
}

enum Step {
    /// The Workflow is `dispatch`: go on, with its key.
    Go(WorkflowKey),
    /// The Workflow is `open` or `merged`: the success to print.
    Already(Output),
    /// The Workflow is `cancelled`: it comes back when its pull request is open on GitHub (pro-d362).
    Reopen(WorkflowKey),
}

/// The Workflow must be `dispatch`. `open` and `merged` are `already`; `waiting` and
/// `cancelled` are `illegal_transition`. A Plan that has not been locked is
/// `illegal_phase`.
fn require_dispatch(plan: &PlanWorkflows, args: &Args) -> Result<Step, CpdError> {
    if matches!(plan.plan.phase, Phase::Grounding | Phase::Plan) {
        return Err(illegal_phase(plan, COMMAND, "dispatch"));
    }
    let workflow = plan.select(&args.workflow)?;
    match workflow.status.as_str() {
        "dispatch" => Ok(Step::Go(workflow.key.clone())),
        "open" | "merged" => Ok(Step::Already(Output::success(
            json!({"plan": plan.plan.id, "phase": plan.plan.phase.as_str(), "pr": workflow.to_json(), "already": true}),
        ))),
        "cancelled" => Ok(Step::Reopen(workflow.key.clone())),
        _ => Err(workflow.refuse(&plan.plan.id, COMMAND, "dispatch")),
    }
}

/// The Workflow's `open` Job that is `draft`, `queued` or `running`, by key, if any (hs-d573): it is about to open the pull request.
fn live_open_job(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<Option<String>, CpdError> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT key FROM jobs WHERE plan = ?1 AND workflow = ?2 AND kind = 'open' AND status IN ('draft', 'queued', 'running') ORDER BY seq LIMIT 1",
            [plan.as_str(), key.as_str()],
            |r| r.get(0),
        )
        .optional()?)
}

/// `pr open` is the repair (contract §7.6, hs-d340): while the Workflow's `open` Job is `draft`, `queued` or `running` that Job is about to open the
/// pull request, and two openers would race, so it is refused `open_job_live` (exit 3, `details.job`) with nothing run or written.
fn require_no_open_job_live(
    env: &mut Env,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<(), CpdError> {
    match env.store.read(|conn| live_open_job(conn, plan, key))? {
        None => Ok(()),
        Some(job) => Err(open_job_live(plan, key, &job)),
    }
}

/// `open_job_live` (exit 3, `details.job`): the Workflow already has a live opener.
fn open_job_live(plan: &PlanId, key: &WorkflowKey, job: &str) -> CpdError {
    CpdError::new(
        ErrorCode::OpenJobLive,
        format!("Workflow {key} of Plan {plan} has its open Job {job} draft, queued or running: it opens the pull request. Wait for it, or cancel it (cpd job cancel {job}) to open the Workflow by hand."),
    )
    .with("plan", plan.as_str())
    .with("workflow", key.as_str())
    .with("job", job)
}

/// Every Job of the Workflow `merged` or `cancelled`, and at least one `merged`; else `jobs_not_done`. The Workflow's `open` and `update` Jobs count
/// for nothing (contract §7.6): an `open` Job that is `failed` is why the Lead is here, and one that is `merged` on a Workflow still
/// `dispatch` is a merge that died before it opened the pull request.
pub(super) fn require_jobs_done(
    env: &mut Env,
    plan: &PlanWorkflows,
    key: &WorkflowKey,
) -> Result<Vec<jobs::Job>, CpdError> {
    let all: Vec<jobs::Job> = env
        .store
        .read(|conn| jobs::load(conn, &plan.plan.id))?
        .into_iter()
        .filter(|j| {
            j.workflow_str() == key.as_str() && !matches!(j.kind.as_str(), "open" | "update")
        })
        .collect();
    let open: Vec<_> = all
        .iter()
        .filter(|j| j.status != "merged" && j.status != "cancelled")
        .map(|j| json!({"key": j.key, "status": j.status}))
        .collect();
    if !open.is_empty() {
        return Err(CpdError::new(
            ErrorCode::JobsNotDone,
            format!("{} Jobs of Workflow {key} of Plan {} are not Merged or cancelled yet: see cpd job list.", open.len(), plan.plan.id),
        )
        .with("plan", plan.plan.id.as_str())
        .with("workflow", key.as_str())
        .with("jobs", open));
    }
    if !all.iter().any(|j| j.status == "merged") {
        return Err(CpdError::new(ErrorCode::JobsNotDone, format!("No Job of Workflow {key} of Plan {} is Merged: there is nothing to open a Workflow for.", plan.plan.id))
            .with("plan", plan.plan.id.as_str())
            .with("workflow", key.as_str())
            .with("jobs", json!([])));
    }
    Ok(all)
}

/// The Workflow's stored title (hs-d285), which it must have: a Workflow that existed before migration `pr-title` has none, and
/// there is no fallback to the Plan's title (`workflow_title_required`).
fn stored_title(env: &mut Env, plan: &PlanId, key: &WorkflowKey) -> Result<String, CpdError> {
    let stored: Option<String> = env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT title FROM workflows WHERE plan = ?1 AND key = ?2",
            [plan.as_str(), key.as_str()],
            |r| r.get(0),
        )?)
    })?;
    stored.ok_or_else(|| {
        CpdError::new(
            ErrorCode::WorkflowTitleRequired,
            format!("Workflow {key} of Plan {plan} has no title: run cpd workflow edit {key} --title \"<text>\" (1-72 characters), then open it again."),
        )
        .with("plan", plan.as_str())
        .with("workflow", key.as_str())
    })
}

/// The Workflow's stored description (§9.5a), which a cpd Workflow must have (`description_required`): the body of its pull request. An
/// adopted Workflow (`head_ref` set) keeps the body of its own pull request and needs none.
fn stored_description(
    env: &mut Env,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<Option<String>, CpdError> {
    env.store
        .read(|conn| stored_description_of(conn, plan, key))
}

/// [`stored_description`] on a connection: the one rule (`description IS NULL AND head_ref IS NULL` is `description_required`).
pub fn stored_description_of(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<Option<String>, CpdError> {
    let (stored, adopted): (Option<String>, bool) = conn.query_row(
        "SELECT description, head_ref IS NOT NULL FROM workflows WHERE plan = ?1 AND key = ?2",
        [plan.as_str(), key.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if stored.is_none() && !adopted {
        return Err(description_required(plan, key));
    }
    Ok(stored)
}

/// `description_required` (exit 3): the Workflow has no stored description.
pub fn description_required(plan: &PlanId, key: &WorkflowKey) -> CpdError {
    CpdError::new(
        ErrorCode::DescriptionRequired,
        format!("Workflow {key} of Plan {plan} has no description: write it (the repo's pull request template, else ## What and why / ## How / ## Acceptance criteria / ## Out of scope and follow-ups / ## Links), run cpd workflow describe {key} --file <body.md>, then open it again."),
    )
    .with("plan", plan.as_str())
    .with("workflow", key.as_str())
}

/// The one ticket rule: a Workflow with no `jira` that is not adopted (`head_ref` null) is `jira_required` when its resolved policy sets
/// `workflow.jira_required`. A Workflow with a ticket, an adopted Workflow, or a policy that leaves it false never resolves a policy.
pub fn jira_required_of(
    conn: &rusqlite::Connection,
    home: &crate::home::CpdHome,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<(), CpdError> {
    let missing: bool = conn.query_row(
        "SELECT jira IS NULL AND head_ref IS NULL FROM workflows WHERE plan = ?1 AND key = ?2",
        [plan.as_str(), key.as_str()],
        |r| r.get(0),
    )?;
    if missing
        && super::policy_of_workflow(conn, home, plan, key)?
            .workflow
            .jira_required
    {
        return Err(jira_required(plan, key));
    }
    Ok(())
}

/// `jira_required` (exit 3): the Workflow has no Jira ticket and its owner's policy requires one.
pub fn jira_required(plan: &PlanId, key: &WorkflowKey) -> CpdError {
    CpdError::new(
        ErrorCode::JiraRequired,
        format!("Workflow {key} of Plan {plan} has no Jira ticket: its owner's policy sets workflow.jira_required; run cpd workflow edit {key} --jira <KEY>, then open it again."),
    )
    .with("plan", plan.as_str())
    .with("workflow", key.as_str())
}

/// The seam from a Workflow's stored title to the title of its pull request on GitHub: the one place a per-owner format
/// (hs-d277, repo-policy) would change it. Today it is unchanged. `gh pr create` uses it.
fn github_title(stored: &str) -> String {
    stored.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    /// Plan `pay` in `dispatch` with Workflow `api` `dispatch` (the Workflow an `open` Job's merge opens), and `waiting` Workflow `web` when `two`.
    fn fixture(two: bool) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let web = if two {
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pay', 'web', '/r/web', 'main', 'waiting', 'cpd/pay/web', cpd_txn(), 't');"
        } else {
            ""
        };
        store
            .write("fixture", |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                       VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                       VALUES ('pay', 'api', '/r/api', 'main', 'dispatch', 'cpd/pay/api', '/w/api', cpd_txn(), 't', 't');
                     {web}"
                ))?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    fn pull(state: PrState, merged_at: Option<&str>) -> PolledPr {
        PolledPr {
            repo: "acme/api".into(),
            number: 7,
            url: "https://github.com/acme/api/pull/7".into(),
            title: "Api".into(),
            draft: false,
            state,
            review_decision: ReviewDecision::None,
            checks: ChecksSummary::None,
            mergeable: Mergeable::Unknown,
            merge_state: MergeState::Unknown,
            created_at: "2026-10-20T10:00:00Z".into(),
            updated_at: "2026-10-20T10:00:00Z".into(),
            merged_at: merged_at.map(str::to_owned),
            base_ref: None,
            default_branch: None,
            open_threads: vec![],
            archived: false,
        }
    }

    fn kinds(store: &mut Store) -> Vec<String> {
        store
            .read(|conn| {
                let mut stmt =
                    conn.prepare("SELECT kind FROM events WHERE plan = 'pay' ORDER BY id")?;
                Ok(stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()?)
            })
            .unwrap()
    }

    /// An `open` Job's merge writes `pr open`'s rows (§7.7): the Workflow `open` with its pull request, `pr-opened`, and the Plan `deploy` when it was the
    /// last Workflow to open. The same call once the Workflow has left `dispatch` (a hand `pr open`, or the poll, got there first) writes nothing.
    #[test]
    fn recording_an_opened_pull_request_moves_the_pr_and_the_plan_and_is_refused_by_a_pr_that_left_dispatch()
     {
        let (_dir, mut store) = fixture(false);
        let (plan, key) = (
            PlanId::parse("pay").unwrap(),
            WorkflowKey::parse("api").unwrap(),
        );
        let pulled = Pulled {
            row: pull(PrState::Open, None),
            merged: false,
        };

        let phase = store
            .write("test", |tx| record(tx, &plan, &key, &pulled))
            .unwrap();

        assert_eq!(phase, Some(Phase::Deploy));
        let (status, number, linked, plan_phase): (String, i64, bool, String) = store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT r.status, r.pr_number, r.pull_request IS NOT NULL, p.phase FROM workflows r JOIN plans p ON p.id = r.plan WHERE r.key = 'api'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap();
        assert_eq!(
            (status.as_str(), number, linked, plan_phase.as_str()),
            ("open", 7, true, "deploy")
        );
        assert_eq!(kinds(&mut store), ["pr-opened", "plan-phase"]);

        let again = store
            .write("test", |tx| record(tx, &plan, &key, &pulled))
            .unwrap();
        assert_eq!(again, None);
        assert_eq!(
            kinds(&mut store),
            ["pr-opened", "plan-phase"],
            "nothing more was written"
        );
    }

    /// A pull request of the Lead Branch already merged on GitHub (§7.6 `pr open` step 0): the Workflow goes `dispatch → open → merged` with no `pr-opened`, and
    /// the Plan `deploy` and then `done` when it is the last Workflow; with another Workflow still `waiting` the Plan stays in `dispatch`.
    #[test]
    fn recording_a_merged_pull_request_merges_the_pr_and_ends_the_plan_only_when_it_is_the_last() {
        let (_dir, mut store) = fixture(false);
        let (plan, key) = (
            PlanId::parse("pay").unwrap(),
            WorkflowKey::parse("api").unwrap(),
        );
        let pulled = Pulled {
            row: pull(PrState::Merged, Some("2026-10-20T11:00:00Z")),
            merged: true,
        };

        let phase = store
            .write("test", |tx| record(tx, &plan, &key, &pulled))
            .unwrap();

        assert_eq!(phase, Some(Phase::Done));
        let (status, merged_at): (String, String) = store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT status, merged_at FROM workflows WHERE key = 'api'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((status.as_str(), merged_at.is_empty()), ("merged", false));
        assert_eq!(kinds(&mut store), ["pr-merged", "plan-phase", "plan-done"]);

        let (_dir, mut store) = fixture(true);
        let phase = store
            .write("test", |tx| record(tx, &plan, &key, &pulled))
            .unwrap();
        assert_eq!(phase, Some(Phase::Dispatch));
        assert_eq!(kinds(&mut store), ["pr-merged"]);
    }
}
