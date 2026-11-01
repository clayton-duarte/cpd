//! The restart of the tmux server onto the installed tmux (contract §3.7 E3's hold, §6.1, §6.4;
//! commands.md §13.3; User decisions hs-d259 and hs-d294).
//!
//! A server keeps the tmux it was started with. When it is older than the installed tmux:
//!
//! - **safe now** (no Attempt `starting`, `running` or `merging`, no Lead or Manager `working` or
//!   `blocked`): bare `cpd` restarts it at once ([`restart`]);
//! - **not safe**: a restart is marked **pending** ([`mark`]: `run/tmux-restart.pending`, no store
//!   change). While it is pending E3 starts no Attempt ([`holding`]); once nothing runs ([`busy`]
//!   is empty) the engine starts the restart detached ([`drain`], `cpd watch --restart-tmux`,
//!   [`detached`]), because the restart kills the server the engine runs in.
//!
//! The restart records every Lead and Manager with a pane (live, or recorded by its row) and its place,
//! kills the server with the server's own binary, writes each recorded Session `stopped` with no pane
//! (as engine E2 does for a pane that is gone), runs `cpd tui` (the cockpit on the installed tmux, and
//! the engine again), reopens each Session in its place, and clears the pending file. Once the server
//! is killed nothing stops it: a failing step is kept and the next one runs. A failure keeps the file
//! with its reason and the engine raises one `error` Event for it; the hold ends with it.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use super::SOURCE;
use super::engine::Engine;
use super::lock;
use super::report::{About, Step};
use super::{detached_launch, record_detached, sweep_detached};
use cpd_core::clock::timestamp;
use cpd_core::config::Config;
use cpd_core::env::Env;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, NewEvent};
use cpd_core::home::CpdHome;
use cpd_core::host::deadline::Failed as RunFailed;
use cpd_core::host::runner::{Call, NO_DEADLINE, Runner};
use cpd_core::host::tmux::{installed_version, older_version};
use cpd_core::host::{Tmux, exe};
use cpd_core::ports::Ports;
use cpd_core::session;
use cpd_core::store::Store;

/// How long the old engine has to let go of its lock after the server was killed.
const ENGINE_EXIT_LIMIT: Duration = Duration::from_secs(20);

/// The engine does not start the detached restart again within this long.
const RESTART_START_PAUSE: Duration = Duration::from_secs(60);

/// `run/tmux-restart.pending`: a restart is pending since `at`, because the server runs tmux
/// `server` and the installed tmux is `installed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub at: String,
    pub server: String,
    pub installed: String,
    /// Why the last restart failed; none while the restart has not run (or is running).
    #[serde(default)]
    pub failed: Option<Failed>,
}

/// A restart that failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failed {
    pub at: String,
    pub code: String,
    pub reason: String,
    /// The engine raised its `error` Event for it.
    #[serde(default)]
    pub reported: bool,
}

impl Pending {
    /// The one line `doctor` and bare `cpd` say about it.
    pub fn describe(&self, zone: &cpd_core::clock::TimeZone) -> String {
        let local = |at: &str| match OffsetDateTime::parse(
            at,
            &time::format_description::well_known::Rfc3339,
        ) {
            Ok(t) => cpd_core::clock::prose(t, zone),
            Err(_) => at.to_owned(),
        };
        match &self.failed {
            None => format!(
                "A tmux restart is pending since {}: the server runs tmux {}, the installed tmux is {}. No Attempt starts until it has run.",
                local(&self.at),
                self.server,
                self.installed
            ),
            Some(f) => format!(
                "The tmux restart failed at {}: {} See run/tmux-restart.log.",
                local(&f.at),
                f.reason
            ),
        }
    }
}

fn now() -> String {
    timestamp(OffsetDateTime::now_utc())
}

fn file_failed(path: &Path, what: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("Cannot use {}: {what}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// The pending restart, if one is marked.
pub fn read(home: &CpdHome) -> Result<Option<Pending>, CpdError> {
    let path = home.tmux_restart_pending();
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| file_failed(&path, format!("it is not a pending restart: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(file_failed(&path, e)),
    }
}

fn write(home: &CpdHome, pending: &Pending) -> Result<(), CpdError> {
    let path = home.tmux_restart_pending();
    let tmp = path.with_extension("pending.tmp");
    let text = serde_json::to_string(pending).expect("a pending restart serializes");
    fs::create_dir_all(home.run_dir()).map_err(|e| file_failed(&path, e))?;
    fs::write(&tmp, text + "\n").map_err(|e| file_failed(&tmp, e))?;
    fs::rename(&tmp, &path).map_err(|e| file_failed(&path, e))
}

/// Removes the pending file; no error when there is none.
pub fn clear(home: &CpdHome) -> Result<(), CpdError> {
    let path = home.tmux_restart_pending();
    match fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(file_failed(&path, e)),
        _ => Ok(()),
    }
}

/// Marks a restart pending (the server runs `server`, the installed tmux is `installed`). Idempotent:
/// a restart already pending keeps its time. A failed restart is marked again, to be tried again.
/// Returns whether it newly marked.
pub fn mark(home: &CpdHome, server: &str, installed: &str) -> Result<bool, CpdError> {
    if read(home)?.is_some_and(|p| p.failed.is_none()) {
        return Ok(false);
    }
    write(
        home,
        &Pending {
            at: now(),
            server: server.to_owned(),
            installed: installed.to_owned(),
            failed: None,
        },
    )?;
    Ok(true)
}

/// Keeps the pending file with the reason the restart failed (a new file when there is none).
fn record_failure(home: &CpdHome, error: &CpdError) -> Result<(), CpdError> {
    let mut pending = read(home)?.unwrap_or_else(|| Pending {
        at: now(),
        server: String::new(),
        installed: String::new(),
        failed: None,
    });
    pending.failed = Some(Failed {
        at: now(),
        code: error.code().as_str().to_owned(),
        reason: error.message().to_owned(),
        reported: false,
    });
    write(home, &pending)
}

/// Whether E3 holds: a restart is pending and has not failed.
pub fn holding(home: &CpdHome) -> Result<bool, CpdError> {
    Ok(read(home)?.is_some_and(|p| p.failed.is_none()))
}

/// What keeps the restart from being safe now, one line each: an Attempt `starting`, `running` or
/// `merging` (its Worker lives in the server), a Lead or Manager `working` (mid-turn) or `blocked`
/// (a dialog is open). Empty: safe.
pub fn busy(store: &mut Store) -> Result<Vec<String>, CpdError> {
    store.read(|conn| {
        let mut busy = Vec::new();
        let mut attempts = conn.prepare("SELECT plan, id, status FROM attempts WHERE status IN ('starting', 'running', 'merging') ORDER BY plan, id")?;
        for row in attempts.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (plan, id, status) = row?;
            busy.push(format!("Attempt {id} of Plan {plan} is {status}"));
        }
        let mut sessions = conn.prepare(
            "SELECT role, name, state FROM sessions WHERE role IN ('lead', 'manager') AND state IN ('working', 'blocked') AND archived_at IS NULL AND runner = 'pi' ORDER BY id",
        )?;
        for row in sessions.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
            let (role, name, state) = row?;
            busy.push(format!("the {role} {name} is {state}"));
        }
        Ok(busy)
    })
}

/// What [`restart`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// There is no server, or it does not run an older tmux than the installed one: nothing to do.
    NotNeeded,
    /// Another run of the restart holds `locks/tmux-restart.lock`.
    Running,
    /// Not safe now: nothing was touched; each line says what is in the way.
    Busy(Vec<String>),
    /// The server was restarted and `reopened` Sessions are back in their places.
    Restarted { reopened: usize },
}

/// The lock of the one run of the restart: `flock` (through `cpd_core::lock`), so a run that dies
/// frees it. The open file holds the lock while it lives.
fn take_lock(home: &CpdHome) -> Result<Option<File>, CpdError> {
    let path = home.tmux_restart_lock();
    fs::create_dir_all(home.locks_dir()).map_err(|e| file_failed(&path, e))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| file_failed(&path, e))?;
    match cpd_core::lock::try_lock(&file) {
        Ok(true) => Ok(Some(file)),
        Ok(false) => Ok(None),
        Err(e) => Err(file_failed(&path, format!("flock failed: {e}"))),
    }
}

/// Restarts the tmux server onto the installed tmux when it is older and it is safe, as the
/// module says, and clears the pending file when it is done. A failure is recorded in the pending
/// file (and returned). `log` gets a line for each step. Never call it from a pane of the server.
pub fn restart(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    log: &mut dyn FnMut(&str),
) -> Result<Outcome, CpdError> {
    let Some(_lock) = take_lock(home)? else {
        return Ok(Outcome::Running);
    };
    match perform(home, config, store, log) {
        Ok(Outcome::Busy(why)) => Ok(Outcome::Busy(why)),
        Ok(done) => {
            clear(home)?;
            Ok(done)
        }
        Err(e) => {
            log(&format!("failed: {} ({})", e.message(), e.code().as_str()));
            record_failure(home, &e)?;
            Err(e)
        }
    }
}

fn perform(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    log: &mut dyn FnMut(&str),
) -> Result<Outcome, CpdError> {
    let tmux = Tmux::new(config.tmux.socket.clone());
    if !tmux.server_running()? {
        return Ok(Outcome::NotNeeded);
    }
    let (server, installed) = (tmux.server_version()?, installed_version()?);
    if !older_version(&server, &installed) {
        return Ok(Outcome::NotNeeded);
    }
    let busy = busy(store)?;
    if !busy.is_empty() {
        return Ok(Outcome::Busy(busy));
    }
    // Everything said to the old server is said by its own binary (`/proc/<pid>/exe`): the installed
    // tmux may be another version, and a dying server answers it with errors (hs-n285).
    let old = tmux.on_server_binary()?;
    let cockpit = config.tmux.cockpit_session.as_str();
    // The list of what to restore is taken before the kill, from the store as well as from the panes.
    let placed = session::placed(store, &old, cockpit)?;
    log(&format!(
        "restarting the tmux server -L {} (tmux {server} -> {installed}); Sessions to reopen: {}",
        tmux.socket(),
        placed.len()
    ));
    old.kill_server()?;
    log("the server is gone");

    // From here on nothing stops the restart: each step that fails is kept, and the next one runs, so
    // the cockpit is rebuilt and every Session is reopened whatever went wrong before.
    let mut problems: Vec<String> = Vec::new();
    let mut note = |what: String| problems.push(what);
    for (id, e) in session::gone(store, &placed, "tmux restart") {
        note(format!(
            "the rows of Session {id} were not written ({})",
            e.message()
        ));
    }
    if let Err(e) = wait_engine_gone(home) {
        note(e.message().to_owned());
    }
    match rebuild_cockpit(home) {
        Ok(()) => log("the cockpit is built on the installed tmux"),
        Err(e) => note(e.message().to_owned()),
    }

    let failed = session::reopen(&placed);
    if !failed.is_empty() {
        let list: Vec<String> = failed
            .iter()
            .map(|(id, e)| format!("{id} ({})", e.message()))
            .collect();
        note(format!(
            "these Sessions were not reopened: {}. Open each with cpd session open",
            list.join("; ")
        ));
    }
    if !problems.is_empty() {
        return Err(CpdError::new(
            ErrorCode::TmuxFailed,
            format!(
                "The server was restarted but not all of it came back: {}.",
                problems.join("; ")
            ),
        ));
    }
    log(&format!("reopened {} Session(s); done", placed.len()));
    Ok(Outcome::Restarted {
        reopened: placed.len(),
    })
}

/// The cockpit on the installed tmux, and the engine again (`cpd tui` runs `watch --ensure`).
fn rebuild_cockpit(home: &CpdHome) -> Result<(), CpdError> {
    rebuild_cockpit_with(&*Ports::current().runner, exe::started_as()?, home)
}

/// [`rebuild_cockpit`] on `runner`, with `exe` the recorded path of cpd's own executable.
pub(super) fn rebuild_cockpit_with(
    runner: &dyn Runner,
    exe: &Path,
    home: &CpdHome,
) -> Result<(), CpdError> {
    let root = home.root().to_owned();
    let call = Call {
        tool: "cpd".to_owned(),
        program: exe.to_string_lossy().into_owned(),
        args: vec!["tui".to_owned()],
        envs: [("CPD_HOME", Some(root.to_string_lossy().into_owned()))]
            .into_iter()
            .chain(
                [
                    "CPD_PLAN",
                    "CPD_SESSION",
                    "CPD_ATTEMPT",
                    "TMUX",
                    "TMUX_PANE",
                ]
                .map(|n| (n, None)),
            )
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        cwd: Some(root),
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    let out = runner.run(&call).map_err(|e| {
        let why = match e {
            RunFailed::Io(e) => e.to_string(),
            RunFailed::Timeout(e) => e.message().to_owned(),
        };
        CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot run cpd tui: {why}."),
        )
    })?;
    if !out.status.success() {
        let tail = String::from_utf8_lossy(&out.stdout).trim().to_owned()
            + &String::from_utf8_lossy(&out.stderr);
        return Err(CpdError::new(
            ErrorCode::TmuxFailed,
            format!(
                "cpd tui failed after the server was killed: {}",
                tail.trim()
            ),
        ));
    }
    Ok(())
}

/// Waits until no engine holds its lock: the killed server's panes got `SIGHUP`, and `cpd tui`
/// starts the next engine with `watch --ensure`, which does nothing while the old one still holds it.
fn wait_engine_gone(home: &CpdHome) -> Result<(), CpdError> {
    let deadline = Instant::now() + ENGINE_EXIT_LIMIT;
    while lock::held(home)? {
        if Instant::now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::WatchRunning,
                "The engine still runs after the tmux server was killed.",
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// `cpd watch --restart-tmux` (internal): the restart, started detached by the engine; its output
/// is `run/tmux-restart.log`. Exits 0 when it did nothing (not needed, not safe, another run).
pub(super) fn detached() -> Result<cpd_core::output::Output, CpdError> {
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let mut log = |line: &str| {
        let _ = writeln!(std::io::stderr(), "{} cpd tmux restart: {line}", now());
    };
    let outcome = restart(&home, &config, &mut store, &mut log)?;
    match &outcome {
        Outcome::Busy(why) => log(&format!("not safe now, not restarted: {}", why.join("; "))),
        Outcome::Running => log("another restart runs"),
        Outcome::NotNeeded => log("the server needs no restart"),
        Outcome::Restarted { .. } => {}
    }
    Ok(cpd_core::output::Output::success(
        json!({"outcome": format!("{outcome:?}")}),
    ))
}

/// The engine's step before E1: reports a failed restart once, and starts the detached restart once
/// nothing in the way is left (E3 holds meanwhile, [`holding`]).
pub(super) fn drain(engine: &mut Engine) {
    sweep_detached(engine);
    let pending = match read(&engine.home) {
        Ok(Some(pending)) => pending,
        Ok(None) => return,
        Err(e) => {
            return engine.log.line(&format!(
                "tmux restart: {} ({})",
                e.message(),
                e.code().as_str()
            ));
        }
    };
    if let Some(failed) = &pending.failed {
        if !failed.reported {
            report(engine, &pending, failed);
        }
        return;
    }
    match busy(&mut engine.store) {
        Ok(busy) if busy.is_empty() => {}
        Ok(_) => return,
        Err(e) => return engine.fail(Step::E3, About::Machine, &e),
    }
    let now = engine.ports.clock.now();
    if engine
        .restart_started
        .is_some_and(|at| now.saturating_duration_since(at) < RESTART_START_PAUSE)
    {
        return;
    }
    match spawn(engine) {
        Ok(pid) => {
            engine.restart_started = Some(now);
            engine.log.line(&format!("tmux restart: nothing in the way; started the restart detached (pid {pid}, log run/tmux-restart.log)"));
        }
        Err(e) => engine.fail(Step::E3, About::Machine, &e),
    }
}

/// One `error` Event for a failed restart, then the file says it was raised.
fn report(engine: &mut Engine, pending: &Pending, failed: &Failed) {
    let text = format!("The tmux restart failed: {} See: cpd doctor", failed.reason);
    let details = json!({"at": failed.at, "server": pending.server, "installed": pending.installed, "log": "run/tmux-restart.log"});
    let event = NewEvent::error(text, SOURCE, &failed.code, &failed.reason, details);
    let appended = engine.store.write(SOURCE, |tx| events::append(tx, event));
    match appended {
        Ok(_) => {
            let mut done = pending.clone();
            if let Some(f) = done.failed.as_mut() {
                f.reported = true;
            }
            match write(&engine.home, &done) {
                Ok(()) => engine
                    .log
                    .line(&format!("tmux restart: failed: {}", failed.reason)),
                Err(e) => engine.log.line(&format!(
                    "tmux restart: cannot update the pending file: {}",
                    e.message()
                )),
            }
        }
        Err(e) => engine.log.line(&format!(
            "tmux restart: cannot append the error Event: {} ({})",
            e.message(),
            e.code().as_str()
        )),
    }
}

/// Starts `cpd watch --restart-tmux` through `engine.ports.spawn`, detached (`setsid`, stdin `/dev/null`, stdout and
/// stderr appended to `run/tmux-restart.log`), as E3 starts a driver, its pid recorded in
/// `run/tmux-restart.pid` ([`record_detached`]). Returns its pid.
fn spawn(engine: &mut Engine) -> Result<u32, CpdError> {
    let log_path = engine.home.tmux_restart_log();
    let failed = |why: String| {
        CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot start the tmux restart: {why}."),
        )
    };
    let exe = engine.ports.spawn.self_exe()?;
    let run_dir = engine.home.run_dir();
    fs::create_dir_all(&run_dir)
        .map_err(|e| failed(format!("cannot create {}: {e}", run_dir.display())))?;
    let launch = detached_launch(&exe, &["watch", "--restart-tmux"], &engine.home, log_path);
    let pid = engine
        .ports
        .spawn
        .spawn(&launch)
        .map_err(|e| failed(format!("cannot run {}: {e}", exe.display())))?;
    let pid_path = engine.home.tmux_restart_pid();
    record_detached(engine, pid, pid_path.clone()).map_err(|e| {
        failed(format!(
            "it runs, but its pid cannot be written to {}: {e}",
            pid_path.display()
        ))
    })
}

#[cfg(test)]
#[path = "restart_runner_tests.rs"]
mod restart_runner_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, CpdHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        (dir, home)
    }

    #[test]
    fn marking_is_idempotent_and_a_failed_restart_is_marked_again() {
        let (_dir, home) = home();
        assert_eq!(read(&home).unwrap(), None);
        assert!(!holding(&home).unwrap());
        assert!(mark(&home, "3.4", "3.8").unwrap());
        let first = read(&home).unwrap().unwrap();
        assert_eq!(
            (
                first.server.as_str(),
                first.installed.as_str(),
                first.failed.is_none()
            ),
            ("3.4", "3.8", true)
        );
        assert!(holding(&home).unwrap());
        assert!(!mark(&home, "3.4", "3.8").unwrap());
        assert_eq!(read(&home).unwrap().unwrap(), first);

        record_failure(&home, &CpdError::new(ErrorCode::TmuxFailed, "it broke.")).unwrap();
        let failed = read(&home).unwrap().unwrap();
        assert_eq!(
            failed
                .failed
                .as_ref()
                .map(|f| (f.reason.as_str(), f.reported)),
            Some(("it broke.", false))
        );
        assert!(!holding(&home).unwrap(), "a failed restart holds nothing");
        assert!(mark(&home, "3.4", "3.8").unwrap());
        assert!(read(&home).unwrap().unwrap().failed.is_none());
        clear(&home).unwrap();
        clear(&home).unwrap();
        assert_eq!(read(&home).unwrap(), None);
    }

    #[test]
    fn a_file_that_is_not_a_pending_restart_is_an_error() {
        let (_dir, home) = home();
        fs::create_dir_all(home.run_dir()).unwrap();
        fs::write(home.tmux_restart_pending(), "nonsense").unwrap();
        assert_eq!(read(&home).unwrap_err().code(), ErrorCode::StoreFailed);
    }
}
