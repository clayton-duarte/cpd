//! Sessions and their tmux panes (contract §2.2, §3.6, §6; commands.md §10,
//! with §19 amendments 1–3: the chat slot, its placeholder, park and open).
//!
//! - [`resolve`]: short Session ids (commands.md §1.3);
//! - [`row`]: the `sessions` row as read, and `session list` / `show`;
//! - [`state`]: the agent states of §3.6, `session state` and `rename`;
//! - [`moves`]: the panes: `session fork`, `open`, `park` and `close`;
//! - [`history`]: `session history`, an archived Session's history in the chat slot;
//! - [`archive`]: `session archive` and `unarchive`, and the archive rule;
//! - [`send`]: `session send`, one message to a Lead;
//! - [`port`]: a Plan's plannotator port;
//! - [`run`]: `session run`, the program in every Lead pane (§6.7).

mod archive;
mod history;
mod moves;
mod port;
mod resolve;
mod row;
pub(crate) mod run;
mod send;
mod state;

use std::path::PathBuf;

use crate::args::{Name, SessionArg, SessionRef, Text};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::store::Store;

pub use archive::{Blocker, archive_blocked, blockers};
pub use moves::{Closed, Place, Placed, close_lead, placed, reopen};
pub(crate) use moves::lead_spawn;
pub use row::{Role, State};
pub use state::Label;
pub(crate) use state::pane_gone;

/// The program in every pane cpd starts, resolved on the tmux server's `PATH`
/// (Escalation `rs-session-a1-q2`): a pane must outlive any one build.
pub const PROGRAM: &str = "cpd";

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// List Sessions (§10.1)
    List(ListArgs),
    /// Show a Session (§10.2)
    Show(ShowArgs),
    /// Fork a Lead into a parked window (§10.3)
    Fork(ForkArgs),
    /// Bring a Lead, or a Context's Manager, into the chat slot (§10.4)
    Open(OpenArgs),
    /// Move the Lead or Manager in the chat slot to a parked window (§10.5)
    Park(ParkArgs),
    /// End a Lead's pane; its session file stays (§10.9)
    Close(CloseArgs),
    /// Show a Lead Session's history, read-only, in the chat slot (§10.12)
    History(HistoryArgs),
    /// Archive a Lead Session (§10.10)
    Archive(ArchiveArgs),
    /// Unarchive a Lead Session (§10.11)
    Unarchive(UnarchiveArgs),
    /// Send a message to a Lead (§10.13)
    Send(SendArgs),
    /// Rename a Session (§10.7)
    Rename(RenameArgs),
    /// Report a Session's agent state (pi adapter) (§10.6)
    #[command(hide = true)]
    State(StateArgs),
    /// The program in every Lead pane (internal; owns its terminal) (§10.8)
    #[command(hide = true)]
    Run(RunArgs),
}

/// Filters, combined with AND; `--context` and `--plan` never fall back to `CPD_PLAN`.
#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Only Sessions of this Context
    #[arg(long, value_name = "id")]
    context: Option<ContextId>,
    /// Only Sessions of this Plan
    #[arg(long, value_name = "id")]
    plan: Option<PlanId>,
    /// Only this role (repeatable)
    #[arg(long = "role", value_name = "role")]
    roles: Vec<Role>,
    /// Only this state (repeatable)
    #[arg(long = "state", value_name = "state")]
    states: Vec<State>,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// The Session [default: $CPD_SESSION]
    #[arg(value_name = "id", env = "CPD_SESSION", hide_env = true)]
    id: Option<SessionRef>,
}

#[derive(clap::Args, Debug)]
pub struct ForkArgs {
    /// The Lead Session to fork
    #[arg(value_name = "source")]
    source: SessionRef,
    /// The new Session's Plan [default: the source's Plan]
    #[arg(long, value_name = "id")]
    plan: Option<PlanId>,
    /// Its name [default: "<source name> (fork)"]
    #[arg(long, value_name = "name")]
    name: Option<Name>,
    /// Its first message
    #[arg(long = "message-file", value_name = "file")]
    message_file: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
pub struct OpenArgs {
    /// The Lead or Manager Session to bring into the chat slot
    #[arg(value_name = "id")]
    id: SessionRef,
}

#[derive(clap::Args, Debug)]
pub struct ParkArgs {
    /// The Lead or Manager Session to park [default: the one in the chat slot]
    #[arg(value_name = "id")]
    id: Option<SessionRef>,
}

#[derive(clap::Args, Debug)]
pub struct HistoryArgs {
    /// The Lead Session whose history to show
    #[arg(value_name = "id")]
    id: SessionRef,
}

#[derive(clap::Args, Debug)]
pub struct CloseArgs {
    /// The Lead Session whose pane to close
    #[arg(value_name = "id")]
    id: SessionRef,
}

#[derive(clap::Args, Debug)]
pub struct ArchiveArgs {
    /// The Lead Session to archive
    #[arg(value_name = "id")]
    id: SessionRef,
}

#[derive(clap::Args, Debug)]
pub struct UnarchiveArgs {
    /// The Lead Session to unarchive
    #[arg(value_name = "id")]
    id: SessionRef,
}

#[derive(clap::Args, Debug)]
#[command(group(clap::ArgGroup::new("message").required(true).multiple(false).args(["text", "file"])))]
pub struct SendArgs {
    /// The Lead Session to send to
    #[arg(value_name = "id")]
    id: SessionRef,
    /// The message (1–20 000 characters)
    #[arg(value_name = "text")]
    text: Option<Text>,
    /// The message, read from a file
    #[arg(long, value_name = "f")]
    file: Option<PathBuf>,
    /// Start a turn when the target is idle (a Manager waking a stalled Lead; use sparingly)
    #[arg(long)]
    wake: bool,
}

#[derive(clap::Args, Debug)]
pub struct RenameArgs {
    /// The new name
    #[arg(value_name = "name")]
    name: Name,
    #[command(flatten)]
    session: SessionArg,
}

#[derive(clap::Args, Debug)]
pub struct StateArgs {
    /// The agent state
    #[arg(value_name = "state")]
    state: State,
    /// Why it is blocked: required with `blocked`, refused otherwise
    #[arg(long, value_name = "text")]
    label: Option<Label>,
    /// The run that ended answered a message the User wrote to this Session: allowed only with `idle`
    #[arg(long)]
    reply: bool,
    #[command(flatten)]
    session: SessionArg,
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// The Lead Session this pane runs
    #[arg(value_name = "id")]
    id: SessionId,
    /// The pid of the pi a previous wrapper left running across its `exec`
    /// (internal: written only by the wrapper's own `exec`, §10.8)
    #[arg(long, value_name = "pid", hide = true, value_parser = clap::value_parser!(i32).range(2..))]
    adopt: Option<i32>,
    /// With `--adopt`: pi's install changed before the `exec` (internal, §10.8)
    #[arg(long, hide = true, requires = "adopt")]
    pi_stale: bool,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List(args) => row::list(args),
        Command::Show(args) => row::show(args),
        Command::Fork(args) => moves::fork(args),
        Command::Open(args) => moves::open(args),
        Command::Park(args) => moves::park(args),
        Command::Close(args) => moves::close(args),
        Command::History(args) => history::history(args),
        Command::Archive(args) => archive::archive(args),
        Command::Unarchive(args) => archive::unarchive(args),
        Command::Send(args) => send::send(args),
        Command::Rename(args) => state::rename(args),
        Command::State(args) => state::state(args),
        Command::Run(args) => run::run(args),
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

    /// The Session `reference` names (commands.md §1.3), resolved once at the boundary.
    fn resolve(&mut self, reference: &SessionRef) -> Result<SessionId, CpdError> {
        self.store.read(|conn| resolve::resolve(conn, reference))
    }

    fn tmux(&self) -> Tmux {
        Tmux::new(self.config.tmux.socket.clone())
    }
}

fn session_not_found(id: &SessionId) -> CpdError {
    CpdError::new(ErrorCode::SessionNotFound, format!("No Session {id}.")).with("session", id.as_str())
}

fn not_a_lead(id: &SessionId, role: Role) -> CpdError {
    CpdError::new(ErrorCode::NotALead, format!("Session {id} is a {}, not a Lead.", role.as_str()))
        .with("session", id.as_str())
        .with("role", role.as_str())
}

pub(crate) fn no_cockpit(cockpit: &str) -> CpdError {
    CpdError::new(
        ErrorCode::NoCockpit,
        format!("The cockpit (tmux session {cockpit:?} with its main window and chat slot) does not exist: run cpd tui."),
    )
    .with("cockpit_session", cockpit)
    .with("next", "cpd tui")
}

/// A row the database holds in a shape the binary does not accept: the
/// database was changed outside cpd, or by a bug.
fn bad_row(what: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid Session row: {what}."))
}
