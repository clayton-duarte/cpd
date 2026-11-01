//! `cpd host approve` and `cpd host repair` (commands.md §9.11; hs-d618): the User-approved
//! repair of the cpd checkout `$HOME/projects/cpd` when dotfiles' install skipped its
//! fast-forward.
//!
//! - `cpd host approve` (internal; the adapter's `cpd_host_approve` tool after the User's
//!   Allow once): Manager only; writes one private approval file for the calling Session,
//!   valid 10 minutes, one repair.
//! - `cpd host repair` (Manager only; contract §3.7 E6): saves the diff and the old HEAD as
//!   a ref, then resets `main` to `origin/main`. Needs a live approval of the calling
//!   Session; the User running it from a terminal needs none.

use std::path::PathBuf;

use clap::Subcommand;
use rusqlite::OptionalExtension;
use serde_json::json;
use time::OffsetDateTime;
use time::macros::format_description;

use crate::args::SessionRef;
use crate::clock;
use crate::error::{CpdError, ErrorCode};
use crate::git::run::ok as git_ok;
use crate::home::CpdHome;
use crate::host_repair_approval::{self as approval, Approval};
use crate::output::Output;
use crate::session::{self, Role};

/// The `cpd host` commands (commands.md §9.11).
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Record the User's approval for one checkout repair (adapter) (§9.11)
    #[command(hide = true)]
    Approve,
    /// Repair the skipped cpd install (Manager) (§9.11)
    Repair,
}

/// The checkout a repair always works in (commands.md §9.11): `$HOME/projects/cpd`, no argument.
fn checkout() -> Result<PathBuf, CpdError> {
    let home = std::env::var_os("HOME").ok_or_else(|| {
        CpdError::new(
            ErrorCode::Internal,
            "HOME is not set: cannot find the cpd checkout.",
        )
    })?;
    Ok(PathBuf::from(home).join("projects/cpd"))
}

fn session_not_found(s: &str) -> CpdError {
    CpdError::new(ErrorCode::SessionNotFound, format!("No Session {s}.")).with("session", s)
}

fn not_a_manager() -> CpdError {
    CpdError::new(
        ErrorCode::NotAManager,
        "`cpd host` is for the Manager: the calling Session is not a Manager.",
    )
}

/// `host_approval_required`: the calling Manager holds no live approval (commands.md §9.11).
fn approval_required() -> CpdError {
    CpdError::new(
        ErrorCode::HostApprovalRequired,
        "`cpd host repair` needs a live approval of the calling Session: ask the User with the tool cpd_host_approve.",
    )
}

/// The Session of `CPD_SESSION` and its role, when there is one. `None` is the User at a terminal
/// (no `CPD_SESSION`): only [`repair`] accepts it. A `CPD_ATTEMPT` makes the caller a Worker.
fn caller(env: &mut crate::env::Env) -> Result<Option<(crate::ids::SessionId, Role)>, CpdError> {
    let Some(raw) = std::env::var("CPD_SESSION").ok().filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let reference: SessionRef = raw.parse().map_err(|_| session_not_found(&raw))?;
    let attempt = std::env::var("CPD_ATTEMPT")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some();
    let (id, role) = env.store.read(|conn| {
        let id = session::resolve(conn, &reference)?;
        let role: String = conn
            .query_row(
                "SELECT role FROM sessions WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| session_not_found(id.as_str()))?;
        let role = match role.as_str() {
            "manager" => Role::Manager,
            "lead" => Role::Lead,
            "worker" => Role::Worker,
            other => return Err(CpdError::bad_row(format!("sessions.role {other:?}"))),
        };
        Ok((id, if attempt { Role::Worker } else { role }))
    })?;
    Ok(Some((id, role)))
}

/// Runs a `host` command.
pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Approve => approve(),
        Command::Repair => repair(),
    }
}

/// `cpd host approve` (commands.md §9.11): one private approval for the calling Manager.
fn approve() -> Result<Output, CpdError> {
    let mut env = crate::env::Env::open()?;
    let Some((session, role)) = caller(&mut env)? else {
        return Err(not_a_manager());
    };
    if role != Role::Manager {
        return Err(not_a_manager());
    }
    let at = OffsetDateTime::now_utc();
    let held = Approval {
        session: session.to_string(),
        at: clock::timestamp(at),
    };
    approval::approve(&env.home, &held)?;
    Ok(Output::success(json!({})))
}

/// `<ts>` of commands.md §9.11: UTC `YYYYMMDDTHHMMSSZ`.
fn stamp(at: OffsetDateTime) -> String {
    at.to_offset(time::UtcOffset::UTC)
        .format(format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .expect("a UTC time always formats")
}

/// Writes the saved diff to `$CPD_HOME/run/cpd-install-backups/<ts>.patch` (0600) and returns its path.
fn save_patch(home: &CpdHome, ts: &str, patch: &str) -> Result<PathBuf, CpdError> {
    let dir = home.run_dir().join("cpd-install-backups");
    approval::write_private(home, &dir, &format!("{ts}.patch"), patch.as_bytes())
}

/// `cpd host repair` (commands.md §9.11): save the diff and the old HEAD, then reset `main`.
fn repair() -> Result<Output, CpdError> {
    let mut env = crate::env::Env::open()?;
    let waiting = match caller(&mut env)? {
        None => None,
        Some((session, role)) => {
            if role != Role::Manager {
                return Err(not_a_manager());
            }
            match approval::live(&env.home, session.as_str(), OffsetDateTime::now_utc())? {
                Some(path) => Some(path),
                None => return Err(approval_required()),
            }
        }
    };
    let dir = checkout()?;
    let ts = stamp(OffsetDateTime::now_utc());
    let old = git_ok(&dir, &["rev-parse", "HEAD"])?.trim().to_owned();
    let patch = git_ok(&dir, &["diff", "HEAD"])?;
    let patch_path = if patch.is_empty() {
        None
    } else {
        Some(save_patch(&env.home, &ts, &patch)?)
    };
    let reference = format!("refs/cpd-host/{ts}");
    git_ok(&dir, &["update-ref", &reference, &old])?;
    git_ok(&dir, &["checkout", "--force", "-B", "main", "origin/main"])?;
    let head = git_ok(&dir, &["rev-parse", "HEAD"])?.trim().to_owned();
    if let Some(path) = &waiting {
        approval::consume(path)?;
    }
    Ok(Output::success(json!({
        "saved": {
            "ref": reference,
            "patch": patch_path.map(|p| p.display().to_string()),
        },
        "head": head,
    })))
}
