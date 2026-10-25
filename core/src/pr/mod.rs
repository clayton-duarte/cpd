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

#[cfg(test)]
mod incident_tests;

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
#[cfg(test)]
pub(crate) use rebase::remote_contains_with;
mod row;
pub mod start;
mod status;
mod view;

use std::path::PathBuf;
use std::time::Duration;

use crate::args::{PlanArg, PrArg, PrTitle};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::git::gate::Slots;
use crate::git::{self, GateHooks, HookOutcome, Refused, Setup};
use crate::home::{CpdHome, Hook};
use crate::ids::{ContextId, HostName, OwnerLogin, PlanId, PrKey, RepoName};
use crate::policy::{Gate, Policy, PolicyKey, owner_unset, resolve};
use crate::output::Output;
use crate::plan::Phase;
use crate::plan::PrRow;
use row::PlanPrs;

pub use gh::close_pr;
pub use open::{Pulled, is_merged_on_github, pull_request, record as record_pull, submit_checks};
#[cfg(test)]
pub(crate) use rebase::{in_progress_with, rebase_onto_with, unresolved_with};

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Add a PR to the Plan (§9.4)
    Add(AddArgs),
    /// Change a waiting PR before the lock (§9.5)
    Edit(EditArgs),
    /// Remove a PR from the Plan before the lock; after it, end a PR with nothing to open, or whose work another PR carried (`--superseded-by`) (§9.6)
    Cancel(CancelArgs),
    /// Start a waiting PR whose needs are merged, after a failed start (§9.7)
    Start(StartArgs),
    /// Push the Lead Branch and open the PR, refused only when it conflicts with its base, never as a draft (§9.1)
    Open(Args),
    /// Poll every open pull request of the User (§9.2)
    Poll(PollArgs),
    /// Rebase the Lead Branch onto the base and push; --continue or --abort a rebase stopped at a conflict (§9.3)
    Rebase(RebaseArgs),
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
pub struct RebaseArgs {
    #[command(flatten)]
    args: Args,
    /// Finish the rebase stopped at a conflict, then Gate and push
    #[arg(long = "continue", conflicts_with = "abort")]
    resume: bool,
    /// Give up the rebase stopped at a conflict
    #[arg(long)]
    abort: bool,
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
    /// The PR's own title: names its change; 1-72 characters on one line
    #[arg(long, value_name = "text")]
    title: PrTitle,
    /// The branch the pull request targets [default: origin's HEAD branch, else main], or <plan>/<key> to stack on that PR
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// PRs that must be merged before this one starts, comma-separated: a key of the Plan, or <plan>/<key> of another Plan of the Context
    #[arg(long, value_name = "r1,r2", value_delimiter = ',', conflicts_with = "no_needs")]
    needs: Vec<add::PrRef>,
    /// No needs, even on a reopened Plan (default there: every merged PR of the Plan)
    #[arg(long = "no-needs")]
    no_needs: bool,
    /// The template of stub drafts the PR starts with, and the kinds its Jobs may have: tdd (docs, red tests, implementation), docs or config (§9.4)
    #[arg(long, value_name = "shape", value_parser = clap::builder::PossibleValuesParser::new(add::SHAPES), default_value = "tdd")]
    shape: String,
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
    /// The new base branch, or <plan>/<key> to stack on that PR
    #[arg(long, value_name = "branch")]
    base: Option<String>,
    /// The new needs, replacing the list, comma-separated: keys of the Plan or <plan>/<key>
    #[arg(long, value_name = "r1,r2", value_delimiter = ',', conflicts_with = "no_needs")]
    needs: Option<Vec<add::PrRef>>,
    /// Remove every need
    #[arg(long = "no-needs")]
    no_needs: bool,
    /// The new title, in any phase while the PR is not merged or cancelled; 1-72 characters on one line
    #[arg(long, value_name = "text")]
    title: Option<PrTitle>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct CancelArgs {
    /// The PR key
    #[arg(value_name = "key")]
    key: PrKey,
    /// Cancel a dispatch PR whose merged work another PR carried: that PR, as <plan>/<key> (a bare <key> is of the Plan)
    #[arg(long = "superseded-by", value_name = "ref")]
    superseded_by: Option<add::PrRef>,
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
    pr_policy_resolved(env, plan, key, resolve)?
}

/// [`pr_policy`] with the two kinds of failure apart: the outer error is the store's (it cannot be read, or holds
/// a value that does not parse), the inner one the resolve's (`owner_unset`, `owner_unconfirmed`, `policy_invalid`),
/// which a consumer of many rows (the poll) reports for its row and goes on. `resolve` is the strict [`resolve`], or
/// the poll's `resolve_lenient` (a missing repo file is no repo scope).
fn pr_policy_resolved(env: &mut Env, plan: &PlanId, key: &PrKey, resolve: ResolveFn) -> Result<Result<Policy, CpdError>, CpdError> {
    let (home, host) = (env.home.clone(), env.config.host.clone());
    env.store.read(|conn| pr_policy_in(conn, &home, &host, plan, key, resolve))
}

type ResolveFn = fn(&CpdHome, &HostName, &PolicyKey) -> Result<crate::policy::Resolved, CpdError>;

/// [`pr_policy_resolved`] on a connection, for a consumer that has no [`Env`] (an Attempt's, the engine's).
fn pr_policy_in(conn: &rusqlite::Connection, home: &CpdHome, host: &HostName, plan: &PlanId, key: &PrKey, resolve: ResolveFn) -> Result<Result<Policy, CpdError>, CpdError> {
    let (context, owner, name): (String, Option<String>, Option<String>) = conn.query_row(
        "SELECT c.id, c.owner, r.name FROM prs r JOIN plans p ON p.id = r.plan JOIN contexts c ON c.id = p.context
          WHERE r.plan = ?1 AND r.key = ?2",
        [plan.as_str(), key.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let Some(owner) = owner else {
        return Ok(Err(owner_unset(&ContextId::parse(&context)?)));
    };
    let owner = OwnerLogin::parse(&owner)?;
    let policy_key = match name {
        Some(name) => PolicyKey::Repo(owner, RepoName::parse(&name)?),
        None => PolicyKey::Owner(owner),
    };
    Ok(resolve(home, host, &policy_key).map(|resolved| resolved.policy))
}

/// The strict policy of PR `key` of `plan`, from a connection: what an Attempt-level consumer (the quick check, the
/// Worker Spec) reads, through its Job's PR. `repo_unconfigured` when the PR's repo has no policy file.
pub fn policy_of_pr(conn: &rusqlite::Connection, home: &CpdHome, host: &HostName, plan: &PlanId, key: &PrKey) -> Result<Policy, CpdError> {
    pr_policy_in(conn, home, host, plan, key, resolve)?
}

/// [`policy_of_pr`] of the PR that holds Job `job` of `plan`.
pub fn policy_of_job(conn: &rusqlite::Connection, home: &CpdHome, host: &HostName, plan: &PlanId, job: &str) -> Result<Policy, CpdError> {
    let pr: String = conn.query_row("SELECT pr FROM jobs WHERE plan = ?1 AND key = ?2", [plan.as_str(), job], |r| r.get(0))?;
    let key = PrKey::parse(&pr).map_err(|_| CpdError::bad_row(format!("Job {job} has the PR key {pr:?}")))?;
    policy_of_pr(conn, home, host, plan, &key)
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

/// How the Gate runs for PR `pr` (contract §7.3, §7.6): `pre-start` with
/// `setup_minutes`, `pre-merge` with `gate_minutes`, `branch` the Lead Branch.
fn gate_hooks<'a>(env: &Env, policy: &'a Policy, plan: &PlanId, pr: &'a PrRow) -> GateHooks<'a> {
    let minutes = |m: u32| Duration::from_secs(u64::from(m) * 60);
    GateHooks {
        lead_branch: &pr.lead_branch,
        base: &pr.base,
        pre_start: Setup { timeout: minutes(env.config.timeouts.setup_minutes), log: pr_log(&env.home, plan, &pr.key, Hook::PreStart) },
        pre_merge: Setup { timeout: minutes(env.config.timeouts.gate_minutes), log: pr_log(&env.home, plan, &pr.key, Hook::PreMerge) },
        pre_start_commands: &policy.hooks.pre_start,
        pre_merge_commands: &policy.hooks.pre_merge,
        slots: Slots::new(&env.home, env.config.test_concurrency),
    }
}

/// The Gate on a snapshot (contract §7.6 steps 1–4) on `sha`: the PR's dead gate
/// worktrees swept, a gate worktree detached at `sha`, `pre-start` and `pre-merge`
/// in it, the gate worktree removed. `again` is the command the Lead runs after fixing it.
/// Nothing runs when the PR's policy says `pr.gate` is not `local` (§8.3).
fn gate(env: &Env, policy: &Policy, plan: &PlanId, pr: &PrRow, sha: &str, again: &str) -> Result<(), CpdError> {
    gate_then(env, policy, plan, pr, sha, &format!("Append fix Jobs (cpd job add), then run cpd {again} again once they are Merged."))
}

/// [`gate`] where `then` is what a refused Gate tells the caller to do next: the Lead's (`pr open`, `pr push`, `pr rebase`) or a Worker's
/// (an `open` or `update` Job's submit, contract §7.7, which fixes what the Gate reports and submits again).
fn gate_then(env: &Env, policy: &Policy, plan: &PlanId, pr: &PrRow, sha: &str, then: &str) -> Result<(), CpdError> {
    if policy.pr.gate != Gate::Local {
        return Ok(());
    }
    git::gate(std::path::Path::new(&pr.repo), plan, &pr.key, sha, &gate_hooks(env, policy, plan, pr))?.map_err(|r| refusal(plan, pr, sha, r, then))
}

/// A Gate that did not pass (contract §7.3, §7.6 step 3): a `pre-merge` that failed
/// is `gate_failed` (exit 3); a `pre-merge` that could not be judged, or a
/// `pre-start` that did not pass in any way, is `gate_error` (exit 1). Both carry
/// the hook's last 60 lines in `details.tail` and its log in `details.log`. `then` follows a failed `pre-merge` in its message.
fn refusal(plan: &PlanId, pr: &PrRow, sha: &str, refused: Refused, then: &str) -> CpdError {
    let tail = refused.tail.unwrap_or_default();
    let log = refused.log.display().to_string();
    let hook = refused.hook.as_str();
    let error = match (refused.hook, &refused.outcome) {
        (Hook::PreMerge, HookOutcome::Failed { exit }) => CpdError::new(
            ErrorCode::GateFailed,
            format!(
                "The Gate failed on {} at {} (exit {exit}): nothing was pushed. {then}",
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
