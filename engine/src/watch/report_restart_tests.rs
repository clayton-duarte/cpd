//! A restarted engine's fresh `Report` keeps `watch.error_repeat_seconds` (contract §3.7): it reads the store.

use std::time::Duration;

use super::log::Log;
use super::report::{About, Failure, Report, Step};
use cpd_core::home::CpdHome;
use cpd_core::ids::{JobKey, PlanId};
use cpd_core::store::Store;
use serde_json::json;

fn failure(code: &str) -> Failure {
    Failure {
        code: code.to_owned(),
        message: "It failed.".to_owned(),
        details: json!({}),
    }
}

fn restarted() -> Report {
    Report::new(Duration::from_secs(600))
}

fn job(name: &str) -> About {
    About::Job(
        PlanId::parse("plan-a").unwrap(),
        JobKey::parse(name).unwrap(),
    )
}

fn errors(store: &mut Store) -> i64 {
    store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM events WHERE kind = 'error'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

/// Context `ctx-a` with a Lead and its Plan `plan-a` (a copy of the fixture of report.rs's `managers`).
fn managers(store: &mut Store) {
    let t = "2026-01-01T00:00:00.000Z";
    store
        .write("test", |tx| {
            tx.execute("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx-a', 'ctx-a', ?1, 'm1')", [t])?;
            tx.execute_batch(&format!(
                "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, archived_at)
                   VALUES ('mgr-a', 'manager', 'ctx-a', 'Ma', '/', 'O3', 'idle', '{t}', '{t}', NULL),
                          ('lead-a', 'lead', 'ctx-a', 'La', '/', 'O3', 'idle', '{t}', '{t}', NULL);
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('plan-a', 'ctx-a', 'Pa', 'grounding', 'lead-a', '{t}');"
            ))?;
            Ok(())
        })
        .unwrap();
}

fn fixture() -> (tempfile::TempDir, Log, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    let log = Log::open(&home).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    managers(&mut store);
    (dir, log, store)
}

/// The text a Report writes for E3, `description_required`, Job `j1`: appended into a scratch store, read back.
fn written_text() -> String {
    let (_dir, mut log, mut store) = fixture();
    restarted().fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );
    store
        .read(|conn| {
            Ok(
                conn.query_row("SELECT text FROM events WHERE kind = 'error'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

/// Inserts an `error` Event of `source` for Job `j1` of `plan-a` appended at `at` (SQL expression).
fn insert_error(store: &mut Store, source: &str, at: &str) {
    let text = written_text();
    let data =
        json!({"source": source, "code": "description_required", "message": "m", "details": {}})
            .to_string();
    store
        .write("test", |tx| {
            tx.execute(
                &format!(
                    "INSERT INTO events (txn, at, kind, context, plan, job, text, data)
                     VALUES (cpd_txn(), {at}, 'error', 'ctx-a', 'plan-a', 'j1', ?1, ?2)"
                ),
                [&text, &data],
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn a_restarted_report_appends_nothing_for_an_error_appended_within_the_repeat_period() {
    let (_dir, mut log, mut store) = fixture();
    restarted().fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );
    assert_eq!(errors(&mut store), 1);

    let mut second = restarted();
    second.fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );
    assert_eq!(errors(&mut store), 1, "a restart forgot the repeat window");
    second.clear(Step::E3, &job("j1"));
    second.fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );
    assert_eq!(
        errors(&mut store),
        1,
        "clear and fail again after a restart"
    );

    restarted().fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );
    assert_eq!(errors(&mut store), 1, "a second restart");
}

#[test]
fn a_restarted_report_still_appends_a_different_code_step_or_scope() {
    let (_dir, mut log, mut store) = fixture();
    restarted().fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );

    let mut report = restarted();
    report.fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("store_failed"),
    );
    assert_eq!(errors(&mut store), 2, "another code");
    report.fail(
        &mut store,
        &mut log,
        Step::E4,
        job("j1"),
        failure("description_required"),
    );
    assert_eq!(errors(&mut store), 3, "another step");
    report.fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j2"),
        failure("description_required"),
    );
    assert_eq!(errors(&mut store), 4, "another Job");
}

#[test]
fn a_restarted_report_appends_again_when_the_last_event_is_older_than_the_repeat_period() {
    let (_dir, mut log, mut store) = fixture();
    insert_error(&mut store, "watch", "'2026-01-01T00:00:00.000Z'");
    assert_eq!(errors(&mut store), 1);
    restarted().fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );
    assert_eq!(errors(&mut store), 2);
}

#[test]
fn an_error_event_of_another_source_does_not_hold_a_restarted_report_back() {
    let (_dir, mut log, mut store) = fixture();
    insert_error(
        &mut store,
        "pr poll",
        "strftime('%Y-%m-%dT%H:%M:%fZ','now')",
    );
    assert_eq!(errors(&mut store), 1);
    restarted().fail(
        &mut store,
        &mut log,
        Step::E3,
        job("j1"),
        failure("description_required"),
    );
    assert_eq!(errors(&mut store), 2);
}
