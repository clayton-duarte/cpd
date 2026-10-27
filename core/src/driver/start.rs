//! The starter (contract §3.4 "The driver lock"; commands.md §5.5): what
//! `job dispatch` and engine E3 share. It takes the driver lock, inserts the
//! `starting` Attempt with its Tier (§9), and spawns the driver `cpd attempt
//! run` detached, handing it the lock as fd 3.

use std::num::NonZeroU32;
use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, params};

use crate::attempt::ledger::{self, Cause, End, Ladder, Stage};
use super::lock::Starter;
use crate::resource;
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
    let (wanted, pools) = store.read(|conn| {
        let wanted = resource::wanted(conn, plan, key.as_str())?;
        let mut pools = Vec::new();
        if let Some(w) = &wanted {
            crate::job::add::check_resources_declared(conn, ctx.home, plan, &w.pr, key.as_str(), Some(&w.names))?;
            let policy = crate::pr::policy_of_pr(conn, ctx.home, plan, &crate::ids::PrKey::parse(&w.pr)?)?;
            pools = resource::pools_of(&policy.resources, &w.owner, &w.repo).into_iter().filter(|p| w.names.iter().any(|n| n == p.name())).collect();
        }
        Ok((wanted, pools))
    })?;
    // §3.4 step 1: the lock before the transaction that inserts the Attempt.
    let lock = Starter::take(ctx.home, plan, &next.attempt)?;
    // The Job's pools (contract §3.4 "Resources of a Job"): a Job that is not yet a tenant takes the guard of each pool, in name order,
    // and finds every pool with a free slot, or none is taken. A Job already a tenant (a retry) competes for none. The guards
    // are held until the Attempt row and the `job_pools` rows have committed: the commit is the moment the Job becomes the tenant.
    let first = next.attempt.number().get() == 1;
    let mut guarded: Vec<(&resource::Pool, resource::Guard)> = Vec::new();
    if first {
        let mut sorted: Vec<&resource::Pool> = pools.iter().collect();
        sorted.sort_by(|a, b| (a.name(), resource::identity(a)).cmp(&(b.name(), resource::identity(b))));
        for pool in sorted {
            guarded.push((pool, resource::Guard::take(ctx.home, pool)?));
        }
        let full = store.read(|conn| {
            for (pool, _) in &guarded {
                if resource::free_slots(conn, ctx.home, pool)? == 0 {
                    return Ok(Some((pool.name().to_owned(), resource::occupants(conn, ctx.home, pool)?)));
                }
            }
            Ok(None)
        })?;
        if let Some((name, occupants)) = full {
            // Nothing changes: the driver lock file made for an Attempt that will not exist goes too.
            drop(guarded);
            drop(lock);
            super::lock::remove(ctx.home, plan, &next.attempt)?;
            let holders: Vec<serde_json::Value> = occupants.into_iter().map(|(slot, holder)| serde_json::json!({"slot": slot, "holder": holder})).collect();
            let w = wanted.as_ref().expect("a Job with pools lists resources");
            return Err(CpdError::new(
                ErrorCode::ResourceBusy,
                format!("Job {key} cannot start: the resource {name} of {}/{} has no free slot.", w.owner, w.repo),
            )
            .with("job", key.as_str())
            .with("resource", name)
            .with("holders", holders));
        }
    }
    let identities: Vec<String> = if first { pools.iter().map(resource::identity).collect() } else { Vec::new() };
    store.write(ctx.command, |tx| {
        if decide(tx, ctx.catalog, plan, key, allocation)? != next {
            return Err(CpdError::new(ErrorCode::CasLost, format!("Job {key} changed while its Attempt was being started: try again."))
                .with("plan", plan.as_str())
                .with("job", key.as_str()));
        }
        // An urgent Job may start one Attempt beyond `concurrency` (§3.7 E3). A Job already a tenant of `workers` takes no slot.
        if first {
            let workers = resource::workers(tx, ctx.config.concurrency)?;
            let urgent: bool = tx.query_row("SELECT urgent FROM jobs WHERE plan = ?1 AND key = ?2", [plan.as_str(), key.as_str()], |r| r.get(0))?;
            // The `workers` pool, the one rule E3 schedules by (§3.7 E3): a free slot, or for an urgent Job the one beyond.
            let pool = workers.pool();
            if pool.free + if urgent { pool.urgent_free } else { 0 } == 0 {
                let (active, limit) = (workers.holders.len(), ctx.config.concurrency + u32::from(urgent));
                return Err(CpdError::new(
                    ErrorCode::NoSlot,
                    format!("{active} Attempts are running, the limit is {limit} (concurrency {}): wait for one to end.", ctx.config.concurrency),
                )
                .with("active", active)
                .with("concurrency", ctx.config.concurrency)
                .with("urgent", urgent));
            }
        }
        insert(tx, plan, key, &next, &identities)
    })?;
    drop(guarded);

    // §3.4 step 2: spawn the driver with the lock as fd 3.
    let spawned = spawn(ports, ctx.home, plan, &next.attempt, &lock);
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
    // pro-d82, hs-d554: a Job stored on a Tier outside H0-H3, S0-S3 is never started (E3 raises one `error` per Job).
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
fn insert(tx: &crate::store::WriteTxn<'_>, plan: &PlanId, key: &JobKey, next: &Next, pools: &[String]) -> Result<(), CpdError> {
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
    // The Job's pools commit with its first Attempt: from this commit it is their tenant (contract §3.4 "Tenancy").
    for pool in pools {
        tx.execute("INSERT INTO job_pools (plan, job, pool) VALUES (?1, ?2, ?3)", params![plan.as_str(), key.as_str(), pool])?;
    }
    // The Job starts: its skip, if E3 recorded one, is cleared (contract §3.7 E3 "The skip order").
    let moved = tx.execute(
        "UPDATE jobs SET status = 'running', skipped_n = NULL WHERE plan = ?1 AND key = ?2 AND status = 'queued'",
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
fn spawn(ports: &Ports, home: &CpdHome, plan: &PlanId, attempt: &AttemptId, lock: &Starter) -> Result<u32, String> {
    // The path recorded at process start: the live path of the file is `(deleted)` after an install.
    let exe = ports.spawn.self_exe().map_err(|e| e.to_string())?;
    let log_path = home.driver_log(plan, attempt);
    let dir = log_path.parent().expect("a log lives in a directory");
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    // `nice` would start with a missing binary and die with 127: a driver that cannot start is a spawn failure.
    if !exe.exists() {
        return Err(format!("cannot run {}: no such file", exe.display()));
    }
    let env_set = vec![("CPD_HOME".to_owned(), home.root().display().to_string())];
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
    Ok(pid.0)
}
