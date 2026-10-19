//! E3, scheduling (contract §3.2, §3.4, §3.7): each Ready Job, urgent ones
//! first, then in `jobs.seq` order across all Plans, while a slot is free (an
//! urgent Job may take one beyond `concurrency`), is started by the starter
//! `job dispatch` uses: fingerprint, driver lock, Attempt, driver.

use super::SOURCE;
use super::engine::Engine;
use super::report::{About, Step};
use rusqlite::{Connection, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::driver::{self, StartCtx};
use crate::error::{CpdError, ErrorCode};
use crate::ids::{JobKey, PlanId};

/// The longest back-off after Attempts whose driver never started (§3.7 E3).
const BACKOFF_CAP_SECONDS: u64 = 300;

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
    let wait = time::Duration::seconds(i64::try_from(backoff_seconds(k)).expect("at most 300"));
    Ok(now < ended + wait)
}

/// Ready (§3.2): `queued`, in a Plan in `dispatch` or `deploy`, whose PR is `dispatch`
/// or `open`, every Dependency `merged`. `driver::start`'s `require_ready` says the same.
const READY: &str = "SELECT j.plan, j.key, j.urgent FROM jobs j JOIN plans p ON p.id = j.plan
  JOIN prs r ON r.plan = j.plan AND r.key = j.pr
  WHERE j.status = 'queued' AND p.phase IN ('dispatch', 'deploy') AND r.status IN ('dispatch', 'open')
    AND NOT EXISTS (SELECT 1 FROM dependencies d JOIN jobs n ON n.plan = d.plan AND n.key = d.needs
                     WHERE d.plan = j.plan AND d.job = j.key AND n.status <> 'merged')
  ORDER BY j.urgent DESC, j.seq";

pub(super) fn run(engine: &mut Engine) {
    let ready = engine.store.read(|conn| {
        let mut stmt = conn.prepare(READY)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let active: i64 = conn.query_row("SELECT count(*) FROM attempts WHERE status <> 'ended'", [], |r| r.get(0))?;
        let now = OffsetDateTime::now_utc();
        let mut ready = Vec::new();
        for (p, k, urgent) in rows {
            let (plan, key) = (PlanId::parse(&p)?, JobKey::parse(&k)?);
            // Its driver could not be started the last times: wait before the next try.
            if !backing_off(conn, &plan, &key, now)? {
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
    let catalog = match engine.catalog() {
        Ok(catalog) => catalog,
        Err(e) => return engine.fail(Step::E3, About::Machine, &e),
    };
    for (plan, key) in ready {
        let about = About::Job(plan.clone(), key.clone());
        let ctx = StartCtx { home: &engine.home, config: &engine.config, catalog: &catalog, command: SOURCE };
        match driver::start(&mut engine.store, &ctx, &plan, &key) {
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
