//! Engine step E8 opens no `curate-docs` Inbox item and closes one an older engine left open (contract §2.2 "Dropped
//! kind", §3.7; drop-curate-docs). Old `record_curations` rows with a pending doc candidate are inert history.

use crate::common::fx::Fx;
use cpd_core::context::{By, Draft, Record};
use cpd_core::ids::{ContextId, RecordId};
use serde_json::Value;

/// The engine over a Context `building` with two records, each curated with a pending doc candidate.
fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.engine_config("context_curation_min_records = 1");
    fx.engine_routes();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('building', 'B', 't', 'm1');",
    );
    let context = ContextId::parse("building").unwrap();
    fx.env
        .store()
        .write("fixture", |tx| {
            for id in ["hs-n1", "hs-n2"] {
                let record = Record {
                    id: RecordId::parse(id).unwrap(),
                    at: "2026-10-20T10:00:00.000Z".into(),
                    by: By {
                        session: None,
                        plan: None,
                    },
                    text: format!("text of {id}"),
                    draft: Draft::Note,
                };
                record.insert(tx, &context).unwrap();
            }
            Ok(())
        })
        .unwrap();
    fx.env.sql(
        "INSERT INTO record_curations (context, record, topic, short, doc_candidate, doc_paths, curated_at) VALUES
           ('building', 'hs-n1', 'engine', 'short 1', 'pending', '[\"docs/a.md\"]', 't'),
           ('building', 'hs-n2', 'engine', 'short 2', 'pending', '[\"docs/a.md\"]', 't');",
    );
    fx
}

fn items(fx: &Fx, args: &[&str]) -> Vec<Value> {
    let json = fx.cpd(args).ok();
    json["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "curate-docs")
        .cloned()
        .collect()
}

#[test]
fn e8_opens_no_curate_docs_item_for_a_pending_doc_candidate() {
    let fx = fx();
    fx.watch_ticks(1).unwrap();
    let open = items(&fx, &["inbox", "list"]);
    assert!(open.is_empty(), "no curate-docs item: {open:?}");
}

#[test]
fn e8_closes_an_open_curate_docs_item_an_older_engine_opened() {
    let fx = fx();
    fx.env.sql(
        "INSERT INTO inbox_items (kind, urgency, subject, context, label, opened_at)
           VALUES ('curate-docs', 'cyan', 'curate-docs:building', 'building', 'building: 2 doc candidates', '2026-01-01T00:00:00.000Z');",
    );
    fx.watch_ticks(1).unwrap();
    let open = items(&fx, &["inbox", "list"]);
    assert!(open.is_empty(), "the item is closed: {open:?}");
    let closed = items(&fx, &["inbox", "list", "--closed"]);
    assert!(
        closed
            .iter()
            .any(|i| i["subject"] == "curate-docs:building"),
        "the closed list has it: {closed:?}"
    );
}
