//! Plans and their phases (contract §2.2, §2.3, §3.1; commands.md §4).
//!
//! - [`jobs`]: a Plan's Jobs as read, the Locked-Job fingerprint of §2.3, the cycle check;
//! - [`display`]: what the views show of cancelled work and of work that waits (one derivation for all of them);
//! - [`fingerprint`]: the plan fingerprint of §2.3, over the draft Jobs and the PRs;
//! - [`render`]: the plan markdown of `plan render` and `plan review`;
//! - [`lead`]: Lead windows in the cockpit (`plan new`, `plan phase plan`);
//! - [`archive`]: `plan archive` and `unarchive`, and the cascade `context archive` shares;
//! - [`cancel`]: `plan cancel`, which abandons a Plan;
//! - `new`, `list`, `show`, `phase`, `review`, `lock`: the commands.

pub mod archive;
pub(crate) mod cancel;
pub(crate) mod delete;
pub mod display;
pub mod fingerprint;
pub mod jobs;
#[cfg(test)]
mod display_tier_tests;
#[cfg(test)]
mod explore_tests;
#[cfg(test)]
mod explore_render_tests;
#[cfg(test)]
mod render_all_jobs_tests;
mod lead;
mod list;
pub(crate) mod lock;
mod new;
mod outline;
mod overlap;
pub mod parked;
pub mod parked_wake;
#[cfg(test)]
mod parked_tests;
#[cfg(test)]
mod tier_advice_tests;
#[cfg(test)]
mod parked_wake_tests;
mod phase;
mod prs;
mod render;
mod rename;
mod review;
pub mod row;
pub(crate) mod shape;
mod show;
mod summary;
pub mod tier_advice;
#[cfg(test)]
mod sql_trace;

use std::path::{Path, PathBuf};

use crate::args::{Name, PlanArg, PrTitle, Text, Title};
use crate::config::Tier;
use crate::context::Jira;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, PlanId, ReviewId};
use crate::output::Output;

pub use fingerprint::plan_fingerprint;
pub use outline::Outline;
pub use jobs::locked_fingerprint;
pub use prs::{PrRow, Pull};
pub use review::{Closing, close_reviews};
pub use row::{Phase, PlanRow};
pub(crate) use new::lead_message;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Create a Plan and start its Lead (§4.1)
    New(NewArgs),
    /// List the Plans (§4.2)
    List(ListArgs),
    /// Show a Plan: the TUI's snapshot (§4.3)
    Show(ShowArgs),
    /// Move the Plan to phase `plan` (§4.4)
    Phase(PhaseArgs),
    /// Render the Plan to run/<plan>/plan.md (§4.5)
    Render(RenderArgs),
    /// Request the User's approval of the draft Jobs (§4.6)
    Review(ReviewArgs),
    /// Record the User's decision on a Plan review (§4.7)
    #[command(hide = true)]
    Decide(DecideArgs),
    /// Set the Plan's outline: what its review summary says (§4.13)
    Outline(OutlineArgs),
    /// Lock the Plan and start dispatching (§4.8)
    Lock(LockArgs),
    /// Change the Plan's title (§4.9)
    Rename(RenameArgs),
    /// Archive the Plan, its Lead and its side forks (§4.10)
    Archive(ArchiveArgs),
    /// Unarchive the Plan, its Lead and its side forks (§4.11)
    Unarchive(UnarchiveArgs),
    /// Cancel the Plan's Jobs and pull requests, close its Lead, archive it (§4.12)
    Cancel(CancelArgs),
    /// Delete an archived or asleep Plan: its rows, files, worktrees and local branches (§4.14)
    Delete(DeleteArgs),
}

#[derive(clap::Args, Debug)]
pub struct NewArgs {
    /// The Plan id
    #[arg(value_name = "id")]
    id: PlanId,
    /// The Plan's Context
    #[arg(long, value_name = "c")]
    context: ContextId,
    /// The title
    #[arg(long, value_name = "text")]
    title: Title,
    /// The Jira task key, stored only
    #[arg(long, value_name = "key")]
    jira: Option<Jira>,
    /// The Lead's pi display name [default: <id>]
    #[arg(long, value_name = "name")]
    name: Option<Name>,
    /// The User's brief for the Lead
    #[arg(long = "brief-file", value_name = "file")]
    brief_file: Option<PathBuf>,
    /// The Lead's Tier [default: review_lead_tier in the review Context, else lead_tier]
    #[arg(long, value_name = "T")]
    tier: Option<Tier>,
    /// The User creates the Plan: its Lead starts with no first message and runs no turn until the User's first prompt
    #[arg(long = "by-user")]
    by_user: bool,
}

/// Filters only: neither falls back to `CPD_PLAN`, so the TUI lists every Plan.
#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Only the Plans of this Context
    #[arg(long, value_name = "id")]
    context: Option<ContextId>,
    /// Only the Plans in this phase
    #[arg(long, value_name = "phase")]
    phase: Option<Phase>,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct PhaseArgs {
    /// The phase to move to: only `plan` is moved by hand
    #[arg(value_name = "phase")]
    phase: Phase,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct RenderArgs {
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ReviewArgs {
    #[command(flatten)]
    plan: PlanArg,
    /// What the drafts under review do: the heading of the review; 1-72 characters on one line
    #[arg(long, value_name = "text")]
    title: PrTitle,
}

#[derive(clap::Args, Debug)]
#[group(id = "decision", required = true, multiple = false, args = ["approve", "reject"])]
pub struct DecideArgs {
    /// The Plan review
    #[arg(value_name = "review")]
    review: ReviewId,
    /// The User approved the draft Jobs
    #[arg(long)]
    approve: bool,
    /// The User rejected them
    #[arg(long)]
    reject: bool,
    /// The User's feedback
    #[arg(long = "feedback-file", value_name = "file")]
    feedback_file: Option<PathBuf>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct OutlineArgs {
    /// The outline, a JSON file (commands.md §4.13)
    #[arg(long, value_name = "outline.json")]
    file: PathBuf,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct LockArgs {
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct RenameArgs {
    #[command(flatten)]
    plan: PlanArg,
    /// The new title
    #[arg(long, value_name = "text")]
    title: Title,
}

#[derive(clap::Args, Debug)]
pub struct ArchiveArgs {
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct UnarchiveArgs {
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct CancelArgs {
    #[command(flatten)]
    plan: PlanArg,
    /// Why [default: plan cancelled]
    #[arg(long, value_name = "text")]
    reason: Option<Text>,
}

#[derive(clap::Args, Debug)]
pub struct DeleteArgs {
    /// The Plan
    #[arg(value_name = "id")]
    id: PlanId,
    /// Delete: without it the command deletes nothing and prints what would go
    #[arg(long)]
    yes: bool,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::New(args) => new::new(args),
        Command::List(args) => list::list(args),
        Command::Show(args) => show::show(args),
        Command::Phase(args) => phase::phase(args),
        Command::Render(args) => review::render(args),
        Command::Review(args) => review::review(args),
        Command::Decide(args) => review::decide(args),
        Command::Outline(args) => outline::outline(args),
        Command::Lock(args) => lock::lock(args),
        Command::Rename(args) => rename::rename(args),
        Command::Archive(args) => archive::plan_archive(args),
        Command::Unarchive(args) => archive::plan_unarchive(args),
        Command::Cancel(args) => cancel::plan_cancel(args),
        Command::Delete(args) => delete::plan_delete(args),
    }
}

/// A file given by path (`--brief-file`, `--feedback-file`): readable UTF-8,
/// else `file_unreadable` (commands.md §1.5).
fn read_text_file(path: &Path) -> Result<String, CpdError> {
    std::fs::read_to_string(path).map_err(|e| {
        CpdError::file_unreadable(path, e)
    })
}

/// Writes a file under `$CPD_HOME` whole, through [`crate::atomic::write_atomic`], so a reader never sees half of it.
fn write_file(path: &Path, text: &str) -> Result<(), CpdError> {
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot write {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    };
    crate::atomic::write_atomic(path, text).map_err(failed)
}
