//! `context delete` removes the Context's provisional topics (decision pro-d11; contract §2.2 "Deleted").

use super::delete_records;
use crate::home::CpdHome;
use crate::ids::ContextId;
use crate::store::Store;

#[test]
fn deleting_a_context_deletes_its_provisional_topics() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES
                   ('gone', 'G', '2026-11-01T00:00:00.000Z', 'hs'), ('kept', 'K', '2026-11-01T00:00:00.000Z', 'hs');
                 INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
                   ('gone', 'hs-n1', 'note', '2026-11-01T00:00:00.000Z', 'hs', 'x'),
                   ('kept', 'hs-n1', 'note', '2026-11-01T00:00:00.000Z', 'hs', 'x');
                 INSERT INTO record_topics (context, record, topic, model, at) VALUES
                   ('gone', 'hs-n1', 'engine', 'kev-4b', '2026-11-02T00:00:00.000Z'),
                   ('kept', 'hs-n1', 'engine', 'kev-4b', '2026-11-02T00:00:00.000Z');",
            )?;
            Ok(())
        })
        .unwrap();
    let gone = ContextId::parse("gone").unwrap();
    store
        .write("context delete", |tx| {
            delete_records(tx, &gone)?;
            Ok(())
        })
        .unwrap();
    let rows = |store: &mut Store, context: &str| -> i64 {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*) FROM record_topics WHERE context = ?1",
                    [context],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    assert_eq!(rows(&mut store, "gone"), 0);
    assert_eq!(rows(&mut store, "kept"), 1);
}
