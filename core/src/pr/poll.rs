//! `pr poll` (commands.md §9.2, contract §7.6): one poll of every open pull
//! request of the User, stored in `pull_requests`, and the merge of the Workflows
//! the Plans own.

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::comments::{self, Read};
use super::deploys;
use super::feedback;
use super::gh::{self, Listed};
use super::posts;
use super::pulls::{self, Change, Excused, PostFacts, Reason};
use super::row::PlanWorkflows;
use super::start::unstack;
use super::status::{ChecksSummary, Mergeable, PrState};
use super::{Env, PollArgs};
use crate::config::Tier;
use crate::error::{CpdError, ErrorCode};
use crate::events::{BasePull, NewEvent, append};
use crate::ids::{JobKey, OwnerLogin, PlanId, RepoName, WorkflowKey};
use crate::inbox::model as inbox;
use crate::job::open::{self, Cause};
use crate::lock;
use crate::output::Output;
use crate::plan::fingerprint::load_workflows;
use crate::policy::Checks;
use crate::ports::Ports;
use crate::store::WriteTxn;

const COMMAND: &str = "pr poll";

/// The `by` of the `workflow-base-changed` Event the poll appends (§5.2).
const COMMAND_BY: &str = "poll";

/// How long `pr poll` waits for `locks/pr-poll.lock` (`lock_timeout`).
const LOCK_WAIT: Duration = Duration::from_secs(60);

/// How often the wait tries the lock again.
const RETRY: Duration = Duration::from_millis(100);

/// A pull request an `open` Workflow of a Plan points to.
type Owned = (PlanId, WorkflowKey, pulls::PullRequest);

/// An owned Workflow whose policy did not resolve (`owner_unset`, `owner_unconfirmed`, `policy_invalid`).
struct Unresolved {
    plan: PlanId,
    workflow: WorkflowKey,
    number: i64,
    error: CpdError,
}

/// A Workflow whose pull request GitHub repointed: the Workflow's `base` follows it (§7.6 step 6, hs-d325).
struct Follow {
    plan: PlanId,
    workflow: WorkflowKey,
    from: String,
    to: String,
    number: i64,
    url: String,
    /// The base Workflow of `from` (`plan/key`), when `from` is some Workflow's Lead Branch.
    from_workflow: Option<String>,
    /// The Workflow whose Lead Branch is `to`, when some Workflow's is.
    to_workflow: Option<String>,
    /// The pull request's repo default branch, from the listing: `to` is the default branch exactly when the base
    /// Workflow merged (`None` = GitHub did not say, and a base no default branch is no merge repoint).
    default_branch: Option<String>,
}

/// What the read phase learned of a [`Follow`] that is a base Workflow's merge to the default branch (§3.7 E5, hs-d794):
/// whether that default branch holds commits the Lead Branch lacks without a conflict.
struct Ahead {
    plan: PlanId,
    workflow: WorkflowKey,
    /// True when the default branch holds commits the Lead Branch lacks and the two merge cleanly.
    ahead: bool,
}

/// What the transaction did.
#[derive(Default)]
struct Outcome {
    changed: Vec<Value>,
    base_changed: Vec<Value>,
    feedback: Vec<(PlanId, WorkflowKey)>,
    /// The `update` Jobs the poll queued and their cause (§3.7 E5).
    update_jobs: Vec<(PlanId, WorkflowKey, JobKey, Cause)>,
    merged: Vec<(PlanId, WorkflowKey)>,
    done: Vec<PlanId>,
}

pub(super) fn poll(ports: &Ports, args: PollArgs) -> Result<Output, CpdError> {
    let mut env = Env::open_with(ports)?;
    let selected = args.plan.plan;
    if let Some(id) = &selected {
        env.plan(id)?;
    }
    let _lock = take_lock_with(&env.ports, &env.home.pr_poll_lock())?;
    // A pass that reopened a Workflow is run again: the Workflow is owned now, so its policy, comments and checks are read as
    // any owned Workflow's are (each pass reopens fewer: a reopened Workflow is no longer a candidate).
    loop {
        let (out, again) = poll_pass(&mut env, &selected)?;
        if !again {
            return Ok(out);
        }
    }
}

fn poll_pass(env: &mut Env, selected: &Option<PlanId>) -> Result<(Output, bool), CpdError> {
    let root = env.home.root().to_owned();

    let search = gh::search_open(&*env.ports.runner, &root, &env.config.inbox.bot_reviewers)?;
    let owned = env.store.read(pulls::owned_open)?;
    // Each owned Workflow's `pr.feedback.checks`, resolved before anything is written (§8.3). A Workflow whose policy does not
    // resolve is told nothing and reported below; the poll goes on for the others (§7.6).
    let mut checks: Vec<Resolved> = Vec::new();
    let mut unresolved = Vec::new();
    for (plan, key, row) in &owned {
        match super::workflow_policy_resolved(env, plan, key, crate::policy::resolve)? {
            Ok(policy) => checks.push(Resolved {
                plan: plan.clone(),
                key: key.clone(),
                checks: policy.pr.feedback.checks,
                post_enabled: policy.pr.post.enabled,
                human_gated: policy.pr.human_gated_contexts.clone(),
                excused_labels: policy.pr.excused_labels.clone(),
                excused: false,
                required: policy.pr.required_checks.clone(),
            }),
            Err(error) => unresolved.push(Unresolved {
                plan: plan.clone(),
                workflow: key.clone(),
                number: row.number,
                error,
            }),
        }
    }

    // The stored open pull requests a search missed are read directly (§7.6 step 3): owned and listed by neither search,
    // or flagged for a search that did not list them. A search's absence deletes, moves and unflags nothing.
    let stored = env.store.read(pulls::stored_open)?;
    let listed_in = |row: &pulls::PullRequest| {
        search
            .prs
            .iter()
            .find(|l| l.pr.repo == row.repo && l.pr.number == row.number)
    };
    let candidates: Vec<&pulls::PullRequest> = stored
        .iter()
        .filter(|row| {
            let found = listed_in(row).map(|l| l.membership).unwrap_or_default();
            let is_owned = owned.iter().any(|(_, _, o)| o.id == row.id);
            (is_owned && listed_in(row).is_none())
                || (row.authored && !found.authored)
                || (row.review_requested && !found.review_requested)
        })
        .collect();
    let keys: Vec<(String, i64)> = candidates
        .iter()
        .map(|row| (row.repo.clone(), row.number))
        .collect();
    let read = gh::read_absent(
        &*env.ports.runner,
        &root,
        &keys,
        &env.config.inbox.bot_reviewers,
    )?;
    let mut seen: Vec<Listed> = search
        .prs
        .iter()
        .filter(|l| !keys.contains(&(l.pr.repo.clone(), l.pr.number)))
        .cloned()
        .collect();
    for (row, read) in candidates.iter().zip(read) {
        let found = listed_in(row);
        match read {
            // `OPEN`: the read is fresher; each flag a search listed it in stays, and the read adds its own.
            Some(Listed { pr, membership }) if pr.state == PrState::Open => {
                let search_flags = found.map(|l| l.membership).unwrap_or_default();
                let membership = gh::Membership {
                    authored: search_flags.authored || membership.authored,
                    review_requested: search_flags.review_requested || membership.review_requested,
                };
                seen.push(Listed { pr, membership });
            }
            // `MERGED` or `CLOSED`: the row takes the state, with no flag.
            Some(decided) => seen.push(decided),
            // Not found: an unowned row loses its flags and keeps its state; an owned row is unchanged.
            None if !owned.iter().any(|(_, _, o)| o.id == row.id) => {
                seen.push(Listed {
                    pr: row.polled()?,
                    membership: Default::default(),
                });
            }
            None => seen.extend(found.cloned()),
        }
    }

    // The comments of the owned open pull requests whose `updatedAt` moved or whose stored feedback holds an open item (§7.6 step 3a): one call, before anything is
    // written, so that a failed read writes nothing.
    let due = due_reads(&owned, &seen);
    let reads = gh::read_comments(
        &*env.ports.runner,
        &root,
        &due,
        &env.config.inbox.bot_reviewers,
    )?;

    // The checks of each owned open pull request (§7.6 step 3b) whose `pr.feedback.checks` is `any` and whose rollup says `failed`
    // (GitHub rolls a CANCELLED check up as FAILURE), or whose rollup says `pending` while its `human_gated_contexts` is not
    // empty, or whose rollup says `passed` or `none` while its `required_checks` is not empty (hs-d743): one call, before anything is written. Each Workflow's summary is the one rule with its own list.
    let mut confirm: Vec<(String, i64)> = Vec::new();
    let mut lists: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    for listed in &seen {
        let (repo, number) = (&listed.pr.repo, listed.pr.number);
        if listed.pr.state != PrState::Open || confirm.contains(&(repo.clone(), number)) {
            continue;
        }
        let mine = owned
            .iter()
            .filter(|(_, _, row)| row.repo == *repo && row.number == number)
            .filter_map(|(plan, key, _)| checks_of(&checks, plan, key));
        // (`checks` is `any`, `human_gated_contexts`, `required_checks`) of each owner; a Workflow no Plan resolved a policy for has its repo's `required_checks` alone.
        let mut policies: Vec<(bool, Vec<String>, Vec<String>)> = mine
            .map(|r| {
                (
                    r.checks == Checks::Any,
                    r.human_gated.clone(),
                    r.required.clone(),
                )
            })
            .collect();
        if policies.is_empty() {
            policies.push((
                false,
                Vec::new(),
                repo_required(&env.home, repo).unwrap_or_default(),
            )); // an unreadable policy is reported for the owned Workflows above
        }
        let wanted =
            |(any, gated, required): &&(bool, Vec<String>, Vec<String>)| match listed.pr.checks {
                ChecksSummary::Failed => *any,
                ChecksSummary::Pending => !gated.is_empty(),
                ChecksSummary::Passed | ChecksSummary::None => !required.is_empty(),
            };
        if let Some((_, gated, required)) = policies.iter().find(wanted) {
            confirm.push((repo.clone(), number));
            lists.push((gated.clone(), required.clone()));
        }
    }
    let confirmed = gh::read_checks(&*env.ports.runner, &root, &confirm)?;
    for listed in &mut seen {
        if let Some(i) = confirm
            .iter()
            .position(|(repo, number)| *repo == listed.pr.repo && *number == listed.pr.number)
        {
            listed.pr.checks = ChecksSummary::of(&confirmed[i].checks, &lists[i].0, &lists[i].1);
        }
    }

    for (plan, key, row) in &owned {
        let Some(i) = confirm
            .iter()
            .position(|(repo, number)| *repo == row.repo && *number == row.number)
        else {
            continue;
        };
        if let Some(r) = checks.iter_mut().find(|r| r.plan == *plan && r.key == *key) {
            r.excused = confirmed[i]
                .labels
                .iter()
                .any(|l| r.excused_labels.iter().any(|x| x.eq_ignore_ascii_case(l)));
        }
    }

    // The owned Workflows whose pull request a search lists with a base other than the Workflow's `base`.
    let follows = env.store.read(|conn| follows(conn, &owned, &search.prs))?;
    // The base Workflow's merge repoints whose default branch is ahead of the Lead Branch (§3.7 E5, hs-d794): the git
    // check of the read phase (§7.6), before the write transaction of D5.
    let ahead = env.store.read(|conn| ahead_of(conn, &follows))?;

    let dirty = !follows.is_empty()
        || env.store.read(|conn| {
            for listed in &seen {
                match pulls::find(conn, &listed.pr.repo, listed.pr.number)? {
                    Some(row) if row.holds(listed, read_of(&reads, listed))? => {}
                    _ => return Ok(true),
                }
            }
            Ok(false)
        })?;
    // The `update` Job of a Workflow runs at `worker_tier` floored to a valid rung of that Workflow's catalog.
    let mut tiers: Vec<(PlanId, WorkflowKey, Tier)> = Vec::new();
    if dirty {
        let mut wanting: Vec<(PlanId, WorkflowKey)> = owned
            .iter()
            .filter(|(_, _, row)| {
                seen.iter().any(|l| {
                    l.pr.repo == row.repo
                        && l.pr.number == row.number
                        && l.pr.mergeable == Mergeable::Conflicting
                })
            })
            .map(|(plan, key, _)| (plan.clone(), key.clone()))
            .collect();
        wanting.extend(follows.iter().map(|f| (f.plan.clone(), f.workflow.clone())));
        for (plan, key) in wanting {
            let tier = open::tier_of_workflow(env, &plan, &key)?;
            tiers.push((plan, key, tier));
        }
    }
    // A poll that finds nothing new opens no write transaction (D5).
    let outcome = if dirty {
        env.store.write(COMMAND, |tx| {
            apply(tx, &seen, &reads, &owned, &checks, &follows, &ahead, &tiers)
        })?
    } else {
        Outcome::default()
    };

    report_unresolved(env, &unresolved)?;
    let reopened = rebind_reopened(env)?;
    // After the poll's own rows are written (§7.6 step 8): a failure of the deploy read loses none of them.
    deploys::read(&*env.ports.runner, &env.home, &crate::clock::now())?;

    let printed_prs = match &selected {
        Some(id) => {
            let plan = env.plan(id)?;
            Some(plan_workflows(env, &plan)?)
        }
        None => None,
    };

    let pair = |(plan, workflow): &(PlanId, WorkflowKey)| json!({"plan": plan, "pr": workflow});
    let mut out = json!({
        "polled": search.prs.len(),
        "changed": outcome.changed,
        "base_changed": outcome.base_changed,
        "feedback": outcome.feedback.iter().map(pair).collect::<Vec<_>>(),
        "update_jobs": outcome.update_jobs.iter().map(|(plan, workflow, job, cause)| json!({"plan": plan, "pr": workflow, "job": job, "reason": cause.as_str()})).collect::<Vec<_>>(),
        "merged": outcome.merged.iter().map(pair).collect::<Vec<_>>(),
        "done": outcome.done,
        "rate_limit": {"cost": search.rate.cost, "remaining": search.rate.remaining, "reset_at": search.rate.reset_at},
    });
    if let Some(workflows) = printed_prs {
        out["workflows"] = Value::Array(workflows);
    }
    Ok((Output::success(out), !reopened.is_empty()))
}

/// `flock` on `path`, waiting at most [`LOCK_WAIT`] on `ports.clock`: `lock_timeout` (exit 5). The
/// descriptor stays open while waiting.
pub(crate) fn take_lock_with(ports: &Ports, path: &Path) -> Result<fs::File, CpdError> {
    let failed = |what: &dyn std::fmt::Display| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot use the lock {}: {what}.", path.display()),
        )
        .with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| failed(&e))?;
    let deadline = ports.clock.now() + LOCK_WAIT;
    loop {
        if lock::try_lock(&file).map_err(|e| failed(&format!("flock failed: {e}")))? {
            return Ok(file);
        }
        if ports.clock.now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::LockTimeout,
                format!(
                    "Another pr poll held {} for longer than {}s.",
                    path.display(),
                    LOCK_WAIT.as_secs()
                ),
            )
            .with("lock", path.display().to_string())
            .with("waited_seconds", LOCK_WAIT.as_secs()));
        }
        ports.clock.sleep(RETRY);
    }
}

/// The one write transaction (D5, D7): every changed row, the Events of the
/// owning Plans.
#[allow(clippy::too_many_arguments)] // a signature change is out of scope for the lint cleanup
fn apply(
    tx: &WriteTxn<'_>,
    seen: &[Listed],
    reads: &[Read],
    owned: &[Owned],
    checks: &[Resolved],
    follows: &[Follow],
    ahead: &[Ahead],
    tiers: &[(PlanId, WorkflowKey, Tier)],
) -> Result<Outcome, CpdError> {
    let tier_of = |plan: &PlanId, key: &WorkflowKey| -> Result<Tier, CpdError> {
        tiers
            .iter()
            .find(|(p, k, _)| p == plan && k == key)
            .map(|(_, _, tier)| *tier)
            .ok_or_else(|| {
                CpdError::bad_row(format!(
                    "no open Tier was read for Workflow {key} of Plan {plan}"
                ))
            })
    };
    let mut out = Outcome::default();
    for listed in seen {
        // A merged row needs a merge time: GitHub's, else this poll's.
        let mut listed = listed.clone();
        if listed.pr.state == PrState::Merged && listed.pr.merged_at.is_none() {
            listed.pr.merged_at = Some(pulls::txn_time(tx)?);
        }
        let polled = &listed.pr;
        let Some(change) = pulls::upsert(tx, &listed, read_of(reads, &listed))? else {
            continue;
        };
        let after = &change.after;
        if after.state == "merged" {
            inbox::close_pr_ready(tx, &after.repo, after.number)?;
        }
        // A row only the comments read changed (its feedback and `updated_at`) is no change of the pull request.
        if change
            .before
            .as_ref()
            .is_none_or(|before| !before.holds(&listed, None).unwrap_or(false))
        {
            out.changed.push(json!({
                "repo": after.repo, "number": after.number, "state": after.state, "draft": after.draft,
                "checks": after.checks, "review_decision": after.review_decision,
                "mergeable": after.mergeable, "merge_state": after.merge_state,
            }));
        }
        for (plan, key, _) in owned.iter().filter(|(_, _, row)| row.id == after.id) {
            if polled.state == PrState::Merged {
                out.merged.push((plan.clone(), key.clone()));
                if merge(tx, plan, key, after)? {
                    out.done.push(plan.clone());
                }
                crate::incident::end_if_until_workflow_merged(tx)?;
                unstack_stacked_on(tx, plan, key)?;
            } else if let Some(resolved) = checks_of(checks, plan, key) {
                if tell(tx, plan, key, resolved, &change)? {
                    out.feedback.push((plan.clone(), key.clone()));
                }
                if let Some(job) =
                    update_for_conflict(tx, plan, key, &change, || tier_of(plan, key))?
                {
                    out.update_jobs
                        .push((plan.clone(), key.clone(), job, Cause::Conflict));
                }
            }
        }
    }
    for follow in follows {
        let is_ahead = ahead
            .iter()
            .any(|a| a.plan == follow.plan && a.workflow == follow.workflow && a.ahead);
        let (base_changed, job) = follow_base(
            tx,
            follow,
            is_ahead,
            tier_of(&follow.plan, &follow.workflow)?,
        )?;
        out.base_changed.push(base_changed);
        if let Some(job) = job {
            out.update_jobs.push((
                follow.plan.clone(),
                follow.workflow.clone(),
                job,
                Cause::BaseMerged,
            ));
        }
    }
    Ok(out)
}

/// The comments read of the pull request `listed`, if this poll made one.
fn read_of<'a>(reads: &'a [Read], listed: &Listed) -> Option<&'a Read> {
    reads
        .iter()
        .find(|r| r.repo == listed.pr.repo && r.number == listed.pr.number)
}

/// Each `open` Workflow of `owned` whose pull request a search lists (as `OPEN`) with a `baseRefName` that is not the Workflow's
/// `base` (§7.6 step 6): a repoint by GitHub, or a base retargeted by hand. A Workflow whose listing has no base is not followed.
/// The base Workflow of the old base, the Workflow of the new one and the repo's default branch are read here, so the write
/// transaction queues the update Job a base Workflow's merge needs in the same transaction as the repoint.
fn follows(
    conn: &rusqlite::Connection,
    owned: &[Owned],
    listed: &[Listed],
) -> Result<Vec<Follow>, CpdError> {
    let mut stored = conn.prepare(
        "SELECT base, repo FROM workflows WHERE plan = ?1 AND key = ?2 AND status = 'open'",
    )?;
    let base_workflow = |repo: &str, branch: &str| -> Result<Option<String>, CpdError> {
        Ok(conn
            .query_row(
                "SELECT plan || '/' || key FROM workflows WHERE repo = ?1 AND lead_branch = ?2",
                [repo, branch],
                |r| r.get(0),
            )
            .optional()?)
    };
    let mut found = Vec::new();
    for (plan, key, row) in owned {
        let Some(listing) = listed
            .iter()
            .find(|l| (&l.pr.repo, l.pr.number) == (&row.repo, row.number))
        else {
            continue;
        };
        let Some(to) = listing.pr.base_ref.as_ref() else {
            continue;
        };
        let stored: Option<(String, String)> = stored
            .query_row([plan.as_str(), key.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        if let Some((from, repo)) = stored.filter(|(from, _)| from != to) {
            found.push(Follow {
                plan: plan.clone(),
                workflow: key.clone(),
                from_workflow: base_workflow(&repo, &from)?,
                to_workflow: base_workflow(&repo, to)?,
                from,
                to: to.clone(),
                number: row.number,
                url: row.url.clone(),
                default_branch: listing.pr.default_branch.clone(),
            });
        }
    }
    Ok(found)
}

/// Sets the Workflow's `base` to the one GitHub reports and appends `workflow-base-changed` for its Plan (§5.2). Nothing else
/// changes: no Gate, no status, no `pr-feedback`, no `gh` call (hs-d326: the tree is unchanged, a ready Workflow stays ready).
/// When the new base is the default branch of a base Workflow's merge and `ahead` says the Lead Branch lacks commits of it
/// without a conflict, the Workflow's `update` Job is queued in the same transaction (§3.7 E5, hs-d794). Returns the Event's
/// data and the Job's key when one was queued.
fn follow_base(
    tx: &WriteTxn<'_>,
    follow: &Follow,
    ahead: bool,
    tier: Tier,
) -> Result<(Value, Option<JobKey>), CpdError> {
    let Follow {
        plan,
        workflow,
        from,
        to,
        number,
        url,
        from_workflow,
        to_workflow,
        default_branch,
    } = follow;
    let moved = tx.execute(
        "UPDATE workflows SET base = ?3 WHERE plan = ?1 AND key = ?2 AND status = 'open' AND base = ?4",
        [plan.as_str(), workflow.as_str(), to.as_str(), from.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(plan, workflow));
    }
    let reason = match (from_workflow, to_workflow) {
        (Some(base), None) => {
            format!("its base Workflow {base} merged and GitHub repointed the pull request")
        }
        _ => "the base changed on GitHub".to_owned(),
    };
    let text = format!(
        "Workflow {workflow} of Plan {plan} now targets {to} (was {from}): {reason}. Its tree is unchanged: no new Gate is needed and it stays ready. \
         Read it with cpd plan show --plan {plan}."
    );
    let event = NewEvent::workflow_base_changed(
        text,
        workflow,
        (from, to),
        (from_workflow.as_deref(), to_workflow.as_deref()),
        COMMAND_BY,
        BasePull::Of(*number, url),
    );
    append(tx, event.plan(plan.clone()))?;
    // A base Workflow's merge repoints the pull request to the default branch; with that branch ahead and no conflict, the
    // Workflow's update Job is the cause (§3.7 E5, hs-d794). A hand retarget to another branch queues nothing.
    let base_merged = default_branch.as_deref() == Some(to.as_str())
        && from_workflow.is_some()
        && to_workflow.is_none();
    let job = if base_merged {
        queue_update_for(tx, plan, workflow, Trigger::BaseMerged, ahead, tier)?
    } else {
        None
    };
    Ok((
        json!({"plan": plan, "pr": workflow, "from": from, "to": to}),
        job,
    ))
}

/// The default-branch-ahead decision of each base Workflow's merge repoint (§3.7 E5, hs-d794), from git in the read phase
/// (§7.6): `git fetch origin <base>`, `git merge-tree --write-tree --name-only origin/<base> <lead>` and
/// `git merge-base --is-ancestor`, in the Workflow's repo (`prs.repo`). A failed fetch or check is no ahead: the poll queues
/// nothing for that Workflow and goes on (A4), with no panic and no unwrap.
fn ahead_of(conn: &rusqlite::Connection, follows: &[Follow]) -> Result<Vec<Ahead>, CpdError> {
    let mut out = Vec::new();
    for follow in follows {
        if !(follow.default_branch.as_deref() == Some(follow.to.as_str())
            && follow.from_workflow.is_some()
            && follow.to_workflow.is_none())
        {
            continue;
        }
        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT repo, lead_branch FROM workflows WHERE plan = ?1 AND key = ?2",
                [follow.plan.as_str(), follow.workflow.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((repo, lead)) = row else { continue };
        out.push(Ahead {
            plan: follow.plan.clone(),
            workflow: follow.workflow.clone(),
            ahead: base_ahead(Path::new(&repo), &follow.to, &lead),
        });
    }
    Ok(out)
}

/// Whether `origin/<base>` holds commits `lead` lacks without a conflict, in `dir`: `fetch`, a clean `merge-tree`
/// (`None`), then `merge-base --is-ancestor` exit 1 (not an ancestor). Any failure answers `false` (A4).
fn base_ahead(dir: &Path, base: &str, lead: &str) -> bool {
    let remote = format!("origin/{base}");
    if crate::git::run::fetch(dir, &["origin", base]).is_err() {
        return false;
    }
    match crate::git::worktree::merge_conflicts(dir, &remote, lead) {
        Ok(None) => {}
        Ok(Some(_)) | Err(_) => return false,
    }
    matches!(crate::git::run::run(dir, &["merge-base", "--is-ancestor", &remote, lead]), Ok(out) if out.code == 1)
}

/// The Workflow's resolved `pr.feedback.checks`; `None` when its policy did not resolve: it is told nothing.
fn checks_of<'a>(checks: &'a [Resolved], plan: &PlanId, key: &WorkflowKey) -> Option<&'a Resolved> {
    checks.iter().find(|r| &r.plan == plan && &r.key == key)
}

/// `required_checks` of `repo`'s own policy (hs-d743): resolved with the owner's file, or, when the owner has none, from the repo's
/// file alone.
pub(super) fn repo_required(
    home: &crate::home::CpdHome,
    repo: &str,
) -> Result<Vec<String>, CpdError> {
    use crate::policy::{
        Domain, PolicyKey, ScopeKind, Source, load_scope_file, repo_path, resolve_unowned_row,
    };
    let bad = || {
        CpdError::new(
            ErrorCode::Usage,
            format!("{repo:?} is not an owner/name repo."),
        )
        .with("repo", repo)
    };
    let (owner, name) = repo.split_once('/').ok_or_else(bad)?;
    let (owner, name) = (OwnerLogin::parse(owner)?, RepoName::parse(name)?);
    let resolved = resolve_unowned_row(home, &PolicyKey::Repo(owner.clone(), name.clone()))?;
    if resolved.sources.get(Domain::Pr).source != Source::DefaultNoOwnerFile {
        return Ok(resolved.policy.pr.required_checks);
    }
    Ok(
        load_scope_file(&repo_path(home, &owner, &name), ScopeKind::Repo)?
            .and_then(|(_, scope)| scope.pr)
            .and_then(|pr| pr.required_checks)
            .unwrap_or_default(),
    )
}

/// What the poll resolved of an owned Workflow's policy: `pr.feedback.checks` and `pr.post.enabled`.
struct Resolved {
    plan: PlanId,
    key: WorkflowKey,
    checks: Checks,
    post_enabled: bool,
    human_gated: Vec<String>,
    excused_labels: Vec<String>,
    /// Set once the checks are read: the Workflow carries one of `excused_labels`. A Workflow not read is never excused.
    excused: bool,
    required: Vec<String>,
}

/// One `error` Event for each Workflow whose policy did not resolve (§7.6): `source` `pr poll`, the resolve error's `code`,
/// `details` the error's with `plan`, `pr` and `number`. Once per episode: not again while the latest such Event of
/// the Workflow has the same code, so a poll every `pr_poll_seconds` does not repeat it, and no write transaction opens.
fn report_unresolved(env: &mut Env, unresolved: &[Unresolved]) -> Result<(), CpdError> {
    let fresh: Vec<&Unresolved> = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT json_extract(data, '$.code') FROM events
              WHERE kind = 'error' AND plan = ?1 AND json_extract(data, '$.source') = ?2
                AND json_extract(data, '$.details.pr') = ?3 AND json_extract(data, '$.code') <> ?4
              ORDER BY id DESC LIMIT 1",
        )?;
        let mut fresh = Vec::new();
        for failed in unresolved {
            let latest: Option<String> = stmt
                .query_row(
                    [
                        failed.plan.as_str(),
                        COMMAND,
                        failed.workflow.as_str(),
                        ErrorCode::CleanupFailed.as_str(),
                    ],
                    |r| r.get(0),
                )
                .optional()?;
            if latest.as_deref() != Some(failed.error.code().as_str()) {
                fresh.push(failed);
            }
        }
        Ok(fresh)
    })?;
    if fresh.is_empty() {
        return Ok(());
    }
    env.store.write(COMMAND, |tx| {
        for Unresolved { plan, workflow, number, error } in &fresh {
            let message = format!("The policy of Workflow {workflow} (#{number}) of Plan {plan} did not resolve: {}", error.message());
            let next = match error.details().get("next").and_then(Value::as_str) {
                Some(next) => next.to_owned(),
                None => format!("cpd policy show --workflow {workflow} --plan {plan}"),
            };
            let text = format!("{message} The poll sends this Workflow no pr-feedback until it does; every other Workflow is polled. Next: {next}");
            let mut details = error.details().clone();
            details.insert("plan".into(), json!(plan));
            details.insert("pr".into(), json!(workflow));
            details.insert("number".into(), json!(number));
            append(tx, NewEvent::error(text, COMMAND, error.code().as_str(), &message, details).plan(plan.clone()))?;
        }
        Ok(())
    })
}

/// Pro-d362: each cancelled Workflow whose pull request the poll stored `open` is reopened (`git fetch` and `gh` run here, in the
/// poll subprocess, outside any write transaction). A refusal appends one `error` Event, not one per tick, and never aborts the poll.
fn rebind_reopened(env: &mut Env) -> Result<Vec<(PlanId, WorkflowKey)>, CpdError> {
    let mut reopened = Vec::new();
    const SOURCE: &str = "workflow reopen";
    for c in env.store.read(crate::workflow::reopen::reopenable)? {
        let error = match crate::workflow::reopen::reopen(env, &c.plan, &c.key) {
            Ok(done) => {
                if done.changed {
                    reopened.push((c.plan.clone(), c.key.clone()));
                }
                continue;
            }
            Err(error) => error,
        };
        let latest: Option<String> = env.store.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT json_extract(data, '$.code') FROM events
                      WHERE kind = 'error' AND plan = ?1 AND json_extract(data, '$.source') = ?2
                        AND json_extract(data, '$.details.pr') = ?3
                      ORDER BY id DESC LIMIT 1",
                    [c.plan.as_str(), SOURCE, c.key.as_str()],
                    |r| r.get(0),
                )
                .optional()?)
        })?;
        if latest.as_deref() == Some(error.code().as_str()) {
            continue;
        }
        let message = format!(
            "Workflow {} of Plan {} could not be reopened: {}",
            c.key,
            c.plan,
            error.message()
        );
        env.store.write(COMMAND, |tx| {
            let mut details = error.details().clone();
            details.insert("plan".into(), json!(c.plan));
            details.insert("pr".into(), json!(c.key));
            append(
                tx,
                NewEvent::error(
                    message.clone(),
                    SOURCE,
                    error.code().as_str(),
                    &message,
                    details,
                )
                .plan(c.plan.clone()),
            )
        })?;
    }
    Ok(reopened)
}

/// D7: `pr-feedback` to the Plan, when the change needs its attention.
fn tell(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    key: &WorkflowKey,
    resolved: &Resolved,
    change: &Change,
) -> Result<bool, CpdError> {
    let open = change.after.feedback.as_deref().unwrap_or_default();
    let new = comments::new_items(
        change.before.as_ref().and_then(|b| b.feedback.as_deref()),
        open,
    );
    let post = PostFacts {
        enabled: resolved.post_enabled,
        posted: posts::find(tx, plan.as_str(), key.as_str())?.is_some(),
    };
    let reasons: Vec<Reason> = pulls::attention(
        change.before.as_ref(),
        &change.after,
        resolved.checks,
        &new,
        post,
        Excused(resolved.excused),
    );
    if reasons.is_empty() {
        return Ok(false);
    }
    let text = feedback::text(
        plan.as_str(),
        key.as_str(),
        change.after.number,
        &reasons,
        (new.len(), open.len()),
    );
    append(
        tx,
        feedback::event(text, key.as_str(), &change.after, &reasons).plan(plan.clone()),
    )?;
    Ok(true)
}

/// The `update` Job of a conflict (§3.7 E5, §7.6 step 6): when `mergeable` became `conflicting` (from anything else, so once per
/// transition: `unknown` is no conflict and a base that merely moved, `merge_state` `behind`, is no cause), the row's `state` is `open`
/// and the Plan is not archived, the Workflow's `update` Job is queued unless one is `queued` or `running`. The caller has resolved the
/// Workflow's policy. The Job's key when one was inserted.
fn update_for_conflict(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    key: &WorkflowKey,
    change: &Change,
    tier: impl FnOnce() -> Result<Tier, CpdError>,
) -> Result<Option<JobKey>, CpdError> {
    let became = change.after.mergeable == "conflicting"
        && change
            .before
            .as_ref()
            .is_none_or(|b| b.mergeable != "conflicting");
    if !became || change.after.state != "open" {
        return Ok(None);
    }
    let archived: bool = tx.query_row(
        "SELECT archived_at IS NOT NULL FROM plans WHERE id = ?1",
        [plan.as_str()],
        |r| r.get(0),
    )?;
    if archived {
        return Ok(None);
    }
    let workflows = load_workflows(tx, plan)?;
    let Some(workflow) = workflows.iter().find(|p| p.key == *key) else {
        return Ok(None);
    };
    open::queue_update(tx, plan, workflow, tier()?, COMMAND_BY, Cause::Conflict)
}

/// Why the poll queues a Workflow's `update` Job (§3.7 E5, hs-d794, hs-n617): the conflict transition
/// ([`update_for_conflict`]), or a base Workflow's merge that left the default branch ahead of the Lead Branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Trigger {
    /// The Workflow's `mergeable` became `conflicting`.
    #[cfg(test)]
    Conflict,
    /// GitHub repointed the Workflow from a base Workflow's Lead Branch to the default branch, and the default
    /// branch holds commits the Lead Branch lacks without a conflict.
    BaseMerged,
}

/// Queues Workflow `key`'s `update` Job for `trigger` (§3.7 E5, hs-d794, hs-n617), unless the Workflow already has one `queued`
/// or `running`. `ahead` is read only for [`Trigger::BaseMerged`]: true when the default branch holds commits the
/// Lead Branch lacks without a conflict; false when it is already contained, which queues nothing. A Plan that is
/// archived or a Workflow that is not `open` queues nothing. The Job's key when one was inserted.
pub(super) fn queue_update_for(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    key: &WorkflowKey,
    trigger: Trigger,
    ahead: bool,
    tier: Tier,
) -> Result<Option<JobKey>, CpdError> {
    let cause = match trigger {
        #[cfg(test)]
        Trigger::Conflict => Cause::Conflict,
        Trigger::BaseMerged if ahead => Cause::BaseMerged,
        Trigger::BaseMerged => return Ok(None),
    };
    let archived: bool = tx.query_row(
        "SELECT archived_at IS NOT NULL FROM plans WHERE id = ?1",
        [plan.as_str()],
        |r| r.get(0),
    )?;
    if archived {
        return Ok(None);
    }
    let workflows = load_workflows(tx, plan)?;
    let Some(workflow) = workflows
        .iter()
        .find(|p| p.key == *key)
        .filter(|p| p.status == "open")
    else {
        return Ok(None);
    };
    open::queue_update(tx, plan, workflow, tier, COMMAND_BY, cause)
}

/// The Workflow `key` of the Plan is merged on GitHub (§7.6): `prs` `merged`,
/// `pr-merged`, and, when it was the last Workflow, the Plan `done` with `plan-done`
/// (§3.1). Whether the Plan became `done`.
pub(super) fn merge(
    tx: &WriteTxn<'_>,
    id: &PlanId,
    key: &WorkflowKey,
    pull: &pulls::PullRequest,
) -> Result<bool, CpdError> {
    let moved = tx.execute(
        "UPDATE workflows SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE plan = ?1 AND key = ?2 AND status = 'open'",
        [id.as_str(), key.as_str()],
    )?;
    if moved == 0 {
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
    let text = format!("Workflow {key} (#{}) of Plan {id} is merged.", pull.number);
    append(
        tx,
        NewEvent::pr_merged(text, key, pull.number, &pull.url, pull.merged_at.as_deref())
            .plan(id.clone()),
    )?;
    // The last Workflow to merge ends the Plan (§3.1).
    let unmerged: i64 = tx.query_row(
        "SELECT count(*) FROM workflows WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled')",
        [id.as_str()],
        |r| r.get(0),
    )?;
    if unmerged > 0 {
        return Ok(false);
    }
    crate::plan::close_reviews(tx, id, &crate::plan::Closing::Done)?;
    let moved = tx.execute(
        "UPDATE plans SET phase = 'done', done_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE id = ?1 AND phase = 'deploy'",
        [id.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(id, key));
    }
    let merged = merged_prs(tx, id, key, pull.merged_at.as_deref())?;
    let text = format!(
        "Every Workflow of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}"
    );
    append(tx, NewEvent::plan_done(text, &merged).plan(id.clone()))?;
    Ok(true)
}

/// The unstack of the Workflows with no pull request yet that were stacked on the Workflow `key` of `plan`, which this transaction
/// merged (§7.6 step 6, hs-d301): every `waiting` or `dispatch` Workflow of any Plan whose `base` is its Lead Branch, told to
/// its own Plan, so that engine E7 of the same tick already sees the new base.
pub(super) fn unstack_stacked_on(
    tx: &WriteTxn<'_>,
    plan: &PlanId,
    key: &WorkflowKey,
) -> Result<(), CpdError> {
    let mut stmt = tx.prepare(
        "SELECT r.plan, r.key FROM workflows r, workflows m
          WHERE m.plan = ?1 AND m.key = ?2 AND r.repo = m.repo AND r.base = m.lead_branch AND r.status IN ('waiting', 'dispatch')
          ORDER BY r.plan, r.key",
    )?;
    let stacked = stmt
        .query_map([plan.as_str(), key.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (plan, key) in stacked {
        unstack(
            tx,
            &PlanId::parse(&plan)?,
            &WorkflowKey::parse(&key)?,
            COMMAND_BY,
        )?;
    }
    Ok(())
}

/// `[{key, status, pull_request}]` of the Plan's Workflows that are not `cancelled`, in key order.
fn plan_workflows(env: &mut Env, plan: &PlanWorkflows) -> Result<Vec<Value>, CpdError> {
    env.store.read(|conn| {
        plan.live()
            .map(|workflow| {
                let row = workflow.pull_request.map(|id| pulls::load(conn, id)).transpose()?;
                Ok(json!({"key": workflow.key, "status": workflow.status, "pull_request": row.map(|r| r.to_json())}))
            })
            .collect()
    })
}

/// `[{key, number, url, merged_at}]` of every Workflow that is not `cancelled`, in key
/// order: `merged_at` is the moment the Workflow was seen merged, and for `just`, the
/// one this poll merged, the time `gh` reports when it does.
fn merged_prs(
    tx: &WriteTxn<'_>,
    id: &PlanId,
    just: &WorkflowKey,
    reported: Option<&str>,
) -> Result<Vec<serde_json::Value>, CpdError> {
    let mut stmt = tx.prepare("SELECT key, pr_number, pr_url, merged_at FROM workflows WHERE plan = ?1 AND status = 'merged' ORDER BY key")?;
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

fn cas_lost(id: &PlanId, key: &WorkflowKey) -> CpdError {
    CpdError::new(
        ErrorCode::CasLost,
        format!(
            "Workflow {key} of Plan {id} changed while it was being polled: rerun the command."
        ),
    )
    .with("plan", id.as_str())
    .with("pr", key.as_str())
}

/// The pull requests whose comments are read (§7.6 step 3a): an owned pull request listed `open` in `seen` whose `updatedAt` moved, or
/// whose stored feedback holds an open item (resolving a thread does not move `updatedAt`, pro-d231). No duplicates.
fn due_reads(
    owned: &[(PlanId, WorkflowKey, pulls::PullRequest)],
    seen: &[Listed],
) -> Vec<(String, i64)> {
    let mut due: Vec<(String, i64)> = Vec::new();
    for (_, _, row) in owned {
        let open_feedback = row.feedback.as_ref().is_some_and(|items| !items.is_empty());
        let is_due = seen.iter().any(|l| {
            l.pr.repo == row.repo
                && l.pr.number == row.number
                && l.pr.state == PrState::Open
                && (open_feedback || row.updated_at.as_deref() != Some(l.pr.updated_at.as_str()))
        });
        if is_due && !due.contains(&(row.repo.clone(), row.number)) {
            due.push((row.repo.clone(), row.number));
        }
    }
    due
}

#[cfg(test)]
mod due_reads_tests {
    use super::super::comments::{Item, Kind};
    use super::super::status::{MergeState, Mergeable, ReviewDecision};
    use super::*;

    fn item() -> Item {
        Item {
            id: "t1".into(),
            kind: Kind::Thread,
            last: "x".into(),
            author: "a".into(),
            bot: false,
            path: None,
            outdated: false,
            url: "u".into(),
            excerpt: "e".into(),
        }
    }

    fn row(
        updated: &str,
        feedback: Option<Vec<Item>>,
    ) -> (PlanId, WorkflowKey, pulls::PullRequest) {
        let workflow = pulls::PullRequest {
            id: 1,
            repo: "o/r".into(),
            number: 7,
            url: "u".into(),
            title: "t".into(),
            draft: false,
            state: "open".into(),
            review_decision: "none".into(),
            checks: "none".into(),
            created_at: "c".into(),
            merged_at: None,
            changed_at: "c".into(),
            mergeable: "unknown".into(),
            merge_state: "unknown".into(),
            authored: true,
            review_requested: false,
            updated_at: Some(updated.into()),
            feedback,
            base_ref: None,
            default_branch: None,
        };
        (
            PlanId::parse("plan").unwrap(),
            WorkflowKey::parse("web").unwrap(),
            workflow,
        )
    }

    fn listed(updated: &str, state: PrState) -> Listed {
        Listed {
            pr: gh::PolledPr {
                repo: "o/r".into(),
                number: 7,
                url: "u".into(),
                title: "t".into(),
                draft: false,
                state,
                review_decision: ReviewDecision::None,
                checks: ChecksSummary::None,
                mergeable: Mergeable::Unknown,
                merge_state: MergeState::Unknown,
                created_at: "c".into(),
                updated_at: updated.into(),
                merged_at: None,
                base_ref: None,
                default_branch: None,
            },
            membership: Default::default(),
        }
    }

    fn key() -> Vec<(String, i64)> {
        vec![("o/r".to_string(), 7)]
    }

    #[test]
    fn a_moved_updated_at_is_due() {
        assert_eq!(
            due_reads(&[row("1", None)], &[listed("2", PrState::Open)]),
            key()
        );
    }

    #[test]
    fn open_feedback_is_due_with_an_unmoved_updated_at() {
        assert_eq!(
            due_reads(
                &[row("1", Some(vec![item()]))],
                &[listed("1", PrState::Open)]
            ),
            key()
        );
    }

    #[test]
    fn empty_feedback_with_an_unmoved_updated_at_is_not_due() {
        assert!(due_reads(&[row("1", Some(vec![]))], &[listed("1", PrState::Open)]).is_empty());
    }

    #[test]
    fn null_feedback_with_an_unmoved_updated_at_is_not_due() {
        assert!(due_reads(&[row("1", None)], &[listed("1", PrState::Open)]).is_empty());
    }

    #[test]
    fn a_closed_pr_is_not_due_even_with_open_feedback() {
        assert!(
            due_reads(
                &[row("1", Some(vec![item()]))],
                &[listed("2", PrState::Closed)]
            )
            .is_empty()
        );
    }

    #[test]
    fn a_pr_owned_twice_is_due_once() {
        let r = row("1", Some(vec![item()]));
        assert_eq!(
            due_reads(&[r.clone(), r], &[listed("2", PrState::Open)]),
            key()
        );
    }
}
