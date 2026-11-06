//! The durable host supervisor (contract §6.19, P1.10): the engine starts and supervises one host per live
//! durable Session — a `lead` or `manager` whose `runner` is `durable`.
//!
//! [`start`] takes the flock on `run/host/<session>.lock` and hands its open file description to the host as fd 3
//! (`Launch::hand_over`), so the lock lives with the host (Q14). [`run`] is the tick step: it starts a due Session's
//! host, restarts a dead one inside the bound (3 restarts in 10 minutes, hs-d785) and latches `host crash loop` when
//! the bound is reached. [`stop`] stops a host (SIGTERM, then SIGKILL) when its Session sleeps or is archived.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::SOURCE;
use super::engine::Engine;
use cpd_core::config::{Catalog, Tier};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{NewEvent, append};
use cpd_core::home::CpdHome;
use cpd_core::host::git_guard;
use cpd_core::host::spawn::{Launch, Pid, Signal};
use cpd_core::ids::{ContextId, PlanId, SessionId};

/// The label a Session gets when its host crash-looped (hs-d785).
pub(super) const CRASH_LABEL: &str = "host crash loop";

/// The restart bound: 3 restarts inside [`WINDOW`], then the Session stops (hs-d785).
const MAX_RESTARTS: u32 = 3;
/// The window of the restart bound (hs-d785, Q15).
const WINDOW: time::Duration = time::Duration::minutes(10);

/// The nap between the liveness checks of a host being stopped.
const STOP_NAP: Duration = Duration::from_secs(1);

/// The marker a `Spawn::cmdline` of a host carries (contract §6.19).
const HOST_FILE: &str = "durable-host/main.ts";

/// Per-Session restart bookkeeping and the host this engine started or adopted (contract §6.19).
#[derive(Default)]
pub struct State {
    hosts: HashMap<SessionId, Host>,
}

/// The one host of a Session.
struct Host {
    pid: Pid,
    /// The lock this engine holds for the host; `None` for an adopted host (a command started it). Held open for
    /// the host's life, so the flock lives with the host's own inherited descriptor (contract §6.19, Q14).
    #[allow(dead_code)]
    lock: Option<File>,
}

impl State {
    fn pid(&self, session: &SessionId) -> Option<Pid> {
        self.hosts.get(session).map(|host| host.pid)
    }
}

/// The stored restart bound of a Session: the count in the window and when the window started (pro-d430).
fn bound(
    engine: &mut Engine,
    session: &SessionId,
) -> Result<(u32, Option<OffsetDateTime>), CpdError> {
    let (count, start): (u32, Option<String>) = engine.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT host_restarts, host_window_start FROM sessions WHERE id = ?1",
            [session.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })?;
    let start = start
        .map(|text| OffsetDateTime::parse(&text, &Rfc3339))
        .transpose()
        .map_err(|e| {
            CpdError::new(
                ErrorCode::SetupFailed,
                format!("The stored restart window of Session {session} is invalid: {e}."),
            )
        })?;
    Ok((count, start))
}

fn set_bound(
    engine: &mut Engine,
    session: &SessionId,
    count: u32,
    window_start: OffsetDateTime,
) -> Result<(), CpdError> {
    let text = window_start.format(&Rfc3339).map_err(|e| {
        CpdError::new(
            ErrorCode::SetupFailed,
            format!("Cannot format the restart window of Session {session}: {e}."),
        )
    })?;
    engine.store.write(SOURCE, |tx| {
        tx.execute(
            "UPDATE sessions SET host_restarts = ?1, host_window_start = ?2 WHERE id = ?3",
            rusqlite::params![count, text, session.as_str()],
        )?;
        Ok(())
    })
}

/// `$CPD_HOME/run/host/`, the directory of the lock, the pid file, the log and the socket (§6.14, §6.19).
fn host_dir(home: &CpdHome) -> PathBuf {
    home.run_dir().join("host")
}

fn lock_path(home: &CpdHome, session: &SessionId) -> PathBuf {
    host_dir(home).join(format!("{session}.lock"))
}

fn pid_path(home: &CpdHome, session: &SessionId) -> PathBuf {
    host_dir(home).join(format!("{session}.pid"))
}

fn log_path(home: &CpdHome, session: &SessionId) -> PathBuf {
    host_dir(home).join(format!("{session}.log"))
}

fn io(path: &Path, e: std::io::Error) -> CpdError {
    CpdError::new(
        ErrorCode::SetupFailed,
        format!("Cannot use {}: {e}.", path.display()),
    )
    .with("path", path.display().to_string())
}

/// A Session the engine starts a host for (contract §6.19).
struct Due {
    id: SessionId,
    context: ContextId,
    plan: Option<PlanId>,
}

/// The durable Lead and Manager Sessions that are due a host: live, not asleep, not crash-looped (§6.19).
const DUE_SESSIONS: &str = "SELECT id, context, plan FROM sessions
  WHERE role IN ('lead', 'manager') AND runner = 'durable'
    AND archived_at IS NULL AND asleep_at IS NULL
    AND (state_label IS NULL OR state_label <> 'host crash loop')
  ORDER BY id";

fn due(engine: &mut Engine) -> Result<Vec<Due>, CpdError> {
    engine.store.read(|conn| {
        let mut stmt = conn.prepare(DUE_SESSIONS)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, context, plan)| {
                Ok(Due {
                    id: SessionId::parse(&id)?,
                    context: ContextId::parse(&context)?,
                    plan: plan.map(|p| PlanId::parse(&p)).transpose()?,
                })
            })
            .collect()
    })
}

/// The hosts step of the engine tick (contract §6.19).
pub fn run(engine: &mut Engine) {
    sweep(engine);
    let due = match due(engine) {
        Ok(due) => due,
        Err(e) => {
            return engine.log.line(&format!(
                "hosts: cannot read the due Sessions: {}",
                e.message()
            ));
        }
    };
    for session in due {
        if let Err(e) = due_one(engine, &session) {
            engine
                .log
                .line(&format!("hosts: Session {}: {}", session.id, e.message()));
        }
    }
}

/// The durable Sessions that are archived or asleep, any role: their hosts are to be stopped (G6/G32).
const SWEEP_SESSIONS: &str = "SELECT id FROM sessions
  WHERE runner = 'durable' AND (archived_at IS NOT NULL OR asleep_at IS NOT NULL)
  ORDER BY id";

/// Stops the live host of every archived or asleep durable Session (contract §6.19); idempotent, no Event.
fn sweep(engine: &mut Engine) {
    let ids: Result<Vec<String>, CpdError> = engine.store.read(|conn| {
        let mut stmt = conn.prepare(SWEEP_SESSIONS)?;
        Ok(stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    });
    let ids = match ids {
        Ok(ids) => ids,
        Err(e) => {
            return engine.log.line(&format!(
                "hosts: cannot read the Sessions to sweep: {}",
                e.message()
            ));
        }
    };
    for id in ids {
        let Ok(id) = SessionId::parse(&id) else {
            continue;
        };
        if engine.hosts.pid(&id).is_some() || live_pid_file(engine, &id).is_some() {
            stop(engine, &id);
            engine
                .log
                .line(&format!("hosts: stopped the host of Session {id}"));
        }
    }
}

/// Whether the idle Session's host started before the adapter swap (contract §6.19): its pid file is older than the
/// lstat mtime of the adapter symlink. Any unreadable time means not stale.
fn stale_idle(engine: &mut Engine, session: &SessionId) -> bool {
    let state: Result<String, CpdError> = engine.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT state FROM sessions WHERE id = ?1",
            [session.as_str()],
            |r| r.get(0),
        )?)
    });
    if state.as_deref() != Ok("idle") {
        return false;
    }
    let Ok(adapter) = engine.config.pi_adapter_dir() else {
        return false;
    };
    let mtime = |m: std::io::Result<std::fs::Metadata>| m.and_then(|m| m.modified()).ok();
    match (
        mtime(std::fs::symlink_metadata(adapter)),
        mtime(std::fs::metadata(pid_path(&engine.home, session))),
    ) {
        (Some(swap), Some(pid)) => pid < swap,
        _ => false,
    }
}

/// Stops a stale idle host and starts it again (contract §6.19); not a crash, so the bound is not used.
fn reload(engine: &mut Engine, session: &SessionId) -> Result<(), CpdError> {
    stop(engine, session);
    start_ignoring_running(engine, session)
}

/// One due Session: adopt its live host, restart a dead one inside the bound, or start it (contract §6.19).
fn due_one(engine: &mut Engine, due: &Due) -> Result<(), CpdError> {
    if let Some(pid) = engine.hosts.pid(&due.id) {
        if engine.ports.spawn.alive(pid) {
            if stale_idle(engine, &due.id) {
                return reload(engine, &due.id);
            }
            return Ok(());
        }
        return restart(engine, due);
    }
    if let Some(pid) = live_pid_file(engine, &due.id) {
        engine
            .hosts
            .hosts
            .insert(due.id.clone(), Host { pid, lock: None });
        if stale_idle(engine, &due.id) {
            return reload(engine, &due.id);
        }
        return Ok(());
    }
    start_ignoring_running(engine, &due.id)
}

/// A host this engine did not start, read from its pid file: alive, and its cmdline names the host and the Session.
fn live_pid_file(engine: &mut Engine, session: &SessionId) -> Option<Pid> {
    let pid = read_pid(&engine.home, session)?;
    if !engine.ports.spawn.alive(pid) {
        return None;
    }
    let line = engine.ports.spawn.cmdline(pid)?;
    (line.contains(HOST_FILE) && line.contains(session.as_str())).then_some(pid)
}

/// The death of a Session's host: reset the window, restart while the bound allows it, else latch (contract §6.19).
fn restart(engine: &mut Engine, due: &Due) -> Result<(), CpdError> {
    let now = engine.ports.clock.utc();
    let (mut count, mut window_start) = bound(engine, &due.id)?;
    if window_start.is_none_or(|start| now - start >= WINDOW) {
        window_start = Some(now);
        count = 0;
    }
    engine.hosts.hosts.remove(&due.id);
    if count >= MAX_RESTARTS {
        return latch(engine, due);
    }
    set_bound(engine, &due.id, count + 1, window_start.unwrap_or(now))?;
    match start(engine, &due.id) {
        Ok(_) => Ok(()),
        Err(e) if e.code() == ErrorCode::HostRunning => Ok(()),
        Err(e) => Err(e),
    }
}

/// The bound is reached: stop the Session with the label `host crash loop` and one `session-state` Event (Q15).
fn latch(engine: &mut Engine, due: &Due) -> Result<(), CpdError> {
    let from: String = engine.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT state FROM sessions WHERE id = ?1",
            [due.id.as_str()],
            |r| r.get(0),
        )?)
    })?;
    engine.store.write(SOURCE, |tx| {
        let changed = tx.execute(
            "UPDATE sessions SET state = 'stopped', state_label = ?1, state_at = (SELECT at FROM txns WHERE id = cpd_txn())
              WHERE id = ?2 AND (state_label IS NULL OR state_label <> ?1)",
            rusqlite::params![CRASH_LABEL, due.id.as_str()],
        )?;
        if changed == 0 {
            return Ok(());
        }
        let text = format!("Session {} is stopped: its host crashed {MAX_RESTARTS} times in 10 minutes.", due.id);
        let mut event = NewEvent::session_state(text, from.as_str(), "stopped", Some(CRASH_LABEL), false)
            .session(due.id.clone())
            .context(due.context.clone());
        if let Some(plan) = &due.plan {
            event = event.plan(plan.clone());
        }
        append(tx, event)?;
        Ok(())
    })?;
    engine.log.line(&format!(
        "hosts: Session {} stopped with the label {CRASH_LABEL:?}",
        due.id
    ));
    Ok(())
}

/// Starts a Session's host, ignoring a `host_running` refusal (another cpd process holds the lock): §6.19.
fn start_ignoring_running(engine: &mut Engine, session: &SessionId) -> Result<(), CpdError> {
    match start(engine, session) {
        Ok(_) => Ok(()),
        Err(e) if e.code() == ErrorCode::HostRunning => Ok(()),
        Err(e) => Err(e),
    }
}

/// The one start of a Session's host (contract §6.19): take the lock, build the Launch, spawn, write the pid file,
/// and remember the host in [`State`]. Returns the pid and the lock, held open for the host's life.
pub(super) fn start(engine: &mut Engine, session: &SessionId) -> Result<(Pid, File), CpdError> {
    let home = engine.home.clone();
    let dir = host_dir(&home);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|e| io(&dir, e))?;
    let lock = lock_path(&home, session);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
        .map_err(|e| io(&lock, e))?;
    if !cpd_core::lock::try_lock(&file).map_err(|e| io(&lock, e))? {
        return Err(CpdError::new(
            ErrorCode::HostRunning,
            format!("A host already runs for Session {session}."),
        )
        .with("session", session.as_str()));
    }
    let launch = launch(engine, session, &home, file.as_raw_fd())?;
    let pid = engine.ports.spawn.spawn(&launch).map_err(|e| {
        CpdError::new(
            ErrorCode::SetupFailed,
            format!("Starting the host of Session {session} failed: {e}."),
        )
        .with("program", launch.program.as_str())
    })?;
    write_pid(&pid_path(&home, session), pid)?;
    let held = file.try_clone().map_err(|e| io(&lock, e))?;
    engine.hosts.hosts.insert(
        session.clone(),
        Host {
            pid,
            lock: Some(held),
        },
    );
    Ok((pid, file))
}

/// The host's `Launch` (contract §6.19): pi's Node, `<pi_adapter>/durable-host/main.ts --session <id> --model
/// <pattern>`, `CPD_HOME`/`CPD_SESSION`/`PATH` (with `$CPD_HOME/bin` first), cwd the Session's, its log, detached,
/// and the lock handed as fd 3.
fn launch(
    engine: &mut Engine,
    session: &SessionId,
    home: &CpdHome,
    lock: RawFd,
) -> Result<Launch, CpdError> {
    let (tier, context, cwd) = session_row(engine, session)?;
    let catalog: Catalog = engine.catalog_of_session(&context)?;
    let model = catalog.model_arg(tier)?;
    let file = engine
        .config
        .pi_adapter_dir()?
        .join("durable-host")
        .join("main.ts");
    git_guard::install(home)?;
    let env_set = vec![
        ("CPD_HOME".to_owned(), home.root().display().to_string()),
        ("CPD_SESSION".to_owned(), session.to_string()),
        (
            "PATH".to_owned(),
            git_guard::path_with_bin(home, std::env::var_os("PATH")),
        ),
    ];
    Ok(Launch {
        program: cpd_core::config::catalog::node().display().to_string(),
        args: vec![
            file.display().to_string(),
            "--session".to_owned(),
            session.to_string(),
            "--model".to_owned(),
            model,
        ],
        env_set,
        env_remove: Vec::new(),
        cwd: Some(PathBuf::from(cwd)),
        setsid: true,
        stdout_to: Some(log_path(home, session)),
        hand_over: Some(lock),
        idle: true,
        foreground: false,
    })
}

/// The Session's Tier, Context and cwd, read when its host starts.
fn session_row(
    engine: &mut Engine,
    session: &SessionId,
) -> Result<(Tier, ContextId, String), CpdError> {
    engine.store.read(|conn| {
        let (tier, context, cwd): (String, String, String) = conn.query_row(
            "SELECT tier, context, cwd FROM sessions WHERE id = ?1",
            [session.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok((Tier::parse(&tier)?, ContextId::parse(&context)?, cwd))
    })
}

/// Writes `pid` and a newline to `path`, renamed into place (the `record_detached` pattern).
fn write_pid(path: &Path, pid: Pid) -> Result<(), CpdError> {
    let tmp = path.with_extension(format!("pid.{}.tmp", pid.0));
    std::fs::write(&tmp, format!("{}\n", pid.0))
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| io(path, e))
}

/// The pid of the host recorded in a Session's pid file, if it parses.
fn read_pid(home: &CpdHome, session: &SessionId) -> Option<Pid> {
    let text = std::fs::read_to_string(pid_path(home, session)).ok()?;
    Some(Pid(text.trim().parse().ok()?))
}

/// Stops the host of `session` (contract §6.19): clears the [`State`] entry (releasing the lock) and stops the host
/// — SIGTERM, then SIGKILL after `timeouts.pi_stop_seconds` — removing the pid file. Used by the sleep and archive
/// hooks; it never touches the Session row.
pub fn stop(engine: &mut Engine, session: &SessionId) {
    let held = engine.hosts.hosts.remove(session);
    let pid = match &held {
        Some(host) => Some(host.pid),
        None => read_pid(&engine.home, session),
    };
    if let Some(pid) = pid
        && engine.ports.spawn.alive(pid)
    {
        let _ = engine.ports.spawn.signal(pid, Signal::Term);
        let grace = Duration::from_secs(u64::from(engine.config.timeouts.pi_stop_seconds));
        let mut waited = Duration::ZERO;
        while waited < grace && engine.ports.spawn.alive(pid) {
            engine.ports.clock.sleep(STOP_NAP);
            waited += STOP_NAP;
        }
        if engine.ports.spawn.alive(pid) {
            let _ = engine.ports.spawn.signal(pid, Signal::Kill);
        }
    }
    let _ = std::fs::remove_file(pid_path(&engine.home, session));
    // `held` drops here, releasing the lock (the host's own fd is already gone).
    drop(held);
}
