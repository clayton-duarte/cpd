//! `session run` (commands.md §10.8, contract §6.7): the program in every
//! Lead pane. It owns its terminal: it runs pi in the foreground, and when pi
//! exits it offers to resume it.

use std::collections::HashSet;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::time::Duration;

use nix::errno::Errno;
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;

use super::archive::archive_on_quit;
use super::fork_wait::wait_for_source_with;
use super::moves::first_message;
use super::reload::{adapter_missing_with, may_exec, reload_due_with, reload_with, stop_pi_with};
use super::row::{self, Role, Row, State};
use super::{Env, RunArgs, not_a_lead, state};
use crate::clock::Clock;
use crate::error::{CpdError, ErrorCode};
use crate::host::exe::{Exe, Watched};
use crate::host::git_guard;
use crate::host::limit::limit;
use crate::host::pi::{self, LeadLaunch, LeadStart, PiDirs};
use crate::host::spawn::{Launch, Pid as SpawnPid, Signal as SpawnSignal, Spawn};
use crate::host::{PaneId, Tmux};
use crate::ids::SessionId;
use crate::output::Output;
use crate::ports::Ports;
use crate::store;

/// How often it looks for pi's session file while pi runs (tests: `CPD_TEST_FILE_POLL`, [`limit`]).
const FILE_POLL: Duration = Duration::from_millis(500);
/// How often the adapter watchdog reads the Session's state while pi runs (commands.md §10.8;
/// tests: `CPD_TEST_WATCHDOG_POLL`).
const WATCHDOG_POLL: Duration = Duration::from_secs(2);
/// How long it waits for `new-window`'s caller to set `@cpd_session` on its pane.
const PANE_WAIT: Duration = Duration::from_secs(5);
/// The first wait after a transient store error before pi runs; each next one doubles, up to [`RETRY_MAX`].
const RETRY_FIRST: Duration = Duration::from_millis(500);
/// The longest wait between two tries of a store access that met a transient error.
const RETRY_MAX: Duration = Duration::from_secs(5);

pub(super) fn run(args: RunArgs) -> Result<Output, CpdError> {
    let mut env = patiently("opening the store", Env::open)?;
    let ports = env.ports.clone();
    let clock = ports.clock.clone();
    let row = patiently_with(&*clock, "reading the Session", || {
        env.store.read(|conn| row::load(conn, &args.id))
    })?;
    if row.role == Role::Worker {
        return Err(not_a_lead(&row.id, row.role));
    }
    check_pane(&ports, &env.tmux(), &row.id)?;
    // The path `spawn` recorded at start (the real one: `current_exe` at process start, which an install does not
    // change under it).
    let exe = Exe::at(ports.spawn.self_exe()?)?;
    let dirs = PiDirs::from_env()?;
    // How long a reload waits for pi to exit after `SIGTERM` before it sends `SIGKILL`.
    let pi_stop = Duration::from_secs(u64::from(env.config.timeouts.pi_stop_seconds));
    let (file_poll, watchdog_poll) = (
        limit("FILE_POLL", FILE_POLL),
        limit("WATCHDOG_POLL", WATCHDOG_POLL),
    );
    // The pi a previous wrapper left running across its `exec` (§6.7 "Reload on install"): the first round adopts it.
    let mut adopted = args
        .adopt
        .map(|pid| (PiProc::adopt(Pid::from_raw(pid)), args.pi_stale));
    // Store errors the loop has logged, so each code is said once (a store error never ends a wrapper while pi runs).
    let mut logged = Logged::default();
    // From here on this process owns the terminal (commands.md §1.2).
    'run: loop {
        let row = patiently_with(&*clock, "reading the Session", || {
            env.store.read(|conn| row::load(conn, &args.id))
        })?;
        let marker = env.home.quit_marker(&row.id);
        let (mut pi, pi_install, mut pi_stale) = match adopted.take() {
            // Steps 1–3 are not run and no pi is started. What the loop needs of the old process is restored here:
            // `recorded` is read from the row below, as at any start; `pi_install` is read now, which would hide a
            // pi that changed before the `exec`, so the old wrapper tells whether it had (`--pi-stale`), and a
            // change after it is seen as usual; `started` below is now, so the adapter watchdog counts the 60 s of
            // a stopped adapter from now at the earliest (it never restarts a pi early).
            Some((pi, stale)) => (pi, dirs.watch_install(), stale),
            None => match launch_pi(&mut env, &dirs, &row)? {
                Some((pi, pi_install)) => (pi, pi_install, false),
                None => return Ok(Output::Owned),
            },
        };
        let mut recorded = row.file.is_some();
        let started = clock.now();
        let mut watched = started;
        let status = loop {
            let exited = pi.try_wait_with(&*ports.spawn).map_err(|e| {
                CpdError::new(ErrorCode::SpawnFailed, format!("Cannot wait for pi: {e}."))
                    .with("pid", pi.id())
            })?;
            if let Some(status) = exited {
                // A new install: restart before any store write (§6.7 "Reload on install").
                if exe.replaced() && may_exec(&exe) {
                    return Err(reload_with(&ports, &exe, &row.id, None));
                }
                if !recorded {
                    patiently_with(&*clock, "recording the session file", || {
                        record_file(&mut env, &dirs, &row.id)
                    })?;
                }
                break status;
            }
            pi_stale = pi_stale || pi_install.as_ref().is_some_and(Watched::changed);
            // A new cpd: re-exec at once, before any store access, and keep pi running (§6.7 "Reload on install").
            // `may_exec` asks the new cpd, so it comes after the cheaper test.
            if exe.replaced() && may_exec(&exe) {
                // Returns only when the `exec` failed; pi still runs, so the wrapper carries on.
                logged.once::<()>(
                    Err(reload_with(&ports, &exe, &row.id, Some((&pi, pi_stale)))),
                    "cannot reload",
                );
            }
            if !recorded {
                recorded = logged
                    .once(
                        record_file(&mut env, &dirs, &row.id),
                        "cannot record the session file",
                    )
                    .unwrap_or(false);
            }
            // A new pi restarts pi, at the Lead's idle. `may_exec` is true unless a cpd that cannot open the store
            // replaced this one, and then pi stays.
            if pi_stale
                && logged
                    .once(
                        reload_due_with(&ports, &mut env, &dirs, &row.id),
                        "cannot decide on restarting pi",
                    )
                    .unwrap_or(false)
                && may_exec(&exe)
            {
                println!("  new pi installed: restarting pi for {}…", row.name);
                let _ = std::io::stdout().flush();
                // The adapter reports `stopped` at `session_shutdown`; this process does not.
                stop_pi_with(&ports, &mut pi, pi_stop);
                return Err(reload_with(&ports, &exe, &row.id, None));
            }
            if clock.now().duration_since(watched) >= watchdog_poll {
                watched = clock.now();
                let missing = logged.once(
                    adapter_missing_with(
                        &ports,
                        &mut env,
                        &row.id,
                        clock.now().duration_since(started),
                    ),
                    "cannot read the adapter's state",
                );
                if missing.unwrap_or(false) {
                    println!("  adapter missing: restarting pi for {}…", row.name);
                    let _ = std::io::stdout().flush();
                    stop_pi_with(&ports, &mut pi, pi_stop);
                    continue 'run;
                }
            }
            clock.sleep(file_poll);
        };
        // The adapter reports `stopped` at shutdown; report it here when it did not.
        if let Err(e) = patiently_with(&*clock, "reporting the Session stopped", || {
            state::set(&mut env.store, "session run", &row.id, State::Stopped, None)
        }) {
            eprintln!(
                "cpd: cannot report Session {} stopped: {}: {}",
                row.id,
                e.code(),
                e.message()
            );
        }
        // The marker is consumed at every pi exit. Only the User's `/quit` leaves one (pro-d361), so a signal exit
        // never archives; a Manager is archived only with its Context (§3.8), and a nonzero exit never is.
        let quit = std::fs::remove_file(&marker).is_ok();
        if quit && status.success() && row.role == Role::Lead {
            match archive_on_quit(&mut env, &row.id) {
                Ok(()) => return Ok(Output::Owned),
                Err(e) => eprintln!("\n  {}: {}", e.code(), e.message()),
            }
        }
        // Every other exit leaves the Session `stopped`: the User resumes it here or closes the pane.
        let name = patiently_with(&*clock, "reading the Session", || {
            env.store.read(|conn| row::load(conn, &args.id))
        })?
        .name;
        print!("\n  pi exited. Press Enter to resume {name} here.\n");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match crate::host::input::read_line(&mut line) {
            Ok(0) | Err(_) => return Ok(Output::Owned),
            Ok(_) => {}
        }
        if exe.replaced() && may_exec(&exe) {
            return Err(reload_with(&ports, &exe, &row.id, None));
        }
    }
}

/// Steps 1–3 and the start of pi (§10.8): how pi starts, pi itself. Also the
/// identity of pi's install, recorded right before pi starts: a change after
/// it is a pi the Lead has not run yet. `None` when a fork's source can never
/// become idle and the User ended the input.
fn launch_pi(
    env: &mut Env,
    dirs: &PiDirs,
    row: &Row,
) -> Result<Option<(PiProc, Option<Watched>)>, CpdError> {
    // `None`: a fork whose source can never become idle reached the end of input (main #60, hs-d196).
    let Some(start) = start(env, dirs, row)? else {
        return Ok(None);
    };
    // pi now runs the row's Tier: an earlier failed switch is stale (commands.md §10.8).
    env.store.write("session run", |tx| {
        tx.execute(
            "UPDATE sessions SET switch_failed = NULL WHERE id = ?1 AND switch_failed IS NOT NULL",
            [row.id.as_str()],
        )?;
        Ok(())
    })?;
    let candidates =
        pi::lead_candidates(&env.config.pi_adapter_dir()?, &env.config.lead_pi_packages);
    let extensions = pi::extensions_to_pass(
        &candidates,
        Path::new(&row.cwd),
        dirs,
        &mut std::io::stderr(),
    );
    git_guard::install(&env.home)?;
    // A marker of an earlier pi is stale: only the User's `/quit` in this one may leave it (pro-d361).
    match std::fs::remove_file(env.home.quit_marker(&row.id)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(CpdError::new(
                ErrorCode::FileUnreadable,
                format!("Cannot remove the stale quit marker: {e}."),
            ));
        }
    }
    let launch = pi::lead(&LeadLaunch {
        home: &env.home,
        session: &row.id,
        plan: row.plan.as_ref(),
        extensions: &extensions,
        start,
    });
    let pi_install = dirs.watch_install();
    // pi owns the pane's terminal (`foreground`) and is this process's child, which the loop waits for by pid.
    let spawn = Launch {
        program: launch.argv[0].clone(),
        args: launch.argv[1..].to_vec(),
        env_set: launch
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        cwd: Some(PathBuf::from(&row.cwd)),
        foreground: true,
        ..Launch::default()
    };
    let pid = env.ports.spawn.spawn(&spawn).map_err(|e| {
        CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start pi: {e}."))
            .with("argv", launch.argv.clone())
    })?;
    Ok(Some((
        PiProc::adopt(Pid::from_raw(pid.0 as i32)),
        pi_install,
    )))
}

/// pi, a child of this process: started by it, or adopted from the wrapper
/// that `exec`ed into this process (a child survives its parent's `exec`, and
/// the pid stays this process's child). Offers what the loop needs of
/// `std::process::Child`, by pid.
#[derive(Debug)]
pub(super) struct PiProc {
    pid: Pid,
    /// How pi exited, once it was waited for: the pid is no longer pi's and is never signalled.
    status: Option<ExitStatus>,
}

impl PiProc {
    /// The child `pid` of this process. Not checked here: a pid that is not a
    /// child fails at the first [`PiProc::try_wait`].
    pub(super) fn adopt(pid: Pid) -> PiProc {
        PiProc { pid, status: None }
    }

    /// [`PiProc::try_wait`] asking `spawn` (`Spawn::try_wait`): how pi exited, or `None` while it runs; a pid `spawn`
    /// does not know is an error.
    pub(super) fn try_wait_with(
        &mut self,
        spawn: &dyn Spawn,
    ) -> std::io::Result<Option<ExitStatus>> {
        if self.status.is_some() {
            return Ok(self.status);
        }
        // Real pi is started here with `Command` (it owns the terminal) or adopted across an `exec`: neither is a child
        // `ProcessSpawn` started, so `waitpid` waits for it, as it always did.
        if spawn.kind() == "ProcessSpawn" {
            return self.try_wait();
        }
        let status = spawn.try_wait(SpawnPid(self.pid.as_raw().unsigned_abs()))?;
        self.status = status;
        Ok(status)
    }

    /// [`PiProc::signal`] through `spawn` (`Spawn::signal`), and only while pi was not waited for.
    pub(super) fn signal_with(&self, spawn: &dyn Spawn, signal: SpawnSignal) {
        if self.status.is_none() {
            let _ = spawn.signal(SpawnPid(self.pid.as_raw().unsigned_abs()), signal);
        }
    }

    pub(super) fn id(&self) -> i32 {
        self.pid.as_raw()
    }

    /// `Child::try_wait`: how pi exited, or `None` while it runs.
    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        if self.status.is_some() {
            return Ok(self.status);
        }
        let status = match waitpid(self.pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(_, code)) => ExitStatus::from_raw(code << 8),
            Ok(WaitStatus::Signaled(_, signal, _)) => ExitStatus::from_raw(signal as i32),
            // Still running (stops are not asked for).
            Ok(_) | Err(Errno::EINTR) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        self.status = Some(status);
        Ok(self.status)
    }
}

/// Store errors already said once on the pane, by code. A store error never
/// ends a wrapper while its pi runs (contract §6.7 "Reload on install"): the
/// loop says it once and tries again at the next poll.
#[derive(Default)]
struct Logged(HashSet<ErrorCode>);

impl Logged {
    /// The value of `result`, or `None` after saying its error on stderr (once per code).
    fn once<T>(&mut self, result: Result<T, CpdError>, what: &str) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(e) => {
                if self.0.insert(e.code()) {
                    eprintln!(
                        "cpd: {what}: {}: {} Retrying at the next poll; pi is untouched.",
                        e.code(),
                        e.message()
                    );
                }
                None
            }
        }
    }
}

/// [`patiently`] whose waits (0.5 s, doubling to 5 s) are naps on `clock`.
pub(super) fn patiently_with<T>(
    clock: &dyn Clock,
    what: &str,
    mut access: impl FnMut() -> Result<T, CpdError>,
) -> Result<T, CpdError> {
    let mut wait = RETRY_FIRST;
    let mut said = false;
    loop {
        match access() {
            Err(e) if store::is_transient(&e) || e.code() == ErrorCode::LockTimeout => {
                if !said {
                    print!(
                        "\n  Waiting for the store ({what}): {}: {} Trying again.\n",
                        e.code(),
                        e.message()
                    );
                    let _ = std::io::stdout().flush();
                    said = true;
                }
                clock.sleep(wait);
                wait = (wait * 2).min(RETRY_MAX);
            }
            other => return other,
        }
    }
}

/// Runs a store access of the wrapper's start or end (before pi runs, after it
/// exited), again and again while it meets a transient error ([`store::is_transient`], and `lock_timeout`,
/// the wait for the store's setup lock in `Env::open`): waits 0.5 s,
/// doubling to 5 s. The pane says once what it waits for and why; the wrapper
/// never ends for it. Any other result, error or not, is returned as it is.
pub(super) fn patiently<T>(
    what: &str,
    access: impl FnMut() -> Result<T, CpdError>,
) -> Result<T, CpdError> {
    patiently_with(&*Ports::current().clock, what, access)
}

/// `wrong_pane` unless this process runs in a pane of cpd's tmux server whose
/// `@cpd_session` is `id`. The command that created the pane sets the option
/// just after `new-window`, so an unset option is waited for, briefly.
fn check_pane(ports: &Ports, tmux: &Tmux, id: &SessionId) -> Result<(), CpdError> {
    let wrong = |why: String| {
        CpdError::new(
            ErrorCode::WrongPane,
            format!("cpd session run {id} must run in the pane cpd started for it: {why}."),
        )
        .with("session", id.as_str())
    };
    // `TMUX` is `<socket path>,<pid>,<session>`: the pane must be on cpd's server.
    let server = std::env::var("TMUX").unwrap_or_default();
    let socket = server
        .split(',')
        .next()
        .and_then(|path| Path::new(path).file_name())
        .map(|n| n.to_string_lossy().into_owned());
    if socket.as_deref() != Some(tmux.socket()) {
        return Err(wrong(format!(
            "this is not a pane of the tmux server {:?}",
            tmux.socket()
        )));
    }
    let pane = std::env::var("TMUX_PANE")
        .ok()
        .and_then(|p| PaneId::parse(&p))
        .ok_or_else(|| wrong("TMUX_PANE is not set".into()))?;
    let deadline = ports.clock.now() + PANE_WAIT;
    loop {
        let panes = tmux.list_panes()?;
        let found = panes
            .into_iter()
            .find(|p| p.pane == pane)
            .ok_or_else(|| wrong(format!("pane {pane} is gone")))?;
        match &found.cpd_session {
            Some(s) if s == id => return Ok(()),
            Some(other) => {
                return Err(wrong(format!("pane {pane} runs Session {other}"))
                    .with("pane", pane.as_str())
                    .with("found", other.as_str()));
            }
            None if ports.clock.now() >= deadline => {
                return Err(
                    wrong(format!("pane {pane} has no @cpd_session")).with("pane", pane.as_str())
                );
            }
            None => ports.clock.sleep(Duration::from_millis(100)),
        }
    }
}

/// The Session's pi file: the recorded one if it exists, else the one pi
/// created (contract §6.5).
pub(super) fn existing_file(dirs: &PiDirs, row: &Row) -> Option<PathBuf> {
    row.file
        .as_ref()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| pi::session_file(dirs, &row.id))
}

/// Records `sessions.file` once pi has created it: a data write, no Event
/// (commands.md §10.8). Returns whether the file is recorded.
fn record_file(env: &mut Env, dirs: &PiDirs, id: &SessionId) -> Result<bool, CpdError> {
    let Some(file) = pi::session_file(dirs, id) else {
        return Ok(false);
    };
    let file = file.display().to_string();
    env.store.write("session run", |tx| {
        tx.execute(
            "UPDATE sessions SET file = ?1 WHERE id = ?2 AND file IS NOT ?1",
            [file.as_str(), id.as_str()],
        )?;
        Ok(())
    })?;
    Ok(true)
}

/// How pi starts this time (§6.7, commands.md §10.8): resume when the file
/// exists, else fork once the source is idle, else fresh. `None` at the end of
/// input while a fork waits for a source that can never become idle.
fn start(env: &mut Env, dirs: &PiDirs, row: &Row) -> Result<Option<LeadStart>, CpdError> {
    // The row's current Tier, also on a resume (hs-d506): pi uses `--model` instead of the model its file recorded.
    let models = row::models_of(env, row)?;
    let (model, level) = row::model_and_level(&models, &row.id, &row.tier)?;
    let model = format!("{model}:{level}");
    let ports = env.ports.clone();
    if let Some(file) = existing_file(dirs, row) {
        if row.file.as_deref() != Some(&file.display().to_string()) {
            patiently_with(&*ports.clock, "recording the session file", || {
                record_file(env, dirs, &row.id)
            })?;
        }
        return Ok(Some(LeadStart::Resume { model, file }));
    }
    if row.by_user && row.forked_from.is_none() {
        return Ok(Some(LeadStart::Fresh {
            model,
            name: row.name.clone(),
            first_message: None,
        }));
    }
    let message = first_message(&env.home, row)?;
    match &row.forked_from {
        Some(source) => {
            let Some(source_file) = wait_for_source_with(&ports, env, dirs, source, &row.name)?
            else {
                return Ok(None);
            };
            Ok(Some(LeadStart::Fork {
                model,
                name: row.name.clone(),
                source_file,
                first_message: message,
            }))
        }
        None => {
            let first_message = message.ok_or_else(|| {
                CpdError::new(
                    ErrorCode::FileUnreadable,
                    format!("Session {} has no first message to start with.", row.id),
                )
                .with("session", row.id.as_str())
                .with(
                    "path",
                    match (&row.plan, row.role) {
                        (Some(plan), _) => Some(env.home.lead_message(plan).display().to_string()),
                        (None, Role::Manager) => {
                            Some(env.home.manager_message(&row.context).display().to_string())
                        }
                        (None, _) => None,
                    },
                )
            })?;
            Ok(Some(LeadStart::Fresh {
                model,
                name: row.name.clone(),
                first_message: Some(first_message),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;
    use crate::clock::ManualClock;
    use crate::driver::ports_tests::process;
    use crate::host::spawn::FakeSpawn;

    /// A process the `FakeSpawn` started, adopted by pid as the new `session run` adopts pi (a pid is all that survives an
    /// `exec`). Nothing runs: the fake is the handle, and what it did is `exit` and the record of `signal`.
    fn adopt(spawn: &FakeSpawn) -> (PiProc, SpawnPid) {
        let pid = process(spawn);
        (PiProc::adopt(Pid::from_raw(pid.0 as i32)), pid)
    }

    #[test]
    fn an_adopted_pi_running_is_not_exited_and_its_pid_is_kept() {
        let spawn = FakeSpawn::new();
        let (mut pi, pid) = adopt(&spawn);
        assert_eq!(pi.try_wait_with(&spawn).unwrap(), None);
        assert_eq!(pi.try_wait_with(&spawn).unwrap(), None);
        assert!(pi.id() > 1);
        assert_eq!(pi.id(), pid.0 as i32);
        pi.signal_with(&spawn, SpawnSignal::Kill);
        assert!(pi.try_wait_with(&spawn).unwrap().is_some());
    }

    #[test]
    fn an_adopted_pi_reports_its_exit_code_and_keeps_reporting_it() {
        let spawn = FakeSpawn::new();
        let (mut pi, pid) = adopt(&spawn);
        spawn.exit(pid, 3);
        let status = pi.try_wait_with(&spawn).unwrap().expect("exited");
        assert_eq!(status.code(), Some(3));
        assert!(!status.success());
        assert_eq!(
            pi.try_wait_with(&spawn).unwrap(),
            Some(status),
            "waited for once, answered again"
        );
        let (mut ok, pid) = adopt(&spawn);
        spawn.exit(pid, 0);
        assert!(ok.try_wait_with(&spawn).unwrap().expect("exited").success());
    }

    #[test]
    fn an_adopted_pi_killed_by_a_signal_reports_it() {
        let spawn = FakeSpawn::new();
        let (mut pi, pid) = adopt(&spawn);
        pi.signal_with(&spawn, SpawnSignal::Term);
        let status = pi.try_wait_with(&spawn).unwrap().expect("signalled");
        assert_eq!(
            status.signal(),
            Some(nix::sys::signal::Signal::SIGTERM as i32)
        );
        assert!(!status.success());
        assert_eq!(spawn.signals(), [(pid, SpawnSignal::Term)]);
    }

    #[test]
    fn an_exited_pi_is_not_signalled_again() {
        // After the wait the pid may be anyone's: `signal_with` must not reach it.
        let spawn = FakeSpawn::new();
        let (mut pi, pid) = adopt(&spawn);
        spawn.exit(pid, 0);
        pi.try_wait_with(&spawn).unwrap();
        let other = process(&spawn);
        pi.pid = Pid::from_raw(other.0 as i32);
        pi.signal_with(&spawn, SpawnSignal::Kill);
        assert!(
            spawn.signals().is_empty(),
            "the pid of an exited pi was signalled"
        );
        assert!(spawn.alive(other));
    }

    #[test]
    fn a_pid_that_is_not_a_child_fails_at_the_first_wait() {
        // The fake knows no pid 1: it is nobody's child. The error is the handle's, reported by `try_wait_with` as it came.
        let spawn = FakeSpawn::new();
        let mut pi = PiProc::adopt(Pid::from_raw(1));
        let err = pi.try_wait_with(&spawn).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(
            pi.try_wait_with(&spawn).is_err(),
            "still an error at the next wait: no status was made up"
        );
    }

    #[test]
    fn a_store_error_is_said_once_per_code() {
        let mut logged = Logged::default();
        let failing =
            || -> Result<bool, CpdError> { Err(CpdError::new(ErrorCode::StoreTooNew, "newer.")) };
        assert_eq!(logged.once(failing(), "cannot read"), None);
        assert!(logged.0.contains(&ErrorCode::StoreTooNew));
        assert_eq!(logged.0.len(), 1);
        assert_eq!(logged.once(failing(), "cannot read"), None);
        assert_eq!(logged.once(Ok(true), "cannot read"), Some(true));
    }

    #[test]
    fn patiently_tries_again_after_a_transient_error_and_returns_what_is_not() {
        let clock = ManualClock::new();
        let mut calls = 0;
        let got = patiently_with(&clock, "test", || {
            calls += 1;
            if calls < 2 {
                Err(CpdError::new(ErrorCode::StoreBusy, "busy."))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(got.unwrap(), 2);
        let mut calls = 0;
        let got = patiently_with(&clock, "test", || {
            calls += 1;
            if calls < 2 {
                Err(CpdError::new(ErrorCode::LockTimeout, "setup lock."))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(
            got.unwrap(),
            2,
            "the wait for the setup lock is waited out too"
        );
        assert_eq!(
            clock.naps(),
            [RETRY_FIRST; 2],
            "each wait is a nap on the clock, not the wall clock"
        );
        let mut calls = 0;
        let err = patiently_with::<()>(&clock, "test", || {
            calls += 1;
            Err(CpdError::new(ErrorCode::SessionNotFound, "gone."))
        })
        .unwrap_err();
        assert_eq!(
            (err.code(), calls),
            (ErrorCode::SessionNotFound, 1),
            "not transient: no retry"
        );
        assert_eq!(
            clock.naps().len(),
            2,
            "no nap for an error that is not transient"
        );
    }
}
