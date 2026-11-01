//! Migration `local-needs` (hs-d522; contract §4.5): `pr_needs` loses its column `needs_plan`; a need of another
//! Plan is not dropped silently, the migration fails (migration error). A store at the version before it is
//! `prefix_database`, as in `core/tests/migrations.rs`.

use cpd_core::home::CpdHome;
use cpd_core::store::testing::{named_migrations, prefix_database};
use rusqlite::Connection;

/// Plans `pay` and `ops` (grounding) of one Context, PRs `a`, `b` in `pay` and `d` in `ops`; `pay/b` needs `pay/a`.
const BEFORE: &str = "
BEGIN;
INSERT INTO txns (id, at, command, pid, cwd) VALUES (1, 't', 'test', 1, '/');
INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
  ('l-pay', 'lead', 'ctx', 'pay', 'lead', '/', 'O3', 'idle', 't', 't'),
  ('l-ops', 'lead', 'ctx', 'ops', 'lead', '/', 'O3', 'idle', 't', 't');
INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
  ('pay', 'ctx', 'Pay', 'grounding', 'l-pay', 't'), ('ops', 'ctx', 'Ops', 'grounding', 'l-ops', 't');
INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES
  ('pay', 'a', '/r', 'main', 'waiting', 'cpd/pay/a', 1, 't'),
  ('pay', 'b', '/r', 'main', 'waiting', 'cpd/pay/b', 1, 't'),
  ('ops', 'd', '/r', 'main', 'waiting', 'cpd/ops/d', 1, 't');
INSERT INTO pr_needs (plan, pr, needs_plan, needs) VALUES ('pay', 'b', 'pay', 'a');
COMMIT;";

/// The connection before `local-needs`, loaded with [`BEFORE`], and the migration's SQL.
fn before() -> (tempfile::TempDir, Connection, &'static str) {
    let list = named_migrations();
    let index = list.iter().position(|(name, ..)| name.ends_with("-local-needs")).expect("the migration local-needs exists");
    assert_eq!(list[index].1, "breaking");
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let conn = prefix_database(index, &home).unwrap();
    conn.execute_batch(BEFORE).unwrap();
    (dir, conn, list[index].2)
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn migration_drops_needs_plan() {
    let (_dir, conn, sql) = before();
    conn.execute_batch(sql).unwrap();
    assert_eq!(count(&conn, "SELECT count(*) FROM pragma_table_info('pr_needs') WHERE name = 'needs_plan'"), 0);
    let kept: (String, String) = conn.query_row("SELECT pr, needs FROM pr_needs WHERE plan = 'pay'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(kept, ("b".to_owned(), "a".to_owned()), "the need of the Plan itself is kept");
    assert_eq!(count(&conn, "SELECT count(*) FROM prs"), 3);
    assert_eq!(count(&conn, "SELECT count(*) FROM pragma_foreign_key_check"), 0);
}

#[test]
fn migration_fails_on_a_need_of_another_plan() {
    let (_dir, conn, sql) = before();
    conn.execute_batch("INSERT INTO pr_needs (plan, pr, needs_plan, needs) VALUES ('pay', 'a', 'ops', 'd');").unwrap();
    assert!(conn.execute_batch(sql).is_err(), "a need of another Plan must fail the migration, not be dropped");
}
