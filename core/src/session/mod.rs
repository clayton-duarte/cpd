//! Sessions and their tmux panes (contract §2.2, §3.6, §6; commands.md §10,
//! with §19 amendments 1–3: the chat slot, its placeholder, park and open).
//!
//! - [`resolve`]: short Session ids (commands.md §1.3);
//! - [`row`]: the `sessions` row as read, and `session list` / `show`;
//! - [`state`]: the agent states of §3.6, `session state` and `rename`;
//! - [`moves`]: the panes: `session fork`, `open`, `park` and `close`;
//! - [`history`]: `session history`, an archived Session's history in the chat slot;
//! - [`archive`]: `session archive` and `unarchive`, and the archive rule;
//! - [`retier`]: `session retier`, a live Lead's or Manager's Tier set to the config's;
//! - [`send`]: `session send`, one message to a Lead;
//! - [`resume`]: `session resume`, a Lead or Manager blocked by the loop guard, resumed;
//! - [`retitle`]: `session retitle`, a Lead's or a Manager's dynamic title;
//! - [`run`]: `session run`, the program in every Lead pane (§6.7);
//! - `reload`: when it restarts pi (reload on install, adapter watchdog);
//! - `fork_wait`: a fork's wait for its source Lead to finish its turn.

mod archive;
#[cfg(test)]
mod durable_moves_tests;
#[cfg(test)]
mod durable_send_tests;
mod first_message;
mod fork_wait;
mod history;
mod moves;
#[cfg(test)]
mod phi_label_tests;
#[cfg(test)]
mod ports_tests;
mod reload;
mod report;
mod resolve;
#[cfg(test)]
mod resolve_tests;
mod resume;
mod retier;
pub(crate) mod retitle;
pub(crate) mod row;
pub(crate) mod run;
pub(crate) mod send;
mod state;
mod switch;
pub mod tail;
#[cfg(test)]
mod tail_tests;
#[cfg(test)]
mod title_tests;
pub(crate) mod working_pr;

use std::path::PathBuf;

use crate::args::{Name, SessionArg, SessionRef, Text};
use crate::config::Tier;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::host::Tmux;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::ports::Ports;

pub use archive::{Blocker, archive_blocked, archive_ended_workers, blockers};
pub(crate) use moves::lead_spawn;
pub use moves::{Closed, Place, Placed, close_lead, gone, placed, reopen};
pub use resume::LOOP_GUARD_LABEL;
pub use row::{Role, State};
pub use send::{plan_inactive, sender, tell, tell_manager_of, tell_managers};
pub use state::Label;

/// The Session `reference` names (commands.md §1.3), for the commands outside this module.
pub fn resolve(conn: &rusqlite::Connection, reference: &SessionRef) -> Result<SessionId, CpdError> {
    resolve::resolve(conn, reference)
}
pub use state::pane_gone;

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
    /// Send a message to a Lead or Manager (§10.13)
    Send(SendArgs),
    /// Stream a durable Session's transcript, read-only (§10.19)
    Tail(TailArgs),
    /// Resume a Lead or Manager blocked by the loop guard (§10.17)
    Resume(ResumeArgs),
    /// Move live Leads and Managers to the Tier the config names now (§10.15)
    Retier(RetierArgs),
    /// Rename a Session (§10.7)
    Rename(RenameArgs),
    /// Report a Session's agent state (pi adapter) (§10.6)
    #[command(hide = true)]
    State(StateArgs),
    /// Render a Lead's or Manager's first message now and write its file (adapter) (§10.15)
    #[command(hide = true, name = "first-message")]
    FirstMessage(FirstMessageArgs),
    /// Write a Lead's or Manager's dynamic title with a one-shot pi (engine E10) (§10.14)
    #[command(hide = true)]
    Retitle(RetitleArgs),
    /// Report an adapter error to the Manager (pi adapter) (§10.18)
    #[command(hide = true)]
    Report(ReportArgs),
    /// Record a failed model switch (pi adapter) (§10.18a)
    #[command(hide = true, name = "switch-failed")]
    SwitchFailed(SwitchFailedArgs),
    /// Record a successful model switch (pi adapter) (§10.18b)
    #[command(hide = true)]
    Switched(SwitchedArgs),
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
    /// List archived Sessions too
    #[arg(long)]
    archived: bool,
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
#[command(override_usage = "cpd session send <id> (<text> | --file <f>) --unblocks <reason>")]
#[command(group(clap::ArgGroup::new("message").required(true).multiple(false).args(["text", "file"])))]
pub struct SendArgs {
    /// The Lead or Manager Session to send to
    #[arg(value_name = "id")]
    id: SessionRef,
    /// The message (1–20 000 characters)
    #[arg(value_name = "text")]
    text: Option<Text>,
    /// The message, read from a file
    #[arg(long, value_name = "f")]
    file: Option<PathBuf>,
    /// What the message unblocks (1–200 characters, one line)
    #[arg(long, value_name = "reason")]
    unblocks: Unblocks,
}

#[derive(clap::Args, Debug)]
pub struct TailArgs {
    /// The durable Session to tail
    #[arg(value_name = "id")]
    id: SessionId,
}

/// What a `session send` unblocks (commands.md §10.13): 1 to 200 characters on one line; else `usage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unblocks(String);

impl Unblocks {
    const MAX_CHARS: usize = 200;

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for Unblocks {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Unblocks, CpdError> {
        let chars = s.chars().count();
        if chars == 0 || chars > Unblocks::MAX_CHARS || s.chars().any(char::is_control) {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!(
                    "--unblocks is 1 to {} characters on one line.",
                    Unblocks::MAX_CHARS
                ),
            )
            .with("chars", chars));
        }
        Ok(Unblocks(s.to_owned()))
    }
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
    /// Why it is waiting or blocked: required with `waiting` and `blocked`, refused otherwise
    #[arg(long, value_name = "text")]
    label: Option<Label>,
    /// The run that ended answered a message the User wrote to this Session: allowed only with `idle`
    #[arg(long)]
    reply: bool,
    #[command(flatten)]
    session: SessionArg,
}

#[derive(clap::Args, Debug)]
pub struct ResumeArgs {
    /// The Lead or Manager Session blocked by the loop guard
    #[arg(value_name = "id")]
    id: SessionRef,
}

#[derive(clap::Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).multiple(false).args(["id", "all"])))]
pub struct RetierArgs {
    /// The Lead or Manager Session to move
    #[arg(value_name = "id")]
    id: Option<SessionRef>,
    /// Every Lead and Manager that is not archived
    #[arg(long)]
    all: bool,
    /// Move the one Session to this Tier instead of the config's target
    #[arg(long, value_name = "T", requires = "id", conflicts_with = "all")]
    tier: Option<Tier>,
}

#[derive(clap::Args, Debug)]
pub struct FirstMessageArgs {
    /// The Lead or Manager Session [default: $CPD_SESSION]
    #[arg(long, value_name = "id", env = "CPD_SESSION", hide_env = true)]
    session: Option<SessionRef>,
}

#[derive(clap::Args, Debug)]
pub struct RetitleArgs {
    /// The Lead or Manager Session to title
    #[arg(value_name = "session")]
    id: SessionRef,
    /// The chat slot pane's width in columns (hs-d543): the title is at most this less 8 characters (at most 200)
    #[arg(long, value_name = "cols", value_parser = clap::value_parser!(u32).range(9..))]
    width: u32,
}

#[derive(clap::Args, Debug)]
pub struct ReportArgs {
    /// What failed
    #[arg(long, value_name = "text")]
    what: report::Short,
    /// The adapter's error code
    #[arg(long, value_name = "code")]
    code: report::Code,
    /// The error's message
    #[arg(long, value_name = "text")]
    message: report::Short,
    /// More details, a JSON object
    #[arg(long, value_name = "json")]
    details: Option<report::Details>,
    /// The Session reporting [default: $CPD_SESSION]
    #[arg(long, value_name = "id", env = "CPD_SESSION", hide_env = true)]
    session: Option<SessionRef>,
}

#[derive(clap::Args, Debug)]
pub struct SwitchFailedArgs {
    /// The adapter's error code
    #[arg(long, value_name = "code")]
    code: report::Code,
    /// The error's message
    #[arg(long, value_name = "text")]
    message: report::Short,
    /// The Session whose switch failed [default: $CPD_SESSION]
    #[arg(long, value_name = "id", env = "CPD_SESSION", hide_env = true)]
    session: Option<SessionRef>,
}

#[derive(clap::Args, Debug)]
pub struct SwitchedArgs {
    /// The Session whose switch succeeded [default: $CPD_SESSION]
    #[arg(long, value_name = "id", env = "CPD_SESSION", hide_env = true)]
    session: Option<SessionRef>,
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

/// The cwd of every Manager and Lead: the User's home directory (decision pro-d343).
pub fn agent_cwd() -> Result<String, CpdError> {
    agent_cwd_in(std::env::home_dir())
}

/// [`agent_cwd`] for a given home directory (`None`: unknown, `usage`).
pub fn agent_cwd_in(home: Option<PathBuf>) -> Result<String, CpdError> {
    let home = crate::host::pi::PiDirs::resolve(home, None, None)?.home;
    Ok(home.canonicalize().unwrap_or(home).display().to_string())
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
        Command::Tail(args) => tail::command(args),
        Command::Resume(args) => resume::resume(args),
        Command::Retier(args) => retier::retier(args),
        Command::Rename(args) => state::rename(args),
        Command::State(args) => state::state(args),
        Command::FirstMessage(args) => first_message::first_message(args),
        Command::Retitle(args) => retitle::retitle(args),
        Command::Report(args) => report::report(args),
        Command::SwitchFailed(args) => switch::failed(args),
        Command::Switched(args) => switch::switched(args),
        Command::Run(args) => run::run(args),
    }
}

/// Starts Session `id` (a Lead or Manager) as a parked window, as `command` (what engine E2 calls): [`moves::start_parked`] on
/// an `Env` of `home` on `ports`. Returns the pane, or none when another process recorded one first.
pub fn start_parked_as(
    ports: &Ports,
    home: crate::home::CpdHome,
    id: &SessionId,
    command: &str,
) -> Result<Option<crate::host::PaneId>, CpdError> {
    Ports::scope(ports, || {
        let mut env = Env::open_home_with(ports, home)?;
        let row = env.store.read(|conn| row::load(conn, id))?;
        moves::start_parked(&mut env, &row, command)
    })
}

impl Env {
    /// The Session `reference` names (commands.md §1.3), resolved once at the boundary.
    fn resolve(&mut self, reference: &SessionRef) -> Result<SessionId, CpdError> {
        self.store.read(|conn| resolve::resolve(conn, reference))
    }

    /// The one construction site of a session command's tmux: its calls run on the runner of the ambient [`Ports`]
    /// (the real one, or the fakes of `run_with`).
    fn tmux(&self) -> Tmux {
        Tmux::new(self.config.tmux.socket.clone()).with_runner(Ports::current().runner)
    }
}

fn session_not_found(id: &SessionId) -> CpdError {
    CpdError::new(ErrorCode::SessionNotFound, format!("No Session {id}."))
        .with("session", id.as_str())
}

fn not_a_lead(id: &SessionId, role: Role) -> CpdError {
    CpdError::new(
        ErrorCode::NotALead,
        format!("Session {id} is a {}, not a Lead.", role.as_str()),
    )
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
