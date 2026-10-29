//! The driver `cpd attempt run` (contract §3.4 A1–A9; commands.md §6.4).
//!
//! Each status change is one transaction with the Event of §3.4's table. Its
//! cancel safe points are before A3 (here) and before A8's rebase and its
//! compare-and-swap (`crate::merge`, §7.4 steps 1 and 7); while `running`, a
//! cancel reaches it as E2's `stop_request = cancelled` (§3.7, A5).

use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::clock;
use crate::attempt::ledger::{self, Attempt as Target, Cause, End, Ladder, Stage};
use super::lock;
use super::runner;
use super::stall;
use super::worker;
use super::RunArgs;
use crate::config::{Catalog, Config, WorkerRunner, load_catalog};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::git::{self, Setup};
use crate::home::{CpdHome, Hook, Infra};
use crate::host::exe::{Exe, Refusal};
use crate::host::spawn::{Pid as SpawnPid, Spawn};
use crate::host::pi::{self, PiDirs};
use crate::host::{PaneId, Tmux};
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::merge;
use crate::output::Output;
use crate::plan::jobs;
use crate::ports::Ports;
use crate::plan::locked_fingerprint;
use crate::store::{Persist, Store};

/// The `txns.command` and the `source` of the driver's `error` Events.
const SOURCE: &str = "attempt run";

/// How often A4 looks at the Attempt, its Session and its pane.
const POLL: Duration = Duration::from_millis(500);

/// What A4 knows of the Worker's pane at a poll.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Look {
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
pub(super) struct PaneWatch {
    pid: Option<u32>,
    failed: bool,
}

/// One A4 poll on `ports`: the nap of `POLL` (500 ms) on `ports.clock`, then the look at the Worker's pane
/// (`PaneWatch::look_with` on `ports.spawn`).
pub(super) fn poll_with(ports: &Ports, watch: &mut PaneWatch, tmux: &Tmux, pane: &PaneId) -> Result<Look, CpdError> {
    ports.clock.sleep(POLL);
    watch.look_with(&*ports.spawn, tmux, pane)
}

impl PaneWatch {
    /// The look at the pane: the pid test (`alive`) asks `spawn`.
    pub(super) fn look_with(&mut self, spawn: &dyn Spawn, tmux: &Tmux, pane: &PaneId) -> Result<Look, CpdError> {
        if let Some(pid) = self.pid.filter(|&pid| spawn.alive(SpawnPid(pid))) {
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

/// What A5 does at a poll of the Worker.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    /// Nothing yet: not idle, mid-turn, or nudged and not idle again.
    Wait,
    /// Idle without a submit, not nudged yet: nudge it, once.
    Nudge,
    /// Idle again after the nudge: the Attempt ends `no_submit`.
    NoSubmit,
    /// pi stopped with this `stopReason`: the Attempt ends `crashed`.
    Crashed(String),
    /// pi stopped on a provider error (the `errorMessage`): the Attempt ends `provider_error`.
    ProviderError(String),
}

/// A5's judgement, pure. `state` is the Session's state; `last_stop` the last
/// assistant message's `stopReason` and `turns` the count of assistant messages
/// (both read from the session file only when `state` is `idle`), `error_message` the last one's `errorMessage`; `nudged` is
/// the turn count when the Worker was nudged, None when it was not.
fn judge(state: &str, last_stop: Option<&str>, error_message: Option<&str>, turns: u64, nudged: Option<u64>) -> Decision {
    if state != "idle" {
        return Decision::Wait;
    }
    match (last_stop, nudged) {
        (Some("stop"), None) => Decision::Nudge,
        (Some("stop"), Some(at)) if turns > at => Decision::NoSubmit,
        // Mid-turn, no assistant message yet, or nudged and no new turn since.
        (Some("stop") | Some("toolUse") | None, _) => Decision::Wait,
        (Some("error"), _) if error_message.is_some_and(worker::is_provider_error) => {
            Decision::ProviderError(error_message.unwrap_or_default().to_owned())
        }
        (Some(other), _) => Decision::Crashed(other.to_owned()),
    }
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
    /// The Attempt's stored runner, read in A1: never today's `worker_runner` (§3.4 "The Worker runner and its seam").
    runner: Option<WorkerRunner>,
    /// Time, processes and `exec`: the real ones, or a test's.
    ports: Ports,
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

/// The Worker A4 watches: a `pi` Worker's pane, or a `durable` Worker's process.
enum Worker {
    Pane(PaneId),
    Process(SpawnPid),
}

/// What the driver does after A1: `entry` with what a `running` Attempt's
/// resume needs from the store.
enum Task {
    Start,
    /// The Worker's Session and what A4 watches.
    Resume(SessionId, Worker),
    Merge,
}

/// What one A4 poll saw of the Worker.
enum Seen {
    Pane(Look),
    /// `None` while the runner's pid is alive, else how it ended.
    Runner(Option<runner::Ended>),
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
    // This executable as the ports tell it (the real ones: the path recorded at process start), so that a test can stand
    // a file of its own for it and replace that file.
    let exe = Exe::at(Ports::current().spawn.self_exe()?)?;
    // A transient store error never ends the Attempt: every read and write of
    // this driver waits it out (§3.4), the open included.
    let opened = Env::open_home_persistent(home.clone(), Persist::new(|line| log(line), || false));
    let Env { home, config, store, ports, .. } = match opened {
        Ok(env) => env,
        Err(e) => {
            leave_infra(&home, &plan, &id, &e);
            return Err(e);
        }
    };
    let tmux = Tmux::new(config.tmux.socket.as_str());
    let mut driver = Driver { home, config, store, tmux, plan, id, lock, exe, catalog: None, runner: None, ports };

    let outcome = (|| {
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
    })();
    if let Err(e) = &outcome {
        leave_infra(&driver.home, &driver.plan, &driver.id, e);
    }
    outcome
}

/// An install or a store migration ended this driver, not its Worker (hs-n278): `error` is `lock_timeout` or
/// `store_too_new`. Leaves the marker E2 reads to end the Attempt as one whose driver never started. Call it
/// while the driver lock is still held: a free lock is what E2 acts on.
fn leave_infra(home: &CpdHome, plan: &PlanId, id: &AttemptId, error: &CpdError) {
    let Some(infra) = Infra::of(error.code()) else { return };
    match home.leave_infra(plan, id, infra, error.message()) {
        Ok(()) => log(&format!("{}: not the Worker's failure; the Job will be queued again at the same Tier", infra.code())),
        Err(e) => log(&format!("cannot leave the infrastructure marker: {}", e.message())),
    }
}

/// One line of the driver's log (`run/<plan>/logs/attempt-<id>.log`), prefixed with the time like `watch.log`'s.
pub(super) fn log(line: &str) {
    eprintln!("{} cpd attempt run: {line}", clock::now());
}

/// Appends an `error` Event about `target` (§5.2): an unexpected failure of
/// the driver (commands.md §6.4).
fn append_error(tx: &crate::store::WriteTxn<'_>, target: &Target, error: &CpdError) -> Result<(), CpdError> {
    let text = format!(
        "The driver of Attempt {} failed: {} See: cpd attempt show {} --plan {}",
        target.id,
        error.message(),
        target.id,
        target.plan
    );
    ledger::append_error(tx, target, SOURCE, error, text)
}

impl Driver {
    fn load(&mut self) -> Result<Target, CpdError> {
        let (plan, id) = (&self.plan, &self.id);
        self.store.read(|conn| ledger::load(conn, plan, id))
    }

    fn catalog(&self) -> &Catalog {
        self.catalog.as_ref().expect("A1 loads the catalog")
    }

    fn runner(&self) -> WorkerRunner {
        self.runner.expect("A1 reads the Attempt's runner")
    }

    /// A9's `metrics` of the Attempt's Worker, by its runner.
    fn metrics(&self, dirs: &PiDirs, session: Option<&SessionId>) -> Option<Value> {
        runner::metrics(self.runner(), dirs, session)
    }

    /// A9 and W1: stops the Worker by the Attempt's stored runner (the window of `pi`; SIGTERM, then SIGKILL, for `durable`).
    fn stop_worker(&mut self) -> Result<(), CpdError> {
        let runner = self.runner();
        let pid = match runner {
            WorkerRunner::Pi => None,
            WorkerRunner::Durable => {
                let (plan, id) = (&self.plan, &self.id);
                self.store.read(|conn| runner::stored(conn, plan, id))?.1
            }
        };
        runner::stop(&self.ports, &self.config, &self.home, &self.tmux, &self.plan, &self.id, (runner, pid))
    }

    /// A1 (after the lock) to A9, from `entry`.
    fn drive(&mut self, task: Task, target: Target) -> Result<Ended, CpdError> {
        let job = self.begin(&target)?;
        let dirs = PiDirs::from_env()?;
        match task {
            Task::Start => self.start(&target, &job, &dirs),
            Task::Resume(session, worker) => {
                log("A1: resuming at A4 after a reload");
                self.wait(&session, worker, &dirs)
            }
            Task::Merge => self.merge(&dirs),
        }
    }

    /// A1: the catalog, the Job, its fingerprint.
    fn begin(&mut self, target: &Target) -> Result<jobs::Job, CpdError> {
        let (plan, id) = (&self.plan, &self.id);
        self.runner = Some(self.store.read(|conn| runner::stored(conn, plan, id))?.0);
        let models = self.store.read(|conn| crate::policy::model::models_of_job(conn, &self.home, &target.plan, target.job.as_str()))?;
        self.catalog = Some(load_catalog(&self.config, &models)?);
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
        // A2: an `open` Job's PR stacked on a merged base PR is unstacked first, so the Worker is told a base that exists (§3.4).
        if job.kind == "open" {
            let pr = crate::ids::PrKey::parse(&job.pr).map_err(|_| CpdError::bad_row(format!("Job {} has the PR key {:?}", job.key, job.pr)))?;
            crate::pr::start::unstack_for_open_job(&mut self.store, SOURCE, &self.plan, &pr)?;
        }
        // A2 (hs-d680): a linear Attempt reuses the PR's workflow worktree as it is; a parallel sibling creates
        // its own worktree from the Lead Branch HEAD and runs pre-start. A linear Attempt never runs pre-start.
        let linear = git::worktree::is_linear(&target.branch, &target.lead_branch);
        log(if linear { "A2: reusing the workflow worktree on the Lead Branch" } else { "A2: creating the worktree and running pre-start" });
        let setup = Setup {
            timeout: Duration::from_secs(u64::from(self.config.timeouts.setup_minutes) * 60),
            log: self.home.hook_log(&self.plan, &self.id, Hook::PreStart),
        };
        let policy = match self.store.read(|conn| crate::pr::policy_of_job(conn, &self.home, &self.plan, &job.key)) {
            Ok(policy) => policy,
            Err(e) => {
                let detail = format!("Reading the policy of the PR failed: {}", e.message());
                return self.end(target, End::Failed { cause: Cause::SetupFailed, detail }, dirs);
            }
        };
        let base_sha = if linear {
            match verify_linear_worktree(target) {
                Ok(sha) => sha,
                Err(e) => {
                    let detail = format!("The workflow worktree is not usable: {}", e.message());
                    return self.end(target, End::Failed { cause: Cause::SetupFailed, detail }, dirs);
                }
            }
        } else {
            let created = match git::create_attempt_worktree_with(&target.repo, &target.lead_worktree, &self.plan, &self.id, &setup, &policy.hooks.pre_start) {
                Ok(created) => created,
                Err(e) => {
                    let detail = format!("Setting up the worktree failed: {}", e.message());
                    return self.end(target, End::Failed { cause: Cause::SetupFailed, detail }, dirs);
                }
            };
            created.base_sha
        };
        let (plan, id) = (&self.plan, &self.id);
        self.store.write(SOURCE, |tx| {
            tx.execute(
                "UPDATE attempts SET base_sha = ?3 WHERE plan = ?1 AND id = ?2 AND status = 'starting'",
                params![plan.as_str(), id.to_string(), base_sha],
            )?;
            Ok(())
        })?;

        // The safe point before A3.
        if self.store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
            log("cancel requested before A3");
            return self.end(target, End::Cancelled, dirs);
        }

        // A3: the Spec, the Session, the Worker.
        log("A3: rendering the Spec and starting the Worker");
        let home = &self.home;
        let spec = self.store.read(|conn| worker::render_spec(conn, home, target, job))?;
        let session = SessionId::new_v4();
        let worker = match self.runner() {
            WorkerRunner::Pi => {
                self.register(target, &session, &spec)?;
                let pane = worker::start(&self.home, &self.config, self.catalog(), &self.tmux, dirs, target, &session)?;
                self.running(target, &session, &pane)?;
                Worker::Pane(pane.pane)
            }
            WorkerRunner::Durable => {
                // The runner file is checked before anything is registered or spawned; a failed spawn ends `setup_failed` too.
                let launched = runner::launch(&self.home, &self.config, self.catalog(), target, &session);
                let launch = match launched {
                    Ok(launch) => launch,
                    Err(e) => return self.end(target, End::Failed { cause: Cause::SetupFailed, detail: e.message().to_owned() }, dirs),
                };
                self.register(target, &session, &spec)?;
                match runner::spawn(&self.ports, &launch) {
                    Ok(pid) => {
                        self.running_durable(target, pid)?;
                        Worker::Process(pid)
                    }
                    Err(e) => return self.end(target, End::Failed { cause: Cause::SetupFailed, detail: e.message().to_owned() }, dirs),
                }
            }
        };

        // A4 and A5.
        self.wait(&session, worker, dirs)
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
                let (plan, id) = (&self.plan, &self.id);
                // By the Attempt's stored runner, never today's flag.
                let (stored, pid) = self.store.read(|conn| runner::stored(conn, plan, id))?;
                let worker = match stored {
                    WorkerRunner::Pi => {
                        let pane: Option<String> = self.store.read(|conn| {
                            Ok(conn.query_row("SELECT tmux_pane FROM sessions WHERE id = ?1", [session.as_str()], |r| r.get(0))?)
                        })?;
                        Worker::Pane(pane.as_deref().and_then(PaneId::parse).ok_or_else(|| nothing("pane"))?)
                    }
                    WorkerRunner::Durable => Worker::Process(pid.ok_or_else(|| nothing("process"))?),
                };
                Ok(Task::Resume(session, worker))
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
        // The argv this driver was started with (`start` makes it so), from its own fields: a driver run in a test has no process argv.
        let id = self.id.to_string();
        let args = ["attempt", "run", id.as_str(), "--plan", self.plan.as_str()];
        let failed = self.exe.exec_with(&*self.ports.spawn, args);
        // Still here: the `exec` failed. Do not leak the lock to the children.
        let _ = self.lock.close_on_exec();
        Err(failed)
    }

    /// A3's first transaction: the Worker's Session, registered before pi
    /// starts (§3.6), and the Attempt's `worker_session` and `spec_path`.
    fn register(&mut self, target: &Target, session: &SessionId, spec: &std::path::Path) -> Result<(), CpdError> {
        let name = format!("{} {} a{}", target.plan, target.job, target.id.number());
        let worktree = target.worktree.to_string_lossy().into_owned();
        let tier = target.tier.to_string();
        self.store.write(SOURCE, |tx| {
            tx.execute(
                "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                                       state, state_label, state_at, created_at)
                 SELECT ?1, 'worker', ?2, ?3, ?4, ?5, NULL, NULL, ?6, ?7, NULL, 'stopped', NULL, at, at
                   FROM txns WHERE id = cpd_txn()",
                params![
                    session.as_str(),
                    target.context.as_str(),
                    target.plan.as_str(),
                    target.id.to_string(),
                    name,
                    worktree,
                    tier
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
            events::append(
                tx,
                NewEvent::session_registered(text, "worker", &name, None::<&str>, &tier)
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
                NewEvent::attempt_status(text, "starting", "running")
                    .plan(target.plan.clone())
                    .job(target.job.clone())
                    .attempt(target.id.clone()),
            )?;
            let text = format!("Worker of Attempt {} is in its work window.", target.id);
            events::append(
                tx,
                NewEvent::session_moved(text, Some(pane.pane.as_str()), Some(pane.window.as_str()), "work")
                    .context(target.context.clone())
                    .plan(target.plan.clone())
                    .session(session.clone()),
            )?;
            Ok(())
        })
    }

    /// A3's second transaction for a `durable` Worker: `starting → running` and the runner's `worker_pid` together. The
    /// Session has no pane, so no `session-moved` Event.
    fn running_durable(&mut self, target: &Target, pid: SpawnPid) -> Result<(), CpdError> {
        self.store.write(SOURCE, |tx| {
            let changed = tx.execute(
                "UPDATE attempts SET status = 'running', worker_pid = ?3 WHERE plan = ?1 AND id = ?2 AND status = 'starting'",
                params![target.plan.as_str(), target.id.to_string(), pid.0],
            )?;
            if changed == 0 {
                return Err(CpdError::new(ErrorCode::CasLost, format!("Attempt {} is no longer starting.", target.id)));
            }
            let text = format!(
                "Attempt {} is running: its Worker started. See: cpd attempt show {} --plan {}",
                target.id, target.id, target.plan
            );
            events::append(
                tx,
                NewEvent::attempt_status(text, "starting", "running")
                    .plan(target.plan.clone())
                    .job(target.job.clone())
                    .attempt(target.id.clone()),
            )?;
            Ok(())
        })
    }

    /// A4's restart of the `durable` runner: the same launch again, in place, and the new `worker_pid` stored. `Err` is the
    /// detail the Attempt ends `crashed` with.
    fn restart_runner(&mut self, target: &Target, session: &SessionId) -> Result<SpawnPid, String> {
        let launch = runner::launch(&self.home, &self.config, self.catalog(), target, session).map_err(|e| e.message().to_owned())?;
        let pid = runner::spawn(&self.ports, &launch).map_err(|e| e.message().to_owned())?;
        let stored = self.store.write(SOURCE, |tx| {
            tx.execute(
                "UPDATE attempts SET worker_pid = ?3 WHERE plan = ?1 AND id = ?2 AND status = 'running'",
                params![target.plan.as_str(), target.id.to_string(), pid.0],
            )?;
            Ok(())
        });
        stored.map_err(|e| format!("Storing the new worker_pid failed: {}", e.message()))?;
        Ok(pid)
    }

    /// A4 and A5: waits until the Attempt is submitted (A8), stopped, or its
    /// Worker is gone or idle for good.
    fn wait(&mut self, session: &SessionId, mut worker: Worker, dirs: &PiDirs) -> Result<Ended, CpdError> {
        log("A4: waiting for the Worker");
        // The Worker's session file, recorded once pi has created it.
        let mut file: Option<std::path::PathBuf> = None;
        // The Worker's assistant messages when it was nudged: after a reload, from the Event.
        let mut nudged_at: Option<u64> = self.nudge_baseline(session)?;
        let mut stall = stall::Watch::new(self.config.timeouts.stall_seconds);
        let mut watch = PaneWatch::default();
        // The restarts of a `durable` runner in this driver run: a reloaded driver counts from 0 again.
        let mut restarts = 0;
        loop {
            // The Worker before the row: a Worker that submits and exits is seen `merging`.
            let seen = match &worker {
                Worker::Pane(pane) => Seen::Pane(poll_with(&self.ports, &mut watch, &self.tmux, pane)?),
                Worker::Process(pid) => {
                    self.ports.clock.sleep(POLL);
                    Seen::Runner(runner::look(&*self.ports.spawn, *pid))
                }
            };
            if matches!(seen, Seen::Pane(_)) && file.is_none() {
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
                Some("cancelled") => {
                    // A real cancel (the Job has `cancel_requested_at`) ends `Cancelled`; incident mode's stop (no
                    // cancel request) ends `Interrupted`, which requeues the Job at the same Tier (contract §3.7).
                    let cancelled = self.store.read(|conn| ledger::cancel_request(conn, &now.plan, &now.job))?.is_some();
                    let end = if cancelled { End::Cancelled } else { End::Interrupted };
                    return self.end(&now, end, dirs);
                }
                Some("timed_out") => {
                    let detail = format!("It used its {} minutes of active time.", self.config.timeouts.attempt_minutes);
                    return self.end(&now, End::Failed { cause: Cause::TimedOut, detail }, dirs);
                }
                Some(other) => {
                    return Err(CpdError::new(ErrorCode::StoreFailed, format!("Unknown stop_request {other:?}.")));
                }
            }
            let look = match seen {
                Seen::Pane(look) => look,
                // A `durable` runner that is alive: not judged yet (A5 for it reads its end report, which is not here), and
                // the stall rule does not run for it.
                Seen::Runner(None) => continue,
                Seen::Runner(Some(ended)) => {
                    let log_file = self.home.worker_log(&now.plan, &now.id);
                    let detail = match ended {
                        runner::Ended::Usage => Some(format!(
                            "The durable runner exited with status 2, a usage error; its output is in {}.",
                            log_file.display()
                        )),
                        runner::Ended::Other if restarts >= runner::MAX_RESTARTS => Some(format!(
                            "The durable runner ended {} times in one driver run, after {restarts} restarts; its output is in {}.",
                            restarts + 1,
                            log_file.display()
                        )),
                        runner::Ended::Other => None,
                    };
                    if let Some(detail) = detail {
                        return self.end(&now, End::Failed { cause: Cause::Crashed, detail }, dirs);
                    }
                    restarts += 1;
                    log(&format!("A4: the durable runner ended; restart {restarts} of {}", runner::MAX_RESTARTS));
                    match self.restart_runner(&now, session) {
                        Ok(pid) => worker = Worker::Process(pid),
                        Err(detail) => return self.end(&now, End::Failed { cause: Cause::Crashed, detail }, dirs),
                    }
                    continue;
                }
            };
            let pi = match look {
                Look::Gone(detail) => return self.end(&now, End::Failed { cause: Cause::Crashed, detail }, dirs),
                // `list-panes` failed: whether the pane is gone is decided at the next poll.
                Look::Unknown => continue,
                Look::Alive(pid) => pid,
            };

            let Some(file) = file.clone() else { continue };
            // The stall rule (§3.4 A4): pi is the pane's process, alive here. tmux
            // gives a live pane's pid; it is empty only for a dead or dying pane (hs-n102).
            // Not while the Worker's `attempt submit` holds a check span (§7.7): it is silent for as long as its Gate runs.
            let span_open = lock::check_span_open(&self.home, &self.plan, &self.id)?;
            let descendants = |pi: u32| self.ports.spawn.descendants(SpawnPid(pi)).into_iter().map(|p| p.0).collect();
            if let Some(line) = pi.and_then(|pi| stall.check_with(&self.ports, &descendants, &file, pi, span_open)) {
                log(&line);
            }
            // Idle is read from the Session state and confirmed from the file (A5).
            let state: String =
                self.store.read(|conn| Ok(conn.query_row("SELECT state FROM sessions WHERE id = ?1", [session.as_str()], |r| r.get(0))?))?;
            let (stop, turns, error) = if state == "idle" {
                let Some((stop, turns)) = worker::last_stop(&file) else { continue };
                let error = if stop.as_deref() == Some("error") { worker::last_error(&file) } else { None };
                (stop, turns, error)
            } else {
                (None, 0, None)
            };
            // An Attempt nudged by a driver that left (a reload) has its baseline in the Event;
            // one nudged before the Event carried it takes the first idle count seen.
            if state == "idle" && now.nudged {
                nudged_at.get_or_insert(turns);
            }
            match judge(&state, stop.as_deref(), error.as_deref(), turns, nudged_at.filter(|_| now.nudged)) {
                Decision::Wait => {}
                Decision::Nudge => {
                    if self.nudge(&now, session, turns)? {
                        nudged_at = Some(turns);
                    }
                }
                Decision::NoSubmit => {
                    let detail = "The Worker went idle twice without submitting, once after a nudge.".to_owned();
                    return self.end(&now, End::Failed { cause: Cause::NoSubmit, detail }, dirs);
                }
                Decision::ProviderError(message) => {
                    let detail = format!("pi stopped on a provider error: \"{message}\".");
                    return self.end(&now, End::Failed { cause: Cause::ProviderError, detail }, dirs);
                }
                Decision::Crashed(reason) => {
                    let detail = format!("pi stopped with stopReason {reason}.");
                    return self.end(&now, End::Failed { cause: Cause::Crashed, detail }, dirs);
                }
            }
        }
    }

    /// The Worker's assistant-turn count at its nudge, from the Attempt's
    /// `attempt-nudge` Event: a resumed driver's baseline (§3.4 A5, §5.2).
    fn nudge_baseline(&mut self, session: &SessionId) -> Result<Option<u64>, CpdError> {
        let id = &self.id;
        self.store.read(|conn| {
            let turns: Option<i64> = conn
                .query_row(
                    "SELECT json_extract(data, '$.turns') FROM events
                      WHERE kind = 'attempt-nudge' AND plan = ?1 AND attempt = ?2 AND session = ?3
                      ORDER BY id DESC LIMIT 1",
                    params![self.plan.as_str(), id.to_string(), session.as_str()],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            Ok(turns.and_then(|t| u64::try_from(t).ok()))
        })
    }

    /// A5's nudge, once: `attempt-nudge` to the Worker's Session, and
    /// `nudged`. False when the Attempt left `running` meanwhile.
    fn nudge(&mut self, target: &Target, session: &SessionId, turns: u64) -> Result<bool, CpdError> {
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
                NewEvent::attempt_nudge(text, &prompt, turns)
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
        let metrics = self.metrics(dirs, target.worker_session.as_ref());
        let ladder = Ladder { catalog: self.catalog.as_ref().expect("A1 loads the catalog"), retry_cap: self.config.retry_cap };
        let (plan, id) = (&self.plan, &self.id);
        let submitted = self.store.write(SOURCE, |tx| {
            // Re-read: the status this transaction swaps from.
            let now = ledger::load(tx, plan, id)?;
            if now.status == "merging" {
                return Ok(true);
            }
            ledger::end(tx, &now, &end, metrics.as_ref(), &ladder, Stage::Driver)?;
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
        // Engine-W1 (§7.4 step 0): the Worker's window goes first, before the merge's
        // steps, so nothing can move the Attempt branch afterwards. A failure is
        // reported as A9's is; the merge still runs, A9 tries the kill again.
        let target = self.load()?;
        if let Err(e) = self.stop_worker() {
            log(&format!("stopping the Worker failed: {}", e.message()));
            let _ = self.store.write(SOURCE, |tx| append_error(tx, &target, &e));
        }
        // The safe point before A8's rebase.
        self.reload_if_installed()?;
        log("A8: merging");
        let runner = self.runner();
        let metrics = |a: &merge::Attempt| -> Option<Value> { runner::metrics(runner, dirs, a.worker_session.as_ref()) };
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
        let linear = git::worktree::is_linear(&target.branch, &target.lead_branch);
        let mut result = self.stop_worker();
        if linear {
            // A9 (hs-d680): a merged linear Attempt keeps the workflow worktree and the Lead Branch. A failed or
            // cancelled one resets the Lead Branch to its `base_sha` and discards what the Attempt left, then keeps
            // the worktree.
            if !merged {
                match self.linear_base_sha() {
                    Ok(Some(base_sha)) => {
                        if let Err(e) = reset_linear_lead_branch(&target.worktree, &target.lead_branch, &base_sha) {
                            result = result.and(Err(e));
                        }
                    }
                    Ok(None) => {}
                    Err(e) => result = result.and(Err(e)),
                }
            }
        } else if merged && result.is_ok() {
            result = git::remove_merged_attempt(&target.lead_worktree, &target.worktree, &target.branch);
        }
        let removed = lock::remove(&self.home, &self.plan, &self.id);
        result = result.and(removed);
        if let Err(e) = result {
            log(&format!("A9 failed: {}", e.message()));
            let _ = self.store.write(SOURCE, |tx| append_error(tx, target, &e));
        }
    }

    /// The `base_sha` the Attempt recorded at A2, for the A9 reset of a failed or cancelled linear Attempt.
    fn linear_base_sha(&mut self) -> Result<Option<String>, CpdError> {
        let (plan, id) = (&self.plan, &self.id);
        self.store.read(|conn| {
            Ok(conn.query_row(
                "SELECT base_sha FROM attempts WHERE plan = ?1 AND id = ?2",
                params![plan.as_str(), id.to_string()],
                |r| r.get::<_, Option<String>>(0),
            )?)
        })
    }

    /// commands.md §6.4: an unexpected failure appends `error` and ends the
    /// Attempt `failed` / `crashed` when it is `starting` or `running`. A
    /// `merging` Attempt is left to R (§7.4), which runs once this driver is
    /// dead. Without the catalog the ladder cannot be applied, so the
    /// Attempt is left to E2, which ends it `crashed` once this driver is dead. So is one that failed on an
    /// install or a store migration ([`Infra`]), which E2 ends uncounted.
    fn fail(&mut self, error: &CpdError) {
        log(&format!("failed: {}: {}", error.code().as_str(), error.message()));
        let Ok(target) = self.load() else { return };
        if let Err(e) = self.store.write(SOURCE, |tx| append_error(tx, &target, error)) {
            log(&format!("cannot append the error Event: {}", e.message()));
        }
        // An install or a store migration, not the Worker (hs-n278): the marker is left; E2 ends the Attempt
        // at the same Tier and uncounted, as one whose driver never started.
        if Infra::of(error.code()).is_some() {
            return;
        }
        if !matches!(target.status.as_str(), "starting" | "running") || self.catalog.is_none() {
            return;
        }
        let detail = format!("Its driver failed: {}", error.message());
        let metrics = PiDirs::from_env().ok().and_then(|dirs| self.metrics(&dirs, target.worker_session.as_ref()));
        let ladder = Ladder { catalog: self.catalog.as_ref().expect("A1 loads the catalog"), retry_cap: self.config.retry_cap };
        let ended = self.store.write(SOURCE, |tx| {
            ledger::end(tx, &target, &End::Failed { cause: Cause::Crashed, detail: detail.clone() }, metrics.as_ref(), &ladder, Stage::Driver)
        });
        if let Err(e) = ended {
            log(&format!("cannot end the Attempt: {}", e.message()));
            return;
        }
        self.cleanup(&target, false);
    }
}

/// A2 for a linear Attempt (hs-d680): its workflow worktree must exist, be on the Lead Branch and be clean; the
/// Lead Branch tip becomes its `base_sha`. No worktree is created and no `pre-start` runs (A2).
fn verify_linear_worktree(target: &Target) -> Result<String, CpdError> {
    let worktree = &target.worktree;
    if !worktree.exists() {
        return Err(CpdError::new(ErrorCode::SetupFailed, format!("the workflow worktree {} does not exist", worktree.display())));
    }
    match git::run::current_branch(worktree)? {
        Some(branch) if branch == target.lead_branch => {}
        Some(branch) => {
            return Err(CpdError::new(
                ErrorCode::SetupFailed,
                format!("the workflow worktree {} is on {branch}, not the Lead Branch {}", worktree.display(), target.lead_branch),
            ))
        }
        None => return Err(CpdError::new(ErrorCode::SetupFailed, format!("the workflow worktree {} has a detached HEAD", worktree.display()))),
    }
    let status = git::run::ok(worktree, &["--no-optional-locks", "status", "--porcelain"])?;
    if !status.trim().is_empty() {
        return Err(CpdError::new(ErrorCode::SetupFailed, format!("the workflow worktree {} is not clean", worktree.display())).with("status", status.trim()));
    }
    git::run::rev_parse(worktree, &format!("refs/heads/{}", target.lead_branch))
}

/// A9 for a linear Attempt (hs-d680/hs-d682/hs-d683): its branch is the PR's Lead Branch, so a
/// failed or cancelled Attempt puts the Lead Branch back to the `base_sha` it started from (`git -C <workflow
/// worktree> reset --hard <base_sha>`), discarding what the Attempt left, and the workflow worktree is kept.
pub(super) fn reset_linear_lead_branch(
    lead_worktree: &std::path::Path,
    lead_branch: &str,
    base_sha: &str,
) -> Result<(), CpdError> {
    match git::run::current_branch(lead_worktree)? {
        Some(branch) if branch == lead_branch => {}
        other => {
            return Err(CpdError::new(
                ErrorCode::SetupFailed,
                format!("Cannot reset the Lead Branch: the workflow worktree {} is on {other:?}, not {lead_branch}.", lead_worktree.display()),
            ))
        }
    }
    // One call moves the Lead Branch and discards the Attempt's working-tree changes.
    git::run::ok(lead_worktree, &["reset", "--hard", base_sha])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::ports_tests::process;
    use crate::host::spawn::FakeSpawn;

    #[test]
    fn a1_takes_up_starting_running_and_merging() {
        let id = AttemptId::parse("api-a1").unwrap();
        assert_eq!(entry(&id, "starting").unwrap(), Entry::Start);
        assert_eq!(entry(&id, "running").unwrap(), Entry::Resume);
        assert_eq!(entry(&id, "merging").unwrap(), Entry::Merge);
    }

    #[test]
    fn a_zombie_or_reaped_process_is_not_alive() {
        // The look at a pane asks `Spawn::alive`; the fake's process that ended is the zombie, then the reaped one.
        let spawn = FakeSpawn::new();
        let pid = process(&spawn);
        assert!(spawn.alive(pid));
        // Not reaped yet: a zombie once it has exited.
        spawn.exit(pid, 0);
        assert!(!spawn.alive(pid));
        assert_eq!(spawn.try_wait(pid).unwrap().map(|s| s.success()), Some(true), "waited for: reaped");
        assert!(!spawn.alive(pid));
        assert!(!spawn.alive(SpawnPid(4_000_000)), "a pid nobody started");
    }

    #[test]
    fn a5_judges_the_worker_by_state_stop_reason_turns_and_nudge() {
        use Decision::*;
        let table: &[(&str, Option<&str>, Option<&str>, u64, Option<u64>, Decision)] = &[
            ("working", Some("stop"), None, 3, None, Wait),
            ("working", Some("stop"), None, 5, Some(3), Wait),
            ("working", Some("error"), None, 3, None, Wait),
            ("stopped", Some("stop"), None, 3, None, Wait),
            ("idle", None, None, 0, None, Wait),
            ("idle", Some("toolUse"), None, 3, None, Wait),
            ("idle", Some("toolUse"), None, 5, Some(3), Wait),
            ("idle", Some("stop"), None, 3, None, Nudge),
            ("idle", Some("stop"), None, 0, None, Nudge),
            ("idle", Some("stop"), None, 3, Some(3), Wait),
            ("idle", Some("stop"), None, 2, Some(3), Wait),
            ("idle", Some("stop"), None, 4, Some(3), NoSubmit),
            ("idle", Some("error"), None, 3, None, Crashed("error".to_owned())),
            ("idle", Some("aborted"), None, 3, Some(3), Crashed("aborted".to_owned())),
            ("idle", Some("error"), Some("Connection error."), 3, None, ProviderError("Connection error.".to_owned())),
            ("idle", Some("error"), Some("503 no available server\n"), 3, Some(3), ProviderError("503 no available server\n".to_owned())),
            ("idle", Some("error"), Some("404 404 page not found"), 3, None, Crashed("error".to_owned())),
            ("idle", Some("aborted"), Some("Connection error."), 3, None, Crashed("aborted".to_owned())),
        ];
        for (state, stop, error, turns, nudged, want) in table {
            assert_eq!(judge(state, *stop, *error, *turns, *nudged), *want, "{state} {stop:?} {error:?} turns {turns} nudged {nudged:?}");
        }
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
