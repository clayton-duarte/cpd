//! `pr rebase` (commands.md §9.3, contract §7.6): the Lead Branch onto the base
//! in a gate worktree, guard U, a compare-and-swap of the Lead Branch, rule S,
//! the Gate on the rebased commit in that gate worktree, and the push with a
//! lease. No lock (hs-d243): merges into the PR go on meanwhile.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;

use super::row::{PlanRow, PrRow};
use super::{Args, Env, gate_hooks, illegal_phase, refusal, short};
use crate::config::{self, Catalog};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git::{self, GateWorktree, Push, TAIL_LINES, tail};
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::merge::{self, CAS_RETRIES};
use crate::output::Output;
use crate::plan::Phase;

const COMMAND: &str = "pr rebase";

/// How long guard U waits for a live driver's step 10 (§7.6 step 5).
const GUARD_WAIT: Duration = Duration::from_secs(60);

/// How often guard U reads the store while it waits.
const GUARD_POLL: Duration = Duration::from_millis(200);

pub(super) fn rebase(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let pr = require_open(&plan, &args)?.clone();
    let key = pr.key.clone();
    let lead = pr.lead(&id)?.clone();
    let repo = pr.repo.as_path();
    let branch = pr.lead_branch.as_str();
    let lead_ref = format!("refs/heads/{branch}");
    let onto = format!("origin/{}", pr.base);

    // 1. The base, fresh (the ref lock on `origin/<base>` is waited for, §7).
    git::run::fetch(repo, &["origin", &pr.base])?;

    let mut retries = 0;
    let (old, new, lease, worktree) = loop {
        // 2. The snapshot S.
        let old = git::run::rev_parse(repo, &lead_ref)?;
        // 3. A conflict refuses before anything changes.
        if let Some(files) = conflicts(repo, &onto, &old)? {
            return Err(conflict(&id, &key, branch, &onto, files));
        }
        // 4. A gate worktree at S, rebased there: R'.
        git::sweep_gate_worktrees(repo, &id, &key)?;
        let worktree = GateWorktree::add(repo, &id, &key, &old)?;
        let new = match rebase_onto(worktree.path(), &onto).and_then(|files| match files {
            Some(files) => Err(conflict(&id, &key, branch, &onto, files)),
            None => git::run::rev_parse(worktree.path(), "HEAD"),
        }) {
            Ok(new) => new,
            Err(e) => return Err(removing(worktree, e)),
        };
        // 5. Guard U: no merge in S that the Ledger has not recorded.
        // The lease X0, what cpd last saw of the remote branch: read after guard U,
        // so that the pushes of the merges in S, which it waited for or ran R for, are seen.
        let lease = match guard_u(&mut env, &id, &key, repo, &old).and_then(|()| last_seen(repo, branch)) {
            Ok(lease) => lease,
            Err(e) => return Err(removing(worktree, e)),
        };
        // 6. The compare-and-swap of the Lead Branch, S to R'.
        let message = format!("cpd: pr rebase {id}/{key}");
        let out = match git::run::run(repo, &["update-ref", "-m", &message, &lead_ref, &new, &old]) {
            Ok(out) => out,
            Err(e) => return Err(removing(worktree, e)),
        };
        if out.code == 0 {
            break (old, new, lease, worktree);
        }
        worktree.remove()?;
        if git::run::rev_parse(repo, &lead_ref)? == old {
            return Err(CpdError::new(ErrorCode::GitFailed, "git update-ref failed, and the Lead Branch did not move.")
                .with("exit", out.code)
                .with("stderr", tail(&out.stderr, TAIL_LINES)));
        }
        // The ref moved (a merge landed, or another `pr rebase` won): back to step 2.
        retries += 1;
        if retries > CAS_RETRIES {
            return Err(CpdError::new(
                ErrorCode::CasLost,
                format!("{branch} kept moving during the rebase (merges into PR {key}); the compare-and-swap lost {} times: rerun the command.", CAS_RETRIES + 1),
            )
            .with("plan", id.as_str())
            .with("pr", key.as_str()));
        }
    };

    // 7. Rule S on the Lead worktree; a stop is reported and fails nothing.
    if let Err(stale) = git::sync_lead(&lead, branch) {
        report_stale(&mut env, &id, &key, &lead, &stale)?;
    }

    // 8. The Gate on R', in the gate worktree; it is removed whatever the result.
    let again = format!("pr rebase --pr {key}");
    worktree.gate_and_remove(&gate_hooks(&env, &id, &pr))?.map_err(|r| refusal(&id, &pr, &new, r, &again))?;
    // The PR is read again: merged during the Gate, nothing is pushed.
    let plan = env.plan(&id)?;
    require_open(&plan, &args)?;

    // 9. R' with the lease X0, then the merges that landed during the Gate, by rule P.
    // A rejection whose remote tip already contains R' is a success that pushed nothing:
    // two `pr rebase` of the PR at once push the same R', and the second can lose the
    // race inside the remote (Escalation nl-pr-a1-q2).
    let lease_arg = format!("--force-with-lease={lead_ref}:{lease}");
    let refspec = format!("{new}:{lead_ref}");
    let args = ["push", "--porcelain", lease_arg.as_str(), "origin", refspec.as_str()];
    let out = git::run::run(&lead, &args)?;
    if out.code != 0 && !remote_contains(&lead, &lead_ref, &new)? {
        let argv: Vec<String> = ["git", "-C", &lead.to_string_lossy()].into_iter().chain(args).map(str::to_owned).collect();
        return Err(CpdError::new(ErrorCode::GitFailed, format!("git push exited {}.", out.code))
            .with("argv", argv)
            .with("exit", out.code)
            .with("stderr", tail(&out.stderr, TAIL_LINES)));
    }
    let tip = git::run::rev_parse(repo, &lead_ref)?;
    if tip != new && git::run::run(repo, &["merge-base", "--is-ancestor", &new, &tip])?.code == 0 {
        if let Push::Rejected { stderr } = git::push_commit(&lead, branch, &tip)? {
            return Err(CpdError::new(
                ErrorCode::GitFailed,
                format!("{} was pushed, but the merges after it ({}) were rejected: run cpd pr push --pr {key}.", short(&new), short(&tip)),
            )
            .with("plan", id.as_str())
            .with("pr", key.as_str())
            .with("sha", tip.as_str())
            .with("stderr", stderr));
        }
    }

    Ok(Output::success(json!({"plan": id, "pr": key, "rebased": old != new, "old": old, "new": new, "pushed": true})))
}

/// The lease X0 (§7.6 step 2, Escalation nl-pr-a1-q1): `refs/remotes/origin/<branch>`,
/// what cpd last saw of the remote branch; every push of it from this machine,
/// rule P's included, updates it. A push cpd has not seen (another machine,
/// GitHub's "Update branch") makes the lease fail instead of being overwritten.
/// The empty string when cpd has never seen the branch: as a lease, "must not exist".
/// A push of what the remote already has is "up to date" whatever the lease (run
/// with git 2.43), so a second `pr rebase` pushing the same R' succeeds.
fn last_seen(repo: &Path, branch: &str) -> Result<String, CpdError> {
    let tracking = format!("refs/remotes/origin/{branch}");
    let out = git::run::run(repo, &["rev-parse", "--verify", "--quiet", &tracking])?;
    match out.code {
        0 => Ok(out.stdout.trim().to_owned()),
        1 => Ok(String::new()),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git rev-parse --verify {tracking} exited {code}."))
            .with("exit", code)
            .with("stderr", tail(&out.stderr, TAIL_LINES))),
    }
}

/// Whether origin's `reference` contains `sha`, as rule P checks (§7.4): its tip `X`
/// (`git ls-remote`) is in the local object store and `sha` is an ancestor of it, or it.
fn remote_contains(lead: &Path, reference: &str, sha: &str) -> Result<bool, CpdError> {
    let listed = git::run::ok(lead, &["ls-remote", "origin", reference])?;
    let tip = listed.lines().find(|l| l.split_whitespace().nth(1) == Some(reference)).and_then(|l| l.split_whitespace().next());
    let Some(tip) = tip else { return Ok(false) };
    let known = git::run::run(lead, &["cat-file", "-e", &format!("{tip}^{{commit}}")])?.code == 0;
    Ok(known && git::run::run(lead, &["merge-base", "--is-ancestor", sha, tip])?.code == 0)
}

/// `git merge-tree --write-tree --name-only <onto> <sha>`: `Some(files)` on a conflict.
fn conflicts(repo: &Path, onto: &str, sha: &str) -> Result<Option<Vec<String>>, CpdError> {
    let merge = git::run::run(repo, &["merge-tree", "--write-tree", "--name-only", onto, sha])?;
    match merge.code {
        0 => Ok(None),
        // The tree's id, the conflicted files, a blank line, then git's messages.
        1 => Ok(Some(merge.stdout.lines().skip(1).take_while(|l| !l.is_empty()).map(str::to_owned).collect())),
        code => Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-tree exited {code}."))
            .with("exit", code)
            .with("stderr", tail(&merge.stderr, TAIL_LINES))),
    }
}

/// `error`, after removing the gate worktree; the removal's own failure is
/// dropped (the leftover is swept once this process has ended), so that the
/// error the Lead acts on is the one that stopped the command.
fn removing(worktree: GateWorktree, error: CpdError) -> CpdError {
    let _ = worktree.remove();
    error
}

/// Guard U (§7.6 step 5): every Attempt of the PR's Jobs in `merging` whose
/// Attempt branch tip is an ancestor of the snapshot `snapshot` (its
/// compare-and-swap happened, its step 10 has not committed) is ended before
/// the rebase rewrites it: by R when its driver is dead, else by its driver,
/// waited for at most 60 s, else `cas_lost` with `details.attempt`.
fn guard_u(env: &mut Env, plan: &PlanId, key: &PrKey, repo: &Path, snapshot: &str) -> Result<(), CpdError> {
    let merging: Vec<String> = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT a.id FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
              WHERE a.plan = ?1 AND j.pr = ?2 AND a.status = 'merging' ORDER BY a.id",
        )?;
        let rows = stmt.query_map([plan.as_str(), key.as_str()], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })?;
    let mut catalog = None;
    for attempt in merging {
        let attempt = AttemptId::parse(&attempt)?;
        let branch = env.store.read(|conn| merge::load(conn, plan, &attempt))?.branch;
        let tip = git::run::rev_parse(repo, &format!("refs/heads/{branch}"))?;
        let contained = git::run::run(repo, &["merge-base", "--is-ancestor", &tip, snapshot])?;
        match contained.code {
            0 => settle(env, &mut catalog, plan, key, &attempt)?,
            1 => {}
            code => {
                return Err(CpdError::new(ErrorCode::GitFailed, format!("git merge-base --is-ancestor exited {code}."))
                    .with("exit", code)
                    .with("stderr", tail(&contained.stderr, TAIL_LINES)));
            }
        }
    }
    Ok(())
}

/// Waits until Attempt `attempt` is no longer `merging`, at most 60 s, reading the
/// store every 200 ms; whenever its driver lock is free (the driver is dead), R
/// ends it. pi's catalog, which R needs only to pick a crashed Attempt's next
/// Tier, is loaded the first time R runs.
fn settle(env: &mut Env, catalog: &mut Option<Catalog>, plan: &PlanId, key: &PrKey, attempt: &AttemptId) -> Result<(), CpdError> {
    let deadline = Instant::now() + GUARD_WAIT;
    loop {
        if merge::take_free_driver_lock(&env.home, plan, attempt)?.is_some() {
            if catalog.is_none() {
                *catalog = Some(config::load_catalog(&env.config)?);
            }
            let catalog = catalog.as_ref().expect("loaded above");
            let ctx = merge::Ctx { home: &env.home, config: &env.config, catalog, source: COMMAND, metrics: &|_| None };
            merge::recover_attempt(&mut env.store, &ctx, plan, attempt)?;
        }
        let status = env.store.read(|conn| merge::load(conn, plan, attempt))?.status;
        if status != "merging" {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::CasLost,
                format!(
                    "Attempt {attempt} of PR {key} is on the Lead Branch but its merge has not been recorded after {} s: nothing was changed. Rerun the command.",
                    GUARD_WAIT.as_secs()
                ),
            )
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("attempt", attempt.to_string()));
        }
        std::thread::sleep(GUARD_POLL);
    }
}

/// Rule S stopped after the compare-and-swap (§7.6 step 7): one `error` Event
/// `lead_worktree_stale` for the Plan; the command goes on.
fn report_stale(env: &mut Env, plan: &PlanId, key: &PrKey, lead: &Path, stale: &CpdError) -> Result<(), CpdError> {
    let fix = stale.details().get("fix").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
    let text = format!(
        "PR {key} of Plan {plan} was rebased, but its Lead worktree {} still holds the previous tree. {fix}. Then: cpd plan show --plan {plan}",
        lead.display()
    );
    let mut details = stale.details().clone();
    details.insert("pr".to_owned(), json!(key));
    let data = json!({"source": COMMAND, "code": stale.code().as_str(), "message": stale.message(), "details": details});
    env.store.write(COMMAND, |tx| {
        append(tx, NewEvent::new(Kind::Error, text, data).plan(plan.clone()))?;
        Ok(())
    })
}

/// The selected PR must be `open` (`illegal_transition`); a Plan that has not
/// been locked is `illegal_phase`.
fn require_open<'a>(plan: &'a PlanRow, args: &Args) -> Result<&'a PrRow, CpdError> {
    if matches!(plan.phase, Phase::Grounding | Phase::Plan) {
        return Err(illegal_phase(plan, COMMAND, "deploy"));
    }
    let pr = plan.select(&args.pr)?;
    if pr.status != "open" {
        return Err(pr.refuse(&plan.id, COMMAND, "open"));
    }
    Ok(pr)
}

fn conflict(plan: &PlanId, key: &PrKey, branch: &str, onto: &str, files: Vec<String>) -> CpdError {
    CpdError::new(
        ErrorCode::RebaseConflict,
        format!("{branch} conflicts with {onto} in {} files: nothing was changed. Decide, for example by appending a Job that resolves it.", files.len()),
    )
    .with("plan", plan.as_str())
    .with("pr", key.as_str())
    .with("files", files)
}

/// `git rebase <onto>` in `worktree`. `Some(files)` on a conflict, after
/// `git rebase --abort`; `None` when it rebased.
fn rebase_onto(worktree: &Path, onto: &str) -> Result<Option<Vec<String>>, CpdError> {
    let out = git::run::run(worktree, &["rebase", onto])?;
    if out.code == 0 {
        return Ok(None);
    }
    let conflicts = git::run::ok(worktree, &["diff", "--name-only", "--diff-filter=U"])?;
    let files: Vec<String> = conflicts.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect();
    let abort = git::run::run(worktree, &["rebase", "--abort"])?;
    if files.is_empty() || abort.code != 0 {
        let (what, failed) = if files.is_empty() {
            ("git rebase failed without a conflict", &out)
        } else {
            ("git rebase --abort failed after a conflict", &abort)
        };
        return Err(CpdError::new(ErrorCode::GitFailed, format!("{what}."))
            .with("exit", failed.code)
            .with("stderr", tail(&failed.stderr, TAIL_LINES)));
    }
    Ok(Some(files))
}
