//! The driver `cpd attempt run` (contract §3.4 A1–A9; commands.md §6.4).
//!
//! Each status change is one transaction with the Event of §3.4's table. Its
//! cancel safe points are before A3 (here) and before A8's rebase and its
//! compare-and-swap (`crate::merge`, §7.4 steps 1 and 7); while `running`, a
//! cancel reaches it as E2's `stop_request = cancelled` (§3.7, A5).

use std::time::Duration;

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use rusqlite::params;
use serde_json::{Value, json};

use super::ledger::{self, Cause, End, Ladder, SOURCE, Target};
use super::lock;
use super::stall;
use super::worker;
use super::RunArgs;
use crate::config::{Catalog, Config, load_catalog};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Kind, NewEvent};
use crate::git::{self, Setup};
use crate::home::{CpdHome, Hook};
use crate::host::exe::{Exe, Refusal};
use crate::host::pi::{self, PiDirs};
use crate::host::{PaneId, Tmux};
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::merge;
use crate::output::Output;
use crate::plan::jobs;
use crate::plan::locked_fingerprint;
use crate::store::Store;

/// How often A4 looks at the Attempt, its Session and its pane.
const POLL: Duration = Duration::from_millis(500);

/// What A4 knows of the Worker's pane at a poll.
enum Look {
    /// Alive, with its process's pid when tmux gave one (hs-n102).
    Alive(Option<u32>),
    /// Dead or missing: the `crashed` detail.
    Gone(String),
    /// `list-panes` failed once: not known until the next poll (engine-W8).
    Unknown,
}

/// A4's watch on the Worker's pane (engine-F1). The pane's pid is read from
/// `list-panes` once and then tested with `kill(pid, 0)` and `/proc`, with no
/// fork; `list-panes` runs again only when there is no live pid, to learn how
/// the pane ended. A failed `list-panes` is retried at the next poll, once,
/// before the pane is judged (contract §6.3 liveness, engine-W8).
#[derive(Default)]
struct PaneWatch {
    pid: Option<u32>,
    failed: bool,
}

impl PaneWatch {
    fn look(&mut self, tmux: &Tmux, pane: &PaneId) -> Result<Look, CpdError> {
        if let Some(pid) = self.pid.filter(|&pid| alive(pid)) {
            return Ok(Look::Alive(Some(pid)));
        }
        let panes = match tmux.list_panes() {
            Ok(panes) => panes,
            Err(e) if !self.failed => {
                log(&format!("A4: listing the panes failed, trying again at the next poll: {}", e.message()));
                self.failed = true;
                return Ok(Look::Unknown);
            }
            Err(e) => return Err(e),
        };
        self.failed = false;
        Ok(match panes.iter().find(|p| p.pane == *pane) {
            None => Look::Gone("The Worker's pane is gone.".to_owned()),
            Some(p) if p.dead => {
                let status = p.dead_status.map_or_else(String::new, |s| format!(" (exit status {s})"));
                Look::Gone(format!("The Worker's pane died{status} before it submitted."))
            }
            Some(p) => {
                self.pid = p.pid;
                Look::Alive(p.pid)
            }
        })
    }
}

/// Whether process `pid` runs: `kill(pid, 0)` finds it and, where `/proc`
/// exists, it is not a zombie (ended, not yet reaped by tmux).
fn alive(pid: u32) -> bool {
    let Ok(raw) = i32::try_from(pid) else { return false };
    if matches!(kill(Pid::from_raw(raw), None), Err(Errno::ESRCH)) {
        return false;
    }
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return true };
    // `pid (comm) state …`; comm may hold spaces and parentheses.
    stat.rfind(')').and_then(|i| stat[i + 1..].split_whitespace().next()) != Some("Z")
}

/// How the Attempt ended, as the driver prints it.
struct Ended {
    outcome: &'static str,
    cause: &'static str,
}

struct Driver {
    home: CpdHome,
    config: Config,
    store: Store,
    tmux: Tmux,
    plan: PlanId,
    id: AttemptId,
    /// The driver lock on fd 3, held for the driver's whole life (A1).
    lock: lock::Inherited,
    /// This executable as it was at start: a different file at its path is a new install.
    exe: Exe,
    /// Loaded in A1; the ladder needs it to end an Attempt.
    catalog: Option<Catalog>,
}

/// Where A1 takes an Attempt up, by its status (§3.4 step 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    /// `starting`: A2 onwards.
    Start,
    /// `running`, after a reload: the Worker is there, A4 onwards.
    Resume,
    /// `merging`, after a reload: A8.
    Merge,
}

/// What the driver does after A1: `entry` with what a `running` Attempt's
/// resume needs from the store.
enum Task {
    Start,
    /// The Worker's Session and pane.
    Resume(SessionId, PaneId),
    Merge,
}

/// A1's dispatch on the Attempt's status; any other is `illegal_transition`.
fn entry(id: &AttemptId, status: &str) -> Result<Entry, CpdError> {
    match status {
        "starting" => Ok(Entry::Start),
        "running" => Ok(Entry::Resume),
        "merging" => Ok(Entry::Merge),
        other => Err(CpdError::new(
            ErrorCode::IllegalTransition,
            format!("Attempt {id} is {other}: a driver runs only a starting, running or merging Attempt."),
        )
        .with("attempt", id.to_string())
        .with("status", other)),
    }
}

/// `cpd attempt run <attempt> --plan <id>` (internal).
pub fn run(args: RunArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let id = args.attempt;
    let home = CpdHome::from_env()?;
    // A1: before anything opens a file, so fd 3 cannot be one of ours. Held
    // until this function returns, which is the driver's whole life.
    let lock = lock::inherited(&home, &plan, &id)?;
    let exe = Exe::current()?;
    let config = Config::load(&home)?;
    let store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.as_str());
    let mut driver = Driver { home, config, store, tmux, plan, id, lock, exe, catalog: None };

    let target = driver.load()?;
    let entry = entry(&target.id, &target.status)?;
    let task = driver.task(entry, &target)?;
    match driver.drive(task, target) {
        Ok(ended) => Ok(Output::success(json!({"attempt": driver.id, "outcome": ended.outcome, "cause": ended.cause}))),
        Err(e) => {
            driver.fail(&e);
            Err(e)
        }
    }
}

fn log(line: &str) {
    eprintln!("cpd attempt run: {line}");
}

impl Driver {
    fn load(&mut self) -> Result<Target, CpdError> {
        let (plan, id) = (&self.plan, &self.id);
        self.store.read(|conn| ledger::load(conn, plan, id))
    }

    fn catalog(&self) -> &Catalog {
        self.catalog.as_ref().expect("A1 loads the catalog")
    }

    /// A1 (after the lock) to A9, from `entry`.
    fn drive(&mut self, task: Task, target: Target) -> Result<Ended, CpdError> {
        let job = self.begin(&target)?;
        let dirs = PiDirs::from_env()?;
        match task {
            Task::Start => self.start(&target, &job, &dirs),
            Task::Resume(session, pane) => {
                log("A1: resuming at A4 after a reload");
                self.wait(&session, &pane, &dirs)
            }
            Task::Merge => self.merge(&dirs),
        }
    }

    /// A1: the catalog, the Job, its fingerprint.
    fn begin(&mut self, target: &Target) -> Result<jobs::Job, CpdError> {
        self.catalog = Some(load_catalog(&self.config)?);
        let all = self.store.read(|conn| jobs::load(conn, &target.plan))?;
        let job = all.into_iter().find(|j| j.key == target.job.as_str()).ok_or_else(|| {
            CpdError::new(ErrorCode::JobNotFound, format!("No Job {} in Plan {}.", target.job, target.plan))
        })?;
        let found = locked_fingerprint(&job);
        if job.fingerprint.as_deref() != Some(found.as_str()) {
            return Err(CpdError::new(
                ErrorCode::LockedJobTampered,
                format!("Job {} does not match its Locked-Job fingerprint.", target.job),
            )
            .with("plan", target.plan.as_str())
            .with("job", target.job.as_str())
            .with("expected", job.fingerprint.clone())
            .with("found", found));
        }
        Ok(job)
    }

    /// A2 to A5, for a `starting` Attempt.
    fn start(&mut self, target: &Target, job: &jobs::Job, dirs: &PiDirs) -> Result<Ended, CpdError> {
        // A2: the worktree from the Lead Branch HEAD, and pre-start.
        log("A2: creating the worktree and running pre-start");
        let setup = Setup {
            timeout: Duration::from_secs(u64::from(self.config.timeouts.setup_minutes) * 60),
            log: self.home.hook_log(&self.plan, &self.id, Hook::PreStart),
        };
        let created = match git::create_attempt_worktree(&target.repo, &target.lead_worktree, &self.plan, &self.id, &setup) {
            Ok(created) => created,
            Err(e) => {
                let detail = format!("Setting up the worktree failed: {}", e.message());
                return self.end(target, End::Failed { cause: Cause::SetupFailed, detail }, dirs);
            }
        };
        let (plan, id) = (&self.plan, &self.id);
        self.store.write(SOURCE, |tx| {
            tx.execute(
                "UPDATE attempts SET base_sha = ?3 WHERE plan = ?1 AND id = ?2 AND status = 'starting'",
                params![plan.as_str(), id.to_string(), created.base_sha],
            )?;
            Ok(())
        })?;

        // The safe point before A3.
        if self.store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
            log("cancel requested before A3");
            return self.end(target, End::Cancelled, dirs);
        }

        // A3 of a review Job (B2): the Worker forks the Lead, once the Lead is idle.
        let review = job.review;
        let fork = if review {
            log("A3: waiting for the Lead to be idle, to fork it");
            match worker::wait_for_lead(&mut self.store, &self.config, dirs, target)? {
                Some(fork) => Some(fork),
                None => return self.end(target, End::Cancelled, dirs),
            }
        } else {
            None
        };

        // A3: the Spec, the Session, the Worker.
        log("A3: rendering the Spec and starting the Worker");
        let home = &self.home;
        let spec = self.store.read(|conn| worker::render_spec(conn, home, target, job))?;
        let session = SessionId::new_v4();
        self.register(target, &session, &spec, fork.as_ref().map(|f| &f.session))?;
        let pane = worker::start(&self.home, &self.config, self.catalog(), &self.tmux, dirs, target, &session, fork.as_ref())?;
        self.running(target, &session, &pane)?;

        // A4 and A5.
        self.wait(&session, &pane.pane, dirs)
    }

    /// What A1 does with an Attempt of this `entry`. A `running` Attempt takes
    /// its Worker's Session and pane from the store (A3 records both before
    /// `running`), nothing is written: the nudge-once state is the Attempt's
    /// `nudged`. A refusal is before any write, like A1's other refusals.
    fn task(&mut self, entry: Entry, target: &Target) -> Result<Task, CpdError> {
        match entry {
            Entry::Start => Ok(Task::Start),
            Entry::Merge => Ok(Task::Merge),
            Entry::Resume => {
                let nothing = |what: &str| {
                    CpdError::new(
                        ErrorCode::IllegalTransition,
                        format!("Attempt {} is running but the Ledger has no {what} of its Worker: there is nothing to resume.", target.id),
                    )
                    .with("attempt", target.id.to_string())
                    .with("status", target.status.as_str())
                };
                let session = target.worker_session.clone().ok_or_else(|| nothing("Session"))?;
                let pane: Option<String> = self.store.read(|conn| {
                    Ok(conn.query_row("SELECT tmux_pane FROM sessions WHERE id = ?1", [session.as_str()], |r| r.get(0))?)
                })?;
                let pane = pane.as_deref().and_then(PaneId::parse).ok_or_else(|| nothing("pane"))?;
                Ok(Task::Resume(session, pane))
            }
        }
    }

    /// The reload (§3.4 "Reload on install"): at a safe point, when a new cpd
    /// is installed, `exec` it with the same argv. Fd 3 stays open across the
    /// `exec`, so the lock is held with no gap; the new process resumes from
    /// the store. Returns only when nothing is installed, or what is installed
    /// cannot open the store (logged once per installed file, hs-d151); `Err`
    /// when the `exec` failed.
    fn reload_if_installed(&self) -> Result<(), CpdError> {
        if !self.exe.replaced() {
            return Ok(());
        }
        match self.exe.check_store() {
            Ok(()) => {}
            Err(Refusal::New(reason)) => {
                log(&format!("reload skipped: {} {reason}", self.exe.path().display()));
                return Ok(());
            }
            Err(Refusal::Known) => return Ok(()),
        }
        log("reload: new cpd installed, re-executing");
        self.lock.keep_across_exec()?;
        let failed = self.exe.exec(std::env::args_os().skip(1));
        // Still here: the `exec` failed. Do not leak the lock to the children.
        let _ = self.lock.close_on_exec();
        Err(failed)
    }

    /// A3's first transaction: the Worker's Session, registered before pi
    /// starts (§3.6), and the Attempt's `worker_session` and `spec_path`.
    fn register(&mut self, target: &Target, session: &SessionId, spec: &std::path::Path, forked_from: Option<&SessionId>) -> Result<(), CpdError> {
        let name = format!("{} {} a{}", target.plan, target.job, target.id.number());
        let worktree = target.worktree.to_string_lossy().into_owned();
        let tier = target.tier.to_string();
        self.store.write(SOURCE, |tx| {
            tx.execute(
                "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                                       state, state_label, state_at, created_at)
                 SELECT ?1, 'worker', ?2, ?3, ?4, ?5, NULL, ?8, ?6, ?7, NULL, 'stopped', NULL, at, at
                   FROM txns WHERE id = cpd_txn()",
                params![
                    session.as_str(),
                    target.context.as_str(),
                    target.plan.as_str(),
                    target.id.to_string(),
                    name,
                    worktree,
                    tier,
                    forked_from.map(SessionId::as_str)
                ],
            )?;
            let changed = tx.execute(
                "UPDATE attempts SET worker_session = ?3, spec_path = ?4 WHERE plan = ?1 AND id = ?2 AND status = 'starting'",
                params![target.plan.as_str(), target.id.to_string(), session.as_str(), spec.to_string_lossy()],
            )?;
            if changed == 0 {
                return Err(CpdError::new(ErrorCode::CasLost, format!("Attempt {} is no longer starting.", target.id)));
            }
            let text = format!("Worker {name} of Attempt {} registered.", target.id);
            let data = json!({"role": "worker", "name": name, "forked_from": forked_from, "tier": tier});
            events::append(
                tx,
                NewEvent::new(Kind::SessionRegistered, text, data)
                    .context(target.context.clone())
                    .plan(target.plan.clone())
                    .session(session.clone()),
            )?;
            Ok(())
        })
    }

    /// A3's second transaction: `starting → running` once the pane runs,
    /// with the Session's pane (`attempt-status`, `session-moved`).
    fn running(&mut self, target: &Target, session: &SessionId, pane: &worker::Pane) -> Result<(), CpdError> {
        self.store.write(SOURCE, |tx| {
            let changed = tx.execute(
                "UPDATE attempts SET status = 'running' WHERE plan = ?1 AND id = ?2 AND status = 'starting'",
                params![target.plan.as_str(), target.id.to_string()],
            )?;
            if changed == 0 {
                return Err(CpdError::new(ErrorCode::CasLost, format!("Attempt {} is no longer starting.", target.id)));
            }
            tx.execute("UPDATE sessions SET tmux_pane = ?2 WHERE id = ?1", params![session.as_str(), pane.pane.as_str()])?;
            let text = format!(
                "Attempt {} is running: its Worker started. See: cpd attempt show {} --plan {}",
                target.id, target.id, target.plan
            );
            events::append(
                tx,
                NewEvent::new(Kind::AttemptStatus, text, json!({"from": "starting", "to": "running"}))
                    .plan(target.plan.clone())
                    .job(target.job.clone())
                    .attempt(target.id.clone()),
            )?;
            let text = format!("Worker of Attempt {} is in its work window.", target.id);
            let data = json!({"pane": pane.pane.as_str(), "window": pane.window.as_str(), "place": "work"});
            events::append(
                tx,
                NewEvent::new(Kind::SessionMoved, text, data)
                    .context(target.context.clone())
                    .plan(target.plan.clone())
                    .session(session.clone()),
            )?;
            Ok(())
        })
    }

    /// A4 and A5: waits until the Attempt is submitted (A8), stopped, or its
    /// Worker is gone or idle for good.
    fn wait(&mut self, session: &SessionId, pane: &PaneId, dirs: &PiDirs) -> Result<Ended, CpdError> {
        log("A4: waiting for the Worker");
        // The Worker's session file, recorded once pi has created it.
        let mut file: Option<std::path::PathBuf> = None;
        // The Worker's assistant messages when it was nudged.
        let mut nudged_at: Option<u64> = None;
        let mut stall = stall::Watch::new(self.config.timeouts.stall_seconds);
        let mut watch = PaneWatch::default();
        loop {
            std::thread::sleep(POLL);
            // The pane before the row: a Worker that submits and exits is seen `merging`.
            let look = watch.look(&self.tmux, pane)?;
            if file.is_none() {
                file = pi::session_file(dirs, session);
                if let Some(found) = &file {
                    // A data write, no Event: metrics, `attempt read` and the TUI read it.
                    let path = found.to_string_lossy().into_owned();
                    self.store.write(SOURCE, |tx| {
                        tx.execute("UPDATE sessions SET file = ?2 WHERE id = ?1 AND file IS NULL", params![session.as_str(), path])?;
                        Ok(())
                    })?;
                }
            }
            let now = self.load()?;
            match now.status.as_str() {
                "running" => self.reload_if_installed()?,
                "merging" => return self.merge(dirs),
                other => {
                    return Err(CpdError::new(
                        ErrorCode::IllegalTransition,
                        format!("Attempt {} became {other} while its driver ran.", now.id),
                    )
                    .with("status", other));
                }
            }
            match now.stop_request.as_deref() {
                None => {}
                Some("cancelled") => return self.end(&now, End::Cancelled, dirs),
                Some("timed_out") => {
                    let detail = format!("It used its {} minutes of active time.", self.config.timeouts.attempt_minutes);
                    return self.end(&now, End::Failed { cause: Cause::TimedOut, detail }, dirs);
                }
                Some(other) => {
                    return Err(CpdError::new(ErrorCode::StoreFailed, format!("Unknown stop_request {other:?}.")));
                }
            }
            let pi = match look {
                Look::Gone(detail) => return self.end(&now, End::Failed { cause: Cause::Crashed, detail }, dirs),
                // `list-panes` failed: whether the pane is gone is decided at the next poll.
                Look::Unknown => continue,
                Look::Alive(pid) => pid,
            };

            let Some(file) = file.clone() else { continue };
            // The stall rule (§3.4 A4): pi is the pane's process, alive here. tmux
            // gives a live pane's pid; it is empty only for a dead or dying pane (hs-n102).
            if let Some(line) = pi.and_then(|pi| stall.check(&file, pi)) {
                log(&line);
            }
            // Idle is read from the Session state and confirmed from the file (A5).
            let state: String =
                self.store.read(|conn| Ok(conn.query_row("SELECT state FROM sessions WHERE id = ?1", [session.as_str()], |r| r.get(0))?))?;
            if state != "idle" {
                continue;
            }
            let Some((stop, turns)) = worker::last_stop(&file) else { continue };
            match stop.as_deref() {
                Some("stop") if !now.nudged => {
                    if self.nudge(&now, session)? {
                        nudged_at = Some(turns);
                    }
                }
                Some("stop") => {
                    if turns > *nudged_at.get_or_insert(turns) {
                        let detail = "The Worker went idle twice without submitting, once after a nudge.".to_owned();
                        return self.end(&now, End::Failed { cause: Cause::NoSubmit, detail }, dirs);
                    }
                }
                // Mid-turn, or no assistant message yet.
                Some("toolUse") | None => {}
                Some(other) => {
                    let detail = format!("pi stopped with stopReason {other}.");
                    return self.end(&now, End::Failed { cause: Cause::Crashed, detail }, dirs);
                }
            }
        }
    }

    /// A5's nudge, once: `attempt-nudge` to the Worker's Session, and
    /// `nudged`. False when the Attempt left `running` meanwhile.
    fn nudge(&mut self, target: &Target, session: &SessionId) -> Result<bool, CpdError> {
        log("A5: the Worker is idle without a submit; nudging it");
        let id = &target.id;
        let prompt = format!(
            "You ended your turn without running cpd attempt submit {id}. Finish the Spec and submit, or run cpd ask {id} \"<question>\" if you are blocked."
        );
        self.store.write(SOURCE, |tx| {
            let changed = tx.execute(
                "UPDATE attempts SET nudged = 1 WHERE plan = ?1 AND id = ?2 AND status = 'running' AND nudged = 0",
                params![target.plan.as_str(), id.to_string()],
            )?;
            if changed == 0 {
                return Ok(false);
            }
            let text = format!("The Worker of Attempt {id} ended its turn without submitting; it was nudged once.");
            events::append(
                tx,
                NewEvent::new(Kind::AttemptNudge, text, json!({"prompt": prompt}))
                    .plan(target.plan.clone())
                    .job(target.job.clone())
                    .attempt(id.clone())
                    .session(session.clone()),
            )?;
            Ok(true)
        })
    }

    /// Ends an Attempt that is not `merging`, with A9's metrics, then the rest of A9.
    /// A Worker that submitted meanwhile wins: the Attempt goes to its merge.
    fn end(&mut self, target: &Target, end: End, dirs: &PiDirs) -> Result<Ended, CpdError> {
        log(&format!("ending: {} / {}", end.outcome(), end.cause()));
        let metrics = worker::metrics(dirs, target.worker_session.as_ref());
        let ladder = Ladder { catalog: self.catalog.as_ref().expect("A1 loads the catalog"), retry_cap: self.config.retry_cap };
        let (plan, id) = (&self.plan, &self.id);
        let submitted = self.store.write(SOURCE, |tx| {
            // Re-read: the status this transaction swaps from.
            let now = ledger::load(tx, plan, id)?;
            if now.status == "merging" {
                return Ok(true);
            }
            ledger::end(tx, &now, &end, metrics.as_ref(), &ladder)?;
            Ok(false)
        })?;
        if submitted {
            log("the Worker submitted meanwhile");
            return self.merge(dirs);
        }
        self.cleanup(target, false);
        Ok(Ended { outcome: end.outcome(), cause: end.cause() })
    }

    /// A8: the merge (§7.4), then the rest of A9. An `Err` leaves the Attempt
    /// `merging`: once this driver has exited, R (§7.4) ends it, `merged` if
    /// the compare-and-swap already happened.
    fn merge(&mut self, dirs: &PiDirs) -> Result<Ended, CpdError> {
        // Engine-W1 (§7.4 step 0): the Worker's window goes first, before the wait for the
        // per-repo lock, so nothing can move the Attempt branch afterwards. A failure is
        // reported as A9's is; the merge still runs, A9 tries the kill again.
        let target = self.load()?;
        if let Err(e) = worker::kill_window(&self.config, &self.tmux, &target) {
            log(&format!("killing the Worker's window failed: {}", e.message()));
            let _ = self.store.write(SOURCE, |tx| ledger::append_error(tx, &target, &e));
        }
        // The safe point before A8's rebase.
        self.reload_if_installed()?;
        log("A8: merging");
        let metrics = |a: &merge::Attempt| -> Option<Value> { worker::metrics(dirs, a.worker_session.as_ref()) };
        let ctx = merge::Ctx {
            home: &self.home,
            config: &self.config,
            catalog: self.catalog.as_ref().expect("A1 loads the catalog"),
            source: SOURCE,
            metrics: &metrics,
        };
        let done = merge::merge(&mut self.store, &ctx, &self.plan, &self.id)?;
        let target = self.load()?;
        let merged = matches!(done.end, merge::End::Merged { .. });
        self.cleanup(&target, merged);
        Ok(Ended { outcome: done.end.outcome(), cause: done.end.cause() })
    }

    /// The rest of A9: kill the Attempt's window; on `merged`, remove the
    /// worktree and branch (§7.2); otherwise keep them for debugging; remove
    /// the driver lock file (this process keeps fd 3 until it exits). A
    /// failure here changes no state: it is reported as an `error` Event.
    fn cleanup(&mut self, target: &Target, merged: bool) {
        let mut result = worker::kill_window(&self.config, &self.tmux, target);
        if merged && result.is_ok() {
            result = git::remove_merged_attempt(&target.lead_worktree, &target.worktree, &target.branch);
        }
        let removed = lock::remove(&self.home, &self.plan, &self.id);
        result = result.and(removed);
        if let Err(e) = result {
            log(&format!("A9 failed: {}", e.message()));
            let _ = self.store.write(SOURCE, |tx| ledger::append_error(tx, target, &e));
        }
    }

    /// commands.md §6.4: an unexpected failure appends `error` and ends the
    /// Attempt `failed` / `crashed` when it is `starting` or `running`. A
    /// `merging` Attempt is left to R (§7.4), which runs once this driver is
    /// dead. Without the catalog the ladder cannot be applied, so the
    /// Attempt is left to E2, which ends it `crashed` once this driver is dead.
    fn fail(&mut self, error: &CpdError) {
        log(&format!("failed: {}: {}", error.code().as_str(), error.message()));
        let Ok(target) = self.load() else { return };
        if let Err(e) = self.store.write(SOURCE, |tx| ledger::append_error(tx, &target, error)) {
            log(&format!("cannot append the error Event: {}", e.message()));
        }
        if !matches!(target.status.as_str(), "starting" | "running") || self.catalog.is_none() {
            return;
        }
        let detail = format!("Its driver failed: {}", error.message());
        let metrics = PiDirs::from_env().ok().and_then(|dirs| worker::metrics(&dirs, target.worker_session.as_ref()));
        let ladder = Ladder { catalog: self.catalog.as_ref().expect("A1 loads the catalog"), retry_cap: self.config.retry_cap };
        let ended = self.store.write(SOURCE, |tx| {
            ledger::end(tx, &target, &End::Failed { cause: Cause::Crashed, detail: detail.clone() }, metrics.as_ref(), &ladder)
        });
        if let Err(e) = ended {
            log(&format!("cannot end the Attempt: {}", e.message()));
            return;
        }
        self.cleanup(&target, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1_takes_up_starting_running_and_merging() {
        let id = AttemptId::parse("api-a1").unwrap();
        assert_eq!(entry(&id, "starting").unwrap(), Entry::Start);
        assert_eq!(entry(&id, "running").unwrap(), Entry::Resume);
        assert_eq!(entry(&id, "merging").unwrap(), Entry::Merge);
    }

    #[test]
    fn a_zombie_or_reaped_process_is_not_alive() {
        assert!(alive(std::process::id()));
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        // Not reaped yet: a zombie once it has exited.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while alive(pid) {
            assert!(std::time::Instant::now() < deadline, "{pid} still alive");
            std::thread::sleep(Duration::from_millis(10));
        }
        child.wait().unwrap();
        assert!(!alive(pid));
    }

    #[test]
    fn a1_refuses_every_other_status() {
        let id = AttemptId::parse("api-a1").unwrap();
        for status in ["ended", "queued", "", "Running"] {
            let error = entry(&id, status).unwrap_err();
            assert_eq!(error.code(), ErrorCode::IllegalTransition, "{status}");
        }
    }
}
