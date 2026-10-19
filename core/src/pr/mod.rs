//! Pull requests (contract §7.6; commands.md §9), through `gh`:
//!
//! - `add`, `edit`, `cancel`: the Plan's PRs before they start (`pr add`, `pr edit`, `pr cancel`);
//! - `start`: starting a PR (`pr start`), the routine `plan lock` and engine E7 call too;
//! - `open`: the Gate, the push and the PR (`pr open`);
//! - `poll`, with `feedback` and `pulls` (the table `pull_requests`, written only there): one poll of every open pull request of the User (`pr poll`);
//! - `view`: a PR's checks, reviews and comments, read-only (`pr view`);
//! - `rebase`: the Lead Branch onto the base, the Gate and the push (`pr rebase`);
//! - `push`: the Gate and a plain push of the Lead Branch (`pr push`);
//! - `merge`: a clean pull request squash-merged on GitHub (`pr merge`);
//! - `gh`: the one gateway to `gh`; `status`: the PR status enums; `row`: the Plan as these commands read it.
//!
//! `pr open`, `pr rebase` and `pr push` take no lock (hs-d243): each runs its Gate on one
//! commit in a gate worktree of its own and pushes by rule P (§7.4, §7.6); each works on
//! one PR of the Plan, `poll` on every open one.

mod add;
mod cancel;
mod edit;
mod feedback;
mod gh;
mod merge;
mod open;
mod poll;
pub mod pulls;
mod push;
mod rebase;
mod row;
pub mod start;
mod status;
mod view;

use std::path::PathBuf;
use std::time::Duration;

use crate::args::{PlanArg, PrArg};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, GateHooks, HookOutcome, Refused, Setup};
use crate::home::{CpdHome, Hook};
use crate::ids::{ContextId, OwnerLogin, PlanId, PrKey, RepoName};
use crate::policy::{Gate, Policy, PolicyKey, owner_unset, resolve};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::PrRow;
use row::PlanPrs;

pub use gh::close_pr;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Add a PR to the Plan (§9.4)
    Add(AddArgs),
    /// Change a waiting PR before the lock (§9.5)
    Edit(EditArgs),
    /// Remove a PR from the Plan before the lock (§9.6)
    Cancel(CancelArgs),
    /// Start a waiting PR whose needs are merged, after a failed start (§9.7)
    Start(StartArgs),
    /// Push the Lead Branch and open the PR, only when it contains its base, never as a draft (§9.1)
    Open(Args),
    /// Poll every open pull request of the User (§9.2)
    Poll(PollArgs),
    /// Rebase the Lead Branch onto the base and push (§9.3)
    Rebase(Args),
    /// Gate, then push the Lead Branch as it is, without a rebase (§9.9)
    Push(Args),
    /// Show the PR's checks, reviews and comments (§9.8)
    View(Args),
    /// Squash-merge a clean pull request on GitHub and delete its branch (§9.10)
    Merge(merge::MergeArgs),
}

#[derive(clap::Args, Debug)]
pub struct Args {
    #[command(flatten)]
    pr: PrArg,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct PollArgs {
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// The PR key, chosen by the Lead
    #[arg(value_name = "key")]
    key: PrKey,
    /// The PR's repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: PathBuf,
    /// The branch the pull request targets [default: origin's HEAD branch, else main]
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// PRs of the Plan that must be merged before this one starts, comma-separated
    #[arg(long, value_name = "k1,k2", value_delimiter = ',', conflicts_with = "no_needs")]
    needs: Vec<PrKey>,
    /// No needs, even on a reopened Plan (default there: every merged PR of the Plan)
    #[arg(long = "no-needs")]
    no_needs: bool,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct EditArgs {
    /// The PR key
    #[arg(value_name = "key")]
    key: PrKey,
    /// The new repo: a path inside it
    #[arg(long, value_name = "path")]
    repo: Option<PathBuf>,
    /// The new base branch
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// The new needs, replacing the list, comma-separated
    #[arg(long, value_name = "k1,k2", value_delimiter = ',', conflicts_with = "no_needs")]
    needs: Option<Vec<PrKey>>,
    /// Remove every need
    #[arg(long = "no-needs")]
    no_needs: bool,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct CancelArgs {
    /// The PR key
    #[arg(value_name = "key")]
    key: PrKey,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// The PR key
    #[arg(value_name = "key")]
    key: PrKey,
    #[command(flatten)]
    plan: PlanArg,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Add(args) => add::add(args),
        Command::Edit(args) => edit::edit(args),
        Command::Cancel(args) => cancel::cancel(args),
        Command::Start(args) => start::run(args),
        Command::Open(args) => open::open(args),
        Command::Poll(args) => poll::poll(args),
        Command::Rebase(args) => rebase::rebase(args),
        Command::Push(args) => push::push(args),
        Command::View(args) => view::view(args),
        Command::Merge(args) => merge::merge(args),
    }
}

impl Env {
    fn plan(&mut self, id: &PlanId) -> Result<PlanPrs, CpdError> {
        self.store.read(|conn| row::load(conn, id))
    }
}

/// The resolved policy of PR `key` of `plan` (contract §8.3 "Keys and resolution"): `Repo(<owner>, <name>)`,
/// `Owner` when the PR has no name. An unset owner is `owner_unset`; a resolve error is returned as it is.
fn pr_policy(env: &mut Env, plan: &PlanId, key: &PrKey) -> Result<Policy, CpdError> {
    pr_policy_resolved(env, plan, key)?
}

/// [`pr_policy`] with the two kinds of failure apart: the outer error is the store's (it cannot be read, or holds
/// a value that does not parse), the inner one the resolve's (`owner_unset`, `owner_unknown`, `policy_invalid`),
/// which a consumer of many rows (the poll) reports for its row and goes on.
fn pr_policy_resolved(env: &mut Env, plan: &PlanId, key: &PrKey) -> Result<Result<Policy, CpdError>, CpdError> {
    let (context, owner, name): (String, Option<String>, Option<String>) = env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT c.id, c.owner, r.name FROM prs r JOIN plans p ON p.id = r.plan JOIN contexts c ON c.id = p.context
              WHERE r.plan = ?1 AND r.key = ?2",
            [plan.as_str(), key.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    })?;
    let Some(owner) = owner else {
        return Ok(Err(owner_unset(&ContextId::parse(&context)?)));
    };
    let owner = OwnerLogin::parse(&owner)?;
    let policy_key = match name {
        Some(name) => PolicyKey::Repo(owner, RepoName::parse(&name)?),
        None => PolicyKey::Owner(owner),
    };
    Ok(resolve(&env.home, &policy_key).map(|resolved| resolved.policy))
}

/// `illegal_phase` (exit 3): `command` needs a Plan in phase `needs`.
fn illegal_phase(plan: &PlanPrs, command: &str, needs: &str) -> CpdError {
    wrong_phase(&plan.plan.id, plan.plan.phase, command, needs)
}

/// A per-PR Gate log, `run/<plan>/logs/pr-<key>.<hook>.log` (§7.6): `pre-start`
/// and `pre-merge` of the gate worktree. The contract's layout (§1.4) names these
/// files; `CpdHome` has no method for them.
fn pr_log(home: &CpdHome, plan: &PlanId, key: &PrKey, hook: Hook) -> PathBuf {
    home.plan_dir(plan).join("logs").join(format!("pr-{key}.{}.log", hook.as_str()))
}

fn wrong_phase(plan: &PlanId, phase: Phase, command: &str, needs: &str) -> CpdError {
    CpdError::new(ErrorCode::IllegalPhase, format!("Plan {plan} is in phase {}: {command} needs phase {needs}.", phase.as_str()))
        .with("plan", plan.as_str())
        .with("phase", phase.as_str())
        .with("needs", needs)
}

/// `pr_locked` (exit 3): the Plan is past the phases where its PRs change (§3.8).
fn pr_locked(plan: &PlanId, phase: Phase, command: &str) -> CpdError {
    CpdError::new(
        ErrorCode::PrLocked,
        format!("Plan {plan} is in phase {}: {command} needs phase grounding or plan, and after the lock its PRs are Locked.", phase.as_str()),
    )
    .with("plan", plan.as_str())
    .with("phase", phase.as_str())
}

/// The Plan's phase, else `plan_not_found`.
fn plan_phase(conn: &rusqlite::Connection, plan: &PlanId) -> Result<Phase, CpdError> {
    use rusqlite::OptionalExtension;
    let phase: String = conn
        .query_row("SELECT phase FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0))
        .optional()?
        .ok_or_else(|| CpdError::plan_not_found(plan))?;
    Phase::parse(&phase)
        .ok_or_else(|| CpdError::bad_row(format!("Plan {plan} has phase {phase:?}")))
}

/// The PR `key` among `prs`, else `pr_not_found` (exit 4).
fn find_pr<'a>(prs: &'a [PrRow], plan: &PlanId, key: &PrKey) -> Result<&'a PrRow, CpdError> {
    prs.iter().find(|p| p.key == *key).ok_or_else(|| {
        CpdError::new(ErrorCode::PrNotFound, format!("No PR {key} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
    })
}

/// `needs` as the sorted keys, refusing a repeat with `usage` (§9.4).
fn sorted_needs(needs: &[PrKey]) -> Result<Vec<String>, CpdError> {
    let mut keys: Vec<String> = needs.iter().map(|k| k.as_str().to_owned()).collect();
    keys.sort();
    if let Some(pair) = keys.windows(2).find(|w| w[0] == w[1]) {
        return Err(CpdError::new(ErrorCode::Usage, format!("--needs lists {} twice.", pair[0])).with("pr", pair[0].as_str()));
    }
    Ok(keys)
}

/// Every key of `needs` is a PR of the Plan other than `own` (`needs_unknown`)
/// and none is `cancelled` (`needs_unavailable`) (§9.4, §9.5).
fn check_needs(prs: &[PrRow], plan: &PlanId, own: Option<&PrKey>, needs: &[String]) -> Result<(), CpdError> {
    let unknown: Vec<String> =
        needs.iter().filter(|n| own.is_some_and(|o| o.as_str() == n.as_str()) || !prs.iter().any(|p| p.key.as_str() == n.as_str())).cloned().collect();
    if !unknown.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnknown, format!("Plan {plan} has no PR {} to need.", unknown.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unknown));
    }
    let unavailable: Vec<String> = needs.iter().filter(|n| prs.iter().any(|p| p.key.as_str() == n.as_str() && p.status == "cancelled")).cloned().collect();
    if !unavailable.is_empty() {
        return Err(CpdError::new(ErrorCode::NeedsUnavailable, format!("A PR cannot need the cancelled PR {}.", unavailable.join(", ")))
            .with("plan", plan.as_str())
            .with("needs", unavailable));
    }
    Ok(())
}

/// How the Gate runs for PR `pr` (contract §7.3, §7.6): `pre-start` with
/// `setup_minutes`, `pre-merge` with `gate_minutes`, `branch` the Lead Branch.
fn gate_hooks<'a>(env: &Env, plan: &PlanId, pr: &'a PrRow) -> GateHooks<'a> {
    let minutes = |m: u32| Duration::from_secs(u64::from(m) * 60);
    GateHooks {
        lead_branch: &pr.lead_branch,
        base: &pr.base,
        pre_start: Setup { timeout: minutes(env.config.timeouts.setup_minutes), log: pr_log(&env.home, plan, &pr.key, Hook::PreStart) },
        pre_merge: Setup { timeout: minutes(env.config.timeouts.gate_minutes), log: pr_log(&env.home, plan, &pr.key, Hook::PreMerge) },
    }
}

/// The Gate on a snapshot (contract §7.6 steps 1–4) on `sha`: the PR's dead gate
/// worktrees swept, a gate worktree detached at `sha`, `pre-start` and `pre-merge`
/// in it, the gate worktree removed. `again` is the command the Lead runs after fixing it.
/// Nothing runs when the PR's policy says `pr.gate` is not `local` (§8.3).
fn gate(env: &Env, policy: &Policy, plan: &PlanId, pr: &PrRow, sha: &str, again: &str) -> Result<(), CpdError> {
    if policy.pr.gate != Gate::Local {
        return Ok(());
    }
    git::gate(std::path::Path::new(&pr.repo), plan, &pr.key, sha, &gate_hooks(env, plan, pr))?.map_err(|r| refusal(plan, pr, sha, r, again))
}

/// A Gate that did not pass (contract §7.3, §7.6 step 3): a `pre-merge` that failed
/// is `gate_failed` (exit 3); a `pre-merge` that could not be judged, or a
/// `pre-start` that did not pass in any way, is `gate_error` (exit 1). Both carry
/// the hook's last 60 lines in `details.tail` and its log in `details.log`.
fn refusal(plan: &PlanId, pr: &PrRow, sha: &str, refused: Refused, again: &str) -> CpdError {
    let tail = refused.tail.unwrap_or_default();
    let log = refused.log.display().to_string();
    let hook = refused.hook.as_str();
    let error = match (refused.hook, &refused.outcome) {
        (Hook::PreMerge, HookOutcome::Failed { exit }) => CpdError::new(
            ErrorCode::GateFailed,
            format!(
                "The Gate failed on {} at {} (exit {exit}): nothing was pushed. Append fix Jobs (cpd job add), then run cpd {again} again once they are Merged.",
                pr.lead_branch,
                short(sha)
            ),
        )
        .with("exit", *exit),
        (_, outcome) => {
            let error = CpdError::new(
                ErrorCode::GateError,
                format!("The Gate could not be judged on {} at {}: {hook} {outcome}. Nothing was pushed.", pr.lead_branch, short(sha)),
            )
            .with("reason", outcome.to_string());
            match outcome {
                HookOutcome::Failed { exit } => error.with("exit", *exit),
                _ => error,
            }
        }
    };
    error.with("plan", plan.as_str()).with("pr", pr.key.as_str()).with("hook", hook).with("sha", sha).with("tail", tail).with("log", log)
}

/// The first 12 characters of a SHA, for messages.
fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}
