//! `cpd incident start | end | status` (commands.md §25, hs-d634): machine-wide incident mode.
//! When on (one row in the `incident` table), the engine pauses most scheduling and every running
//! Attempt is requeued when it ends (contract §2.2 "Incident", §3.7). `start` and `end` are
//! Manager-only; `status` is for any caller.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use crate::args::Text;
use crate::clock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::SessionId;
use crate::output::Output;
use crate::store::Store;

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
    /// The explanation of the incident (1 or more characters)
    #[arg(long, value_name = "text")]
    pub reason: Text,
}

/// Runs one `incident` command. The caller's Session is `$CPD_SESSION` (commands.md §25).
pub fn run(command: Command) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let caller = match std::env::var_os("CPD_SESSION").filter(|v| !v.is_empty()) {
        Some(value) => Some(SessionId::parse(&value.to_string_lossy())?),
        None => None,
    };
    match command {
        Command::Start(args) => start(&mut store, caller.as_ref(), args.reason.as_str()),
        Command::End => end(&mut store, caller.as_ref()),
        Command::Status => status(&mut store),
    }
}

/// The refusal of a caller that is not a live Manager (commands.md §25).
fn manager_only() -> CpdError {
    CpdError::new(ErrorCode::ManagerOnly, "Only a live Manager Session may change incident mode.")
}

/// The caller must be a live (not archived) Manager Session.
fn require_manager<'a>(conn: &Connection, caller: Option<&'a SessionId>) -> Result<&'a SessionId, CpdError> {
    let caller = caller.ok_or_else(manager_only)?;
    let manager: Option<bool> = conn
        .query_row(
            "SELECT role = 'manager' FROM sessions WHERE id = ?1 AND archived_at IS NULL",
            [caller.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    match manager {
        Some(true) => Ok(caller),
        _ => Err(manager_only()),
    }
}

/// The current `incident` row as the output's `incident` value, if any.
fn row(conn: &Connection) -> Result<Option<Value>, CpdError> {
    Ok(conn
        .query_row(
            "SELECT started_at, started_by, reason FROM incident WHERE id = 1",
            [],
            |r| Ok(json!({"started_at": r.get::<_, String>(0)?, "by": r.get::<_, String>(1)?, "reason": r.get::<_, String>(2)?})),
        )
        .optional()?)
}

/// Whether incident mode is on (contract §2.2): one row in `incident`.
pub fn is_on(conn: &Connection) -> bool {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM incident)", [], |r| r.get::<_, bool>(0)).unwrap_or(false)
}

/// `cpd incident start` (commands.md §25.1): one transaction that inserts the row, requests the
/// stop of every `starting` or `running` Attempt with no `stop_request` and appends one
/// `incident-started` Event. `caller` is the Manager's Session.
pub fn start(store: &mut Store, caller: Option<&SessionId>, reason: &str) -> Result<Output, CpdError> {
    if reason.is_empty() {
        return Err(CpdError::new(ErrorCode::Usage, "The incident needs a reason of at least 1 character."));
    }
    store.write("incident start", |tx| {
        let caller = require_manager(tx, caller)?;
        if let Some(existing) = row(tx)? {
            return Ok(Output::success(json!({"changed": false, "incident": existing})));
        }
        let started_at = clock::now();
        tx.execute(
            "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, ?1, ?2, ?3, ?4)",
            rusqlite::params![started_at, caller.as_str(), reason, tx.id()],
        )?;
        tx.execute(
            "UPDATE attempts SET stop_request = 'cancelled'
             WHERE status IN ('starting', 'running') AND stop_request IS NULL",
            [],
        )?;
        append(
            tx,
            NewEvent::incident_started("Incident mode on.", caller.as_str(), reason, &started_at),
        )?;
        Ok(Output::success(json!({
            "changed": true,
            "incident": {"started_at": started_at, "by": caller.as_str(), "reason": reason},
        })))
    })
}

/// `cpd incident end` (commands.md §25.2): one transaction that deletes the row and appends one
/// `incident-ended` Event. `caller` is the Manager's Session.
pub fn end(store: &mut Store, caller: Option<&SessionId>) -> Result<Output, CpdError> {
    store.write("incident end", |tx| {
        let caller = require_manager(tx, caller)?;
        if row(tx)?.is_none() {
            return Ok(Output::success(json!({"changed": false, "incident": null})));
        }
        tx.execute("DELETE FROM incident WHERE id = 1", [])?;
        let ended_at = clock::now();
        append(tx, NewEvent::incident_ended("Incident mode off.", caller.as_str(), &ended_at))?;
        Ok(Output::success(json!({"changed": true, "incident": null})))
    })
}

/// `cpd incident status` (commands.md §25.3): a read of the row, for any caller.
pub fn status(store: &mut Store) -> Result<Output, CpdError> {
    let incident = store.read(row)?;
    Ok(Output::success(json!({"on": incident.is_some(), "incident": incident})))
}
