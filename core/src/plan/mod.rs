//! Plans and their phases (contract §2.2, §2.3, §3.1; commands.md §4).
//!
//! - [`jobs`]: a Plan's Jobs as read, the Locked-Job fingerprint of §2.3, the cycle check;
//! - [`fingerprint`]: the plan fingerprint of §2.3, over the draft Jobs and the PRs;
//! - [`render`]: the plan markdown of `plan render` and `plan review`;
//! - [`lead`]: Lead windows in the cockpit (`plan new`, `plan phase plan`);
//! - [`archive`]: `plan archive` and `unarchive`, and the cascade `context archive` shares;
//! - `new`, `list`, `show`, `phase`, `review`, `lock`: the commands.

pub mod archive;
pub mod fingerprint;
pub mod jobs;
mod lead;
mod list;
mod lock;
mod new;
mod phase;
mod render;
mod rename;
mod review;
mod row;
mod show;

use std::path::{Path, PathBuf};

use crate::args::{Name, PlanArg, Title};
use crate::config::Config;
use crate::context::Jira;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, PlanId, ReviewId};
use crate::output::Output;
use crate::store::Store;

pub use fingerprint::plan_fingerprint;
pub use jobs::locked_fingerprint;
pub use row::Phase;

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
    Decide(DecideArgs),
    /// Lock the Plan and start dispatching (§4.8)
    Lock(LockArgs),
    /// Change the Plan's title (§4.9)
    Rename(RenameArgs),
    /// Archive the Plan, its Lead and its side forks (§4.10)
    Archive(ArchiveArgs),
    /// Unarchive the Plan, its Lead and its side forks (§4.11)
    Unarchive(UnarchiveArgs),
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

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::New(args) => new::new(args),
        Command::List(args) => list::list(args),
        Command::Show(args) => show::show(args),
        Command::Phase(args) => phase::phase(args),
        Command::Render(args) => review::render(args),
        Command::Review(args) => review::review(args),
        Command::Decide(args) => review::decide(args),
        Command::Lock(args) => lock::lock(args),
        Command::Rename(args) => rename::rename(args),
        Command::Archive(args) => archive::plan_archive(args),
        Command::Unarchive(args) => archive::plan_unarchive(args),
    }
}

/// What every command opens: `$CPD_HOME`, its config and the store.
struct Env {
    home: CpdHome,
    config: Config,
    store: Store,
}

impl Env {
    fn open() -> Result<Env, CpdError> {
        let home = CpdHome::from_env()?;
        let config = Config::load(&home)?;
        let store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
        Ok(Env { home, config, store })
    }
}

fn plan_not_found(plan: &PlanId) -> CpdError {
    CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str())
}

/// A row the database holds in a shape the binary does not accept: it was
/// changed outside cpd, or by a bug.
fn bad_row(what: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid row: {what}."))
}

/// A file given by path (`--brief-file`, `--feedback-file`): readable UTF-8,
/// else `file_unreadable` (commands.md §1.5).
fn read_text_file(path: &Path) -> Result<String, CpdError> {
    std::fs::read_to_string(path).map_err(|e| {
        CpdError::new(ErrorCode::FileUnreadable, format!("Cannot read {} as UTF-8 text: {e}.", path.display()))
            .with("path", path.display().to_string())
    })
}

/// Writes a file under `$CPD_HOME` whole: to a temporary file beside it, then
/// renamed over it, so a reader never sees half of it.
fn write_file(path: &Path, text: &str) -> Result<(), CpdError> {
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot write {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    };
    let dir = path.parent().expect("a file under $CPD_HOME has a directory");
    std::fs::create_dir_all(dir).map_err(failed)?;
    let tmp = dir.join(format!(".{}.{}.tmp", path.file_name().expect("a file name").to_string_lossy(), std::process::id()));
    std::fs::write(&tmp, text).map_err(failed)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        failed(e)
    })
}
