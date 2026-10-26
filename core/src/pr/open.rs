//! `pr open` (commands.md §9.1, contract §7.6): the PR's `open` Job, whose merge runs
//! the Gate in its Attempt worktree and opens the pull request, for one PR of the Plan.
//! No lock and no R (hs-d243).

use std::path::Path;

use serde_json::json;

use super::row::PlanPrs;
use crate::plan::PrRow;
use super::gh::{self, HeadState, Listed, Membership, PolledPr, PrRef};
use super::pulls;
use super::start::base_pr;
use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use super::{Args, Env, illegal_phase, pr_policy, short};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git;
use crate::ports::Ports;
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::output::Output;
use crate::job;
use crate::plan::fingerprint::load_prs;
use crate::plan::{Phase, jobs};
use crate::store::WriteTxn;

const COMMAND: &str = "pr open";

/// Whether the caller is an agent (hs-d643): `CPD_SESSION` set and non-empty.
pub(super) fn agent() -> bool {
    std::env::var_os("CPD_SESSION").is_some_and(|v| !v.is_empty())
}

/// `incident_hold` (exit 3, contract §7.6, commands.md §9.1/§9.3/§9.9, hs-d643): `plan` is held
/// by incident mode and the caller is an agent. `pr` names the PR the caller resolved, for
/// `details.pr`; `details.incident` is the incident as `cpd incident status` prints it.
pub fn hold_refused(conn: &rusqlite::Connection, plan: &PlanId, pr: &PrKey, agent: bool) -> Result<(), CpdError> {
    if !agent || !crate::incident::is_held(conn, plan) {
        return Ok(());
    }
    let incident = crate::incident::row(conn)?.unwrap_or(serde_json::Value::Null);
    Err(CpdError::new(
        ErrorCode::IncidentHold,
        format!("Plan {plan} is held by incident mode: an agent cannot open, push or rebase PR {pr}. The Manager ends the incident (cpd incident end) to release it."),
    )
    .with("pr", pr.as_str())
    .with("plan", plan.as_str())
    .with("incident", incident))
}

pub(super) fn open(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let key = match require_dispatch(&plan, &args)? {
        Step::Already(done) => return Ok(done),
        Step::Go(key) => key,
    };
    env.store.read(|conn| hold_refused(conn, &id, &key, agent()))?;
    require_no_open_job_live(&mut env, &id, &key)?;
    require_jobs_done(&mut env, &plan, &key)?;
    stored_title(&mut env, &id, &key)?;

    // `pr open` no longer opens the pull request itself: it adds and queues the PR's opener, whose submit runs the
    // Gate, pushes and opens the pull request (contract §7.6, §7.7). The write transaction decides again so two
    // openers can never race a concurrent `pr open` or engine E4 (hs-d573).
    let tier = env.config.open_tier;
    let job = env.store.write(COMMAND, |tx| {
        if let Some(live) = live_open_job(tx, &id, &key)? {
            return Err(open_job_live(&id, &key, &live));
        }
        let pr = load_prs(tx, &id)?.into_iter().find(|p| p.key == key).ok_or_else(|| {
            CpdError::bad_row(format!("PR {key} of Plan {id} vanished while it was being queued"))
        })?;
        job::open::add(tx, &id, &pr, tier, false, COMMAND)
    })?;
    Ok(Output::success(json!({"plan": id, "pr": key, "job": job, "status": "queued"})))
}

/// A stacked PR's base branch, its base PR's Lead Branch, is on the remote (contract §7.6 `pr open` step 1), else
/// `base_not_found` (exit 4): the pull request targets it, and the base PR's Lead pushes it. A PR that is not stacked
/// has its base fetched next, and a repo with no `origin` has no pull request to target a branch of.
/// A stacked PR's base is on the remote: the PR's base PR, read from `store` by [`base_check`].
fn require_base_on_remote_pr(repo: &Path, pr: &PrRow, base_pr: Option<&str>, then: &str) -> Result<(), CpdError> {
    let Some(base_pr) = base_pr else { return Ok(()) };
    if !git::run::has_origin(repo)? {
        return Ok(());
    }
    let listed = git::run::run(repo, &["ls-remote", "--exit-code", "--heads", "origin", &pr.base])?;
    match listed.code {
        0 => Ok(()),
        2 => Err(CpdError::new(
            ErrorCode::BaseNotFound,
            format!(
                "PR {}'s base {}, the Lead Branch of {base_pr}, is not on origin: the pull request cannot target it. {then}",
                pr.key, pr.base
            ),
        )
        .with("pr", pr.key.as_str())
        .with("base", pr.base.as_str())
        .with("base_pr", base_pr)),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git ls-remote origin {} exited {code}.", pr.base))
            .with("exit", code)
            .with("stderr", listed.stderr)),
    }
}

/// The `open` or `update` Job's base check at its merge (contract §7.7; hs-d679): a stacked PR's base is on the remote, and
/// `sha` does not conflict with the freshly fetched base. It runs before the Gates, so nothing is pushed or opened when it refuses.
pub fn base_check(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    repo: &Path,
    worktree: &Path,
    pr: &PrRow,
    sha: &str,
    again: &str,
) -> Result<(), CpdError> {
    let base_pr = base_pr(conn, plan, pr)?;
    require_base_on_remote_pr(repo, pr, base_pr.as_deref(), &format!("Its Lead pushes it (cpd pr push): ask the Lead (cpd ask), then run {again} again."))?;
    let base_sha = fresh_base(repo, &pr.base)?;
    require_no_conflict(repo, worktree, pr, &base_sha, sha, again, "your branch")
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
fn require_no_conflict(repo: &Path, worktree: &Path, pr: &PrRow, base_sha: &str, sha: &str, again: &str, into: &str) -> Result<(), CpdError> {
    let Some(files) = git::merge_conflicts(repo, base_sha, sha)? else { return Ok(()) };
    let remote = if git::run::has_origin(repo)? { format!("origin/{}", pr.base) } else { pr.base.clone() };
    Err(CpdError::new(
        ErrorCode::BaseConflict,
        format!(
            "{} of PR {} conflicts with {remote} ({}) in {}: merge it into {into} and resolve the conflict, then run {again} again. Nothing was gated, pushed or opened.",
            short(sha),
            pr.key,
            short(base_sha),
            files.join(", ")
        ),
    )
    .with("pr", pr.key.as_str())
    .with("base", pr.base.as_str())
    .with("base_sha", base_sha)
    .with("sha", sha)
    .with("files", files)
    .with("next", json!([format!("git -C {} merge --no-edit {remote}", worktree.display()), again])))
}

/// The open pull request of the PR's head, adopted when it exists (an earlier run
/// died before its transaction), else `gh pr create`. When the create fails, the
/// lookup runs again and adopts what it finds (a concurrent `pr open` of the PR
/// created it, §7.6 step 3); otherwise the failure stands.
/// The open pull request step 0 found, else a new one; when `gh pr create` fails, the Lead Branch's pull request is
/// looked up again and adopted if it is open now (another `pr open` of the PR created it).
fn find_or_create(lead: &Path, pr: &PrRow, existing: Option<PrRef>, title: &str, list: &[jobs::Job]) -> Result<PrRef, CpdError> {
    if let Some(found) = existing {
        return Ok(found);
    }
    match gh::create_pr(lead, &pr.base, pr.remote_branch(), title, &summary(title, list)) {
        Ok(created) => Ok(created),
        Err(failed) => gh::head_pr(lead, pr.remote_branch())
            .ok()
            .flatten()
            .filter(|h| h.state == HeadState::Open)
            .map(|h| PrRef { number: h.number, url: h.url })
            .ok_or(failed),
    }
}

/// A pull request of the PR's Lead Branch, as the rows of `dispatch → open` record it (contract §7.6 `pr open` steps 0 and 4, §7.4 step 8b).
#[derive(Clone, Debug)]
pub struct Pulled {
    row: PolledPr,
    /// Already `merged` on GitHub and never recorded (step 0): the PR goes `dispatch → open → merged`, with no Gate and no push. Else an
    /// open pull request, created or adopted (step 3): the PR goes `dispatch → open`.
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

/// The pull request of PR `pr`'s Lead Branch for an `open` Job's merge (§7.4 step 8b), `pr open`'s steps 0 and 3 (§7.6): the newest pull
/// request of the branch in any state decides, `gh pr list --head <lead branch> --state all`. `MERGED` is adopted as it is; `CLOSED` is
/// `pr_closed`; an `OPEN` one is adopted, and with none one is created, never a draft (hs-d329), through the one seam from the stored
/// title to GitHub's. `lead` is the PR's Lead worktree, so `gh` addresses its repo; `list` the PR's Jobs, for the body.
pub fn pull_request(lead: &Path, pr: &PrRow, stored_title: &str, list: &[jobs::Job]) -> Result<Pulled, CpdError> {
    match gh::head_pr(lead, pr.remote_branch())? {
        Some(head) if head.state == HeadState::Merged => Ok(Pulled { row: merged_row(lead, &head)?, merged: true }),
        Some(head) if head.state == HeadState::Closed => Err(closed(&pr.key, pr.remote_branch(), &head)),
        Some(head) => Ok(Pulled { row: opened_row(lead, pr, Some(PrRef { number: head.number, url: head.url }), stored_title, list)?, merged: false }),
        None => Ok(Pulled { row: opened_row(lead, pr, None, stored_title, list)?, merged: false }),
    }
}

/// Whether the newest pull request of the Lead Branch is already `merged` on GitHub and never recorded (§7.6 `pr open` step 0, §7.7 submit
/// step 1): an `open` Job adopts it, with no Gate and no push.
pub fn is_merged_on_github(lead: &Path, branch: &str) -> Result<bool, CpdError> {
    Ok(gh::head_pr(lead, branch)?.is_some_and(|head| head.state == HeadState::Merged))
}

/// `pr_closed` (exit 3): the newest pull request of the Lead Branch is closed without a merge (§7.6 `pr open` step 0, §7.7 submit check 1).
pub fn closed(key: &PrKey, branch: &str, head: &gh::HeadPr) -> CpdError {
    CpdError::new(
        ErrorCode::PrClosed,
        format!(
            "The pull request {} of PR {key}'s Lead Branch {branch} is closed without a merge: reopen it on GitHub, or move the work to a new branch.",
            head.url
        ),
    )
    .with("pr", key.as_str())
    .with("number", head.number)
    .with("url", head.url.as_str())
}

/// The row of the open pull request `existing` (an earlier run died before its transaction), else of a new one: `gh pr create`
/// (never `--draft`), and when that fails the Lead Branch's pull request is looked up again ([`find_or_create`]). GitHub's creation time is
/// read by the poller's own lookup, which a pull request it cannot find fails as `gh_failed`; a rerun adopts the one created.
fn opened_row(lead: &Path, pr: &PrRow, existing: Option<PrRef>, stored_title: &str, list: &[jobs::Job]) -> Result<PolledPr, CpdError> {
    let title = github_title(stored_title);
    let found = find_or_create(lead, pr, existing, &title, list)?;
    let (repo, _) = pulls::parse_url(&found.url).ok_or_else(|| {
        CpdError::new(ErrorCode::GhFailed, format!("gh printed the pull request URL {:?}, which is not https://github.com/<owner>/<name>/pull/<n>.", found.url))
            .with("url", found.url.as_str())
    })?;
    let (created_at, updated_at) = match gh::lookup(&*Ports::current().runner, lead, &[(repo.clone(), found.number)])?.into_iter().next().flatten() {
        Some(looked_up) => (looked_up.created_at, looked_up.updated_at),
        None => {
            return Err(CpdError::new(ErrorCode::GhFailed, format!("gh cannot find pull request {repo}#{}, which it just opened.", found.number))
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
    })
}

/// The lookup of a pull request of the Lead Branch that `gh pr list` says is merged (§7.6 `pr open` step 0): one lookup of the poll's.
fn merged_row(lead: &Path, head: &gh::HeadPr) -> Result<PolledPr, CpdError> {
    let (repo, _) = pulls::parse_url(&head.url).ok_or_else(|| {
        CpdError::new(ErrorCode::GhFailed, format!("gh printed the pull request URL {:?}, which is not https://github.com/<owner>/<name>/pull/<n>.", head.url))
            .with("url", head.url.as_str())
    })?;
    match gh::lookup(&*Ports::current().runner, lead, &[(repo.clone(), head.number)])?.into_iter().next().flatten() {
        Some(looked_up) if looked_up.state == PrState::Merged => Ok(looked_up),
        Some(other) => Err(CpdError::new(
            ErrorCode::GhFailed,
            format!("gh lists pull request {repo}#{} as merged but reads it {}.", head.number, other.state.as_str()),
        )
        .with("url", head.url.as_str())),
        None => Err(CpdError::new(ErrorCode::GhFailed, format!("gh cannot find pull request {repo}#{}, which it lists.", head.number))
            .with("url", head.url.as_str())),
    }
}

/// The rows of `dispatch → open` (§7.6 `pr open` step 4, and step 0 for a pull request already merged), in the caller's transaction: the
/// `pull_requests` row (inserted, or kept as the poll wrote it), the PR `open` with its pull request, `pr-opened`, and the Plan
/// `dispatch → deploy` when this was the last PR not yet open; for a merged pull request `pr-merged`, and `plan-done` when it completes
/// the Plan. `None`, with nothing written, when the PR is no longer `dispatch` (another opener, or the poll, got there first): `pr open`
/// refuses `cas_lost`, an `open` Job's merge ends `merged` all the same (§7.7). Else the Plan's phase after.
pub fn record(tx: &WriteTxn<'_>, id: &PlanId, key: &PrKey, pulled: &Pulled) -> Result<Option<Phase>, CpdError> {
    let status: Option<String> = {
        use rusqlite::OptionalExtension;
        tx.query_row("SELECT status FROM prs WHERE plan = ?1 AND key = ?2", [id.as_str(), key.as_str()], |r| r.get(0)).optional()?
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
        let membership = pulls::find(tx, &polled.repo, polled.number)?
            .map_or(Membership { authored: true, review_requested: false }, |row| row.membership());
        merged_at = polled.merged_at.clone();
        pulls::upsert(tx, &Listed { pr: polled, membership }, None)?;
        crate::inbox::model::close_pr_ready(tx, &pulled.row.repo, pulled.row.number)?;
        pulls::find(tx, &pulled.row.repo, pulled.row.number)?.expect("the row was just written").id
    } else {
        pulls::ensure(tx, &pulled.row)?
    };
    // `prs_status` allows no `dispatch → merged`: through `open`, in this one transaction.
    let opened = tx.execute(
        "UPDATE prs SET status = 'open', pr_number = ?1, pr_url = ?2, pr_opened_at = (SELECT at FROM txns WHERE id = cpd_txn()),
                pull_request = ?3
          WHERE plan = ?4 AND key = ?5 AND status = 'dispatch'",
        rusqlite::params![number, url, pull_request, id.as_str(), key.as_str()],
    )?;
    if opened == 0 {
        return Ok(None);
    }
    if pulled.merged {
        let merged = tx.execute(
            "UPDATE prs SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND key = ?2 AND status = 'open'",
            [id.as_str(), key.as_str()],
        )?;
        if merged == 0 {
            return Err(cas_lost(id, key));
        }
        let text = format!("PR {key} (#{number}) of Plan {id} is merged.");
        append(tx, NewEvent::pr_merged(text, key, number, url, merged_at.as_deref()).plan(id.clone()))?;
    } else {
        let text = format!("PR {key} of Plan {id} opened as #{number}: {url}. The engine polls it; to follow it by hand: cpd pr poll --plan {id}");
        append(tx, NewEvent::pr_opened(text, key, number, url).plan(id.clone()))?;
    }

    // The last PR to open moves the Plan on (§3.1, A10).
    let waiting: i64 =
        tx.query_row("SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('open', 'merged', 'cancelled')", [id.as_str()], |r| r.get(0))?;
    if waiting > 0 {
        return Ok(Some(Phase::Dispatch));
    }
    let moved = tx.execute("UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'", [id.as_str()])?;
    if moved == 0 {
        return Err(cas_lost(id, key));
    }
    let opened = opened_prs(tx, id)?;
    let text = if pulled.is_merged() {
        format!("Every PR of Plan {id} is open or merged. The engine polls them; to follow them by hand: cpd pr poll --plan {id}")
    } else {
        format!("Every PR of Plan {id} is open. The engine polls them; to follow them by hand: cpd pr poll --plan {id}")
    };
    append(tx, NewEvent::plan_phase_deployed(text, opened).plan(id.clone()))?;
    if !pulled.is_merged() {
        return Ok(Some(Phase::Deploy));
    }
    // The last PR to merge ends the Plan (§3.1).
    let unmerged: i64 = tx.query_row("SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled')", [id.as_str()], |r| r.get(0))?;
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
    let text = format!("Every PR of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}");
    append(tx, NewEvent::plan_done(text, merged_prs(tx, id)?).plan(id.clone()))?;
    Ok(Some(Phase::Done))
}

/// What an `open` or `update` Job's `attempt submit` checks (contract §7.7 "An `open` or `update` Job at submit", hs-d340), in this
/// order, each refusal changing nothing: for an `open` Job the stored title (`pr_title_required`) and the newest pull request of the Lead
/// Branch not closed without a merge (`pr_closed`; a merged one is adopted by the Job's merge, so nothing below it runs, as `pr open` runs no
/// Gate for it); for an `update` Job the PR is `open`; the PR's policy resolves; and the Lead Branch's tip is an ancestor of HEAD
/// (`lead_branch_moved`: a Job merged into it since this Attempt began, and A8 never rebases these kinds). The base check and the Gates run
/// at the merge (§7.4 step 6, hs-d679). `root` is the Attempt worktree, `sha` its HEAD.
pub fn submit_checks(env: &mut Env, plan: &PlanId, attempt: &AttemptId, job: &jobs::Job, root: &Path, sha: &str) -> Result<(), CpdError> {
    let key = PrKey::parse(&job.pr).map_err(|_| CpdError::bad_row(format!("Job {} has the PR key {:?}", job.key, job.pr)))?;
    let prs = env.plan(plan)?;
    let pr = pr_of(&prs, &key);
    let lead = pr.lead(plan)?.clone();
    let again = format!("cpd attempt submit {attempt}");
    // 1. The pull request of the Lead Branch (open); the PR's status (update).
    if job.kind == "update" && pr.status != "open" {
        return Err(pr.refuse(plan, "an update Job's submit", "open"));
    }
    let mut adopting = false;
    if job.kind == "open" {
        stored_title(env, plan, &key)?;
        if let Some(head) = gh::head_pr(&lead, pr.remote_branch())? {
            match head.state {
                HeadState::Closed => return Err(closed(&key, pr.remote_branch(), &head).with("plan", plan.as_str())),
                HeadState::Merged => adopting = true,
                HeadState::Open => {}
            }
        }
    }
    // 2. The policy resolves, as for `pr open`.
    let _policy = pr_policy(env, plan, &key)?;
    if adopting {
        return Ok(());
    }
    // 3. The Lead Branch's tip is in HEAD.
    let repo = Path::new(&pr.repo);
    let tip = git::run::rev_parse(repo, &format!("refs/heads/{}", pr.lead_branch))?;
    let contained = git::run::run(root, &["merge-base", "--is-ancestor", &tip, sha])?;
    match contained.code {
        0 => {}
        1 => {
            return Err(CpdError::new(
                ErrorCode::LeadBranchMoved,
                format!(
                    "A Job merged into {} since your Attempt began ({} is not in HEAD), and an open or update Job is never rebased: merge it, then run {again} again.",
                    pr.lead_branch,
                    short(&tip)
                ),
            )
            .with("pr", key.as_str())
            .with("lead_branch", pr.lead_branch.as_str())
            .with("lead_sha", tip.as_str())
            .with("sha", sha)
            .with("next", format!("git -C {} merge --no-edit {}", root.display(), pr.lead_branch)));
        }
        code => {
            return Err(CpdError::new(ErrorCode::GitFailed, "git merge-base --is-ancestor failed.")
                .with("exit", code)
                .with("stderr", git::tail(&contained.stderr, git::TAIL_LINES)));
        }
    }
    // The base check and the Gate run at the merge (§7.4 step 6, hs-d679), not at submit.
    Ok(())
}

/// `[{key, number, url, merged_at}]` of every `merged` PR, in key order (`plan-done`'s `prs`).
pub(super) fn merged_prs(tx: &WriteTxn<'_>, id: &PlanId) -> Result<Vec<serde_json::Value>, CpdError> {
    let mut stmt = tx.prepare("SELECT key, pr_number, pr_url, merged_at FROM prs WHERE plan = ?1 AND status = 'merged' ORDER BY key")?;
    let rows = stmt
        .query_map([id.as_str()], |r| {
            Ok(json!({"key": r.get::<_, String>(0)?, "number": r.get::<_, i64>(1)?, "url": r.get::<_, String>(2)?, "merged_at": r.get::<_, String>(3)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn cas_lost(id: &PlanId, key: &PrKey) -> CpdError {
    CpdError::new(ErrorCode::CasLost, format!("PR {key} of Plan {id} left status dispatch meanwhile: rerun the command."))
        .with("plan", id.as_str())
        .with("pr", key.as_str())
}

/// `[{key, number, url}]` of every PR that is not `cancelled`, in key order.
pub(super) fn opened_prs(tx: &WriteTxn<'_>, id: &PlanId) -> Result<Vec<serde_json::Value>, CpdError> {
    let mut stmt = tx.prepare("SELECT key, pr_number, pr_url FROM prs WHERE plan = ?1 AND status <> 'cancelled' ORDER BY key")?;
    let rows = stmt
        .query_map([id.as_str()], |r| Ok(json!({"key": r.get::<_, String>(0)?, "number": r.get::<_, i64>(1)?, "url": r.get::<_, String>(2)?})))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The PR `key`, which the Plan has.
fn pr_of<'a>(plan: &'a PlanPrs, key: &PrKey) -> &'a PrRow {
    plan.prs.iter().find(|p| p.key == *key).expect("the selected PR is still in the Plan")
}

enum Step {
    /// The PR is `dispatch`: go on, with its key.
    Go(PrKey),
    /// The PR is `open` or `merged`: the success to print.
    Already(Output),
}

/// The PR must be `dispatch`. `open` and `merged` are `already`; `waiting` and
/// `cancelled` are `illegal_transition`. A Plan that has not been locked is
/// `illegal_phase`.
fn require_dispatch(plan: &PlanPrs, args: &Args) -> Result<Step, CpdError> {
    if matches!(plan.plan.phase, Phase::Grounding | Phase::Plan) {
        return Err(illegal_phase(plan, COMMAND, "dispatch"));
    }
    let pr = plan.select(&args.pr)?;
    match pr.status.as_str() {
        "dispatch" => Ok(Step::Go(pr.key.clone())),
        "open" | "merged" => {
            Ok(Step::Already(Output::success(json!({"plan": plan.plan.id, "phase": plan.plan.phase.as_str(), "pr": pr.to_json(), "already": true}))))
        }
        _ => Err(pr.refuse(&plan.plan.id, COMMAND, "dispatch")),
    }
}

/// The PR's `open` Job that is `draft`, `queued` or `running`, by key, if any (hs-d573): it is about to open the pull request.
fn live_open_job(conn: &rusqlite::Connection, plan: &PlanId, key: &PrKey) -> Result<Option<String>, CpdError> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT key FROM jobs WHERE plan = ?1 AND pr = ?2 AND kind = 'open' AND status IN ('draft', 'queued', 'running') ORDER BY seq LIMIT 1",
            [plan.as_str(), key.as_str()],
            |r| r.get(0),
        )
        .optional()?)
}

/// `pr open` is the repair (contract §7.6, hs-d340): while the PR's `open` Job is `draft`, `queued` or `running` that Job is about to open the
/// pull request, and two openers would race, so it is refused `open_job_live` (exit 3, `details.job`) with nothing run or written.
fn require_no_open_job_live(env: &mut Env, plan: &PlanId, key: &PrKey) -> Result<(), CpdError> {
    match env.store.read(|conn| live_open_job(conn, plan, key))? {
        None => Ok(()),
        Some(job) => Err(open_job_live(plan, key, &job)),
    }
}

/// `open_job_live` (exit 3, `details.job`): the PR already has a live opener.
fn open_job_live(plan: &PlanId, key: &PrKey, job: &str) -> CpdError {
    CpdError::new(
        ErrorCode::OpenJobLive,
        format!("PR {key} of Plan {plan} has its open Job {job} draft, queued or running: it opens the pull request. Wait for it, or cancel it (cpd job cancel {job}) to open the PR by hand."),
    )
    .with("plan", plan.as_str())
    .with("pr", key.as_str())
    .with("job", job)
}

/// Every Job of the PR `merged` or `cancelled`, and at least one `merged`; else `jobs_not_done`. The PR's `open` and `update` Jobs count
/// for nothing (contract §7.6): an `open` Job that is `failed` is why the Lead is here, and one that is `merged` on a PR still
/// `dispatch` is a merge that died before it opened the pull request.
pub(super) fn require_jobs_done(env: &mut Env, plan: &PlanPrs, key: &PrKey) -> Result<Vec<jobs::Job>, CpdError> {
    let all: Vec<jobs::Job> = env
        .store
        .read(|conn| jobs::load(conn, &plan.plan.id))?
        .into_iter()
        .filter(|j| j.pr == key.as_str() && !matches!(j.kind.as_str(), "open" | "update"))
        .collect();
    let open: Vec<_> = all
        .iter()
        .filter(|j| j.status != "merged" && j.status != "cancelled")
        .map(|j| json!({"key": j.key, "status": j.status}))
        .collect();
    if !open.is_empty() {
        return Err(CpdError::new(
            ErrorCode::JobsNotDone,
            format!("{} Jobs of PR {key} of Plan {} are not Merged or cancelled yet: see cpd job list.", open.len(), plan.plan.id),
        )
        .with("plan", plan.plan.id.as_str())
        .with("pr", key.as_str())
        .with("jobs", open));
    }
    if !all.iter().any(|j| j.status == "merged") {
        return Err(CpdError::new(ErrorCode::JobsNotDone, format!("No Job of PR {key} of Plan {} is Merged: there is nothing to open a PR for.", plan.plan.id))
            .with("plan", plan.plan.id.as_str())
            .with("pr", key.as_str())
            .with("jobs", json!([])));
    }
    Ok(all)
}

/// The PR's stored title (hs-d285), which it must have: a PR that existed before migration `pr-title` has none, and
/// there is no fallback to the Plan's title (`pr_title_required`).
fn stored_title(env: &mut Env, plan: &PlanId, key: &PrKey) -> Result<String, CpdError> {
    let stored: Option<String> =
        env.store.read(|conn| Ok(conn.query_row("SELECT title FROM prs WHERE plan = ?1 AND key = ?2", [plan.as_str(), key.as_str()], |r| r.get(0))?))?;
    stored.ok_or_else(|| {
        CpdError::new(
            ErrorCode::PrTitleRequired,
            format!("PR {key} of Plan {plan} has no title: run cpd pr edit {key} --title \"<text>\" (1-72 characters), then open it again."),
        )
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
    })
}

/// The seam from a PR's stored title to the title of its pull request on GitHub: the one place a per-owner format
/// (hs-d277, repo-policy) would change it. Today it is unchanged. `gh pr create` and the body's `# <title>` both use it.
fn github_title(stored: &str) -> String {
    stored.to_owned()
}

/// A table cell: one line, no `|`.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The PR body: the title, the PR's Jobs table with each Job's Merged commit, and the generator line.
fn summary(title: &str, list: &[jobs::Job]) -> String {
    let mut lines = vec![format!("# {title}"), String::new(), "| Job | Title | Status | Merged commit |".to_owned(), "| --- | --- | --- | --- |".to_owned()];
    for j in list {
        let sha = j.merged_sha.as_deref().map_or("-".to_owned(), |s| format!("`{s}`"));
        lines.push(format!("| {} | {} | {} | {sha} |", cell(&j.key), cell(&j.title), j.status));
    }
    lines.push(String::new());
    lines.push("Generated by cpd".to_owned());
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    /// Plan `pay` in `dispatch` with PR `api` `dispatch` (the PR an `open` Job's merge opens), and `waiting` PR `web` when `two`.
    fn fixture(two: bool) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let web = if two {
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
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
                     INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
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
        }
    }

    fn kinds(store: &mut Store) -> Vec<String> {
        store
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT kind FROM events WHERE plan = 'pay' ORDER BY id")?;
                Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
            })
            .unwrap()
    }

    /// An `open` Job's merge writes `pr open`'s rows (§7.7): the PR `open` with its pull request, `pr-opened`, and the Plan `deploy` when it was the
    /// last PR to open. The same call once the PR has left `dispatch` (a hand `pr open`, or the poll, got there first) writes nothing.
    #[test]
    fn recording_an_opened_pull_request_moves_the_pr_and_the_plan_and_is_refused_by_a_pr_that_left_dispatch() {
        let (_dir, mut store) = fixture(false);
        let (plan, key) = (PlanId::parse("pay").unwrap(), PrKey::parse("api").unwrap());
        let pulled = Pulled { row: pull(PrState::Open, None), merged: false };

        let phase = store.write("test", |tx| record(tx, &plan, &key, &pulled)).unwrap();

        assert_eq!(phase, Some(Phase::Deploy));
        let (status, number, linked, plan_phase): (String, i64, bool, String) = store
            .read(|conn| {
                Ok(conn.query_row(
                    "SELECT r.status, r.pr_number, r.pull_request IS NOT NULL, p.phase FROM prs r JOIN plans p ON p.id = r.plan WHERE r.key = 'api'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap();
        assert_eq!((status.as_str(), number, linked, plan_phase.as_str()), ("open", 7, true, "deploy"));
        assert_eq!(kinds(&mut store), ["pr-opened", "plan-phase"]);

        let again = store.write("test", |tx| record(tx, &plan, &key, &pulled)).unwrap();
        assert_eq!(again, None);
        assert_eq!(kinds(&mut store), ["pr-opened", "plan-phase"], "nothing more was written");
    }

    /// A pull request of the Lead Branch already merged on GitHub (§7.6 `pr open` step 0): the PR goes `dispatch → open → merged` with no `pr-opened`, and
    /// the Plan `deploy` and then `done` when it is the last PR; with another PR still `waiting` the Plan stays in `dispatch`.
    #[test]
    fn recording_a_merged_pull_request_merges_the_pr_and_ends_the_plan_only_when_it_is_the_last() {
        let (_dir, mut store) = fixture(false);
        let (plan, key) = (PlanId::parse("pay").unwrap(), PrKey::parse("api").unwrap());
        let pulled = Pulled { row: pull(PrState::Merged, Some("2026-10-20T11:00:00Z")), merged: true };

        let phase = store.write("test", |tx| record(tx, &plan, &key, &pulled)).unwrap();

        assert_eq!(phase, Some(Phase::Done));
        let (status, merged_at): (String, String) =
            store.read(|conn| Ok(conn.query_row("SELECT status, merged_at FROM prs WHERE key = 'api'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
        assert_eq!((status.as_str(), merged_at.is_empty()), ("merged", false));
        assert_eq!(kinds(&mut store), ["pr-merged", "plan-phase", "plan-done"]);

        let (_dir, mut store) = fixture(true);
        let phase = store.write("test", |tx| record(tx, &plan, &key, &pulled)).unwrap();
        assert_eq!(phase, Some(Phase::Dispatch));
        assert_eq!(kinds(&mut store), ["pr-merged"]);
    }
}
