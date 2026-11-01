//! Plannotator removed (hs-d738, hs-d739): the store migration `drop-plannotator` (contract §4.5, breaking: a DROP and a
//! rebuild) and the Inbox kind `review-waiting` gone.

use cpd_core::home::CpdHome;
use cpd_core::inbox::model::Kind;
use cpd_core::store::testing::{named_migrations, prefix_database};
use rusqlite::Connection;

fn home(dir: &tempfile::TempDir) -> CpdHome {
    CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// A store with every migration of the binary applied.
fn fresh(dir: &tempfile::TempDir) -> Connection {
    prefix_database(named_migrations().len(), &home(dir)).unwrap()
}

const INSERT_ITEM: &str = "INSERT INTO inbox_items (kind, urgency, subject, label, opened_at) VALUES (?1, 'magenta', 'subject', 'label', 't')";

#[test]
fn drop_plannotator_fresh_store_has_no_plannotator_reviews_table() {
    let dir = tempfile::tempdir().unwrap();
    let conn = fresh(&dir);
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM sqlite_master WHERE name = 'plannotator_reviews'"
        ),
        0
    );
}

#[test]
fn drop_plannotator_inbox_items_refuse_the_review_waiting_kind() {
    let dir = tempfile::tempdir().unwrap();
    let conn = fresh(&dir);
    assert!(
        conn.execute(INSERT_ITEM, ["review-waiting"]).is_err(),
        "the check must refuse review-waiting"
    );
    conn.execute(INSERT_ITEM, ["plan-review"])
        .expect("plan-review is still accepted");
}

#[test]
fn drop_plannotator_inbox_kind_parse_knows_no_review_waiting() {
    assert!(Kind::parse("review-waiting").is_none());
    assert!(Kind::parse("plan-review").is_some());
}

#[test]
fn drop_plannotator_migration_drops_table_and_open_review_waiting_rows_and_keeps_other_rows() {
    // The store before the migration: every migration that sorts before `drop-plannotator` (all of today's, until it
    // exists); the migration's SQL is applied once it exists.
    let list = named_migrations();
    let at = list
        .iter()
        .position(|(name, ..)| name.ends_with("-drop-plannotator"));
    let dir = tempfile::tempdir().unwrap();
    let conn = prefix_database(at.unwrap_or(list.len()), &home(&dir)).unwrap();
    conn.execute_batch(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', NULL, 'lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plannotator_reviews (session, pn_id, kind, subject, port, opened_at)
           VALUES ('l-pay', 'pn-abcdef', 'plan', 'a plan', 20000, 't');
         INSERT INTO inbox_items (id, kind, urgency, subject, context, label, url, opened_at)
           VALUES (7, 'review-waiting', 'magenta', 'waiting', 'ctx', 'review', NULL, 't'),
                  (8, 'plan-review', 'yellow', 'a plan', 'ctx', 'plan label', 'http://x', 't');",
    )
    .unwrap();
    let before: (i64, String, String, String, Option<String>, String) = conn
        .query_row("SELECT id, kind, urgency, subject, url, label FROM inbox_items WHERE kind = 'plan-review'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .unwrap();
    if let Some(index) = at {
        conn.execute_batch(list[index].2).unwrap();
    }
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM sqlite_master WHERE name = 'plannotator_reviews'"
        ),
        0,
        "the table is dropped"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM inbox_items WHERE kind = 'review-waiting'"
        ),
        0,
        "the review-waiting row is gone"
    );
    let after: (i64, String, String, String, Option<String>, String) = conn
        .query_row("SELECT id, kind, urgency, subject, url, label FROM inbox_items WHERE kind = 'plan-review'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .unwrap();
    assert_eq!(after, before, "the plan-review row is the same row");
    assert_eq!(count(&conn, "SELECT count(*) FROM inbox_items"), 1);
}
