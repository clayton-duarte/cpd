//! The starter (contract §3.4 "The driver lock"; commands.md §5.5): what
//! `job dispatch` and engine E3 share. It takes the driver lock, inserts the
//! `starting` Attempt with its Tier (§9), and spawns the driver `cpd attempt
//! run` detached, handing it the lock as fd 3.

use std::fs::OpenOptions;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

use super::ledger::{self, Cause, End, Ladder};
use super::lock::Starter;
use crate::config::{Catalog, Config, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Kind, NewEvent};
use crate::git;
use crate::home::CpdHome;
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::plan::jobs::{self, Job};
use crate::plan::locked_fingerprint;
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
    let next = store.read(|conn| decide(conn, ctx.catalog, plan, key))?;
    // §3.4 step 1: the lock before the transaction that inserts the Attempt.
    let lock = Starter::take(ctx.home, plan, &next.attempt)?;
    store.write(ctx.command, |tx| {
        if decide(tx, ctx.catalog, plan, key)? != next {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Job {key} changed while its Attempt was being started: try again."))
                .with("plan", plan.as_str())
                .with("job", key.as_str()));
        }
        let active: i64 = tx.query_row("SELECT count(*) FROM attempts WHERE status <> 'ended'", [], |r| r.get(0))?;
        if active >= i64::from(ctx.config.concurrency) {
            return Err(CpdError::new(
                ErrorCode::NoSlot,
                format!("{active} Attempts are running, the machine's concurrency is {}: wait for one to end.", ctx.config.concurrency),
            )
            .with("active", active)
            .with("concurrency", ctx.config.concurrency));
        }
        insert(tx, plan, key, &next)
    })?;

    // §3.4 step 2: spawn the driver with the lock as fd 3.
    let spawned = spawn(ctx.home, plan, &next.attempt, &lock);
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
                ledger::end(tx, &target, &End::Failed { cause: Cause::Crashed, detail: detail.clone() }, None, &ladder)
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
/// does not match its fingerprint.
fn decide(conn: &Connection, catalog: &Catalog, plan: &PlanId, key: &JobKey) -> Result<Next, CpdError> {
    let found: Option<String> =
        conn.query_row("SELECT phase FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get(0)).optional()?;
    let phase = found.ok_or_else(|| CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str()))?;
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
    let previous: Option<(u32, String)> = conn
        .query_row(
            "SELECT number, tier FROM attempts WHERE plan = ?1 AND job = ?2 ORDER BY number DESC LIMIT 1",
            params![plan.as_str(), key.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let retry = match &job.retry_tier {
        Some(tier) if crate::job::retried_since_last_start(conn, plan, key)? => Some(tier),
        _ => None,
    };
    let (tier, origin) = match (retry, &previous) {
        (Some(tier), _) => (Tier::parse(tier).map_err(|_| bad(format!("retry_tier {tier:?} of Job {key}")))?, Origin::Retry),
        (None, None) => (Tier::parse(&job.tier).map_err(|_| bad(format!("tier {:?} of Job {key}", job.tier)))?, Origin::Ladder),
        (None, Some((number, tier))) => {
            let last = Tier::parse(tier).map_err(|_| bad(format!("tier {tier:?} of Attempt {key}-a{number}")))?;
            let next = catalog.next(last)?.ok_or_else(|| {
                bad(format!("Job {key} is queued after Attempt {number} at Tier {last}, but the ladder has no next Tier"))
            })?;
            (next, Origin::Ladder)
        }
    };
    let number = previous.map_or(1, |(n, _)| n + 1);
    let attempt = AttemptId::new(key.clone(), NonZeroU32::new(number).expect("numbers start at 1"));
    let worktree = git::attempt_worktree_path(std::path::Path::new(&repo), plan, &attempt)?;
    Ok(Next { branch: git::attempt_branch(plan, &attempt), attempt, tier, origin, worktree })
}

/// Ready (§3.2): `queued`, in a Plan in phase `dispatch` or `deploy`, whose PR is
/// `dispatch` or `open` (a `waiting` PR has no Lead worktree yet), with every
/// Dependency `merged`; else `job_not_ready`.
fn require_ready(all: &[Job], job: &Job, plan: &PlanId, phase: &str, pr_status: &str) -> Result<(), CpdError> {
    let waiting: Vec<&str> = job
        .needs
        .iter()
        .filter(|n| !all.iter().any(|j| j.key == **n && j.status == "merged"))
        .map(String::as_str)
        .collect();
    let why = if job.status != "queued" {
        format!("it is {}, not queued", job.status)
    } else if !matches!(phase, "dispatch" | "deploy") {
        format!("Plan {plan} is in phase {phase}")
    } else if !matches!(pr_status, "dispatch" | "open") {
        format!("its PR {} is {pr_status}", job.pr)
    } else if !waiting.is_empty() {
        format!("it needs {}, not merged yet", waiting.join(", "))
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
    let data = json!({"number": number, "tier": next.tier.to_string(), "origin": next.origin.as_str()});
    events::append(tx, NewEvent::new(Kind::AttemptStarted, text, data).plan(plan.clone()).job(key.clone()).attempt(attempt.clone()))?;
    Ok(())
}

/// Spawns `cpd attempt run <attempt> --plan <plan>` detached (`setsid`), with
/// the lock as fd 3, stdin from `/dev/null`, and stdout and stderr appended to
/// `run/<plan>/logs/attempt-<attempt>.log`. Returns its pid.
fn spawn(home: &CpdHome, plan: &PlanId, attempt: &AttemptId, lock: &Starter) -> Result<u32, String> {
    // The path recorded at process start: the live path of the file is `(deleted)` after an install.
    let exe = crate::host::exe::started_as().map_err(|e| e.to_string())?;
    let log_path = home.driver_log(plan, attempt);
    let dir = log_path.parent().expect("a log lives in a directory");
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("cannot open {}: {e}", log_path.display()))?;
    let stdout = log.try_clone().map_err(|e| format!("cannot use {}: {e}", log_path.display()))?;

    let mut command = Command::new(exe);
    command
        .args(["attempt", "run", &attempt.to_string(), "--plan", plan.as_str()])
        .current_dir(home.root())
        .env("CPD_HOME", home.root())
        // The caller's own identity is not the driver's.
        .env_remove("CPD_PLAN")
        .env_remove("CPD_SESSION")
        .env_remove("CPD_ATTEMPT")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(log);
    lock.hand_over(&mut command);
    let mut child = command.spawn().map_err(|e| format!("cannot run {}: {e}", exe.display()))?;
    let pid = child.id();
    // Reaps it if this process outlives it (the engine); detached otherwise.
    std::thread::spawn(move || child.wait());
    Ok(pid)
}
