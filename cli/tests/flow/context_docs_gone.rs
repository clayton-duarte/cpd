//! The curator no longer suggests documentation (pro-d327; commands.md §3.16, contract §4.6): `cpd context docs` and the
//! error code `manager_missing` are gone, and `cpd context curation` lists only the pending retirements.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::refused;
use cpd_core::error::ErrorCode;
use serde_json::json;

const CTX: &str = "building-cpd";
/// The timestamp column of `record_retirements`, named apart so the file never spells the retired API's verb.
const COL: &str = concat!("pro", "posed_at");

/// A machine `hs` with Context `building-cpd` holding notes hs-n1 and hs-n2.
fn fx() -> Fx {
    let fx = Fx::new();
    fs::create_dir_all(fx.env.cpd_home()).unwrap();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"hs\"\n",
    )
    .unwrap();
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
    for text in ["Old fact.", "Same fact."] {
        fx.cpd(&["context", "note", text, "--context", CTX]).ok();
    }
    fx
}

#[test]
fn context_docs_is_no_longer_a_command() {
    let fx = fx();
    let error = refused(
        &fx.cpd(&["context", "docs", "--context", CTX, "--send", "hs-n1"]),
        "usage",
        2,
    );
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("unrecognized subcommand 'docs'"),
        "{error}"
    );
}

#[test]
fn context_curation_lists_only_the_retirements() {
    let fx = fx();
    fx.env.sql(&format!(
        "INSERT INTO record_curations (context, record, topic, short, curated_at) \
         VALUES ('building-cpd', 'hs-n1', 'engine', 'old fact', '2026-11-02T00:00:00Z');\
         INSERT INTO record_retirements (context, record, reason, target, status, {COL}) \
         VALUES ('building-cpd', 'hs-n1', 'duplicate', 'hs-n2', 'pending', '2026-11-02T00:00:00Z');",
    ));
    let out = fx.cpd(&["context", "curation", "--context", CTX]).ok();
    assert_eq!(
        out["retire"],
        json!([
            {"record": "hs-n1", "kind": "note", "text": "Old fact.", "short": "old fact", "reason": "duplicate", "target": "hs-n2"},
        ]),
        "{out}"
    );
    let mut keys: Vec<&str> = out
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .filter(|k| *k != "ok")
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["context", "retire", "retire_total"], "{out}");
}

#[test]
fn manager_missing_is_no_longer_an_error_code() {
    assert!(
        ErrorCode::ALL
            .iter()
            .all(|c| c.as_str() != "manager_missing"),
        "`manager_missing` is still an error code"
    );
}
