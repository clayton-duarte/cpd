//! Forward migration preserves prior rows and allows only shaping-origin unlocked completion.

use cpd_core::home::CpdHome;
use cpd_core::store::testing::{named_migrations, prefix_database};
use rusqlite::{Connection, types::Value};

const MIGRATION: &str = "20261115T1200-shaping-origin";
const PLAN_COLUMNS: &str = "id,context,title,jira,phase,lead_session,created_at,locked_at,done_at,archived_at,outline,deleted_at,pr_subject,changes_requested_at,reviewed_repo,reviewed_number,reviewed_title";

fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect()).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

#[test]
fn shaping_origin_migration_preserves_history_and_existing_guards() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let migrations = named_migrations();
    let index = migrations.iter().position(|(name, _, _)| *name == MIGRATION).expect("forward shaping-origin migration is present");
    assert_eq!(index + 1, migrations.len(), "migration sorts after all existing migrations");
    let conn = prefix_database(index, &home).unwrap();
    conn.execute_batch("INSERT INTO txns (id,at,command,pid,cwd) VALUES (1,'t','fixture',1,'/');
        INSERT INTO contexts (id,title,created_at,created_on) VALUES ('ctx','Context','t','m1');
        INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES
          ('lead','lead','ctx','pair','ctx:pair','/','O3','stopped','t','t'),
          ('normallead','lead','ctx','normal','ctx:normal','/','O3','stopped','t','t');
        INSERT INTO plans (id,context,title,phase,lead_session,created_at,done_at) VALUES ('legacy','ctx','Historical','done',NULL,'t','t');
        INSERT INTO plans (id,context,title,phase,lead_session,created_at) VALUES
          ('pair','ctx','Paired','shaping','lead','t'),
          ('oldshape','ctx','Leadless','shaping',NULL,'t'),
          ('normal','ctx','Normal','grounding','normallead','t');
        INSERT INTO jobs (plan,workflow,key,title,status,appended,created_txn,created_at,kind) VALUES ('legacy',NULL,'scout','Scout','draft',1,1,'t','scout');
        INSERT INTO events (txn,at,kind,context,plan,text,data) VALUES (1,'t','plan-created','ctx','legacy','Historical','{}');
        INSERT INTO context_records (context,id,kind,at,machine,by_plan,text) VALUES ('ctx','m1-n1','note','t','m1','legacy','Historical');").unwrap();
    let plans = rows(&conn, &format!("SELECT {PLAN_COLUMNS} FROM plans ORDER BY id"));
    let jobs = rows(&conn, "SELECT * FROM jobs ORDER BY seq");
    let events = rows(&conn, "SELECT * FROM events ORDER BY id");
    let history = rows(&conn, "SELECT * FROM history ORDER BY id");
    let records = rows(&conn, "SELECT * FROM context_records ORDER BY seq");
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.pragma_update(None, "legacy_alter_table", "ON").unwrap();
    conn.execute_batch(migrations[index].2).unwrap();
    assert_eq!(rows(&conn, &format!("SELECT {PLAN_COLUMNS} FROM plans ORDER BY id")), plans);
    assert_eq!(rows(&conn, "SELECT * FROM jobs ORDER BY seq"), jobs);
    assert_eq!(rows(&conn, "SELECT * FROM events ORDER BY id"), events);
    assert_eq!(rows(&conn, "SELECT * FROM history ORDER BY id"), history);
    assert_eq!(rows(&conn, "SELECT * FROM context_records ORDER BY seq"), records);
    assert_eq!(rows(&conn, "SELECT id,shaping_origin FROM plans ORDER BY id"), vec![
        vec![Value::Text("legacy".into()), Value::Integer(0)],
        vec![Value::Text("normal".into()), Value::Integer(0)],
        vec![Value::Text("oldshape".into()), Value::Integer(1)],
        vec![Value::Text("pair".into()), Value::Integer(1)],
    ]);
    assert!(rows(&conn, "PRAGMA foreign_key_check").is_empty());
    conn.execute_batch("UPDATE plans SET phase='done',done_at='t2' WHERE id='pair'").unwrap();
    assert_eq!(rows(&conn, "SELECT phase,locked_at FROM plans WHERE id='pair'"), vec![vec![Value::Text("done".into()), Value::Null]]);
    assert!(conn.execute_batch("UPDATE plans SET shaping_origin=1 WHERE id='normal'").is_err());
    assert!(conn.execute_batch("UPDATE plans SET phase='done',done_at='t2' WHERE id='normal'").is_err());
    assert!(conn.execute_batch("UPDATE plans SET phase='dispatch',locked_at='t2' WHERE id='oldshape'").is_err());
    assert!(conn.execute_batch("UPDATE plans SET locked_at='t2' WHERE id='oldshape'").is_err());
    conn.execute_batch("UPDATE plans SET phase='plan',done_at=NULL WHERE id='pair'; UPDATE plans SET phase='dispatch',locked_at='t3' WHERE id='pair'; UPDATE plans SET phase='deploy' WHERE id='pair'; UPDATE plans SET phase='done',done_at='t4' WHERE id='pair';").unwrap();
    assert_eq!(rows(&conn, "SELECT phase,locked_at FROM plans WHERE id='pair'"), vec![vec![Value::Text("done".into()), Value::Text("t3".into())]]);
}
