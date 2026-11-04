//! Hybrid `cpd context search` (the Plan ctx-fts-remove-holo, pro-d12), run in this process (`Fx`): the vectors are
//! seeded through `VectorStore::put_hashed`, and the query embedding comes from the harness's `FakeRunner` `node`
//! route, which answers the embedder helper's JSON line. Nothing runs. Assumed new keys: `mode` (`fts`, `hybrid`,
//! `fts-only`) and, with `fts-only`, `semantic_error`.

use crate::common;

use common::fx::Fx;
use cpd_core::context::curate_vectors::VectorStore;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{ContextId, RecordId};
use serde_json::{Value, json};

const CTX: &str = "building-cpd";
const MODEL: &str = "vertex/gemini-embedding-2";
const DIMS: usize = 768;

/// Machine `hs`, pi installed (so the embedder resolves), Context `building-cpd`.
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
    fx
}

fn record(fx: &Fx, verb: &str, text: &str) -> String {
    fx.cpd(&["context", verb, text, "--context", CTX]).ok()["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn note(fx: &Fx, text: &str) -> String {
    record(fx, "note", text)
}

/// A 768-dim vector with the two leading components `a` and `b`: its cosine with `vector(1, 0)` is a / norm.
fn vector(a: f32, b: f32) -> Vec<f32> {
    let mut v = vec![0.0; DIMS];
    v[0] = a;
    v[1] = b;
    v
}

/// The query embedding: cosine 1 with `vector(1, 0)`.
fn answer_with_query(fx: &Fx) {
    let reply = json!({"ok": true, "vector": vector(1.0, 0.0), "model": MODEL, "tokens": 3});
    fx.fake("node", Reply::ok(&format!("{reply}\n")));
}

/// A vector for `id` whose cosine with the query is about `cosine`.
fn seed(fx: &Fx, id: &str, cosine: f32) {
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into_os_string()), None).unwrap();
    VectorStore::at(&home)
        .put_hashed(
            &ContextId::parse(CTX).unwrap(),
            MODEL,
            &RecordId::parse(id).unwrap(),
            &vector(cosine, (1.0 - cosine * cosine).sqrt()),
            "hash",
        )
        .unwrap();
}

fn search(fx: &Fx, query: &str, extra: &[&str]) -> Value {
    let mut args = vec!["context", "search", query, "--context", CTX];
    args.extend(extra);
    fx.cpd(&args).ok()
}

fn ids(out: &Value) -> Vec<String> {
    out["records"]
        .as_array()
        .unwrap_or_else(|| panic!("{out}"))
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

fn pending_duplicate(fx: &Fx, id: &str, target: &str) {
    fx.env.sql(&format!(
        "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at, decided_at, decided_by) \
         VALUES ('{CTX}', '{id}', 'duplicate', '{target}', 'pending', '2026-01-01T00:00:00.000Z', NULL, NULL);"
    ));
}

#[test]
fn a_context_without_vectors_searches_fts_only_and_never_calls_the_embedder() {
    let fx = fx();
    let n1 = note(&fx, "Cache invalidation rules.");
    note(&fx, "Unrelated words entirely.");
    let out = search(&fx, "cache", &[]);
    assert_eq!(out["mode"], "fts", "{out}");
    assert_eq!(ids(&out), [n1], "{out}");
    assert!(
        fx.calls("node").is_empty(),
        "no vectors: the query is never embedded"
    );
}

#[test]
fn a_context_with_vectors_fuses_the_query_embedding() {
    let fx = fx();
    let n1 = note(&fx, "Cache invalidation rules.");
    let n2 = note(&fx, "Unrelated words entirely.");
    seed(&fx, &n1, 0.8);
    seed(&fx, &n2, 0.9);
    answer_with_query(&fx);
    let out = search(&fx, "cache", &[]);
    assert_eq!(out["mode"], "hybrid", "{out}");
    // n1 is found by both legs; n2 only by cosine.
    assert_eq!(ids(&out), [n1, n2], "{out}");
    assert_eq!(fx.calls("node").len(), 1);
}

#[test]
fn an_unreachable_embedder_falls_back_to_fts_only_and_says_so() {
    let fx = fx();
    let n1 = note(&fx, "Cache invalidation rules.");
    let n2 = note(&fx, "Unrelated words entirely.");
    seed(&fx, &n1, 0.8);
    seed(&fx, &n2, 0.9);
    let down = json!({"ok": false, "error": {"code": "embedding_unreachable", "message": "down"}});
    fx.fake("node", Reply::ok(&format!("{down}\n")));
    let out = search(&fx, "cache", &[]);
    assert_eq!(out["mode"], "fts-only", "{out}");
    assert_eq!(out["semantic_error"], "embedding_unreachable", "{out}");
    assert_eq!(ids(&out), [n1], "{out}");
}

#[test]
fn a_semantic_hit_below_the_cosine_floor_is_dropped() {
    let fx = fx();
    let n1 = note(&fx, "Cache invalidation rules.");
    let n2 = note(&fx, "Unrelated words entirely.");
    seed(&fx, &n1, 0.8);
    seed(&fx, &n2, 0.3);
    answer_with_query(&fx);
    let out = search(&fx, "cache", &[]);
    assert_eq!(out["mode"], "hybrid", "{out}");
    assert_eq!(ids(&out), [n1], "{out}");
}

#[test]
fn the_record_named_by_id_stays_first_in_hybrid() {
    let fx = fx();
    note(&fx, "First filler note.");
    let n2 = note(&fx, "Second filler note.");
    let n3 = note(&fx, "Third filler note.");
    seed(&fx, &n2, 0.7);
    seed(&fx, &n3, 0.95);
    answer_with_query(&fx);
    let out = search(&fx, &n2, &[]);
    assert_eq!(out["mode"], "hybrid", "{out}");
    let found = ids(&out);
    assert_eq!(found[0], n2, "{out}");
    assert!(found.contains(&n3), "{out}");
}

#[test]
fn hidden_records_never_enter_the_semantic_leg() {
    let fx = fx();
    let n1 = note(&fx, "Cache invalidation rules.");
    let n2 = note(&fx, "Hidden duplicate words.");
    let d1 = record(&fx, "decide", "Decision about other words.");
    seed(&fx, &n1, 0.8);
    seed(&fx, &n2, 0.9);
    seed(&fx, &d1, 0.85);
    pending_duplicate(&fx, &n2, &n1);
    answer_with_query(&fx);
    let out = search(&fx, "cache", &[]);
    assert_eq!(out["mode"], "hybrid", "{out}");
    assert_eq!(ids(&out), [n1.clone(), d1], "{out}");
    let notes = search(&fx, "cache", &["--kind", "note"]);
    assert_eq!(ids(&notes), [n1], "{notes}");
}

#[test]
fn records_without_a_vector_still_rank_lexically() {
    let fx = fx();
    let n1 = note(&fx, "Cache invalidation rules.");
    let n2 = note(&fx, "Unrelated words entirely.");
    seed(&fx, &n2, 0.9);
    answer_with_query(&fx);
    let out = search(&fx, "cache", &[]);
    assert_eq!(out["mode"], "hybrid", "{out}");
    let mut found = ids(&out);
    found.sort();
    assert_eq!(found, [n1, n2], "{out}");
}
