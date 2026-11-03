//! The table `context_reads` (decision pro-d10 item 5): one row per (Context, record, Session, channel) read, a
//! STRICT history table whose rows a Context delete may remove (`no_delete` 0).

use crate::error::CpdError;
use crate::store::Store;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?))
        .unwrap()
}

fn open() -> (tempfile::TempDir, Store) {
    let (dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    write(
        &mut store,
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n1', 'note', 't', 'm1', 'x');",
    )
    .unwrap();
    (dir, store)
}

fn insert(channel: &str, rank: &str, used: &str, session: &str, record: &str) -> String {
    format!(
        "INSERT INTO context_reads (context, record, session, channel, read_at, rank, score, used)
         VALUES ('ctx', '{record}', '{session}', '{channel}', 't', {rank}, NULL, {used});"
    )
}

#[test]
fn the_table_has_the_contract_columns_and_primary_key() {
    let (_dir, mut store) = open();
    let columns: Vec<(String, i64)> = store
        .read(|c| {
            let mut s =
                c.prepare("SELECT name, pk FROM pragma_table_info('context_reads') ORDER BY cid")?;
            Ok(s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    let names: Vec<&str> = columns.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "context", "record", "session", "channel", "read_at", "rank", "score", "used"
        ]
    );
    let pk: Vec<(&str, i64)> = columns
        .iter()
        .filter(|(_, pk)| *pk > 0)
        .map(|(n, pk)| (n.as_str(), *pk))
        .collect();
    let mut by_position = pk.clone();
    by_position.sort_by_key(|(_, p)| *p);
    assert_eq!(
        by_position,
        [
            ("context", 1),
            ("record", 2),
            ("session", 3),
            ("channel", 4)
        ]
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT strict FROM pragma_table_list('context_reads')"
        ),
        1
    );
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'context_reads_record' AND tbl_name = 'context_reads'"
        ),
        1
    );
    write(&mut store, &insert("pushed", "NULL", "NULL", "s1", "m1-n1"))
        .expect_err("a NULL used is fine, a duplicate is not");
}

#[test]
fn the_checks_and_foreign_key_refuse_bad_rows() {
    let (_dir, mut store) = open();
    write(&mut store, &insert("pushed", "1", "0", "s1", "m1-n1")).expect("a good row");
    for (what, sql) in [
        ("channel", insert("bogus", "NULL", "NULL", "s1", "m1-n1")),
        ("rank 0", insert("search", "0", "NULL", "s1", "m1-n1")),
        ("used 2", insert("show", "NULL", "2", "s1", "m1-n1")),
        (
            "empty session",
            insert("shadow", "NULL", "NULL", "", "m1-n1"),
        ),
        (
            "unknown record",
            insert("show", "NULL", "NULL", "s1", "m1-n9"),
        ),
        (
            "duplicate key",
            insert("pushed", "NULL", "NULL", "s1", "m1-n1"),
        ),
    ] {
        let err = write(&mut store, &sql).expect_err(what);
        assert!(
            err.details()["sqlite_message"].as_str().is_some(),
            "{what}: {err:?}"
        );
    }
    assert_eq!(count(&mut store, "SELECT count(*) FROM context_reads"), 1);
}

#[test]
fn it_is_a_history_table_without_no_delete() {
    let (_dir, mut store) = open();
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history_tables WHERE name = 'context_reads' AND no_delete = 0"
        ),
        1
    );
    write(&mut store, &insert("pushed", "NULL", "NULL", "s1", "m1-n1")).unwrap();
    write(&mut store, "UPDATE context_reads SET used = 1").unwrap();
    for op in ["insert", "update"] {
        assert_eq!(
            count(
                &mut store,
                &format!(
                    "SELECT count(*) FROM history WHERE tbl = 'context_reads' AND op = '{op}'"
                )
            ),
            1,
            "{op}"
        );
    }
    write(&mut store, "DELETE FROM context_reads").expect("a delete is allowed");
    assert_eq!(count(&mut store, "SELECT count(*) FROM context_reads"), 0);
    assert_eq!(
        count(
            &mut store,
            "SELECT count(*) FROM history WHERE tbl = 'context_reads' AND op = 'delete'"
        ),
        1
    );
}
