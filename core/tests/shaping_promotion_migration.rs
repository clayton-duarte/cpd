//! The promotion migration changes only the permitted phase transitions, never stored history.

use cpd_core::home::CpdHome;
use cpd_core::store::testing::{named_migrations, prefix_database};
use rusqlite::{Connection, types::Value};

const MIGRATION: &str = "20261115T1201-shaping-promotion";

fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn promotion_trigger_preserves_rows_and_other_guards() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let migrations = named_migrations();
    let index = migrations
        .iter()
        .position(|(name, _, _)| *name == MIGRATION)
        .unwrap();
    assert_eq!(index + 1, migrations.len());
    let conn = prefix_database(index, &home).unwrap();
    conn.execute_batch("INSERT INTO txns (id,at,command,pid,cwd) VALUES (1,'t','fixture',1,'/');
        INSERT INTO contexts (id,title,created_at,created_on) VALUES ('ctx','Context','t','m1');
        INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES
          ('lead','lead','ctx','pair','Lead','/','O3','stopped','t','t'),
          ('normallead','lead','ctx','normal','Normal','/','O3','stopped','t','t'),
          ('lockedlead','lead','ctx','locked','Locked','/','O3','stopped','t','t');
        INSERT INTO plans (id,context,title,phase,lead_session,created_at,shaping_origin) VALUES
          ('pair','ctx','Paired','shaping','lead','t',1),
          ('orphan','ctx','Orphan','shaping',NULL,'t',1),
          ('normal','ctx','Normal','plan','normallead','t',0);
        INSERT INTO plans (id,context,title,phase,lead_session,created_at,locked_at,shaping_origin)
          VALUES ('locked','ctx','Locked','plan','lockedlead','t','t',1);
        INSERT INTO plans (id,context,title,phase,lead_session,created_at,done_at,shaping_origin)
          VALUES ('legacy','ctx','Historical','done',NULL,'t','t',0);
        INSERT INTO jobs (plan,workflow,key,title,status,appended,created_txn,created_at,kind)
          VALUES ('legacy',NULL,'scout','Scout','draft',1,1,'t','scout');
        INSERT INTO events (txn,at,kind,context,plan,text,data) VALUES (1,'t','plan-created','ctx','legacy','Historical','{}');
        INSERT INTO context_records (context,id,kind,at,machine,by_plan,text)
          VALUES ('ctx','m1-n1','note','t','m1','legacy','Historical');").unwrap();
    let queries = [
        "SELECT * FROM plans ORDER BY id",
        "SELECT * FROM jobs ORDER BY seq",
        "SELECT * FROM events ORDER BY id",
        "SELECT * FROM history ORDER BY id",
        "SELECT * FROM context_records ORDER BY seq",
    ];
    let before: Vec<_> = queries.iter().map(|q| rows(&conn, q)).collect();
    conn.execute_batch(migrations[index].2).unwrap();
    for (query, expected) in queries.iter().zip(before) {
        assert_eq!(rows(&conn, query), expected, "{query}");
    }
    assert!(rows(&conn, "PRAGMA foreign_key_check").is_empty());
    assert!(
        conn.execute_batch("UPDATE plans SET phase='plan' WHERE id='orphan'")
            .is_err()
    );
    assert!(
        conn.execute_batch("UPDATE plans SET phase='done',done_at='t2' WHERE id='normal'")
            .is_err()
    );
    assert!(
        conn.execute_batch("UPDATE plans SET phase='done',done_at='t2' WHERE id='locked'")
            .is_err()
    );
    assert!(
        conn.execute_batch("UPDATE plans SET shaping_origin=1 WHERE id='normal'")
            .is_err()
    );
    assert!(
        conn.execute_batch("UPDATE plans SET locked_at='t2' WHERE id='orphan'")
            .is_err()
    );
    conn.execute_batch(
        "UPDATE plans SET phase='plan' WHERE id='pair';
        UPDATE plans SET phase='done',done_at='t2' WHERE id='pair';
        UPDATE plans SET phase='plan',done_at=NULL WHERE id='pair';
        UPDATE plans SET phase='dispatch',locked_at='t3' WHERE id='pair';
        UPDATE plans SET phase='deploy' WHERE id='pair';
        UPDATE plans SET phase='done',done_at='t4' WHERE id='pair';",
    )
    .unwrap();
    assert_eq!(
        rows(&conn, "SELECT phase,locked_at FROM plans WHERE id='pair'"),
        vec![vec![Value::Text("done".into()), Value::Text("t3".into())]]
    );
}
