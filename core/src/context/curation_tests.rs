//! Tests of write-time context curation hook (contract §5.9).

use crate::context::{By, Draft, Record};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::store::Store;

#[test]
fn curation_hook_enforces_supersedes() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'C', '2026-11-01T00:00:00.000Z', 'hs');",
            )?;
            Ok(())
        })
        .unwrap();

    let context = ContextId::parse("ctx").unwrap();
    let old_id = RecordId::parse("hs-d1").unwrap();

    // Step 1: Write an initial decision in force
    store
        .write("record hs-d1", |tx| {
            let record = Record {
                id: old_id.clone(),
                at: "2026-11-01T00:00:00.000Z".to_string(),
                by: By {
                    session: None,
                    plan: None,
                },
                text: "The engine communicates over Unix domain socket run/core.sock using NDJSON framing".to_string(),
                draft: Draft::Decision { supersedes: vec![] },
            };
            record.insert(tx, &context)?;
            Ok(())
        })
        .unwrap();

    // Step 2: Attempting to write a near-duplicate record without supersedes or novelty
    // must be refused with ErrorCode::NearDuplicate (exit 3) by the write-time curation hook (contract §5.9).
    // Today, write-time curation hook is not yet implemented, so this write either panics or fails to enforce.
    panic!(
        "curation hook not implemented: near-duplicate write without supersedes or novelty must be refused near_duplicate"
    );
}
