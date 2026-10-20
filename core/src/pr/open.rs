//! `pr open` (commands.md §9.1, contract §7.6): the Gate on the Lead Branch's
//! tip G in a gate worktree, the push of G by rule P, the PR, for one PR of the
//! Plan. No lock and no R (hs-d243).

use std::path::Path;

use serde_json::json;

use super::row::PlanPrs;
use crate::plan::PrRow;
use super::gh::{self, HeadState, Listed, Membership, PolledPr, PrRef};
use super::pulls;
use super::start::{base_pr, unstack_stored};
use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use super::{Args, Env, gate, illegal_phase, pr_policy, short};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::git;
use crate::ids::{PlanId, PrKey};
use crate::output::Output;
use crate::plan::{Phase, jobs};
use crate::store::WriteTxn;

const COMMAND: &str = "pr open";

pub(super) fn open(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let key = match require_dispatch(&plan, &args)? {
        Step::Already(done) => return Ok(done),
        Step::Go(key) => key,
    };
    let policy = pr_policy(&mut env, &id, &key)?;
    let list = require_jobs_done(&mut env, &plan, &key)?;
    let stored = stored_title(&mut env, &id, &key)?;
    // A merged base PR unstacks the PR first, before step 0 and every check after it, and the unstack stays when one of
    // them refuses (contract §7.6 `pr open` "Stacked", hs-d301): GitHub deleted the merged branch, never look it up.
    unstack_stored(&mut env.store, COMMAND, &id, &key, "pr open")?;
    let plan = env.plan(&id)?;

    // No lock and no R (hs-d243): an unrecorded merge leaves its Job `running`,
    // which `jobs_not_done` refused above.
    let pr = pr_of(&plan, &key);
    let lead = pr.lead(&id)?.clone();
    let branch = pr.lead_branch.clone();
    // 0. The newest pull request of the Lead Branch, in any state, before the Gate (contract §7.6, hs-n236): a merged
    // one is adopted as it is, with no Gate and no push; a closed one is refused; an open one is adopted after the push.
    let mut existing = None;
    if let Some(head) = gh::head_pr(&lead, &branch)? {
        match head.state {
            HeadState::Merged => return adopt_merged(&mut env, &plan, &key, &lead, &head),
            HeadState::Closed => {
                return Err(CpdError::new(
                    ErrorCode::PrClosed,
                    format!(
                        "The pull request {} of PR {key}'s Lead Branch {branch} is closed without a merge: reopen it on GitHub, or move the work to a new branch.",
                        head.url
                    ),
                )
                .with("plan", id.as_str())
                .with("pr", key.as_str())
                .with("number", head.number)
                .with("url", head.url.as_str()));
            }
            HeadState::Open => existing = Some(PrRef { number: head.number, url: head.url }),
        }
    }
    // 1. The base check on the Lead Branch's head, then G, read once, and the same check on G (hs-d329), before the
    // Gate, under `pr.gate` `ci` as under `local`; 2. the Gate on G, in a gate worktree of its own.
    let repo = Path::new(&pr.repo);
    require_base_on_remote(&mut env, repo, pr)?;
    let base_sha = fresh_base(repo, &pr.base)?;
    let lead_ref = format!("refs/heads/{branch}");
    require_base(repo, &lead, pr, &base_sha, &git::run::rev_parse(repo, &lead_ref)?)?;
    let sha = git::run::rev_parse(repo, &lead_ref)?;
    require_base(repo, &lead, pr, &base_sha, &sha)?;
    gate(&env, &policy, &id, pr, &sha, &format!("pr open --pr {key}"))?;

    // The PR is read again: another `pr open` may have opened it during the Gate.
    let plan = env.plan(&id)?;
    if let Step::Already(done) = require_dispatch(&plan, &args)? {
        return Ok(done);
    }
    let pr = pr_of(&plan, &key);

    // 3. Rule P with G, then the pull request.
    let (pushed, remote) = match git::push_commit(&lead, &branch, &sha)? {
        git::Push::Pushed { new } => (new, sha.clone()),
        git::Push::Contained { remote } => (false, remote),
        git::Push::Rejected { stderr } => {
            return Err(CpdError::new(
                ErrorCode::GitFailed,
                format!("git push of {} to {branch} on origin was rejected, and the remote branch does not contain it: nothing was opened.", short(&sha)),
            )
            .with("plan", id.as_str())
            .with("pr", key.as_str())
            .with("sha", sha.as_str())
            .with("stderr", stderr));
        }
    };
    let title = github_title(&stored);
    let found = find_or_create(&lead, pr, existing, &title, &list)?;
    let (repo, _) = pulls::parse_url(&found.url).ok_or_else(|| {
        CpdError::new(ErrorCode::GhFailed, format!("gh printed the pull request URL {:?}, which is not https://github.com/<owner>/<name>/pull/<n>.", found.url))
            .with("url", found.url.as_str())
    })?;
    // GitHub's creation time, written once with the row (store-W5): `gh pr create` and `gh pr list` do not give it here,
    // the poller's own lookup does. A pull request it cannot find is `gh_failed`; a rerun adopts the one created.
    let created_at = match gh::lookup(&lead, &[(repo.clone(), found.number)])?.into_iter().next().flatten() {
        Some(looked_up) => looked_up.created_at,
        None => {
            return Err(CpdError::new(ErrorCode::GhFailed, format!("gh cannot find pull request {repo}#{}, which it just opened.", found.number))
                .with("url", found.url.as_str()));
        }
    };
    let phase = env.store.write(COMMAND, |tx| {
        // The pull request's row (D6): inserted, or kept as the poll wrote it when it stored it first.
        let row = PolledPr {
            repo,
            number: found.number,
            url: found.url.clone(),
            title,
            draft: false,
            state: PrState::Open,
            review_decision: ReviewDecision::None,
            checks: ChecksSummary::None,
            mergeable: Mergeable::Unknown,
            merge_state: MergeState::Unknown,
            created_at,
            merged_at: None,
        };
        let pull_request = pulls::ensure(tx, &row)?;
        let moved = tx.execute(
            "UPDATE prs SET status = 'open', pr_number = ?1, pr_url = ?2, pr_opened_at = (SELECT at FROM txns WHERE id = cpd_txn()),
                    pull_request = ?3
              WHERE plan = ?4 AND key = ?5 AND status = 'dispatch'",
            rusqlite::params![found.number, found.url, pull_request, id.as_str(), key.as_str()],
        )?;
        if moved == 0 {
            return Err(cas_lost(&id, &key));
        }
        let text = format!("PR {key} of Plan {id} opened as #{}: {}. The engine polls it; to follow it by hand: cpd pr poll --plan {id}", found.number, found.url);
        append(tx, NewEvent::pr_opened(text, &key, found.number, &found.url).plan(id.clone()))?;

        // The last PR to open moves the Plan on (§3.1, A10).
        let waiting: i64 = tx.query_row(
            "SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('open', 'merged', 'cancelled')",
            [id.as_str()],
            |r| r.get(0),
        )?;
        if waiting > 0 {
            return Ok(Phase::Dispatch);
        }
        let moved = tx.execute("UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'", [id.as_str()])?;
        if moved == 0 {
            return Err(cas_lost(&id, &key));
        }
        let opened = opened_prs(tx, &id)?;
        let text = format!("Every PR of Plan {id} is open. The engine polls them; to follow them by hand: cpd pr poll --plan {id}");
        append(tx, NewEvent::plan_phase_deployed(text, &opened).plan(id.clone()))?;
        Ok(Phase::Deploy)
    })?;

    let plan = env.plan(&id)?;
    Ok(Output::success(
        json!({"plan": id, "phase": phase.as_str(), "pr": pr_of(&plan, &key).to_json(), "sha": sha, "pushed": pushed, "remote": remote}),
    ))
}

/// A stacked PR's base branch, its base PR's Lead Branch, is on the remote (contract §7.6 `pr open` step 1), else
/// `base_not_found` (exit 4): the pull request targets it, and the base PR's Lead pushes it. A PR that is not stacked
/// has its base fetched next, and a repo with no `origin` has no pull request to target a branch of.
fn require_base_on_remote(env: &mut Env, repo: &Path, pr: &PrRow) -> Result<(), CpdError> {
    let Some(base_pr) = env.store.read(|conn| base_pr(conn, pr))? else { return Ok(()) };
    if !git::run::has_origin(repo)? {
        return Ok(());
    }
    let listed = git::run::run(repo, &["ls-remote", "--exit-code", "--heads", "origin", &pr.base])?;
    match listed.code {
        0 => Ok(()),
        2 => Err(CpdError::new(
            ErrorCode::BaseNotFound,
            format!(
                "PR {}'s base {}, the Lead Branch of {base_pr}, is not on origin: the pull request cannot target it. Its Lead pushes it (cpd pr push), then run cpd pr open again. Nothing was gated, pushed or opened.",
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

/// `base_behind` (exit 3) unless `base_sha` is an ancestor of `sha`: a pull request is opened only when its head
/// contains its base (hs-d329). `details.next` is what the Lead runs, as written.
fn require_base(repo: &Path, lead: &Path, pr: &PrRow, base_sha: &str, sha: &str) -> Result<(), CpdError> {
    let out = git::run::run(repo, &["merge-base", "--is-ancestor", base_sha, sha])?;
    match out.code {
        0 => Ok(()),
        1 => {
            let remote = if git::run::has_origin(repo)? { format!("origin/{}", pr.base) } else { pr.base.clone() };
            Err(CpdError::new(
                ErrorCode::BaseBehind,
                format!(
                    "{} of PR {} does not contain {remote} ({}): merge it into the Lead Branch, then run cpd pr open again. Nothing was gated, pushed or opened.",
                    short(sha),
                    pr.key,
                    short(base_sha)
                ),
            )
            .with("pr", pr.key.as_str())
            .with("base", pr.base.as_str())
            .with("base_sha", base_sha)
            .with("sha", sha)
            .with(
                "next",
                json!([format!("git -C {} merge --no-edit {remote}", lead.display()), format!("cpd pr open --pr {}", pr.key)]),
            ))
        }
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-base --is-ancestor exited {code}."))
            .with("exit", code)
            .with("stderr", out.stderr)),
    }
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
    match gh::create_pr(lead, &pr.base, &pr.lead_branch, title, &summary(title, list)) {
        Ok(created) => Ok(created),
        Err(failed) => gh::head_pr(lead, &pr.lead_branch)
            .ok()
            .flatten()
            .filter(|h| h.state == HeadState::Open)
            .map(|h| PrRef { number: h.number, url: h.url })
            .ok_or(failed),
    }
}

/// A pull request of the Lead Branch that is already `merged` on GitHub (contract §7.6 `pr open` step 0): no Gate and
/// no push. One transaction records it and merges the PR as `pr poll` does (`pr-merged`, the Plan `deploy` and `done`).
fn adopt_merged(env: &mut Env, plan: &PlanPrs, key: &PrKey, lead: &Path, head: &gh::HeadPr) -> Result<Output, CpdError> {
    let id = &plan.plan.id;
    let (repo, _) = pulls::parse_url(&head.url).ok_or_else(|| {
        CpdError::new(ErrorCode::GhFailed, format!("gh printed the pull request URL {:?}, which is not https://github.com/<owner>/<name>/pull/<n>.", head.url))
            .with("url", head.url.as_str())
    })?;
    let looked_up = match gh::lookup(lead, &[(repo.clone(), head.number)])?.into_iter().next().flatten() {
        Some(looked_up) if looked_up.state == PrState::Merged => looked_up,
        Some(other) => {
            return Err(CpdError::new(
                ErrorCode::GhFailed,
                format!("gh lists pull request {repo}#{} as merged but reads it {}.", head.number, other.state.as_str()),
            )
            .with("url", head.url.as_str()));
        }
        None => {
            return Err(CpdError::new(ErrorCode::GhFailed, format!("gh cannot find pull request {repo}#{}, which it lists.", head.number))
                .with("url", head.url.as_str()));
        }
    };
    let phase = env.store.write(COMMAND, |tx| {
        // A merge time: GitHub's, else this transaction's (as the poll).
        let mut polled = looked_up.clone();
        if polled.merged_at.is_none() {
            polled.merged_at = Some(pulls::txn_time(tx)?);
        }
        // The row as this lookup reads it, with the searches that listed it as the poll left them (the User's own when new).
        let membership = pulls::find(tx, &polled.repo, polled.number)?
            .map_or(Membership { authored: true, review_requested: false }, |row| row.membership());
        pulls::upsert(tx, &Listed { pr: polled.clone(), membership })?;
        let pull_request = pulls::find(tx, &polled.repo, polled.number)?.expect("the row was just written").id;
        // `prs_status` allows no `dispatch → merged`: through `open`, in this one transaction.
        let opened = tx.execute(
            "UPDATE prs SET status = 'open', pr_number = ?1, pr_url = ?2, pr_opened_at = (SELECT at FROM txns WHERE id = cpd_txn()),
                    pull_request = ?3
              WHERE plan = ?4 AND key = ?5 AND status = 'dispatch'",
            rusqlite::params![head.number, head.url, pull_request, id.as_str(), key.as_str()],
        )?;
        let merged = tx.execute(
            "UPDATE prs SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND key = ?2 AND status = 'open'",
            [id.as_str(), key.as_str()],
        )?;
        if opened == 0 || merged == 0 {
            return Err(cas_lost(id, key));
        }
        let text = format!("PR {key} (#{}) of Plan {id} is merged.", head.number);
        append(tx, NewEvent::pr_merged(text, key, head.number, &head.url, polled.merged_at.as_deref()).plan(id.clone()))?;

        let waiting: i64 = tx.query_row(
            "SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('open', 'merged', 'cancelled')",
            [id.as_str()],
            |r| r.get(0),
        )?;
        if waiting > 0 {
            return Ok(Phase::Dispatch);
        }
        let moved = tx.execute("UPDATE plans SET phase = 'deploy' WHERE id = ?1 AND phase = 'dispatch'", [id.as_str()])?;
        if moved == 0 {
            return Err(cas_lost(id, key));
        }
        let text = format!("Every PR of Plan {id} is open or merged. The engine polls them; to follow them by hand: cpd pr poll --plan {id}");
        append(tx, NewEvent::plan_phase_deployed(text, opened_prs(tx, id)?).plan(id.clone()))?;
        // The last PR to merge ends the Plan (§3.1).
        let unmerged: i64 =
            tx.query_row("SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled')", [id.as_str()], |r| r.get(0))?;
        if unmerged > 0 {
            return Ok(Phase::Deploy);
        }
        let moved = tx.execute(
            "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND phase = 'deploy'",
            [id.as_str()],
        )?;
        if moved == 0 {
            return Err(cas_lost(id, key));
        }
        let text = format!("Every PR of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}");
        append(tx, NewEvent::plan_done(text, merged_prs(tx, id)?).plan(id.clone()))?;
        Ok(Phase::Done)
    })?;

    let plan = env.plan(id)?;
    Ok(Output::success(json!({"adopted": "merged", "plan": id, "phase": phase.as_str(), "pr": pr_of(&plan, key).to_json()})))
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

/// Every Job of the PR `merged` or `cancelled`, and at least one `merged`; else `jobs_not_done`.
fn require_jobs_done(env: &mut Env, plan: &PlanPrs, key: &PrKey) -> Result<Vec<jobs::Job>, CpdError> {
    let all: Vec<jobs::Job> = env.store.read(|conn| jobs::load(conn, &plan.plan.id))?.into_iter().filter(|j| j.pr == key.as_str()).collect();
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
