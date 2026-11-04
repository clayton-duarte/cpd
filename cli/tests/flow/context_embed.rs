//! `cpd context embed` (the Plan ctx-fts-remove-holo, pro-d12), run in this process (`Fx`): the embedder is the real
//! `BifrostEmbedding` over the harness's `FakeRunner`, whose `node` route answers the helper's JSON line. Nothing runs.
//! Assumed output shape: `{"context", "embedded", "skipped", "remaining"}`.

use crate::common;

use common::fx::Fx;
use cpd_core::context::curate_vectors::VectorStore;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{ContextId, RecordId};
use serde_json::{Value, json};

const CTX: &str = "building-cpd";
const MODEL: &str = "vertex/gemini-embedding-2";

/// Machine `hs`, pi installed (so `locate_pi` resolves), Context `building-cpd` holding the in-force decisions
/// hs-d1 and hs-d2 and the note hs-n2; the note hs-n1 is superseded by hs-d2, so it is out of force.
fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"hs\"\n");
    fx.install_pi();
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
    fx.cpd(&["context", "note", "Old fact.", "--context", CTX])
        .ok();
    fx.cpd(&["context", "note", "Kept fact.", "--context", CTX])
        .ok();
    fx.cpd(&["context", "decide", "First decision.", "--context", CTX])
        .ok();
    fx.cpd(&[
        "context",
        "decide",
        "Second decision.",
        "--supersedes",
        "hs-n1",
        "--context",
        CTX,
    ])
    .ok();
    fx
}

/// The helper's reply for one text: a 768-dim vector of the model.
fn helper_reply() -> Reply {
    let answer = json!({"ok": true, "vector": vec![0.5_f64; 768], "model": MODEL, "tokens": 3});
    Reply::ok(&format!("{answer}\n"))
}

fn store(fx: &Fx) -> VectorStore {
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into_os_string()), None).unwrap();
    VectorStore::at(&home)
}

fn embedded_ids(fx: &Fx) -> Vec<String> {
    let mut ids: Vec<String> = store(fx)
        .embedded(&ContextId::parse(CTX).unwrap(), MODEL)
        .unwrap()
        .iter()
        .map(RecordId::to_string)
        .collect();
    ids.sort();
    ids
}

fn counts(out: &Value) -> (u64, u64, u64) {
    (
        out["embedded"].as_u64().unwrap_or_else(|| panic!("{out}")),
        out["skipped"].as_u64().unwrap_or_else(|| panic!("{out}")),
        out["remaining"].as_u64().unwrap_or_else(|| panic!("{out}")),
    )
}

#[test]
fn context_embed_reports_embedded_skipped_and_remaining() {
    let fx = fx();
    fx.fake("node", helper_reply());
    let first = fx
        .cpd(&["context", "embed", "--context", CTX, "--limit", "2"])
        .ok();
    assert_eq!(first["context"], CTX);
    assert_eq!(counts(&first), (2, 0, 1), "{first}");
    assert_eq!(embedded_ids(&fx).len(), 2);
    let second = fx.cpd(&["context", "embed", "--context", CTX]).ok();
    assert_eq!(counts(&second), (1, 2, 0), "{second}");
    // The superseded note is never embedded.
    assert_eq!(embedded_ids(&fx), ["hs-d1", "hs-d2", "hs-n2"]);
    let third = fx.cpd(&["context", "embed", "--context", CTX]).ok();
    assert_eq!(counts(&third), (0, 3, 0), "{third}");
    assert_eq!(
        fx.calls("node").len(),
        3,
        "an unchanged row is never embedded again"
    );
}

#[test]
fn context_embed_is_not_gated_by_the_write_curation_flag() {
    let fx = fx();
    fx.cpd(&["config", "experimental", "write_curation", "off"])
        .ok();
    fx.fake("node", helper_reply());
    let out = fx.cpd(&["context", "embed", "--context", CTX]).ok();
    assert_eq!(counts(&out), (3, 0, 0), "{out}");
    assert_eq!(embedded_ids(&fx), ["hs-d1", "hs-d2", "hs-n2"]);
}

#[test]
fn context_embed_with_the_embedder_down_is_embedding_unreachable_and_keeps_written_rows() {
    let fx = fx();
    fx.fake_once("node", helper_reply());
    fx.fake("node", Reply::exit(1, "", ""));
    let run = fx.cpd(&["context", "embed", "--context", CTX]);
    assert_eq!(run.error()["code"], "embedding_unreachable", "{run:?}");
    assert_eq!(
        embedded_ids(&fx).len(),
        1,
        "the row written before the failure stays"
    );
}
