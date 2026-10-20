//! `cpd watch --ensure` (commands.md §12, contract §6.4, §6.6): starts the
//! engine in `cpd-work:watch` when `locks/watch.lock` is free, and returns
//! once its process has taken the lock.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::time::{Duration, Instant};

use serde_json::json;

use super::lock;
use super::log::Log;
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::highest_id;
use crate::home::CpdHome;
use crate::host::layout::WATCH_WINDOW;
use crate::host::{PaneId, Spawn, Tmux};
use crate::output::Output;
use crate::session::PROGRAM;
use crate::store::Store;

/// How long the new engine has to take the lock (commands.md §12).
const START_LIMIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(100);
/// With `--wait`: how long a dying engine's process has to let go of the lock
/// (its window is gone: it is stopping, as `kill-window`'s SIGHUP says).
const LOCK_LIMIT: Duration = Duration::from_secs(15);
/// Without `--wait`: how long a call waits for `locks/watch-ensure.lock` behind another:
/// a supervising call holds it for at most [`LOCK_LIMIT`] + [`START_LIMIT`] + [`BACKOFF`] (27 s).
const PLAIN_LIMIT: Duration = Duration::from_secs(30);
/// With `--wait`: the pause after a failed start, so an engine that cannot start
/// is tried once every few seconds, not in a loop of the hooks it triggers.
const BACKOFF: Duration = Duration::from_secs(2);

/// `watch --ensure`; `wait` is the supervising hooks' call (contract §6.6).
pub(super) fn ensure(wait: bool) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let tmux = Tmux::new(config.tmux.socket.as_str());
    let work = config.tmux.work_session.as_str();
    // One call at a time, plain or supervising: a pane that dies at once makes its
    // hook run again while this call still starts it, and the next waits out the
    // back-off; and a plain call that probed the engine's lock beside a supervising
    // one would start a second window.
    let _one_at_a_time = serialize(&home, wait)?;
    if !free(&home, &tmux, work, wait)? {
        return Ok(Output::success(json!({"started": false, "already": true})));
    }
    let result = start(&home, &config, &tmux);
    if wait {
        // Nobody reads a hook's output (§6.6): its failures are in the engine's log.
        if let Ok(mut log) = Log::open(&home) {
            match &result {
                Ok(_) => log.line("supervision: the engine runs"),
                Err(e) => log.line(&format!("supervision: cannot start the engine: {} ({})", e.message(), e.code().as_str())),
            }
        }
        if result.is_err() {
            std::thread::sleep(BACKOFF);
        }
    }
    result
}

/// Takes `locks/watch-ensure.lock`, which every call holds while it checks and starts the
/// engine. A supervising call (`wait`) waits for it without a limit; a plain call waits
/// [`PLAIN_LIMIT`] and then fails.
fn serialize(home: &CpdHome, wait: bool) -> Result<File, CpdError> {
    let path = home.locks_dir().join("watch-ensure.lock");
    let failed = |e: std::io::Error| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot use {}: {e}.", path.display())).with("lock", path.display().to_string())
    };
    std::fs::create_dir_all(home.locks_dir()).map_err(failed)?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(failed)?;
    if wait {
        crate::lock::blocking(&file).map_err(failed)?;
        return Ok(file);
    }
    let deadline = Instant::now() + PLAIN_LIMIT;
    while !crate::lock::try_lock(&file).map_err(failed)? {
        if Instant::now() >= deadline {
            return Err(CpdError::new(
                ErrorCode::WatchStartFailed,
                format!("Another watch --ensure still holds {} after {}s.", path.display(), PLAIN_LIMIT.as_secs()),
            )
            .with("lock", path.display().to_string())
            .with("next", "cpd watch --ensure"));
        }
        std::thread::sleep(POLL);
    }
    Ok(file)
}

/// Whether the engine's lock is free, so that it has to be started. A held lock
/// is the engine, except for a supervising call (`wait`) that finds no live pane
/// in the engine's window: that engine's window is gone and its process is on
/// its way out, so the call waits [`LOCK_LIMIT`] for the lock.
fn free(home: &CpdHome, tmux: &Tmux, work: &str, wait: bool) -> Result<bool, CpdError> {
    if !lock::held(home)? {
        return Ok(true);
    }
    if !wait || engine_panes(tmux, work)?.iter().any(|p| !p.dead) {
        return Ok(false);
    }
    let deadline = Instant::now() + LOCK_LIMIT;
    while Instant::now() < deadline {
        std::thread::sleep(POLL);
        if !lock::held(home)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The panes of the engine's window.
fn engine_panes(tmux: &Tmux, work: &str) -> Result<Vec<crate::host::Pane>, CpdError> {
    Ok(tmux.list_panes()?.into_iter().filter(|p| p.session_name == work && p.window_name == WATCH_WINDOW).collect())
}

fn start(home: &CpdHome, config: &Config, tmux: &Tmux) -> Result<Output, CpdError> {
    let work = config.tmux.work_session.as_str();
    let mut store = Store::open(home, u64::from(config.timeouts.store_busy_ms))?;
    let since = store.read(highest_id)?;
    // `-e CPD_HOME`: a server started by another process does not have this
    // process's environment, and the engine must use this `$CPD_HOME`.
    let spawn = Spawn {
        cwd: Some(home.root().to_owned()),
        env: BTreeMap::from([("CPD_HOME".to_owned(), home.root().display().to_string())]),
        argv: vec![PROGRAM.to_owned(), "watch".to_owned()],
    };
    // A server that an older cpd started has no supervising hooks yet (§6.6).
    if tmux.server_running()? {
        tmux.apply_server_options()?;
    }
    // A dead engine's pane starts again in its window; the other dead windows
    // go. When the first was the session's last, respawning keeps it.
    let mut dead = engine_panes(tmux, work)?.into_iter().filter(|p| p.dead);
    let reuse = dead.next();
    for other in dead {
        tmux.kill_window(&other.window)?;
    }
    let pane = match reuse {
        Some(dead) => {
            tmux.respawn_pane(&dead.pane, &spawn)?;
            dead.pane
        }
        // The server and the work session when missing (§6.1): the session's
        // first window is then the engine's.
        None => match tmux.create_session(work, WATCH_WINDOW, &spawn)? {
            Some((_, pane)) => pane,
            None => tmux.new_window(work, WATCH_WINDOW, &spawn)?.1,
        },
    };
    match wait_started(tmux, &mut store, &pane, since) {
        Ok(()) => Ok(Output::success(json!({"started": true, "pane": pane.as_str()}))),
        // A concurrent call started an engine first: ours lost the lock and its
        // pane died. The engine runs, which is what the caller asked for.
        Err(e) if e.code() == ErrorCode::WatchStartFailed && lock::held(home)? => {
            let dead = tmux.list_panes()?.into_iter().find(|p| p.pane == pane && p.dead);
            if let Some(dead) = dead {
                tmux.kill_window(&dead.window)?;
            }
            Ok(Output::success(json!({"started": false, "already": true})))
        }
        Err(e) => Err(e),
    }
}

/// Waits until the engine in `pane` has taken the lock, which it proves by
/// appending `watch-started` with its pid (the pane's: tmux runs a command of
/// several arguments without a shell). Fails early when the pane dies.
fn wait_started(tmux: &Tmux, store: &mut Store, pane: &PaneId, since: i64) -> Result<(), CpdError> {
    let deadline = Instant::now() + START_LIMIT;
    let failed = |why: String| {
        CpdError::new(ErrorCode::WatchStartFailed, format!("The engine did not start in pane {pane}: {why}"))
            .with("pane", pane.as_str())
            .with("next", "cpd watch --ensure")
    };
    loop {
        let found = tmux.list_panes()?.into_iter().find(|p| p.pane == *pane);
        let Some(found) = found else { return Err(failed("its pane is gone.".to_owned())) };
        if found.dead {
            let status = found.dead_status.map_or_else(|| ", its exit status is unknown".to_owned(), |s| format!(" with status {s}"));
            return Err(failed(format!("it exited{status}; see the pane and run/watch.log.")).with("exit", found.dead_status));
        }
        // No pid: tmux >= 3.8 has seen its tty close, so it is dying and is
        // dead at a later poll.
        let started = match found.pid {
            Some(pid) => store.read(|conn| {
                Ok(conn.query_row(
                    "SELECT EXISTS (SELECT 1 FROM events WHERE id > ?1 AND kind = 'watch-started' AND json_extract(data, '$.pid') = ?2)",
                    rusqlite::params![since, pid],
                    |r| r.get::<_, bool>(0),
                )?)
            })?,
            None => false,
        };
        if started {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(failed(format!("its process did not take the lock within {}s.", START_LIMIT.as_secs())));
        }
        std::thread::sleep(POLL);
    }
}
