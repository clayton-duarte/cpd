//! `cpd2 watch --ensure` (commands.md §12, contract §6.4, §6.6): starts the
//! engine in `cpd-work:watch` when `locks/watch.lock` is free, and returns
//! once its process has taken the lock.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::json;

use super::lock;
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

pub(super) fn ensure() -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    if lock::held(&home)? {
        return Ok(Output::success(json!({"started": false, "already": true})));
    }
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let since = store.read(highest_id)?;
    let tmux = Tmux::new(config.tmux.socket.as_str());
    let work = config.tmux.work_session.as_str();
    // `-e CPD_HOME`: a server started by another process does not have this
    // process's environment, and the engine must use this `$CPD_HOME`.
    let spawn = Spawn {
        cwd: Some(home.root().to_owned()),
        env: BTreeMap::from([("CPD_HOME".to_owned(), home.root().display().to_string())]),
        argv: vec![PROGRAM.to_owned(), "watch".to_owned()],
    };
    // The server and the work session when missing (§6.1): the session's first
    // window is then the engine's.
    let pane = match tmux.create_session(work, WATCH_WINDOW, &spawn)? {
        Some((_, pane)) => pane,
        None => {
            for dead in tmux.list_panes()?.into_iter().filter(|p| p.session_name == work && p.window_name == WATCH_WINDOW && p.dead) {
                tmux.kill_window(&dead.window)?;
            }
            tmux.new_window(work, WATCH_WINDOW, &spawn)?.1
        }
    };
    wait_started(&tmux, &mut store, &pane, since)?;
    Ok(Output::success(json!({"started": true, "pane": pane.as_str()})))
}

/// Waits until the engine in `pane` has taken the lock, which it proves by
/// appending `watch-started` with its pid (the pane's: tmux runs a command of
/// several arguments without a shell). Fails early when the pane dies.
fn wait_started(tmux: &Tmux, store: &mut Store, pane: &PaneId, since: i64) -> Result<(), CpdError> {
    let deadline = Instant::now() + START_LIMIT;
    let failed = |why: String| {
        CpdError::new(ErrorCode::WatchStartFailed, format!("The engine did not start in pane {pane}: {why}"))
            .with("pane", pane.as_str())
            .with("next", "cpd2 watch --ensure")
    };
    loop {
        let found = tmux.list_panes()?.into_iter().find(|p| p.pane == *pane);
        let Some(found) = found else { return Err(failed("its pane is gone.".to_owned())) };
        if found.dead {
            let status = found.dead_status.map_or_else(String::new, |s| format!(" with status {s}"));
            return Err(failed(format!("it exited{status}; see the pane and run/watch.log.")).with("exit", found.dead_status));
        }
        let started: bool = store.read(|conn| {
            Ok(conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM events WHERE id > ?1 AND kind = 'watch-started' AND json_extract(data, '$.pid') = ?2)",
                rusqlite::params![since, found.pid],
                |r| r.get(0),
            )?)
        })?;
        if started {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(failed(format!("its process did not take the lock within {}s.", START_LIMIT.as_secs())));
        }
        std::thread::sleep(POLL);
    }
}
