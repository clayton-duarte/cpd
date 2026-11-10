//! Red tests of the `quota` cause and of preserved crash diagnostics (contract §3.4, §9).

use std::path::Path;

use crate::config::{Letter, RoleTiers, Tier};
use crate::ids::{AttemptId, PlanId};

use super::incident_tests::{Fixture, catalog, fixture};
use super::ledger::{self, Cause, End, Ladder, Stage, classify_stop_text, crashed_detail};

fn add_running(f: &mut Fixture) {
    f.store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, driver_pid, started_at, ended_at)
                   VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', NULL, NULL, 'cpd/pay--a-a1', '/w', 4242, 't', NULL);",
            )?;
            Ok(())
        })
        .unwrap();
}

fn end_failed(f: &mut Fixture, cause: Cause, detail: &str, retry_cap: u32) {
    let attempt = f
        .store
        .read(|conn| {
            ledger::load(
                conn,
                &PlanId::parse("pay").unwrap(),
                &AttemptId::parse("a-a1").unwrap(),
            )
        })
        .unwrap();
    let cat = catalog();
    let ladder = Ladder {
        catalog: &cat,
        retry_cap,
        roles: RoleTiers {
            decide: Tier::x(Letter::O),
            auxiliar: Tier::parse("GX").unwrap(),
            worker: Tier::parse("H0").unwrap(),
            lead: Tier::parse("S4").unwrap(),
            manager: Tier::parse("O4").unwrap(),
        },
    };
    f.store
        .write("test", |tx| {
            ledger::end(
                tx,
                &attempt,
                &End::Failed {
                    cause,
                    detail: detail.into(),
                },
                None,
                &ladder,
                Stage::Driver,
            )
        })
        .expect("the ledger ends the Attempt");
}

fn events_of(f: &mut Fixture, kind: &str) -> Vec<String> {
    let kind = kind.to_owned();
    f.store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = ?1 ORDER BY id")?;
            Ok(stmt
                .query_map([&kind], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

#[test]
fn quota_cause_is_classified_from_provider_text() {
    for text in [
        "You have hit your usage limit",
        "quota exceeded",
        "429 Too Many Requests: you exceeded your current quota",
    ] {
        assert_eq!(classify_stop_text(text), "quota", "{text:?} is a quota");
    }
    assert_eq!(classify_stop_text("Connection error."), "provider_error");
    assert_eq!(classify_stop_text("the model wandered off"), "crashed");
}

#[test]
fn quota_attempt_gets_no_automatic_retry() {
    let mut f = fixture();
    add_running(&mut f);
    let quota = Cause::parse("quota").expect("`quota` is a cause");
    end_failed(&mut f, quota, "You have hit your usage limit", 2);
    let status: String = f
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT status FROM jobs WHERE plan = 'pay' AND key = 'a'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(status, "failed", "a quota fails the Job at once");
    assert!(events_of(&mut f, "attempt-retry").is_empty());
    assert_eq!(events_of(&mut f, "job-failed").len(), 1);
}

#[test]
fn crashed_failure_keeps_actionable_diagnostics() {
    let log = Path::new("/home/x/driver-a-a1.log");
    let detail = crashed_detail(
        "error",
        Some("boom: provider exploded"),
        "panic: stderr line 7",
        Some(137),
        log,
    );
    let mut f = fixture();
    add_running(&mut f);
    end_failed(&mut f, Cause::Crashed, &detail, 0);
    let data = events_of(&mut f, "job-failed").join("\n");
    for needle in [
        "boom: provider exploded",
        "panic: stderr line 7",
        "137",
        "/home/x/driver-a-a1.log",
    ] {
        assert!(data.contains(needle), "job-failed lacks {needle:?}: {data}");
    }
}
