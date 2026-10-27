//! `pr poll` (commands.md §9.2, contract §7.6): one poll of every open pull
//! request of the User, stored in `pull_requests`, and the merge of the PRs
//! the Plans own.

use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::Duration;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::comments::{self, Read};
use super::feedback;
use super::gh::{self, Listed};
use super::pulls::{self, Change, Reason};
use super::row::PlanPrs;
use super::start::unstack;
use super::status::{ChecksSummary, PrState};
use super::{Env, PollArgs};
use crate::error::{CpdError, ErrorCode};
use crate::events::{BasePull, NewEvent, append};
use crate::git;
use crate::home::CpdHome;
use crate::inbox::model as inbox;
use crate::host::layout::attempt_window;
use crate::host::tmux::Tmux;
use crate::ids::{AttemptId, JobKey, PlanId, PrKey};
use crate::config::Tier;
use crate::job::open::{self, Cause};
use crate::plan::fingerprint::load_prs;
use crate::lock;
use crate::ports::Ports;
use crate::output::Output;
use crate::policy::Checks;
use crate::store::WriteTxn;

const COMMAND: &str = "pr poll";

/// The `by` of the `pr-base-changed` Event the poll appends (§5.2).
const COMMAND_BY: &str = "poll";

/// How long `pr poll` waits for `locks/pr-poll.lock` (`lock_timeout`).
const LOCK_WAIT: Duration = Duration::from_secs(60);

/// How often the wait tries the lock again.
const RETRY: Duration = Duration::from_millis(100);

/// A pull request an `open` PR of a Plan points to.
type Owned = (PlanId, PrKey, pulls::PullRequest);

/// An owned PR whose policy did not resolve (`owner_unset`, `owner_unconfirmed`, `policy_invalid`).
struct Unresolved {
    plan: PlanId,
    pr: PrKey,
    number: i64,
    error: CpdError,
}

/// A PR whose pull request GitHub repointed: the PR's `base` follows it (§7.6 step 6, hs-d325).
struct Follow {
    plan: PlanId,
    pr: PrKey,
    from: String,
    to: String,
    number: i64,
    url: String,
}

/// What the transaction did.
#[derive(Default)]
struct Outcome {
    changed: Vec<Value>,
    base_changed: Vec<Value>,
    feedback: Vec<(PlanId, PrKey)>,
    /// The `update` Jobs the poll queued, for a conflict (§3.7 E5).
    update_jobs: Vec<(PlanId, PrKey, JobKey)>,
    merged: Vec<(PlanId, PrKey)>,
    done: Vec<PlanId>,
}

pub(super) fn poll(ports: &Ports, args: PollArgs) -> Result<Output, CpdError> {
    let mut env = Env::open_with(ports)?;
    let selected = args.plan.plan;
    if let Some(id) = &selected {
        env.plan(id)?;
    }
    let _lock = take_lock_with(&env.ports, &env.home.pr_poll_lock())?;
    let root = env.home.root().to_owned();

    let search = gh::search_open(&*env.ports.runner, &root)?;
    let owned = env.store.read(pulls::owned_open)?;
    // Each owned PR's `pr.feedback.checks`, resolved before anything is written (§8.3). A PR whose policy does not
    // resolve is told nothing and reported below; the poll goes on for the others (§7.6).
    let mut checks = Vec::new();
    let mut unresolved = Vec::new();
    for (plan, key, row) in &owned {
        match super::pr_policy_resolved(&mut env, plan, key, crate::policy::resolve)? {
            Ok(policy) => checks.push((plan.clone(), key.clone(), policy.pr.feedback.checks)),
            Err(error) => unresolved.push(Unresolved { plan: plan.clone(), pr: key.clone(), number: row.number, error }),
        }
    }

    // The stored open pull requests a search missed are read directly (§7.6 step 3): owned and listed by neither search,
    // or flagged for a search that did not list them. A search's absence deletes, moves and unflags nothing.
    let stored = env.store.read(pulls::stored_open)?;
    let listed_in = |row: &pulls::PullRequest| search.prs.iter().find(|l| l.pr.repo == row.repo && l.pr.number == row.number);
    let candidates: Vec<&pulls::PullRequest> = stored
        .iter()
        .filter(|row| {
            let found = listed_in(row).map(|l| l.membership).unwrap_or_default();
            let is_owned = owned.iter().any(|(_, _, o)| o.id == row.id);
            (is_owned && listed_in(row).is_none()) || (row.authored && !found.authored) || (row.review_requested && !found.review_requested)
        })
        .collect();
    let keys: Vec<(String, i64)> = candidates.iter().map(|row| (row.repo.clone(), row.number)).collect();
    let read = gh::read_absent(&*env.ports.runner, &root, &keys)?;
    let mut seen: Vec<Listed> = search.prs.iter().filter(|l| !keys.contains(&(l.pr.repo.clone(), l.pr.number))).cloned().collect();
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
                seen.push(Listed { pr: row.polled()?, membership: Default::default() });
            }
            None => seen.extend(found.cloned()),
        }
    }

    // The comments of the owned open pull requests whose `updatedAt` moved (§7.6 step 3a): one call, before anything is
    // written, so that a failed read writes nothing.
    let mut due: Vec<(String, i64)> = Vec::new();
    for (_, _, row) in &owned {
        let moved = seen.iter().any(|l| {
            l.pr.repo == row.repo && l.pr.number == row.number && l.pr.state == PrState::Open && row.updated_at.as_deref() != Some(l.pr.updated_at.as_str())
        });
        if moved && !due.contains(&(row.repo.clone(), row.number)) {
            due.push((row.repo.clone(), row.number));
        }
    }
    let reads = gh::read_comments(&*env.ports.runner, &root, &due, &env.config.inbox.bot_reviewers)?;

    // The checks of each owned open pull request whose `pr.feedback.checks` is `any` and whose rollup says `failed`
    // (§7.6 step 3b): GitHub rolls a CANCELLED check up as FAILURE, so one call confirms it, before anything is written.
    let mut confirm: Vec<(String, i64)> = Vec::new();
    for listed in &seen {
        let (repo, number) = (&listed.pr.repo, listed.pr.number);
        let any = owned.iter().any(|(plan, key, row)| {
            row.repo == *repo && row.number == number && checks_of(&checks, plan, key) == Some(Checks::Any)
        });
        if any && listed.pr.state == PrState::Open && listed.pr.checks == ChecksSummary::Failed && !confirm.contains(&(repo.clone(), number)) {
            confirm.push((repo.clone(), number));
        }
    }
    let confirmed = gh::read_checks(&*env.ports.runner, &root, &confirm)?;
    for listed in &mut seen {
        if let Some(i) = confirm.iter().position(|(repo, number)| *repo == listed.pr.repo && *number == listed.pr.number) {
            listed.pr.checks = ChecksSummary::of(&confirmed[i]);
        }
    }

    // The owned PRs whose pull request a search lists with a base other than the PR's `base`.
    let follows = env.store.read(|conn| follows(conn, &owned, &search.bases))?;

    let dirty = !follows.is_empty() || env.store.read(|conn| {
        for listed in &seen {
            match pulls::find(conn, &listed.pr.repo, listed.pr.number)? {
                Some(row) if row.holds(listed, read_of(&reads, listed))? => {}
                _ => return Ok(true),
            }
        }
        Ok(false)
    })?;
    let open_tier = env.config.open_tier;
    // A poll that finds nothing new opens no write transaction (D5).
    let outcome = if dirty { env.store.write(COMMAND, |tx| apply(tx, &seen, &reads, &owned, &checks, &follows, open_tier))? } else { Outcome::default() };

    report_unresolved(&mut env, &unresolved)?;

    // After the transaction: the clean-up of each PR that merged, then the silent retry of the earlier ones.
    clean_up(&mut env, &outcome.merged)?;
    let printed_prs = match &selected {
        Some(id) => {
            let plan = env.plan(id)?;
            Some(plan_prs(&mut env, &plan)?)
        }
        None => None,
    };

    let pair = |(plan, pr): &(PlanId, PrKey)| json!({"plan": plan, "pr": pr});
    let mut out = json!({
        "polled": search.prs.len(),
        "changed": outcome.changed,
        "base_changed": outcome.base_changed,
        "feedback": outcome.feedback.iter().map(pair).collect::<Vec<_>>(),
        "update_jobs": outcome.update_jobs.iter().map(|(plan, pr, job)| json!({"plan": plan, "pr": pr, "job": job, "reason": Cause::Conflict.as_str()})).collect::<Vec<_>>(),
        "merged": outcome.merged.iter().map(pair).collect::<Vec<_>>(),
        "done": outcome.done,
        "rate_limit": {"cost": search.rate.cost, "remaining": search.rate.remaining, "reset_at": search.rate.reset_at},
    });
    if let Some(prs) = printed_prs {
        out["prs"] = Value::Array(prs);
    }
    Ok(Output::success(out))
}

/// `flock` on `path`, waiting at most [`LOCK_WAIT`] on `ports.clock`: `lock_timeout` (exit 5). The
/// descriptor stays open while waiting.
pub(crate) fn take_lock_with(ports: &Ports, path: &Path) -> Result<fs::File, CpdError> {
    let failed = |what: &dyn std::fmt::Display| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the lock {}: {what}.", path.display())).with("lock", path.display().to_string())
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| failed(&e))?;
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).map_err(|e| failed(&e))?;
    let deadline = ports.clock.now() + LOCK_WAIT;
    loop {
        if lock::try_lock(&file).map_err(|e| failed(&format!("flock failed: {e}")))? {
            return Ok(file);
        }
        if ports.clock.now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::LockTimeout,
                format!("Another pr poll held {} for longer than {}s.", path.display(), LOCK_WAIT.as_secs()),
            )
            .with("lock", path.display().to_string())
            .with("waited_seconds", LOCK_WAIT.as_secs()));
        }
        ports.clock.sleep(RETRY);
    }
}

/// The one write transaction (D5, D7): every changed row, the Events of the
/// owning Plans.
fn apply(
    tx: &WriteTxn<'_>,
    seen: &[Listed],
    reads: &[Read],
    owned: &[Owned],
    checks: &[(PlanId, PrKey, Checks)],
    follows: &[Follow],
    open_tier: Tier,
) -> Result<Outcome, CpdError> {
    let mut out = Outcome::default();
    for listed in seen {
        // A merged row needs a merge time: GitHub's, else this poll's.
        let mut listed = listed.clone();
        if listed.pr.state == PrState::Merged && listed.pr.merged_at.is_none() {
            listed.pr.merged_at = Some(pulls::txn_time(tx)?);
        }
        let polled = &listed.pr;
        let Some(change) = pulls::upsert(tx, &listed, read_of(reads, &listed))? else { continue };
        let after = &change.after;
        if after.state == "merged" {
            inbox::close_pr_ready(tx, &after.repo, after.number)?;
        }
        // A row only the comments read changed (its feedback and `updated_at`) is no change of the pull request.
        if change.before.as_ref().is_none_or(|before| !before.holds(&listed, None).unwrap_or(false)) {
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
                crate::incident::end_if_until_pr_merged(tx)?;
                unstack_stacked_on(tx, plan, key)?;
            } else if let Some(checks) = checks_of(checks, plan, key) {
                if tell(tx, plan, key, checks, &change)? {
                    out.feedback.push((plan.clone(), key.clone()));
                }
                if let Some(job) = update_for_conflict(tx, plan, key, &change, open_tier)? {
                    out.update_jobs.push((plan.clone(), key.clone(), job));
                }
            }
        }
    }
    for follow in follows {
        out.base_changed.push(follow_base(tx, follow)?);
    }
    Ok(out)
}

/// The comments read of the pull request `listed`, if this poll made one.
fn read_of<'a>(reads: &'a [Read], listed: &Listed) -> Option<&'a Read> {
    reads.iter().find(|r| r.repo == listed.pr.repo && r.number == listed.pr.number)
}

/// Each `open` PR of `owned` whose pull request a search lists (as `OPEN`) with a `baseRefName` that is not the PR's
/// `base` (§7.6 step 6): a repoint by GitHub, or a base retargeted by hand. A PR with no entry in `bases` is not followed.
fn follows(conn: &rusqlite::Connection, owned: &[Owned], bases: &[((String, i64), String)]) -> Result<Vec<Follow>, CpdError> {
    let mut stored = conn.prepare("SELECT base FROM prs WHERE plan = ?1 AND key = ?2 AND status = 'open'")?;
    let mut found = Vec::new();
    for (plan, key, row) in owned {
        let Some((_, to)) = bases.iter().find(|((repo, number), _)| (repo, number) == (&row.repo, &row.number)) else { continue };
        let from: Option<String> = stored.query_row([plan.as_str(), key.as_str()], |r| r.get(0)).optional()?;
        if let Some(from) = from.filter(|from| from != to) {
            found.push(Follow { plan: plan.clone(), pr: key.clone(), from, to: to.clone(), number: row.number, url: row.url.clone() });
        }
    }
    Ok(found)
}

/// Sets the PR's `base` to the one GitHub reports and appends `pr-base-changed` for its Plan (§5.2). Nothing else
/// changes: no Gate, no status, no `pr-feedback`, no `gh` call (hs-d326: the tree is unchanged, a ready PR stays ready).
fn follow_base(tx: &WriteTxn<'_>, follow: &Follow) -> Result<Value, CpdError> {
    let Follow { plan, pr, from, to, number, url } = follow;
    let repo: String = tx.query_row("SELECT repo FROM prs WHERE plan = ?1 AND key = ?2", [plan.as_str(), pr.as_str()], |r| r.get(0))?;
    let base_pr = |branch: &str| -> Result<Option<String>, CpdError> {
        Ok(tx
            .query_row("SELECT plan || '/' || key FROM prs WHERE repo = ?1 AND lead_branch = ?2", [repo.as_str(), branch], |r| r.get(0))
            .optional()?)
    };
    let (from_pr, to_pr) = (base_pr(from)?, base_pr(to)?);
    let moved = tx.execute(
        "UPDATE prs SET base = ?3 WHERE plan = ?1 AND key = ?2 AND status = 'open' AND base = ?4",
        [plan.as_str(), pr.as_str(), to.as_str(), from.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(plan, pr));
    }
    let reason = match (&from_pr, &to_pr) {
        (Some(base), None) => format!("its base PR {base} merged and GitHub repointed the pull request"),
        _ => "the base changed on GitHub".to_owned(),
    };
    let text = format!(
        "PR {pr} of Plan {plan} now targets {to} (was {from}): {reason}. Its tree is unchanged: no new Gate is needed and it stays ready. \
         Read it with cpd plan show --plan {plan}."
    );
    let event = NewEvent::pr_base_changed(text, pr, (from, to), (from_pr.as_deref(), to_pr.as_deref()), COMMAND_BY, BasePull::Of(*number, url));
    append(tx, event.plan(plan.clone()))?;
    Ok(json!({"plan": plan, "pr": pr, "from": from, "to": to}))
}

/// The PR's resolved `pr.feedback.checks`; `None` when its policy did not resolve: it is told nothing.
fn checks_of(checks: &[(PlanId, PrKey, Checks)], plan: &PlanId, key: &PrKey) -> Option<Checks> {
    checks.iter().find(|(p, k, _)| p == plan && k == key).map(|(_, _, c)| *c)
}

/// One `error` Event for each PR whose policy did not resolve (§7.6): `source` `pr poll`, the resolve error's `code`,
/// `details` the error's with `plan`, `pr` and `number`. Once per episode: not again while the latest such Event of
/// the PR has the same code, so a poll every `pr_poll_seconds` does not repeat it, and no write transaction opens.
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
                    [failed.plan.as_str(), COMMAND, failed.pr.as_str(), ErrorCode::CleanupFailed.as_str()],
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
        for Unresolved { plan, pr, number, error } in &fresh {
            let message = format!("The policy of PR {pr} (#{number}) of Plan {plan} did not resolve: {}", error.message());
            let next = match error.details().get("next").and_then(Value::as_str) {
                Some(next) => next.to_owned(),
                None => format!("cpd policy show --pr {pr} --plan {plan}"),
            };
            let text = format!("{message} The poll sends this PR no pr-feedback until it does; every other PR is polled. Next: {next}");
            let mut details = error.details().clone();
            details.insert("plan".into(), json!(plan));
            details.insert("pr".into(), json!(pr));
            details.insert("number".into(), json!(number));
            append(tx, NewEvent::error(text, COMMAND, error.code().as_str(), &message, details).plan(plan.clone()))?;
        }
        Ok(())
    })
}

/// D7: `pr-feedback` to the Plan, when the change needs its attention.
fn tell(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey, checks: Checks, change: &Change) -> Result<bool, CpdError> {
    let open = change.after.feedback.as_deref().unwrap_or_default();
    let new = comments::new_items(change.before.as_ref().and_then(|b| b.feedback.as_deref()), open);
    let reasons: Vec<Reason> = pulls::attention(change.before.as_ref(), &change.after, checks, &new);
    if reasons.is_empty() {
        return Ok(false);
    }
    let text = feedback::text(plan.as_str(), key.as_str(), change.after.number, &reasons, (new.len(), open.len()));
    append(tx, feedback::event(text, key.as_str(), &change.after, &reasons).plan(plan.clone()))?;
    Ok(true)
}

/// The `update` Job of a conflict (§3.7 E5, §7.6 step 6): when `mergeable` became `conflicting` (from anything else, so once per
/// transition: `unknown` is no conflict and a base that merely moved, `merge_state` `behind`, is no cause), the row's `state` is `open`
/// and the Plan is not archived, the PR's `update` Job is queued unless one is `queued` or `running`. The caller has resolved the
/// PR's policy. The Job's key when one was inserted.
fn update_for_conflict(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey, change: &Change, tier: Tier) -> Result<Option<JobKey>, CpdError> {
    let became = change.after.mergeable == "conflicting" && change.before.as_ref().is_none_or(|b| b.mergeable != "conflicting");
    if !became || change.after.state != "open" {
        return Ok(None);
    }
    let archived: bool = tx.query_row("SELECT archived_at IS NOT NULL FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))?;
    if archived {
        return Ok(None);
    }
    let prs = load_prs(tx, plan)?;
    let Some(pr) = prs.iter().find(|p| p.key == *key) else { return Ok(None) };
    open::queue_update(tx, plan, pr, tier, COMMAND_BY, Cause::Conflict)
}

/// The PR `key` of the Plan is merged on GitHub (§7.6): `prs` `merged`,
/// `pr-merged`, and, when it was the last PR, the Plan `done` with `plan-done`
/// (§3.1). Whether the Plan became `done`.
pub(super) fn merge(tx: &WriteTxn<'_>, id: &PlanId, key: &PrKey, pull: &pulls::PullRequest) -> Result<bool, CpdError> {
    let moved = tx.execute(
        "UPDATE prs SET status = 'merged', merged_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE plan = ?1 AND key = ?2 AND status = 'open'",
        [id.as_str(), key.as_str()],
    )?;
    if moved == 0 {
        return Err(cas_lost(id, key));
    }
    let text = format!("PR {key} (#{}) of Plan {id} is merged.", pull.number);
    append(tx, NewEvent::pr_merged(text, key, pull.number, &pull.url, pull.merged_at.as_deref()).plan(id.clone()))?;
    // The last PR to merge ends the Plan (§3.1).
    let unmerged: i64 =
        tx.query_row("SELECT count(*) FROM prs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled')", [id.as_str()], |r| r.get(0))?;
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
    let text = format!("Every PR of Plan {id} is merged: the Plan is done. See cpd plan show --plan {id}");
    append(tx, NewEvent::plan_done(text, &merged).plan(id.clone()))?;
    Ok(true)
}

/// The unstack of the PRs with no pull request yet that were stacked on the PR `key` of `plan`, which this transaction
/// merged (§7.6 step 6, hs-d301): every `waiting` or `dispatch` PR of any Plan whose `base` is its Lead Branch, told to
/// its own Plan, so that engine E7 of the same tick already sees the new base.
pub(super) fn unstack_stacked_on(tx: &WriteTxn<'_>, plan: &PlanId, key: &PrKey) -> Result<(), CpdError> {
    let mut stmt = tx.prepare(
        "SELECT r.plan, r.key FROM prs r, prs m
          WHERE m.plan = ?1 AND m.key = ?2 AND r.repo = m.repo AND r.base = m.lead_branch AND r.status IN ('waiting', 'dispatch')
          ORDER BY r.plan, r.key",
    )?;
    let stacked = stmt
        .query_map([plan.as_str(), key.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (plan, key) in stacked {
        unstack(tx, &PlanId::parse(&plan)?, &PrKey::parse(&key)?, COMMAND_BY)?;
    }
    Ok(())
}

/// `[{key, status, pull_request}]` of the Plan's PRs that are not `cancelled`, in key order.
fn plan_prs(env: &mut Env, plan: &PlanPrs) -> Result<Vec<Value>, CpdError> {
    env.store.read(|conn| {
        plan.live()
            .map(|pr| {
                let row = pr.pull_request.map(|id| pulls::load(conn, id)).transpose()?;
                Ok(json!({"key": pr.key, "status": pr.status, "pull_request": row.map(|r| r.to_json())}))
            })
            .collect()
    })
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

/// A merged PR with what its clean-up removes.
pub(super) struct Merged {
    pub(super) plan: PlanId,
    pub(super) pr: PrKey,
    pub(super) repo: String,
    pub(super) lead_branch: String,
    pub(super) lead_worktree: Option<String>,
    /// The PR's pull request, `(owner/name, number)`, from its `pull_requests` row (contract §7.2 "The recorded head").
    pub(super) pull_request: Option<(String, i64)>,
    pub(super) attempts: Vec<Attempt>,
    /// Whether this poll reports its failures: it merged in this poll, or it is cancelled and no `cleanup_failed`
    /// Event of it exists yet (the poll that first sees it).
    pub(super) fresh: bool,
}

/// An Attempt of one of the PR's Jobs.
pub(super) struct Attempt {
    pub(super) id: AttemptId,
    pub(super) branch: String,
    pub(super) worktree: String,
    pub(super) ended: bool,
}

impl Merged {
    /// Whether a path this clean-up removes is still on disk: an Attempt or Lead worktree (contract §7.1).
    fn on_disk(&self) -> bool {
        self.lead_worktree.iter().chain(self.attempts.iter().map(|a| &a.worktree)).any(|p| Path::new(p).exists())
    }
}

/// A removal that failed or a Lead worktree that was kept: `(path, reason)`.
type Failure = (String, String);

/// §7.2 (hs-d138, hs-d142): the clean-up of every PR that `just` merged in this poll, which
/// kills its Attempt windows and appends one `error` Event `cleanup_failed` for each failure
/// or kept Lead worktree; then, with no Event, the retry of every other `merged` PR that
/// still has a path on disk. Best effort: only a failure to read or write the store fails it.
fn clean_up(env: &mut Env, just: &[(PlanId, PrKey)]) -> Result<(), CpdError> {
    let all = env.store.read(|conn| merged_prs_to_clean(conn, just))?;
    let (fresh, earlier): (Vec<Merged>, Vec<Merged>) = all.into_iter().partition(|m| m.fresh);
    for merged in earlier.iter().filter(|m| m.on_disk()) {
        clean_when_idle(&env.home, merged);
    }
    if fresh.is_empty() {
        return Ok(());
    }
    let tmux = Tmux::new(env.config.tmux.socket.as_str());
    let work = env.config.tmux.work_session.as_str();
    let panes = tmux.list_panes()?;
    let mut events = Vec::new();
    for merged in &fresh {
        let mut failures = Vec::new();
        for attempt in &merged.attempts {
            let name = attempt_window(&merged.plan, &attempt.id);
            for pane in panes.iter().filter(|p| p.session_name == work && p.window_name == name) {
                if let Err(e) = tmux.kill_window(&pane.window) {
                    failures.push((attempt.worktree.clone(), reason(&e)));
                }
            }
        }
        failures.extend(clean_when_idle(&env.home, merged));
        events.extend(failures.into_iter().map(|(path, reason)| (merged.plan.clone(), merged.pr.clone(), path, reason)));
    }
    if events.is_empty() {
        return Ok(());
    }
    env.store.write(COMMAND, |tx| {
        for (plan, pr, path, reason) in &events {
            let message = format!("The clean-up of the merged PR {pr} of Plan {plan} did not remove {path}: {reason}");
            let text = format!("{message} It is retried by every later poll, except a dirty or unpushed Lead worktree, which stays until you deal with it.");
            let details = json!({"plan": plan, "pr": pr, "path": path, "reason": reason});
            let event = NewEvent::error(text, COMMAND, ErrorCode::CleanupFailed.as_str(), &message, details);
            append(tx, event.plan(plan.clone()))?;
        }
        Ok(())
    })
}

/// Every PR in status `merged` or `cancelled`, with its Attempts. A cancelled PR's row has no `lead_worktree`
/// (`pr cancel` clears it), so its Lead worktree is the one contract §7.1 names, when that exists and is a worktree
/// of the repo on the PR's Lead Branch.
fn merged_prs_to_clean(conn: &rusqlite::Connection, just: &[(PlanId, PrKey)]) -> Result<Vec<Merged>, CpdError> {
    let mut prs = conn.prepare("SELECT p.plan, p.key, p.repo, p.lead_branch, p.lead_worktree, q.repo, q.number, p.status = 'cancelled'
           FROM prs p LEFT JOIN pull_requests q ON q.id = p.pull_request WHERE p.status IN ('merged', 'cancelled') ORDER BY p.plan, p.key",
    )?;
    let rows = prs
        .query_map([], |r| {
            let pull_request = r.get::<_, Option<String>>(5)?.zip(r.get::<_, Option<i64>>(6)?);
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, pull_request, r.get::<_, bool>(7)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut attempts = conn.prepare(
        "SELECT a.id, a.branch, a.worktree, a.status = 'ended' FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE j.plan = ?1 AND j.pr = ?2 ORDER BY a.id",
    )?;
    let mut found = Vec::new();
    let mut reported = conn.prepare(
        "SELECT count(*) FROM events WHERE plan = ?1 AND kind = 'error' AND json_extract(data, '$.code') = 'cleanup_failed'
            AND json_extract(data, '$.details.pr') = ?2",
    )?;
    for (plan, pr, repo, lead_branch, lead_worktree, pull_request, cancelled) in rows {
        let list = attempts
            .query_map([&plan, &pr], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|(id, branch, worktree, ended)| Ok(Attempt { id: AttemptId::parse(&id)?, branch, worktree, ended }))
            .collect::<Result<Vec<_>, CpdError>>()?;
        let plan = PlanId::parse(&plan)?;
        let pr = PrKey::parse(&pr)?;
        let (lead_worktree, fresh) = if cancelled {
            let lead = git::pr_lead_worktree_path(Path::new(&repo), &plan, &pr, &lead_branch)
                .ok()
                .filter(|path| {
                    path.exists()
                        && git::worktree::list(Path::new(&repo)).is_ok_and(|all| {
                            all.iter().any(|w| w.branch.as_deref() == Some(lead_branch.as_str()) && w.path.canonicalize().ok() == path.canonicalize().ok())
                        })
                })
                .map(|path| path.to_string_lossy().into_owned());
            let seen: i64 = reported.query_row([plan.as_str(), pr.as_str()], |r| r.get(0))?;
            (lead, seen == 0)
        } else {
            let fresh = just.contains(&(plan.clone(), pr.clone()));
            (lead_worktree, fresh)
        };
        found.push(Merged { plan, pr, repo, lead_branch, lead_worktree, pull_request, attempts: list, fresh });
    }
    Ok(found)
}

/// [`clean`] once the PR is idle (contract §7.2, hs-d243, hs-n124): every Attempt of its
/// Jobs is `ended` and its driver lock is free, probed without creating the file, since
/// a driver that just ended its Attempt may still be in A9, removing its worktree. While
/// it is not, nothing is cleaned and there is no failure: the PR is still on disk, so a
/// later poll cleans it up. A driver lock that cannot be probed is a failed removal.
fn clean_when_idle(home: &CpdHome, merged: &Merged) -> Vec<Failure> {
    for attempt in &merged.attempts {
        if !attempt.ended {
            return Vec::new();
        }
        match crate::driver::lock::probe(home, &merged.plan, &attempt.id) {
            Ok(Some(_free)) => {}
            Ok(None) => return Vec::new(),
            Err(e) => return vec![(attempt.worktree.clone(), reason(&e))],
        }
    }
    clean(merged)
}

/// Removes the Attempt worktrees of the PR that are on disk, then its Lead worktree, first
/// brought up to date by rule S. Each removal runs in the PR's repo, so the order does not
/// matter; a failure does not stop the others. What it did not remove.
pub(super) fn clean(merged: &Merged) -> Vec<Failure> {
    let repo = Path::new(&merged.repo);
    let mut failures = Vec::new();
    for attempt in &merged.attempts {
        // A linear Attempt runs in the workflow worktree on the Lead Branch: `remove_merged_lead` below handles it
        // (with the dirty/unpushed keep), never `discard_worktree` (hs-d680).
        if git::worktree::is_linear(&attempt.branch, &merged.lead_branch) {
            continue;
        }
        if Path::new(&attempt.worktree).exists()
            && let Err(e) = git::discard_worktree(repo, Path::new(&attempt.worktree), &attempt.branch)
        {
            failures.push((attempt.worktree.clone(), reason(&e)));
        }
    }
    if let Some(lead) = &merged.lead_worktree {
        // Rule S first, so a tree a merge left stale is not kept as dirty. Every stop of it is
        // `lead_worktree_stale` and leaves the worktree as it was: the check below keeps such a
        // worktree and reports it as `cleanup_failed`, never as `lead_worktree_stale` (§7.2).
        if Path::new(lead).exists() {
            match git::sync_lead(Path::new(lead), &merged.lead_branch) {
                Ok(_) => {}
                Err(e) if e.code() == ErrorCode::LeadWorktreeStale => {}
                Err(e) => {
                    failures.push((lead.clone(), reason(&e)));
                    return failures;
                }
            }
        }
        // The pull request's head commit, asked of GitHub only when `HEAD` has commits outside every `origin/*` ref:
        // a squash merge deletes the head branch, so the PR's own commits look unpushed (contract §7.2).
        let recorded_head = || {
            let (gh_repo, number) = merged.pull_request.as_ref()?;
            super::gh::head_ref_oid(repo, gh_repo, *number).ok()
        };
        match git::remove_merged_lead(repo, Path::new(lead), &merged.lead_branch, recorded_head) {
            Ok(git::LeadRemoval::Removed) => {}
            Ok(git::LeadRemoval::Kept(why)) => failures.push((lead.clone(), why.as_str().to_owned())),
            Err(e) => failures.push((lead.clone(), reason(&e))),
        }
    }
    failures
}

/// git's stderr tail when the error has one, else its message.
fn reason(error: &CpdError) -> String {
    match error.details().get("stderr").and_then(Value::as_str) {
        Some(stderr) if !stderr.trim().is_empty() => stderr.trim().to_owned(),
        _ => error.message().to_owned(),
    }
}
