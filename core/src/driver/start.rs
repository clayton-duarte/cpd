//! The starter (contract §3.4 "The driver lock"; commands.md §5.5): what
//! `job dispatch` and engine E3 share. It takes the driver lock, inserts the
//! `starting` Attempt with its Tier (§9), and spawns the driver `cpd attempt
//! run` detached, handing it the lock as fd 3.

use std::num::NonZeroU32;
use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, params};

use crate::attempt::ledger::{self, Cause, End, Ladder, Stage};
use super::lock::{RESOURCE_FDS_ENV, Starter};
use crate::resource::{self, Held, Holder, Take};
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

/// The lowest descriptor number a resource lock is handed to the driver on.
const HAND_OVER_MIN: i32 = 100;
use crate::config::{Catalog, Config, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, NewEvent};
use crate::git;
use crate::home::CpdHome;
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;
use crate::host::spawn::Launch;
use crate::ports::Ports;
use crate::store::Store;

/// What the starter needs from its caller.
pub struct StartCtx<'a> {
    pub home: &'a CpdHome,
    /// `concurrency` and `retry_cap`.
    pub config: &'a Config,
    /// pi's catalog: the ladder's next Tier (§9).
    pub catalog: &'a Catalog,
    /// The caller, as `txns.command`: `job dispatch` or `watch`.
    pub command: &'a str,
}

/// How many Attempts may be not `ended` for the Attempt about to start: `concurrency`,
/// and one more for an urgent Job (§3.7 E3).
pub fn slot_limit(concurrency: u32, urgent: bool) -> i64 {
    i64::from(concurrency) + i64::from(urgent)
}

/// How an Attempt came to be (§2.2 Attempt `origin`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Created by the engine or `job dispatch` from the Job's Tier or the ladder.
    Ladder,
    /// The first Attempt after `job retry`, at the Tier the Lead chose.
    Retry,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Ladder => "ladder",
            Origin::Retry => "retry",
        }
    }
}

/// A started Attempt: its driver runs and holds the lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub attempt: AttemptId,
    pub tier: Tier,
    pub origin: Origin,
    /// The driver's pid.
    pub pid: u32,
}

/// Whether the PR has a live (not `ended`) linear Attempt other than `except` (pro-d52): its branch is the PR's Lead
/// Branch. The one place that derives it, for the starter and E3.
pub fn pr_has_live_linear(conn: &Connection, plan: &PlanId, pr: &str, except: &str) -> Result<bool, CpdError> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
           JOIN prs ON prs.plan = j.plan AND prs.key = j.pr
          WHERE a.plan = ?1 AND j.pr = ?2 AND a.status <> 'ended' AND a.id <> ?3 AND a.branch = prs.lead_branch",
        params![plan.as_str(), pr, except],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// How an Attempt of a PR is allocated (hs-d680/hs-d682/hs-d683): a **linear** Attempt reuses the PR's workflow worktree on
/// its Lead Branch; a **parallel** one gets its own worktree and branch. E3 derives it and passes it to the starter, which
/// enforces it atomically, refusing any start next to a live linear Attempt of the PR. `None` means the caller passed no allocation: the starter derives it from the PR's live Attempts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Allocation {
    /// The workflow worktree on the PR's Lead Branch.
    Linear,
    /// Its own `cpd/<plan>--<attempt>` worktree and branch.
    Parallel,
}

/// The Attempt the starter is about to insert, decided from one read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Next {
    attempt: AttemptId,
    tier: Tier,
    origin: Origin,
    branch: String,
    worktree: PathBuf,
}

/// Starts the next Attempt of Job `key` (§3.4, commands.md §5.5). Refusals,
/// in order: `plan_not_found`, `job_not_found`, `job_not_ready`,
/// `locked_job_tampered`, `cas_lost` (the driver lock is held, or the Job
/// changed meanwhile), `no_slot`. When the spawn fails the Attempt is ended
/// `failed` / `crashed` and the result is `spawn_failed`.
pub fn start(store: &mut Store, ctx: &StartCtx<'_>, plan: &PlanId, key: &JobKey) -> Result<Started, CpdError> {
    start_with_allocation(&Ports::current(), store, ctx, plan, key, None)
}

/// [`start`] on `ports`: the driver is started through `ports.spawn` (`Spawn::spawn`, which a `FakeSpawn` only records),
/// from `ports.spawn.self_exe()`.
pub fn start_with(ports: &Ports, store: &mut Store, ctx: &StartCtx<'_>, plan: &PlanId, key: &JobKey) -> Result<Started, CpdError> {
    start_with_allocation(ports, store, ctx, plan, key, None)
}

/// [`start_with`] with the linear vs parallel allocation E3 derived (`None`: derive it here from the PR's live Attempts),
/// which the starter enforces atomically in its write transaction.
pub fn start_with_allocation(
    ports: &Ports,
    store: &mut Store,
    ctx: &StartCtx<'_>,
    plan: &PlanId,
    key: &JobKey,
    allocation: Option<Allocation>,
) -> Result<Started, CpdError> {
    let next = store.read(|conn| decide(conn, ctx.catalog, plan, key, allocation))?;
    // A name the repo policy no longer declares refuses the start before any lock is taken (§3.4 "Resources of a Job").
    let wanted = store.read(|conn| {
        let wanted = resource::wanted(conn, plan, key.as_str())?;
        if let Some(w) = &wanted {
            crate::job::add::check_resources_declared(conn, ctx.home, plan, &w.pr, key.as_str(), Some(&w.names))?;
        }
        Ok(wanted)
    })?;
    // §3.4 step 1: the lock before the transaction that inserts the Attempt.
    let lock = Starter::take(ctx.home, plan, &next.attempt)?;
    // Then the Job's resources, all or none, before the Attempt row exists. Dropping `held` on any later failure releases them.
    let held = match &wanted {
        None => None,
        Some(w) => {
            let holder = Holder::Attempt {
                plan: plan.to_string(),
                job: key.to_string(),
                attempt: next.attempt.to_string(),
                at: crate::clock::timestamp(ports.clock.utc()),
            };
            let names: Vec<&str> = w.names.iter().map(String::as_str).collect();
            match resource::take(ctx.home, &w.owner, &w.repo, &names, &holder)? {
                Take::Held(held) => Some(held),
                Take::Busy { resource, holder } => {
                    // Nothing changes: the driver lock file made for an Attempt that will not exist goes too.
                    drop(lock);
                    super::lock::remove(ctx.home, plan, &next.attempt)?;
                    return Err(CpdError::new(
                        ErrorCode::ResourceBusy,
                        format!("Job {key} cannot start: the resource {resource} of {}/{} is held by another holder.", w.owner, w.repo),
                    )
                    .with("job", key.as_str())
                    .with("resource", resource)
                    .with("holder", serde_json::to_value(holder).expect("a holder serialises")));
                }
            }
        }
    };
    store.write(ctx.command, |tx| {
        if decide(tx, ctx.catalog, plan, key, allocation)? != next {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Job {key} changed while its Attempt was being started: try again."))
                .with("plan", plan.as_str())
                .with("job", key.as_str()));
        }
        let active: i64 = tx.query_row("SELECT count(*) FROM attempts WHERE status <> 'ended'", [], |r| r.get(0))?;
        // An urgent Job may start one Attempt beyond `concurrency` (§3.7 E3).
        let urgent: bool = tx.query_row("SELECT urgent FROM jobs WHERE plan = ?1 AND key = ?2", [plan.as_str(), key.as_str()], |r| r.get(0))?;
        let limit = slot_limit(ctx.config.concurrency, urgent);
        if active >= limit {
            return Err(CpdError::new(
                ErrorCode::NoSlot,
                format!("{active} Attempts are running, the limit is {limit} (concurrency {}): wait for one to end.", ctx.config.concurrency),
            )
            .with("active", active)
            .with("concurrency", ctx.config.concurrency)
            .with("urgent", urgent));
        }
        insert(tx, plan, key, &next)
    })?;

    // §3.4 step 2: spawn the driver with the lock as fd 3.
    let spawned = spawn(ports, ctx.home, plan, &next.attempt, &lock, held);
    let pid = match spawned {
        Ok(pid) => pid,
        Err(e) => {
            // As E2 would: the Attempt ends crashed; then the descriptor closes.
            let error = CpdError::new(ErrorCode::SpawnFailed, format!("Cannot start the driver of Attempt {}: {e}.", next.attempt))
                .with("plan", plan.as_str())
                .with("attempt", next.attempt.to_string());
            let detail = format!("Its driver could not be started: {e}.");
            let ladder = Ladder { catalog: ctx.catalog, retry_cap: ctx.config.retry_cap };
            store.write(ctx.command, |tx| {
                let target = ledger::load(tx, plan, &next.attempt)?;
                ledger::end(tx, &target, &End::Failed { cause: Cause::Crashed, detail: detail.clone() }, None, &ladder, Stage::Driver)
            })?;
            drop(lock);
            return Err(error);
        }
    };
    // §3.4 step 3: close our copy without unlocking; the driver holds it now.
    drop(lock);
    // A data write, no Event. The driver may already have ended the Attempt.
    store.write(ctx.command, |tx| {
        tx.execute(
            "UPDATE attempts SET driver_pid = ?3 WHERE plan = ?1 AND id = ?2 AND status <> 'ended'",
            params![plan.as_str(), next.attempt.to_string(), pid],
        )?;
        Ok(())
    })?;
    Ok(Started { attempt: next.attempt, tier: next.tier, origin: next.origin, pid })
}

/// Decides the next Attempt of Job `key`, refusing a Job that is not Ready or
/// does not match its fingerprint. `allocation` is E3's derived linear vs parallel choice (`None`: derive it from the
/// PR's live Attempts).
fn decide(conn: &Connection, catalog: &Catalog, plan: &PlanId, key: &JobKey, allocation: Option<Allocation>) -> Result<Next, CpdError> {
    let found: Option<String> =
        conn.query_row("SELECT phase FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0)).optional()?;
    let phase = found.ok_or_else(|| CpdError::plan_not_found(plan))?;
    let all = jobs::load(conn, plan)?;
    let job = all.iter().find(|j| j.key == key.as_str()).ok_or_else(|| {
        CpdError::new(ErrorCode::JobNotFound, format!("No Job {key} in Plan {plan}."))
            .with("plan", plan.as_str())
            .with("job", key.as_str())
    })?;
    // The repo and status of the Job's PR (R34 A7).
    let (repo, pr_status): (String, String) = conn.query_row(
        "SELECT r.repo, r.status FROM jobs j JOIN prs r ON r.plan = j.plan AND r.key = j.pr WHERE j.plan = ?1 AND j.key = ?2",
        [plan.as_str(), key.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    require_ready(&all, job, plan, &phase, &pr_status)?;
    let found = locked_fingerprint(job);
    if job.fingerprint.as_deref() != Some(found.as_str()) {
        return Err(CpdError::new(ErrorCode::LockedJobTampered, format!("Job {key} does not match its Locked-Job fingerprint."))
            .with("plan", plan.as_str())
            .with("job", key.as_str())
            .with("expected", job.fingerprint.clone())
            .with("found", found));
    }

    let bad = |what: String| CpdError::new(ErrorCode::StoreFailed, format!("The Ledger holds an invalid value: {what}."));
    // `never_started`: the previous Attempt's driver never started (`driver_pid` null, §3.4).
    let previous: Option<(u32, String, bool)> = conn
        .query_row(
            "SELECT number, tier, driver_pid IS NULL FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number DESC LIMIT 1",
            params![plan.as_str(), key.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let retry = match &job.retry_tier {
        Some(tier) if crate::job::retried_since_last_start(conn, plan, key)? => Some(tier),
        _ => None,
    };
    let (tier, origin) = match (retry, &previous) {
        (Some(tier), _) => (Tier::parse(tier).map_err(|_| bad(format!("retry_tier {tier:?} of Job {key}")))?, Origin::Retry),
        (None, None) => {
            let tier = job.require_tier()?;
            (Tier::parse(tier).map_err(|_| bad(format!("tier {tier:?} of Job {key}")))?, Origin::Ladder)
        }
        (None, Some((number, tier, never_started))) => {
            let last = Tier::parse(tier).map_err(|_| bad(format!("tier {tier:?} of Attempt {key}-a{number}")))?;
            if *never_started || !catalog.is_valid(last) {
                // The ledger queued the Job at that Tier again: the Attempt was not a try.
                // A Tier the catalog refuses is left as is here: step_up below raises it.
                (last, Origin::Ladder)
            } else {
                let next = catalog.next(last)?.ok_or_else(|| {
                    bad(format!("Job {key} is queued after Attempt {number} at Tier {last}, but the ladder has no next Tier"))
                })?;
                (next, Origin::Ladder)
            }
        }
    };
    // hs-n465, hs-d632: a stored or retry Tier the catalog refuses starts at the next valid Worker Tier.
    let tier = if catalog.is_valid(tier) { tier } else { catalog.step_up(tier).unwrap_or(tier) };
    // pro-d53, hs-d554: a Job stored on a Tier outside H0-H4, S0-S3 is never started (E3 raises one `error` per Job).
    tier.check_worker()?;
    let number = previous.map_or(1, |(n, _, _)| n + 1);
    let attempt = AttemptId::new(key.clone(), NonZeroU32::new(number).expect("numbers start at 1"));
    // A linear Attempt (hs-d680, pro-d52) is the PR's only live Attempt for its whole life: it runs in the PR's one
    // workflow worktree on the Lead Branch, which a parallel Attempt's merge would move under it. Counted here, inside
    // the same transaction that inserts the Attempt, so two concurrent starts cannot both take the workflow worktree.
    if pr_has_live_linear(conn, plan, job.pr.as_str(), &attempt.to_string())? {
        return Err(CpdError::new(
            ErrorCode::CasLost,
            format!("Attempt {attempt} of Job {key} cannot start: the workflow worktree of PR {} is held by a live linear Attempt.", job.pr),
        )
        .with("plan", plan.as_str())
        .with("job", key.as_str())
        .with("pr", job.pr.as_str()));
    }
    let live: i64 = conn.query_row(
        "SELECT count(*) FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE a.plan = ?1 AND j.pr = ?2 AND a.status <> 'ended' AND a.id <> ?3",
        params![plan.as_str(), job.pr.as_str(), attempt.to_string()],
        |r| r.get(0),
    )?;
    // E3's derived allocation wins when it passed one; otherwise the starter derives it from the live Attempts.
    // A requested Linear while another Attempt of the PR is live cannot take the PR's one workflow worktree: CasLost,
    // so the caller re-reads and retries (§3.7 E3, hs-d680/hs-d682/hs-d683).
    let linear = match allocation {
        Some(Allocation::Linear) if live > 0 => {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Attempt {attempt} of Job {key} cannot be linear: another Attempt of PR {} is live.", job.pr))
                .with("plan", plan.as_str())
                .with("job", key.as_str())
                .with("pr", job.pr.as_str()));
        }
        Some(Allocation::Linear) => true,
        Some(Allocation::Parallel) => false,
        None => live == 0,
    };
    let (branch, worktree) = if linear {
        let (lead_branch, lead_worktree): (String, Option<String>) = conn.query_row(
            "SELECT lead_branch, lead_worktree FROM prs WHERE plan = ?1 AND key = ?2",
            params![plan.as_str(), job.pr.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let lead_worktree = lead_worktree.ok_or_else(|| {
            CpdError::new(ErrorCode::StoreFailed, format!("PR {} of Plan {plan} has no workflow worktree.", job.pr)).with("pr", job.pr.as_str())
        })?;
        (lead_branch, PathBuf::from(lead_worktree))
    } else {
        (git::attempt_branch(plan, &attempt), git::attempt_worktree_path(std::path::Path::new(&repo), plan, &attempt)?)
    };
    Ok(Next { branch, attempt, tier, origin, worktree })
}

/// Ready (§3.2): `queued`, in a Plan in phase `dispatch` or `deploy`, whose PR is
/// `dispatch` or `open` (a `waiting` PR has no Lead worktree yet), with every
/// Dependency `merged`; else `job_not_ready`.
fn require_ready(all: &[Job], job: &Job, plan: &PlanId, phase: &str, pr_status: &str) -> Result<(), CpdError> {
    // An `open` or `update` Job has no Dependency rows: it waits for the PR's other Jobs (§3.2, derived).
    let derived = crate::job::open::unfinished(all, job);
    let waiting: Vec<&str> = match &derived {
        Some(keys) => keys.iter().map(String::as_str).collect(),
        None => job
            .needs
            .iter()
            .filter(|n| !all.iter().any(|j| j.key == **n && j.status == "merged"))
            .map(String::as_str)
            .collect(),
    };
    let why = if job.status != "queued" {
        format!("it is {}, not queued", job.status)
    } else if !matches!(phase, "dispatch" | "deploy") {
        format!("Plan {plan} is in phase {phase}")
    } else if !matches!(pr_status, "dispatch" | "open") {
        format!("its PR {} is {pr_status}", job.pr)
    } else if job.kind == "open" && pr_status != "dispatch" {
        format!("its PR {} is {pr_status}, and an open Job runs only while its PR is dispatch", job.pr)
    } else if !waiting.is_empty() && derived.is_some() {
        format!("it waits for {}, not finished yet", waiting.join(", "))
    } else if !waiting.is_empty() {
        format!("it needs {}, not merged yet", waiting.join(", "))
    } else if job.kind == "open" && crate::job::open::nothing_merged(all, job) {
        format!("no other Job of its PR {} is merged: there is nothing to open", job.pr)
    } else {
        return Ok(());
    };
    Err(CpdError::new(ErrorCode::JobNotReady, format!("Job {} is not Ready: {why}.", job.key))
        .with("plan", plan.as_str())
        .with("job", job.key.as_str())
        .with("status", job.status.as_str())
        .with("phase", phase)
        .with("pr", job.pr.as_str())
        .with("pr_status", pr_status)
        .with("waiting_for", waiting))
}

/// The `starting` Attempt and its Job `running`, with `attempt-started`.
fn insert(tx: &crate::store::WriteTxn<'_>, plan: &PlanId, key: &JobKey, next: &Next) -> Result<(), CpdError> {
    let worktree = next.worktree.to_str().ok_or_else(|| {
        CpdError::new(ErrorCode::GitFailed, "The Attempt's worktree path is not valid UTF-8.")
            .with("path", next.worktree.to_string_lossy().into_owned())
    })?;
    let number = next.attempt.number().get();
    tx.execute(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, 'starting', ?7, ?8, at FROM txns WHERE id = cpd_txn()",
        params![
            plan.as_str(),
            next.attempt.to_string(),
            key.as_str(),
            number,
            next.tier.to_string(),
            next.origin.as_str(),
            next.branch,
            worktree
        ],
    )?;
    let moved = tx.execute(
        "UPDATE jobs SET status = 'running' WHERE plan = ?1 AND key = ?2 AND status = 'queued'",
        params![plan.as_str(), key.as_str()],
    )?;
    if moved == 0 {
        return Err(CpdError::new(ErrorCode::CasLost, format!("Job {key} changed while its Attempt was being started: try again.")));
    }
    let attempt = &next.attempt;
    let text = format!(
        "Attempt {attempt} of Job {key} started at Tier {} ({}). See: cpd attempt show {attempt} --plan {plan}",
        next.tier,
        next.origin.as_str()
    );
    let event = NewEvent::attempt_started(text, number, &next.tier.to_string(), next.origin.as_str());
    events::append(tx, event.plan(plan.clone()).job(key.clone()).attempt(attempt.clone()))?;
    Ok(())
}

/// Spawns `cpd attempt run <attempt> --plan <plan>` detached (`setsid`), with
/// the lock as fd 3, stdin from `/dev/null`, and stdout and stderr appended to
/// `run/<plan>/logs/attempt-<attempt>.log`, through `ports.spawn`. Returns its pid.
fn spawn(ports: &Ports, home: &CpdHome, plan: &PlanId, attempt: &AttemptId, lock: &Starter, held: Option<Held>) -> Result<u32, String> {
    // The path recorded at process start: the live path of the file is `(deleted)` after an install.
    let exe = ports.spawn.self_exe().map_err(|e| e.to_string())?;
    let log_path = home.driver_log(plan, attempt);
    let dir = log_path.parent().expect("a log lives in a directory");
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    // `nice` would start with a missing binary and die with 127: a driver that cannot start is a spawn failure.
    if !exe.exists() {
        return Err(format!("cannot run {}: no such file", exe.display()));
    }
    // The resource locks go to the driver as copies of the same open file descriptions, numbered from `HAND_OVER_MIN` up:
    // clear of the numbers the starter's own files take, and not close-on-exec. The driver learns the numbers from the
    // environment and makes them close-on-exec again (A1). The starter's own descriptors close here; the copies close
    // when this function returns, after the spawn.
    let mut env_set = vec![("CPD_HOME".to_owned(), home.root().display().to_string())];
    let mut handed: Vec<OwnedFd> = Vec::new();
    if let Some(held) = held {
        for name in held.names() {
            let fd = held.fd(name).expect("a name of the held resources");
            // SAFETY: `held` owns the descriptor for the whole call.
            let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
            let copy = fcntl(borrowed, FcntlArg::F_DUPFD(HAND_OVER_MIN)).map_err(|e| format!("cannot hand over the resource {name}: {e}"))?;
            // SAFETY: `F_DUPFD` returned a new descriptor that nothing else owns.
            handed.push(unsafe { OwnedFd::from_raw_fd(copy) });
        }
        drop(held);
        let numbers: Vec<String> = handed.iter().map(|fd| fd.as_raw_fd().to_string()).collect();
        env_set.push((RESOURCE_FDS_ENV.to_owned(), numbers.join(",")));
    }
    // hs-d528: nice and ionice exec (`idle`), so the recorded pid is the driver's.
    let launch = Launch {
        program: exe.display().to_string(),
        args: ["attempt", "run", &attempt.to_string(), "--plan", plan.as_str()].map(str::to_owned).to_vec(),
        env_set,
        // The caller's own identity is not the driver's.
        env_remove: ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT", "TMUX", "TMUX_PANE"].map(str::to_owned).to_vec(),
        cwd: Some(home.root().to_path_buf()),
        setsid: true,
        stdout_to: Some(log_path),
        hand_over: Some(lock.fd()),
        idle: true,
        foreground: false,
    };
    // The real `Spawn` reaps the child on a thread, if this process outlives it (the engine); detached otherwise.
    let pid = ports.spawn.spawn(&launch).map_err(|e| format!("cannot run {}: {e}", exe.display()))?;
    drop(handed);
    Ok(pid.0)
}
