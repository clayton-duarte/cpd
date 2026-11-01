//! `cpd tui action host` (commands.md §9.12, hs-d618): a click on the red version pill reads the
//! install state file, finds the machine's host Manager, brings it into the chat slot and wakes it
//! with the install incident once per incident.
//!
//! The Manager is the live (not archived) `manager` Session of Context `host` (contract §3.7 E6),
//! created by dotfiles' `scripts/cpd-install.sh` (assumption A4); none: the click reports on the
//! client and exits 0. The message is one `session send` as the User (`--unblocks "cpd install
//! skipped"`); `$CPD_HOME/run/cpd-install-notified` holds the `since` of the incident already
//! announced, so a later click on the same incident only opens the Manager (commands.md §9.12).

use std::ffi::OsString;
use std::path::Path;

use rusqlite::OptionalExtension;

use super::RunCommand;

use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::host::install_state::InstallState;
use cpd_core::ids::SessionId;
use cpd_core::session::PROGRAM;
use cpd_core::store::Store;

/// The Context of the machine's host Manager (contract §3.7 E6).
const CONTEXT: &str = "host";
/// The report of a click with no host Manager.
const NO_MANAGER: &str = "φ: no host Manager: apply dotfiles";
/// What the incident message unblocks (`session send --unblocks`).
const UNBLOCKS: &str = "cpd install skipped";
/// The checkout the incident message names, literally.
const CHECKOUT: &str = "~/projects/cpd";

/// `cpd tui action host --client <client>` (commands.md §9.12): `run_command` runs `cpd`'s commands in
/// this process, as [`crate::cockpit::action`]'s other callers do.
pub fn run(
    home: &CpdHome,
    config: &Config,
    tmux: &Tmux,
    client: &str,
    run_command: RunCommand,
) -> Result<(), CpdError> {
    let Some(state) = InstallState::read(home)? else {
        return Ok(());
    };
    let Some(reason) = state.alert() else {
        return Ok(());
    };
    let Some(manager) = host_manager(home, config)? else {
        // The click prints nothing; the failure is a `display-message` on the client, as the other actions do.
        let _ = tmux.run(&[
            "display-message".to_owned(),
            "-l".to_owned(),
            "-c".to_owned(),
            client.to_owned(),
            NO_MANAGER.to_owned(),
        ]);
        return Ok(());
    };
    // Bring the Manager into the chat slot as `session open` does (a stopped or asleep one is resumed).
    super::widgets::open_lead(
        run_command,
        tmux,
        config.tmux.cockpit_session.as_str(),
        &manager,
    )?;
    let marker = home.run_dir().join("cpd-install-notified");
    if std::fs::read_to_string(&marker)
        .is_ok_and(|held| held.trim_end_matches('\n') == state.since())
    {
        return Ok(());
    }
    send(run_command, &manager, reason, state.since())?;
    write_atomic(&marker, &format!("{}\n", state.since()))
}

/// The live (not archived) Manager of Context `host`, oldest first: the one the install script made.
fn host_manager(home: &CpdHome, config: &Config) -> Result<Option<SessionId>, CpdError> {
    let mut store = Store::open(home, u64::from(config.timeouts.store_busy_ms))?;
    let id: Option<String> = store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT id FROM sessions WHERE context = ?1 AND role = 'manager' AND archived_at IS NULL ORDER BY created_at, id LIMIT 1",
                [CONTEXT],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    id.map(|id| SessionId::parse(&id)).transpose()
}

/// One `session send` as the User (`--unblocks "cpd install skipped"`), the code `cpd session send` uses.
fn send(
    run_command: RunCommand,
    manager: &SessionId,
    reason: &str,
    since: &str,
) -> Result<(), CpdError> {
    let text = format!(
        "cpd install skipped: {reason} (since {since}). Checkout: {CHECKOUT}. Read it with git -C {CHECKOUT} status / diff / log origin/main..HEAD (read only; never write in the checkout). Show the User what a repair would discard, ask with the tool cpd_host_approve, then run cpd host repair."
    );
    let argv = [
        PROGRAM,
        "session",
        "send",
        manager.as_str(),
        "--unblocks",
        UNBLOCKS,
        "--",
        text.as_str(),
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    run_command(argv).map(drop)
}

/// Writes `text` to `path` atomically: a temp file beside it, then `rename` (commands.md §9.12).
fn write_atomic(path: &Path, text: &str) -> Result<(), CpdError> {
    let failed = |e: std::io::Error| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("Cannot write {}: {e}.", path.display()),
        )
        .with("path", path.display().to_string())
        .with("reason", e.to_string())
    };
    let dir = path
        .parent()
        .expect("a file under $CPD_HOME has a directory");
    std::fs::create_dir_all(dir).map_err(failed)?;
    let tmp = path.with_extension(format!("notified-{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(failed)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(failed(e));
    }
    Ok(())
}
