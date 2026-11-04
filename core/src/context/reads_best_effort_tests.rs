//! The best-effort wrappers of `context::reads` (decision pro-d10 item 5, A4): never a failure, nothing without a Session.
//! The environment is process-global and this crate's tests do not set it for these variables, so only the unset case is tested.

use super::reads::{Channel, Read, log_reads, mark_used};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::store::Store;

#[test]
fn the_wrappers_are_silent_without_a_session() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c1', 'C1', 't', 'm1');
                 INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
                   ('c1', 'm1-d1', 'decision', 't', 'm1', 'x');",
            )?;
            Ok(())
        })
        .unwrap();
    let context = ContextId::parse("c1").unwrap();
    let record: RecordId = "m1-d1".parse().unwrap();
    let reads = [Read {
        record: record.clone(),
        rank: None,
        score: None,
    }];
    // The wrappers return nothing: reaching the count below is the "returns normally" check.
    log_reads(&mut store, &context, Channel::Show, &reads);
    mark_used(&mut store, &context, "see m1-d1", &[]);
    let count: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM context_reads", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(count, 0);
}
