//! `session run` (commands.md §10.8, contract §6.7): the program in every
//! Lead pane. It owns its terminal: it runs pi in the foreground, and when pi
//! exits it offers to resume it.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use super::archive::archive_on_quit;
use super::fork_wait::wait_for_source;
use super::moves::first_message;
use super::port;
use super::reload::{PI_STOP, adapter_missing, may_exec, reload, reload_due, stop_pi};
use super::row::{self, Role, Row, State};
use super::{Env, RunArgs, not_a_lead, state};
use crate::config::{Letter, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::host::exe::{Exe, Watched};
use crate::host::pi::{self, LeadLaunch, LeadStart, PiDirs, Plannotator};
use crate::host::{PaneId, Tmux};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;

// Shared with the review Worker's fork wait (`driver::worker`), which reads them here.
pub(crate) use super::fork_wait::{quiet_since, turn_finished};

/// How often it looks for pi's session file while pi runs.
const FILE_POLL: Duration = Duration::from_millis(500);
/// How often the adapter watchdog reads the Session's state while pi runs (commands.md §10.8).
const WATCHDOG_POLL: Duration = Duration::from_secs(2);
/// How long it waits for `new-window`'s caller to set `@cpd_session` on its pane.
const PANE_WAIT: Duration = Duration::from_secs(5);

pub(super) fn run(args: RunArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let row = env.store.read(|conn| row::load(conn, &args.id))?;
    if row.role == Role::Worker {
        return Err(not_a_lead(&row.id, row.role));
    }
    check_pane(&env.tmux(), &row.id)?;
    let exe = Exe::current()?;
    let dirs = PiDirs::from_env()?;
    // From here on this process owns the terminal (commands.md §1.2).
    'run: loop {
        let row = env.store.read(|conn| row::load(conn, &args.id))?;
        // A Lead with a Plan gets the Plan's port; a Context's Manager gets none (§6.5).
        let port = match &row.plan {
            Some(plan) => match assign_port(&mut env, plan)? {
                Some(port) => Some(port),
                None => return Ok(Output::Owned),
            },
            None => None,
        };
        let Some(start) = start(&mut env, &dirs, &row)? else { return Ok(Output::Owned) };
        let candidates = pi::lead_candidates(&env.config.pi_adapter_dir()?, &env.config.lead_pi_packages);
        let extensions = pi::extensions_to_pass(&candidates, Path::new(&row.cwd), &dirs, &mut std::io::stderr());
        // `PLANNOTATOR_REMOTE` comes from cpd's environment; the port is the Plan's, whatever the environment holds.
        let plannotator = Plannotator { port: port.map(|p| p.to_string()), remote: Plannotator::from_env().remote };
        let launch = pi::lead(&LeadLaunch {
            home: &env.home,
            session: &row.id,
            plan: row.plan.as_ref(),
            extensions: &extensions,
            plannotator: &plannotator,
            start,
        });
        // Recorded right before pi starts: a change after it is a pi the Lead has not run yet.
        let pi_install = dirs.watch_install();
        let mut pi = Command::new(&launch.argv[0]);
        pi.args(&launch.argv[1..]);
        // A Context's Manager has no plannotator variable but the one cpd sets (§6.5), whatever its caller holds.
        if row.plan.is_none() {
            for (key, _) in std::env::vars_os().filter(|(k, _)| k.to_string_lossy().starts_with("PLANNOTATOR_")) {
                pi.env_remove(key);
            }
        }
        let mut child = pi
            .envs(&launch.env)
            .current_dir(&row.cwd)
            .spawn()
            .map_err(|e| {
                CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start pi: {e}.")).with("argv", launch.argv.clone())
            })?;
        let mut recorded = row.file.is_some();
        let started = Instant::now();
        let mut watched = started;
        let status = loop {
            let exited = child.try_wait().map_err(|e| {
                CpdError::new(ErrorCode::SpawnFailed, format!("Cannot wait for pi: {e}.")).with("argv", launch.argv.clone())
            })?;
            if let Some(status) = exited {
                // A new install: restart before any store write (§6.7 "Reload on install").
                if exe.replaced() && may_exec(&exe) {
                    return Err(reload(&exe, &row.id));
                }
                if !recorded {
                    record_file(&mut env, &dirs, &row.id)?;
                }
                break status;
            }
            if !recorded {
                recorded = record_file(&mut env, &dirs, &row.id)?;
            }
            let pi_changed = pi_install.as_ref().is_some_and(Watched::changed);
            // `may_exec` asks the new cpd, so it comes after the cheaper tests and before pi is signalled.
            if (exe.replaced() || pi_changed) && reload_due(&mut env, &dirs, &row.id)? && may_exec(&exe) {
                let what = if exe.replaced() { "cpd" } else { "pi" };
                print!("  new {what} installed: restarting pi for {}…\n", row.name);
                let _ = std::io::stdout().flush();
                // The adapter reports `stopped` at `session_shutdown`; this process does not.
                stop_pi(&mut child, PI_STOP);
                return Err(reload(&exe, &row.id));
            }
            if watched.elapsed() >= WATCHDOG_POLL {
                watched = Instant::now();
                if adapter_missing(&mut env, &row.id, started.elapsed())? {
                    print!("  adapter missing: restarting pi for {}…\n", row.name);
                    let _ = std::io::stdout().flush();
                    stop_pi(&mut child, PI_STOP);
                    continue 'run;
                }
            }
            std::thread::sleep(FILE_POLL);
        };
        // The adapter reports `stopped` at shutdown; report it here when it did not.
        if let Err(e) = state::set(&mut env.store, "session run", &row.id, State::Stopped, None) {
            eprintln!("cpd: cannot report Session {} stopped: {}: {}", row.id, e.code(), e.message());
        }
        // pi exits 0 on the User's `/quit` and Ctrl+D (§10.8); a reload never gets here. A Manager is archived only
        // with its Context (§3.8), so its quit is not archived.
        if status.success() && row.role == Role::Lead {
            match archive_on_quit(&mut env, &row.id) {
                Ok(()) => return Ok(Output::Owned),
                Err(e) => print!("\n  {}: {}\n", e.code(), e.message()),
            }
        }
        let name = env.store.read(|conn| row::load(conn, &args.id))?.name;
        print!("\n  pi exited. Press Enter to resume {name} here.\n");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => return Ok(Output::Owned),
            Ok(_) => {}
        }
        if exe.replaced() && may_exec(&exe) {
            return Err(reload(&exe, &row.id));
        }
    }
}

/// The Plan's plannotator port. When the range is full the pane says so, names the
/// Plans that hold it and waits for Enter to try again; `None` at the end of input.
fn assign_port(env: &mut Env, plan: &PlanId) -> Result<Option<u16>, CpdError> {
    loop {
        match port::assign(&env.home, &mut env.store, &env.config, plan) {
            Ok(port) => return Ok(Some(port)),
            Err(e) if e.code() == ErrorCode::NoPort => {
                print!("\n  {}: {}\n  Press Enter to retry.\n", e.code(), e.message());
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                match std::io::stdin().lock().read_line(&mut line) {
                    Ok(0) | Err(_) => return Ok(None),
                    Ok(_) => {}
                }
            }
            Err(e) => return Err(e),
        }
    }
}

/// `wrong_pane` unless this process runs in a pane of cpd's tmux server whose
/// `@cpd_session` is `id`. The command that created the pane sets the option
/// just after `new-window`, so an unset option is waited for, briefly.
fn check_pane(tmux: &Tmux, id: &SessionId) -> Result<(), CpdError> {
    let wrong = |why: String| {
        CpdError::new(ErrorCode::WrongPane, format!("cpd session run {id} must run in the pane cpd started for it: {why}."))
            .with("session", id.as_str())
    };
    // `TMUX` is `<socket path>,<pid>,<session>`: the pane must be on cpd's server.
    let server = std::env::var("TMUX").unwrap_or_default();
    let socket = server.split(',').next().and_then(|path| Path::new(path).file_name()).map(|n| n.to_string_lossy().into_owned());
    if socket.as_deref() != Some(tmux.socket()) {
        return Err(wrong(format!("this is not a pane of the tmux server {:?}", tmux.socket())));
    }
    let pane = std::env::var("TMUX_PANE")
        .ok()
        .and_then(|p| PaneId::parse(&p))
        .ok_or_else(|| wrong("TMUX_PANE is not set".into()))?;
    let deadline = SystemTime::now() + PANE_WAIT;
    loop {
        let panes = tmux.list_panes()?;
        let found = panes.into_iter().find(|p| p.pane == pane).ok_or_else(|| wrong(format!("pane {pane} is gone")))?;
        match &found.cpd_session {
            Some(s) if s == id => return Ok(()),
            Some(other) => {
                return Err(wrong(format!("pane {pane} runs Session {other}")).with("pane", pane.as_str()).with("found", other.as_str()));
            }
            None if SystemTime::now() >= deadline => {
                return Err(wrong(format!("pane {pane} has no @cpd_session")).with("pane", pane.as_str()));
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// The Session's pi file: the recorded one if it exists, else the one pi
/// created (contract §6.5).
pub(super) fn existing_file(dirs: &PiDirs, row: &Row) -> Option<PathBuf> {
    row.file.as_ref().map(PathBuf::from).filter(|p| p.is_file()).or_else(|| pi::session_file(dirs, &row.id))
}

/// Records `sessions.file` once pi has created it: a data write, no Event
/// (commands.md §10.8). Returns whether the file is recorded.
fn record_file(env: &mut Env, dirs: &PiDirs, id: &SessionId) -> Result<bool, CpdError> {
    let Some(file) = pi::session_file(dirs, id) else { return Ok(false) };
    let file = file.display().to_string();
    env.store.write("session run", |tx| {
        tx.execute("UPDATE sessions SET file = ?1 WHERE id = ?2 AND file IS NOT ?1", [file.as_str(), id.as_str()])?;
        Ok(())
    })?;
    Ok(true)
}

/// How pi starts this time (§6.7, commands.md §10.8): resume when the file
/// exists, else fork once the source is idle, else fresh. `None` at the end of
/// input while a fork waits for a source that can never become idle.
fn start(env: &mut Env, dirs: &PiDirs, row: &Row) -> Result<Option<LeadStart>, CpdError> {
    if let Some(file) = existing_file(dirs, row) {
        if row.file.as_deref() != Some(&file.display().to_string()) {
            record_file(env, dirs, &row.id)?;
        }
        return Ok(Some(LeadStart::Resume { file }));
    }
    let tier = Tier::parse(&row.tier).map_err(|_| CpdError::bad_row(format!("{} has tier {:?}", row.id, row.tier)))?;
    let models = &env.config.tiers;
    let model = match tier.letter() {
        Letter::O => &models.o,
        Letter::S => &models.s,
        Letter::H => &models.h,
    };
    let model = format!("{model}:{}", tier.level());
    let message = first_message(&env.home, row)?;
    match &row.forked_from {
        Some(source) => {
            let Some(source_file) = wait_for_source(env, dirs, source, &row.name)? else { return Ok(None) };
            Ok(Some(LeadStart::Fork { model, name: row.name.clone(), source_file, first_message: message }))
        }
        None => {
            let first_message = message.ok_or_else(|| {
                CpdError::new(ErrorCode::FileUnreadable, format!("Session {} has no first message to start with.", row.id))
                    .with("session", row.id.as_str())
                    .with("path", match (&row.plan, row.role) {
                        (Some(plan), _) => Some(env.home.lead_message(plan).display().to_string()),
                        (None, Role::Manager) => Some(env.home.manager_message(&row.context).display().to_string()),
                        (None, _) => None,
                    })
            })?;
            Ok(Some(LeadStart::Fresh { model, name: row.name.clone(), first_message }))
        }
    }
}
