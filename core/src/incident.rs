//! `cpd incident start | end | status` (commands.md §25, hs-d643): Plan-scoped incident mode.
//! When on (one row in the `incident` table plus one row per Plan in `incident_plans`), the
//! engine holds scheduling for every Plan not in the incident (`is_held`, contract §2.2
//! "Incident", §3.7). `start` and `end` are Manager-only; `status` is for any caller.
//!
//! This file is the stub set of the test Job `inc-v2-tests`: the signatures the red tests call,
//! with the real work marked `unimplemented!()`. Job `inc-v2-impl` replaces them.

use rusqlite::Connection;

use crate::args::Text;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;
use crate::store::{Store, WriteTxn};

/// The `incident` commands.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Turn incident mode on (Manager) (commands.md §25.1)
    Start(StartArgs),
    /// Turn incident mode off (Manager) (commands.md §25.2)
    End,
    /// Report whether incident mode is on (commands.md §25.3)
    Status,
}

#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// The Plans in the incident (at least one)
    #[arg(long = "plan", value_name = "id", required = true)]
    pub plan: Vec<PlanId>,
    /// The explanation of the incident (1 or more characters)
    #[arg(long, value_name = "text")]
    pub reason: Text,
    /// The `<plan>/<key>` PR that ends the incident when it merges
    #[arg(long, value_name = "<plan>/<key>")]
    pub until_pr: Option<String>,
}

/// Runs one `incident` command. The caller's Session is `$CPD_SESSION` (commands.md §25).
pub fn run(command: Command) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let caller = match std::env::var_os("CPD_SESSION").filter(|v| !v.is_empty()) {
        Some(value) => Some(SessionId::parse(&value.to_string_lossy())?),
        None => None,
    };
    match command {
        Command::Start(args) => start(&mut store, caller.as_ref(), &args.plan, args.reason.as_str(), args.until_pr.as_deref()),
        Command::End => end(&mut store, caller.as_ref()),
        Command::Status => status(&mut store),
    }
}

/// Whether incident mode is on (contract §2.2): one row in `incident`.
pub fn is_on(conn: &Connection) -> bool {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM incident)", [], |r| r.get::<_, bool>(0)).unwrap_or(false)
}

/// Whether `plan` is held by incident mode (contract §2.2): the incident is on and `plan` is not
/// in `incident_plans`.
pub fn is_held(_conn: &Connection, _plan: &PlanId) -> bool {
    unimplemented!("incident v2: is_held")
}

/// The set of Plans in the incident, `incident_plans` (contract §2.2).
pub fn plans(_conn: &Connection) -> Result<Vec<PlanId>, CpdError> {
    unimplemented!("incident v2: plans")
}

/// `cpd incident start` (commands.md §25.1): one transaction that inserts the `incident` row and
/// one `incident_plans` row per named Plan, requests the stop of every `starting` or `running`
/// Attempt with no `stop_request`, and appends one `incident-started` Event. `until_pr` is the
/// optional `<plan>/<key>` auto-end PR.
pub fn start(
    _store: &mut Store,
    _caller: Option<&SessionId>,
    _plans: &[PlanId],
    _reason: &str,
    _until_pr: Option<&str>,
) -> Result<Output, CpdError> {
    unimplemented!("incident v2: start")
}

/// `cpd incident end` (commands.md §25.2): one transaction that deletes the row and every
/// `incident_plans` row and appends one `incident-ended` Event with `cause` `"manual"`.
pub fn end(_store: &mut Store, _caller: Option<&SessionId>) -> Result<Output, CpdError> {
    unimplemented!("incident v2: end")
}

/// `cpd incident status` (commands.md §25.3): a read of the row and its Plan set, for any caller.
pub fn status(_store: &mut Store) -> Result<Output, CpdError> {
    unimplemented!("incident v2: status")
}

/// The auto-end (contract §2.2, hs-d643): called in a write transaction right after a merge is
/// recorded. When the stored `until_pr` PR is merged, deletes the `incident` row and every
/// `incident_plans` row and appends one `incident-ended` Event (`cause` `"until-pr"`, `by`
/// `"engine"`), returning true; false when incident mode is off, no `until_pr` is stored, or the
/// PR is not merged.
pub fn end_if_until_pr_merged(_tx: &WriteTxn<'_>) -> Result<bool, CpdError> {
    unimplemented!("incident v2: end_if_until_pr_merged")
}

/// The coarse human duration from the incident row's `started_at` to `now` (hs-d640, contract
/// §6.1): under 60 s `{n}s`; under 1 h `{n}m`; under 1 d `{n}h {n}m`; otherwise `{n}d {n}h`;
/// values truncated, a `started_at` in the future reads `0s`. `started_at` is RFC 3339, the format
/// `clock::timestamp` writes; anything else is a loud `StoreFailed`.
pub fn elapsed(started_at: &str, now: time::OffsetDateTime) -> Result<String, CpdError> {
    let started = time::OffsetDateTime::parse(started_at, &time::format_description::well_known::Rfc3339).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("The incident holds a started_at that is not RFC 3339: {started_at:?} ({e}).")).with("started_at", started_at)
    })?;
    let seconds = (now - started).whole_seconds().max(0);
    Ok(if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
    } else {
        format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3_600)
    })
}
