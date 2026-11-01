//! The `pr_posts` table (contract §4.4, PR post; pro-d64, pro-d65): one row per PR, its checks, its foreign key and its history.

use crate::home::Home;
use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;

use cpd_core::error::CpdError;
use cpd_core::store::{Store, WriteTxn};
use crate::home::HasCpdHome;

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn refused(store: &mut Store, sql: &str) -> CpdError {
    store.write("test", |tx| exec(tx, sql)).expect_err(sql)
}

fn sqlite_message(err: &CpdError) -> String {
    err.details()["sqlite_message"].as_str().unwrap_or_default().to_owned()
}

/// A store holding a Context, its Lead, Plan `pay` and its PR `pay`.
fn fixture_store(env: &Home) -> Store {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                     INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                       VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');
                     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                       VALUES ('pay', 'pay', '/r', 'main', 'waiting', 'cpd/pay/pay', {}, 't');",
                    tx.id()
                ),
            )
        })
        .unwrap();
    store
}

const GOOD: &str = "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
    VALUES ('pay', 'pay', 'C02JU6C2Z1D', '1793040197.490209', 'https://missionlane.slack.com/archives/C02JU6C2Z1D/p1793040197490209', 't', 'git-merge', NULL)";

#[test]
fn a_second_row_for_one_pr_is_refused() {
    let env = Home::new();
    let mut store = fixture_store(&env);
    store.write("test", |tx| exec(tx, GOOD)).unwrap();

    let err = refused(&mut store, GOOD);

    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert!(sqlite_message(&err).contains("UNIQUE constraint failed: pr_posts.plan, pr_posts.workflow"), "{err:?}");
}

#[test]
fn a_row_outside_the_checks_is_refused() {
    let env = Home::new();
    let mut store = fixture_store(&env);
    // the table exists and takes a good row; a CHECK is evaluated before the primary key, so the refusals below are the CHECKs'
    store.write("test", |tx| exec(tx, GOOD)).unwrap();
    for (channel, ts, link, reaction, reacted_at) in [
        ("''", "'1.2'", "'https://x'", "NULL", "NULL"),
        ("'C02JU6C2Z1D'", "''", "'https://x'", "NULL", "NULL"),
        ("'C02JU6C2Z1D'", "'1.2'", "''", "NULL", "NULL"),
        ("'C02JU6C2Z1D'", "'1.2'", "'https://x'", "''", "NULL"),
        ("'C02JU6C2Z1D'", "'1.2'", "'https://x'", "NULL", "'t'"),
    ] {
        let sql = format!(
            "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
             VALUES ('pay', 'pay', {channel}, {ts}, {link}, 't', {reaction}, {reacted_at})"
        );
        let err = refused(&mut store, &sql);
        assert_eq!(err.code(), ErrorCode::StoreFailed, "{sql}: {err:?}");
        assert!(sqlite_message(&err).contains("CHECK constraint failed"), "{sql}: {err:?}");
    }
}

#[test]
fn a_row_of_a_missing_pr_is_refused() {
    let env = Home::new();
    let mut store = fixture_store(&env);
    store.write("test", |tx| exec(tx, GOOD)).unwrap();

    let err = refused(&mut store, &GOOD.replace("'pay', 'pay'", "'pay', 'nope'"));

    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert!(sqlite_message(&err).contains("FOREIGN KEY constraint failed"), "{err:?}");
}

#[test]
fn the_row_has_history_and_cannot_be_deleted_of_a_live_plan() {
    let env = Home::new();
    let mut store = fixture_store(&env);
    store.write("test", |tx| exec(tx, GOOD)).unwrap();

    let rows: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM history WHERE tbl = 'pr_posts' AND op = 'insert' AND pk = '[\"pay\",\"pay\"]'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 1);
    let err = refused(&mut store, "DELETE FROM pr_posts");
    assert_eq!(err.code(), ErrorCode::StoreFailed, "{err:?}");
    assert_eq!(sqlite_message(&err), "append_only: pr_posts");
}
