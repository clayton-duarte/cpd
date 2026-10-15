//! Sessions and their tmux panes (contract §2.2, §3.6, §6; commands.md §10,
//! with §19 amendments 1–3: the chat slot, its placeholder, park and open).
//!
//! - [`row`]: the `sessions` row as read, and `session list` / `show`;
//! - [`state`]: the agent states of §3.6, `session state` and `rename`;
//! - [`moves`]: the panes: `session fork`, `open` and `park`;
//! - [`run`]: `session run`, the program in every Lead pane (§6.7).

mod moves;
mod row;
mod run;
mod state;

use std::path::PathBuf;

use crate::args::{Name, SessionArg};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::store::Store;

pub use row::{Role, State};
pub use state::Label;

/// The program in every pane cpd starts, resolved on the tmux server's `PATH`
/// (Escalation `rs-session-a1-q2`): a pane must outlive any one build.
pub(crate) const PROGRAM: &str = "cpd2";

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// List Sessions (§10.1)
    List(ListArgs),
    /// Show a Session (§10.2)
    Show(ShowArgs),
    /// Fork a Lead into a parked window (§10.3)
    Fork(ForkArgs),
    /// Bring a Lead into the chat slot (§10.4)
    Open(OpenArgs),
    /// Move the Lead in the chat slot to a parked window (§10.5)
    Park(ParkArgs),
    /// Rename a Session (§10.7)
    Rename(RenameArgs),
    /// Report a Session's agent state (pi adapter) (§10.6)
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
    id: Option<SessionId>,
}

#[derive(clap::Args, Debug)]
pub struct ForkArgs {
    /// The Lead Session to fork
    #[arg(value_name = "source")]
    source: SessionId,
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
    /// The Lead Session to bring into the chat slot
    #[arg(value_name = "id")]
    id: SessionId,
}

#[derive(clap::Args, Debug)]
pub struct ParkArgs {
    /// The Lead Session to park [default: the one in the chat slot]
    #[arg(value_name = "id")]
    id: Option<SessionId>,
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
    #[command(flatten)]
    session: SessionArg,
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// The Lead Session this pane runs
    #[arg(value_name = "id")]
    id: SessionId,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List(args) => row::list(args),
        Command::Show(args) => row::show(args),
        Command::Fork(args) => moves::fork(args),
        Command::Open(args) => moves::open(args),
        Command::Park(args) => moves::park(args),
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

fn no_cockpit(cockpit: &str) -> CpdError {
    CpdError::new(
        ErrorCode::NoCockpit,
        format!("The cockpit (tmux session {cockpit:?} with its main window and chat slot) does not exist: run cpd2 tui."),
    )
    .with("cockpit_session", cockpit)
    .with("next", "cpd2 tui")
}

/// A row the database holds in a shape the binary does not accept: the
/// database was changed outside cpd, or by a bug.
fn bad_row(what: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid Session row: {what}."))
}
