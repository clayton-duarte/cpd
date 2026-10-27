//! E3, scheduling (contract §3.2, §3.4, §3.7): each Ready Job, urgent ones
//! first, then in `jobs.seq` order across all Plans, while a slot is free (an
//! urgent Job may take one beyond `concurrency`), is started by the starter
//! `job dispatch` uses: fingerprint, driver lock, Attempt, driver.

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use super::restart;
use rusqlite::{Connection, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use cpd_core::clock::Clock;
use cpd_core::driver::{self, Allocation, StartCtx};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::limit::limit;
use cpd_core::ids::{JobKey, PlanId};

/// The longest back-off after Attempts whose driver never started (§3.7 E3).
const BACKOFF_CAP_SECONDS: u64 = 300;
/// The back-off's unit: `2^k` of it (tests shorten it with `CPD_TEST_BACKOFF_UNIT`, [`limit`]).
const BACKOFF_UNIT: std::time::Duration = std::time::Duration::from_secs(1);

/// Whether incident mode lets E3 start a Ready Job of Plan `plan` and urgency `urgent` (contract
/// §3.7, hs-d643): the incident is off, or the Plan is in `incident_plans`, or the Job is
/// explicitly urgent; every other Ready Job holds (the `concurrency + 1` urgent slot rule is
/// unchanged).
pub(super) fn incident_starts(conn: &Connection, plan: &PlanId, urgent: bool) -> bool {
    urgent || !cpd_core::incident::is_held(conn, plan)
}

/// How long E3 waits after the last of `k` Attempts in a row whose driver never
/// started: `min(2^k, 300)` seconds.
fn backoff_seconds(k: u32) -> u64 {
    1u64.checked_shl(k).map_or(BACKOFF_CAP_SECONDS, |s| s.min(BACKOFF_CAP_SECONDS))
}

/// The latest Attempts of Job `key` in a row that ended with `driver_pid` null,
/// as `(k, ended_at of the last)`; `None` when the latest Attempt's driver started
/// or there is none. One query; an Attempt whose driver started resets `k`.
fn spawn_failures(conn: &Connection, plan: &PlanId, key: &JobKey) -> Result<Option<(u32, String)>, CpdError> {
    let (k, last): (u32, Option<String>) = conn.query_row(
        "SELECT count(*), max(ended_at) FROM attempts
          WHERE plan = ?1 AND job = ?2 AND status = 'ended' AND driver_pid IS NULL
            AND number > coalesce((SELECT max(number) FROM attempts WHERE plan = ?1 AND job = ?2 AND driver_pid IS NOT NULL), 0)",
        params![plan.as_str(), key.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(last.filter(|_| k >= 1).map(|at| (k, at)))
}

/// Whether the back-off of Job `key` still runs at `now`.
fn backing_off(conn: &Connection, plan: &PlanId, key: &JobKey, now: OffsetDateTime) -> Result<bool, CpdError> {
    let Some((k, ended_at)) = spawn_failures(conn, plan, key)? else { return Ok(false) };
    let ended = OffsetDateTime::parse(&ended_at, &Rfc3339).map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("The Ledger holds an invalid value: ended_at {ended_at:?} of an Attempt of Job {key}: {e}."))
    })?;
    let unit = limit("BACKOFF_UNIT", BACKOFF_UNIT);
    let wait = time::Duration::try_from(unit * u32::try_from(backoff_seconds(k)).expect("at most 300")).expect("at most 300 units");
    Ok(now < ended + wait)
}

/// [`backing_off`] at the time `clock` reads (`Clock::utc`). The unit is [`BACKOFF_UNIT`] unless `CPD_TEST_BACKOFF_UNIT`
/// says otherwise (cli/tests/engine/watch.rs still reads it).
pub(super) fn backing_off_with(conn: &Connection, plan: &PlanId, key: &JobKey, clock: &dyn Clock) -> Result<bool, CpdError> {
    backing_off(conn, plan, key, clock.utc())
}

/// Ready (§3.2): `queued`, in a Plan in `dispatch` or `deploy`, whose PR is `dispatch`
/// or `open`, every Dependency `merged`. `driver::start`'s `require_ready` says the same.
/// An `open` or `update` Job has no Dependency rows: its rule is derived from the PR's other Jobs (§3.2),
/// which are the Jobs that are neither `open` nor `update` (those do not count each other). An `open` Job is
/// Ready when its PR is `dispatch`, none of them is `draft`, `queued`, `running` or `failed`, and one is `merged`;
/// an `update` Job when its PR is `open` and none of them is `queued` or `running`.
/// The `workers` step counts these rows too (the StatusBar's `W waiting`).
pub(super) const READY: &str = "SELECT j.plan, j.key, j.urgent FROM jobs j JOIN plans p ON p.id = j.plan
  JOIN prs r ON r.plan = j.plan AND r.key = j.pr
  WHERE j.status = 'queued' AND p.phase IN ('dispatch', 'deploy') AND r.status IN ('dispatch', 'open')
    AND NOT EXISTS (SELECT 1 FROM dependencies d JOIN jobs n ON n.plan = d.plan AND n.key = d.needs
                     WHERE d.plan = j.plan AND d.job = j.key AND n.status <> 'merged')
    AND (j.kind <> 'open' OR (r.status = 'dispatch'
         AND NOT EXISTS (SELECT 1 FROM jobs o WHERE o.plan = j.plan AND o.pr = j.pr AND o.key <> j.key AND o.kind NOT IN ('open', 'update')
                           AND o.status NOT IN ('merged', 'cancelled'))
         AND EXISTS (SELECT 1 FROM jobs o WHERE o.plan = j.plan AND o.pr = j.pr AND o.key <> j.key AND o.kind NOT IN ('open', 'update')
                       AND o.status = 'merged')))
    AND (j.kind <> 'update' OR (r.status = 'open'
         AND NOT EXISTS (SELECT 1 FROM jobs o WHERE o.plan = j.plan AND o.pr = j.pr AND o.key <> j.key AND o.kind NOT IN ('open', 'update')
                           AND o.status IN ('queued', 'running'))))
  ORDER BY j.urgent DESC, j.seq";

/// The allocation E3 derives for each Ready Job it is about to start (pro-d52), per PR, from the Ledger: a PR with a
/// live linear Attempt starts nothing (`None`: its Jobs are held); a PR with live parallel Attempts starts all
/// `Parallel`; a PR with no live Attempt starts one Job `Linear`, two or more all `Parallel`. One entry per Job of
/// `ready`, in its order.
fn allocations(conn: &Connection, ready: &[(PlanId, JobKey)]) -> Result<Vec<Option<Allocation>>, CpdError> {
    let mut prs = Vec::with_capacity(ready.len());
    for (plan, key) in ready {
        let pr: String = conn.query_row(
            "SELECT pr FROM jobs WHERE plan = ?1 AND key = ?2",
            params![plan.as_str(), key.as_str()],
            |r| r.get(0),
        )?;
        prs.push((plan.clone(), pr));
    }
    let mut out = Vec::with_capacity(ready.len());
    for (plan, pr) in &prs {
        if driver::pr_has_live_linear(conn, plan, pr, "")? {
            out.push(None);
            continue;
        }
        let live: i64 = conn.query_row(
            "SELECT count(*) FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
              WHERE a.plan = ?1 AND j.pr = ?2 AND a.status <> 'ended'",
            params![plan.as_str(), pr.as_str()],
            |r| r.get(0),
        )?;
        let siblings = prs.iter().filter(|p| *p == &(plan.clone(), pr.clone())).count();
        out.push(Some(if live == 0 && siblings == 1 { Allocation::Linear } else { Allocation::Parallel }));
    }
    Ok(out)
}

pub(super) fn run(engine: &mut Engine) {
    // A tmux restart is pending: no new Attempt until it has run; running ones finish (§3.7 E3).
    match restart::holding(&engine.home) {
        Ok(true) => return engine.log.line("E3: holding: a tmux restart is pending, no new Attempt starts until it has run"),
        Ok(false) => {}
        Err(e) => return engine.fail(Step::E3, About::Machine, &e),
    }
    let clock = engine.ports.clock.clone();
    let ready = engine.store.read(|conn| {
        let mut stmt = conn.prepare(READY)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let active: i64 = conn.query_row("SELECT count(*) FROM attempts WHERE status <> 'ended'", [], |r| r.get(0))?;
        let mut ready = Vec::new();
        for (p, k, urgent) in rows {
            let plan = PlanId::parse(&p)?;
            // Incident mode on: a non-urgent Ready Job of a held Plan waits (§3.7 E3).
            if !incident_starts(conn, &plan, urgent) {
                continue;
            }
            let key = JobKey::parse(&k)?;
            // Its driver could not be started the last times: wait before the next try.
            // Its latest Attempt ended provider_error less than 30 s ago: the Job stays queued (pro-d102).
            if !backing_off_with(conn, &plan, &key, &*clock)? && !super::provider_hold::holding_with(conn, &plan, &key, &*clock)? {
                ready.push((plan, key, urgent));
            }
        }
        Ok((ready, active))
    });
    let ready = match ready {
        Ok((ready, active)) => {
            engine.clear(Step::E3, &About::Machine);
            // Only an urgent Job may take the slot beyond `concurrency` (§3.7 E3).
            if active >= driver::slot_limit(engine.config.concurrency, true) {
                return;
            }
            let full = active >= driver::slot_limit(engine.config.concurrency, false);
            ready.into_iter().filter(|(_, _, urgent)| *urgent || !full).map(|(plan, key, _)| (plan, key)).collect::<Vec<_>>()
        }
        Err(e) => return engine.fail(Step::E3, About::Machine, &e),
    };
    if ready.is_empty() {
        return;
    }
    let allocations = match engine.store.read(|conn| allocations(conn, &ready)) {
        Ok(allocations) => allocations,
        Err(e) => return engine.fail(Step::E3, About::Machine, &e),
    };
    for ((plan, key), allocation) in ready.into_iter().zip(allocations) {
        // Held: a live linear Attempt of its PR keeps the workflow worktree; the Job stays queued, no Event.
        let Some(allocation) = allocation else { continue };
        let about = About::Job(plan.clone(), key.clone());
        // The Job's PR repo key sets its models: one catalog per distinct `[model]` table, a key that does not resolve fails its Job.
        let catalog = match engine.catalog_of_job(&plan, key.as_str()) {
            Ok(catalog) => catalog,
            Err(e) => {
                engine.fail(Step::E3, about, &e);
                continue;
            }
        };
        let ctx = StartCtx { home: &engine.home, config: &engine.config, catalog: &catalog, command: SOURCE };
        match driver::start_with_allocation(&engine.ports, &mut engine.store, &ctx, &plan, &key, Some(allocation)) {
            Ok(started) => {
                engine.log.line(&format!(
                    "E3: started {} of Plan {plan} at Tier {} ({}), driver pid {}",
                    started.attempt,
                    started.tier,
                    started.origin.as_str(),
                    started.pid
                ));
                engine.clear(Step::E3, &about);
            }
            // Every slot is taken: the next tick tries again.
            Err(e) if e.code() == ErrorCode::NoSlot => break,
            // A resource of the Job is held: it stays queued, no Attempt, no back-off, no Event; the next tick tries again
            // (§3.7 E3 "Resources").
            Err(e) if e.code() == ErrorCode::ResourceBusy => {
                engine.log.line(&format!("E3: Job {key} of Plan {plan} waits: {}", e.message()));
            }
            // The Job changed since it was read (another starter, a cancel): decided again next tick.
            Err(e) if matches!(e.code(), ErrorCode::CasLost | ErrorCode::JobNotReady) => {
                engine.log.line(&format!("E3: skipped Job {key} of Plan {plan}: {}", e.message()));
            }
            Err(e) => engine.fail(Step::E3, about, &e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backoff_doubles_with_each_spawn_failure_up_to_300_seconds() {
        let seconds: Vec<u64> = [1, 2, 3, 8, 9, 10, 63, 64, 1000].map(backoff_seconds).to_vec();
        assert_eq!(seconds, [2, 4, 8, 256, 300, 300, 300, 300, 300]);
    }

    /// The columns the queries read, with their types.
    fn attempts(rows: &[(i64, Option<i64>, &str)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE attempts (plan TEXT, id TEXT, job TEXT, number INTEGER, status TEXT, driver_pid INTEGER, ended_at TEXT);",
        )
        .unwrap();
        for (number, pid, ended_at) in rows {
            conn.execute(
                "INSERT INTO attempts (plan, id, job, number, status, driver_pid, ended_at) VALUES ('pay', 'api-a' || ?1, 'api', ?1, 'ended', ?2, ?3)",
                params![number, pid, ended_at],
            )
            .unwrap();
        }
        conn
    }

    fn failures(conn: &Connection) -> Option<(u32, String)> {
        spawn_failures(conn, &PlanId::parse("pay").unwrap(), &JobKey::parse("api").unwrap()).unwrap()
    }

    #[test]
    fn the_latest_attempts_in_a_row_without_a_driver_are_counted_and_a_started_driver_resets_them() {
        assert_eq!(failures(&attempts(&[])), None);
        assert_eq!(failures(&attempts(&[(1, Some(7), "2026-10-18T00:00:01.000Z")])), None);
        let two = attempts(&[(1, None, "2026-10-18T00:00:01.000Z"), (2, None, "2026-10-18T00:00:03.000Z")]);
        assert_eq!(failures(&two), Some((2, "2026-10-18T00:00:03.000Z".to_owned())));
        let reset = attempts(&[
            (1, None, "2026-10-18T00:00:01.000Z"),
            (2, Some(7), "2026-10-18T00:00:02.000Z"),
            (3, None, "2026-10-18T00:00:03.000Z"),
        ]);
        assert_eq!(failures(&reset), Some((1, "2026-10-18T00:00:03.000Z".to_owned())));
        let started_last = attempts(&[(1, None, "2026-10-18T00:00:01.000Z"), (2, Some(7), "2026-10-18T00:00:02.000Z")]);
        assert_eq!(failures(&started_last), None);
    }

    #[test]
    fn a_job_backs_off_for_2_to_the_k_seconds_after_the_last_spawn_failure() {
        let at = |s: &str| OffsetDateTime::parse(s, &Rfc3339).unwrap();
        let (plan, key) = (PlanId::parse("pay").unwrap(), JobKey::parse("api").unwrap());
        let one = attempts(&[(1, None, "2026-10-18T00:00:10.000Z")]);
        assert!(backing_off(&one, &plan, &key, at("2026-10-18T00:00:11.999Z")).unwrap());
        assert!(!backing_off(&one, &plan, &key, at("2026-10-18T00:00:12.000Z")).unwrap());
        let two = attempts(&[(1, None, "2026-10-18T00:00:01.000Z"), (2, None, "2026-10-18T00:00:10.000Z")]);
        assert!(backing_off(&two, &plan, &key, at("2026-10-18T00:00:13.999Z")).unwrap());
        assert!(!backing_off(&two, &plan, &key, at("2026-10-18T00:00:14.000Z")).unwrap());
        assert!(!backing_off(&attempts(&[]), &plan, &key, at("2026-10-18T00:00:00.000Z")).unwrap());
    }
}
