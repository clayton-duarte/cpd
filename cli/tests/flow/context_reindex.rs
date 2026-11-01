//! `cpd context reindex` answered by the holographic engine (docs/research/context-holographic.md
//! `## Index and staleness`; contract §4.6), in this process (`Fx`).
//!
//! The test drives a fake bridge ([`common::FakeBridge`]) through the engine config keys; no test
//! imports or runs holographic. It is red until `context reindex` and the engine client exist.

use crate::common;

use common::FakeBridge;
use common::fx::Fx;
use serde_json::json;

const CTX: &str = "building-cpd";

/// An `Fx` on machine `hs` whose `$CPD_HOME` points the engine at `bridge`, with Context `building-cpd`.
fn env() -> (Fx, FakeBridge) {
    let fx = Fx::new();
    let bridge = FakeBridge::new(&fx.env);
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"hs\"\n{}",
        bridge.config()
    ));
    fx.cpd(&[
        "context",
        "new",
        CTX,
        "--title",
        "Building cpd",
        "--owner",
        "acme",
    ])
    .ok();
    (fx, bridge)
}

/// Records a note and returns its id.
fn note(fx: &Fx, text: &str) -> String {
    fx.cpd(&["context", "note", text, "--context", CTX]).ok()["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Takes `id` out of force with an approved retirement row (a fixture): reindex still projects it.
fn retire(fx: &Fx, id: &str) {
    fx.env.sql(&format!(
        "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at, decided_at, decided_by) \
         VALUES ('{CTX}', '{id}', 'duplicate', 'hs-n1', 'approved', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z', 'user');"
    ));
}

#[test]
fn reindex_resets_and_indexes_every_record_with_no_event() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "The live note.");
    let n2 = note(&fx, "The retired note.");
    retire(&fx, &n2);
    bridge.reply("reset", &json!({"ok": true}).to_string());
    bridge.reply(
        "index",
        &json!({"ok": true, "inserted": 2, "updated": 0}).to_string(),
    );

    let events_before: i64 = fx.env.query("SELECT count(*) FROM events");
    let records_before: i64 = fx.env.query("SELECT count(*) FROM context_records");

    let out = fx.cpd(&["context", "reindex", "--context", CTX]).ok();
    assert_eq!(out["ok"], true, "{out}");

    assert_eq!(
        bridge.commands(),
        ["reset", "index"],
        "reset then one index: {:?}",
        bridge.requests()
    );
    let requests = bridge.requests();
    let indexed: Vec<&str> = requests[1]["facts"]
        .as_array()
        .expect("the index request names its facts")
        .iter()
        .map(|fact| fact["record"].as_str().unwrap())
        .collect();
    assert_eq!(
        indexed,
        [n1.as_str(), n2.as_str()],
        "every record, in force or not: {:?}",
        bridge.requests()
    );

    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events"),
        events_before,
        "reindex appends no Event"
    );
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM context_records"),
        records_before,
        "reindex changes no record"
    );
}
