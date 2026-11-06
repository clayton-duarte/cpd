//! pro-d401: unread is cyan; the notices `curate-retire` and `models-changed` are white.

use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Kind, Urgency};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
const T: &str = "2026-01-01T00:00:00.000Z";

fn open() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn sql(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok(())
        })
        .unwrap();
}

fn of(store: &mut Store, home: &CpdHome, kind: Kind) -> Vec<Condition> {
    conditions_with(store, home, NOW, 20, 1, None)
        .unwrap()
        .into_iter()
        .filter(|c| c.kind == kind)
        .collect()
}

fn only(found: Vec<Condition>) -> Condition {
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

const CONTEXT: &str = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '2026-01-01T00:00:00.000Z', 'm1');";

#[test]
fn an_unread_item_is_cyan_not_white() {
    let (_dir, home, mut store) = open();
    sql(
        &mut store,
        &format!(
            "{CONTEXT}
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l-pay', '{T}', '{T}', NULL);
             INSERT INTO events (txn, at, kind, context, plan, job, session, text, data)
               VALUES (cpd_txn(), '{T}', 'session-state', 'ctx', NULL, NULL, 'l-pay', 'x',
                       '{{\"from\":\"working\",\"label\":null,\"to\":\"idle\",\"reply\":true}}');"
        ),
    );
    let c = only(of(&mut store, &home, Kind::Unread));
    assert_eq!(c.urgency, Urgency::Cyan);
}

#[test]
fn a_curate_retire_notice_is_white() {
    let (_dir, home, mut store) = open();
    sql(
        &mut store,
        &format!(
            "{CONTEXT}
             INSERT INTO context_records (context, id, kind, at, machine, text)
               VALUES ('ctx', 'n1', 'note', '{T}', 'm1', 'a note');
             INSERT INTO record_retirements (context, record, reason, target, status, proposed_at)
               VALUES ('ctx', 'n1', 'duplicate', 'n1', 'pending', '{T}');"
        ),
    );
    let c = only(of(&mut store, &home, Kind::CurateRetire));
    assert_eq!(c.urgency, Urgency::White);
}

#[test]
fn a_models_changed_notice_is_white() {
    let (_dir, home, mut store) = open();
    let catalog = serde_json::json!({
        "pi_version": "0.0.0",
        "source": {"kind": "openai-models", "auth": "none", "base": null},
        "ids": ["gpt-new"],
    });
    let changes = serde_json::json!({"at": "2026-01-01T00:00:00Z", "appeared": ["gpt-new"], "went_away": []});
    sql(
        &mut store,
        &format!(
            "INSERT INTO swr_models (provider, catalog_json, changes_json, polled_at)
             VALUES ('acme', '{catalog}', '{changes}', '2026-01-01T00:00:00Z');"
        ),
    );
    let c = only(of(&mut store, &home, Kind::ModelsChanged));
    assert_eq!(c.urgency, Urgency::White);
}
