//! The Worker runner seam (contract §3.4 "The Worker runner and its seam"): everything that differs between the two runners.
//!
//! - `pi`: a tmux window and pi's session file, exactly as before (`worker.rs`, `run.rs`'s `PaneWatch`);
//! - `durable`: a child process of the driver, watched by its pid, started again in place, stopped by SIGTERM then SIGKILL.
//!
//! A driver, a reload, E2 and R follow the Attempt's stored `runner`, never today's `worker_runner`. This module holds the
//! start (A3), the liveness (A4) and the stop (A9, W1) of the `durable` runner, and the choice between the runners for the
//! stop and the end report. The end report (A5, A9) is [`Report`], one value with two producers: [`pi_report`] and
//! [`durable_report`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::Value;

use crate::attempt::ledger::{Attempt as Target, corrupt};
use crate::config::{Catalog, Config, WorkerRunner};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::host::git_guard;
use crate::host::pi::{self, PiDirs};
use crate::host::spawn::{Launch, Pid, Signal, Spawn};
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::metrics::{self, Metrics};
use crate::ports::Ports;
use crate::store::Store;

use super::worker;

/// After the 3rd restart of one driver run, the next death of the runner ends the Attempt `crashed` (A4).
pub(super) const MAX_RESTARTS: u32 = 3;

/// The exit status of a `usage` error: the runner refused its arguments, and the same launch would be refused again.
const USAGE: i32 = 2;

/// The runner's stop waits for the pid to end in naps of this length.
const STOP_NAP: Duration = Duration::from_secs(1);

/// The Attempt's stored runner and, for a `durable` one, the pid of its process.
pub(super) fn stored(
    conn: &Connection,
    plan: &PlanId,
    id: &AttemptId,
) -> Result<(WorkerRunner, Option<Pid>), CpdError> {
    let (runner, pid): (String, Option<u32>) = conn.query_row(
        "SELECT runner, worker_pid FROM attempts WHERE plan = ?1 AND id = ?2",
        params![plan.as_str(), id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let runner = WorkerRunner::parse(&runner)
        .map_err(|e| corrupt(format!("runner of Attempt {id}: {e}")))?;
    Ok((runner, pid.map(Pid)))
}

/// `<pi_adapter>/durable-worker/main.ts`: the file the `durable` runner runs.
pub(crate) fn runner_file(adapter: &Path) -> PathBuf {
    adapter.join("durable-worker").join("main.ts")
}

/// A3 for `durable`: the runner's `Launch`. `setup_failed` when its file is absent, before anything is spawned. The same
/// launch starts the runner again (A4), so the store and the conversation resume.
pub(super) fn launch(
    home: &CpdHome,
    config: &Config,
    catalog: &Catalog,
    target: &Target,
    session: &SessionId,
) -> Result<Launch, CpdError> {
    let file = runner_file(&config.pi_adapter_dir()?);
    if !file.is_file() {
        return Err(CpdError::new(
            ErrorCode::SetupFailed,
            format!("The durable runner file {} does not exist.", file.display()),
        ));
    }
    let model = catalog.model_arg(target.tier)?;
    git_guard::install(home)?;
    let spec = home.worker_spec(&target.plan, &target.id);
    let mut env_set: std::collections::BTreeMap<String, String> =
        pi::worker_env(home, session, &target.plan, &target.id);
    explore_env(&mut env_set, target);
    Ok(Launch {
        program: crate::config::catalog::node().display().to_string(),
        args: vec![
            file.display().to_string(),
            "--model".to_owned(),
            model,
            "--spec".to_owned(),
            spec.display().to_string(),
        ],
        env_set: env_set.into_iter().collect(),
        env_remove: Vec::new(),
        cwd: Some(target.worktree.clone()),
        setsid: true,
        stdout_to: Some(home.worker_log(&target.plan, &target.id)),
        hand_over: None,
        idle: true,
        foreground: false,
    })
}

/// An explore Worker's environment adds `CPD_JOB_KIND=<kind>` (contract §3.4 A3, explore variant); no other Worker has it.
pub(super) fn explore_env(env: &mut std::collections::BTreeMap<String, String>, target: &Target) {
    if crate::plan::jobs::is_explore(&target.kind) {
        env.insert("CPD_JOB_KIND".to_owned(), target.kind.clone());
    }
}

/// Starts the `durable` runner: `setup_failed` when the spawn fails, as every A2 or A3 failure does.
pub(super) fn spawn(ports: &Ports, launch: &Launch) -> Result<Pid, CpdError> {
    ports.spawn.spawn(launch).map_err(|e| {
        CpdError::new(
            ErrorCode::SetupFailed,
            format!("Starting the durable runner failed: {e}."),
        )
        .with("program", launch.program.as_str())
    })
}

/// How the `durable` runner ended, as A4 tells it apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// Exit status 2, a `usage` error: no restart.
    Usage,
    /// Exit 1, 143, a signal, or a status that cannot be read: started again, up to [`MAX_RESTARTS`].
    Other,
}

/// A4's look at the `durable` runner: `None` while its pid is alive, else how it ended. An exit status that cannot be
/// read (a reload loses the child table) counts as exit 1.
pub(super) fn look(spawn: &dyn Spawn, pid: Pid) -> Option<Ended> {
    if spawn.alive(pid) {
        return None;
    }
    let code = spawn
        .try_wait(pid)
        .ok()
        .flatten()
        .and_then(|status| status.code());
    Some(if code == Some(USAGE) {
        Ended::Usage
    } else {
        Ended::Other
    })
}

/// The proof that a pid is this Attempt's runner: its command line holds the runner file and the Attempt's Spec path. Every
/// signal to a stored `worker_pid` goes through it, so a reused pid is never signalled.
pub(crate) struct Guard {
    runner_file: PathBuf,
    spec: PathBuf,
}

impl Guard {
    pub(crate) fn of(
        config: &Config,
        home: &CpdHome,
        plan: &PlanId,
        id: &AttemptId,
    ) -> Result<Guard, CpdError> {
        Ok(Guard {
            runner_file: runner_file(&config.pi_adapter_dir()?),
            spec: home.worker_spec(plan, id),
        })
    }

    fn holds(&self, spawn: &dyn Spawn, pid: Pid) -> bool {
        spawn.cmdline(pid).is_some_and(|line| {
            line.contains(&*self.runner_file.to_string_lossy())
                && line.contains(&*self.spec.to_string_lossy())
        })
    }

    /// Sends `signal` to `pid` when it is the runner of this Attempt; whether it was sent. A pid that is gone is fine.
    pub(crate) fn signal(
        &self,
        spawn: &dyn Spawn,
        pid: Pid,
        signal: Signal,
    ) -> Result<bool, CpdError> {
        if !spawn.alive(pid) || !self.holds(spawn, pid) {
            return Ok(false);
        }
        match spawn.signal(pid, signal) {
            Ok(()) => Ok(true),
            Err(_) if !spawn.alive(pid) => Ok(false),
            Err(e) => Err(CpdError::new(
                ErrorCode::SpawnFailed,
                format!("Signalling the durable runner (pid {}) failed: {e}.", pid.0),
            )),
        }
    }
}

/// A9 and W1 for `durable`: SIGTERM, then up to `grace` of naps on `ports.clock` for the pid to end, then SIGKILL.
fn stop_durable(ports: &Ports, guard: &Guard, pid: Pid, grace: Duration) -> Result<(), CpdError> {
    if !guard.signal(&*ports.spawn, pid, Signal::Term)? {
        return Ok(());
    }
    let mut waited = Duration::ZERO;
    while waited < grace {
        if !ports.spawn.alive(pid) {
            return Ok(());
        }
        ports.clock.sleep(STOP_NAP);
        waited += STOP_NAP;
    }
    guard.signal(&*ports.spawn, pid, Signal::Kill)?;
    Ok(())
}

/// Stops the Worker of Attempt `id` by its stored `runner` (A9, and W1 before the merge): `pi` kills its window, `durable`
/// signals `pid` (`worker_pid`) as [`stop_durable`] says. A `durable` Attempt with no pid never started a runner.
pub(super) fn stop(
    ports: &Ports,
    config: &Config,
    home: &CpdHome,
    tmux: &Tmux,
    plan: &PlanId,
    id: &AttemptId,
    stored: (WorkerRunner, Option<Pid>),
) -> Result<(), CpdError> {
    match stored {
        (WorkerRunner::Pi, _) => worker::kill_attempt_window_with(ports, config, tmux, plan, id),
        (WorkerRunner::Durable, None) => Ok(()),
        (WorkerRunner::Durable, Some(pid)) => {
            let grace = Duration::from_secs(u64::from(config.timeouts.pi_stop_seconds));
            stop_durable(ports, &Guard::of(config, home, plan, id)?, pid, grace)
        }
    }
}

/// E2 and R end the Worker of Attempt `id` whose driver is dead (R8), by the Attempt's stored `runner` (never today's
/// `worker_runner`): `pi` kills its window, `durable` sends SIGKILL at once to the stored `worker_pid` through the [`Guard`]
/// (the runner's commits survive a kill and the last report is on disk, so there is no SIGTERM and no wait). A `durable`
/// Attempt with no pid never started a runner.
pub fn kill_orphan(
    ports: &Ports,
    config: &Config,
    home: &CpdHome,
    tmux: &Tmux,
    store: &mut Store,
    plan: &PlanId,
    id: &AttemptId,
) -> Result<(), CpdError> {
    match store.read(|conn| stored(conn, plan, id))? {
        (WorkerRunner::Pi, _) => worker::kill_attempt_window_with(ports, config, tmux, plan, id),
        (WorkerRunner::Durable, None) => Ok(()),
        (WorkerRunner::Durable, Some(pid)) => Guard::of(config, home, plan, id)?
            .signal(&*ports.spawn, pid, Signal::Kill)
            .map(drop),
    }
}

/// The end report, as A5 and A9 read it whichever the runner: the fields of [`Metrics`] and the last turn's `errorMessage`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Report {
    pub(super) metrics: Metrics,
    pub(super) error_message: Option<String>,
}

/// The `pi` producer: the Worker's session file at `file`. `None` while it cannot be parsed (a line being written).
pub(super) fn pi_report(file: &Path) -> Option<Report> {
    let metrics = metrics::parse_file(file).ok()?;
    Some(Report {
        metrics,
        error_message: metrics::last_error_message(file),
    })
}

/// The `pi` Worker's session file, once pi has created it.
pub(super) fn pi_file(dirs: &PiDirs, session: &SessionId) -> Option<PathBuf> {
    pi::session_file(dirs, session)
}

/// The `EndReport` JSON of contract §6.12.
#[derive(Deserialize)]
struct EndReport {
    stop_reason: String,
    error_message: Option<String>,
    turns: u64,
    tokens_in: u64,
    tokens_out: u64,
    cache_read: u64,
    cache_write: u64,
    cost_usd: f64,
    model: Option<String>,
}

/// The `durable` producer: the end report at `file`, with `duration_s` the wall seconds of the Attempt so far (`wall_s`) and
/// `thinking_level` null. `Ok(None)` while the runner has written none; `Err` for a file that exists
/// and does not parse (the runner writes it atomically, so that is its fault), naming the file and the parse error.
pub(super) fn durable_report(file: &Path, wall_s: f64) -> Result<Option<Report>, CpdError> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CpdError::file_unreadable(file, e)),
    };
    let report: EndReport = serde_json::from_str(&text).map_err(|e| {
        CpdError::new(
            ErrorCode::SessionMalformed,
            format!("The end report {} does not parse: {e}.", file.display()),
        )
        .with("path", file.display().to_string())
    })?;
    Ok(Some(Report {
        metrics: Metrics {
            turns: report.turns,
            tokens_in: report.tokens_in,
            tokens_out: report.tokens_out,
            cache_read: report.cache_read,
            cache_write: report.cache_write,
            cost_usd: report.cost_usd,
            duration_s: wall_s,
            model: report.model,
            thinking_level: None,
            stop_reason: Some(report.stop_reason),
        },
        error_message: report.error_message,
    }))
}

/// A9's `metrics` by the stored `runner`: `pi` from its session file, `durable` from its end report at `report`; the
/// Attempt's `wall_s` gives the latter its duration. `None` (and why, in the log) when there is none to read.
pub(super) fn metrics(
    runner: WorkerRunner,
    dirs: &PiDirs,
    session: Option<&SessionId>,
    report: &Path,
    wall_s: f64,
) -> Option<Value> {
    match runner {
        WorkerRunner::Pi => {
            let file = pi::session_file(dirs, session?)?;
            match metrics::parse_file(&file) {
                Ok(m) => Some(m.to_json()),
                Err(e) => {
                    super::run::log(&format!(
                        "no metrics from {}: {}",
                        file.display(),
                        e.message()
                    ));
                    None
                }
            }
        }
        WorkerRunner::Durable => match durable_report(report, wall_s) {
            Ok(found) => found.map(|r| r.metrics.to_json()),
            Err(e) => {
                super::run::log(&format!("no metrics: {}", e.message()));
                None
            }
        },
    }
}
