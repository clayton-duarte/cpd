//! Tests of the `context embed` backfill, `embed_records`, over `FakeEmbedding` and a tempdir home.

use tempfile::TempDir;

use super::curate_vectors::{VectorStore, content_hash};
use super::embed::{EmbedReport, embed_records, embed_run};
use crate::clock::{Clock, ManualClock};
use crate::embedding::Embedded;
use crate::error::{CpdError, ErrorCode};
use crate::fakes::embedding::FakeEmbedding;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};

const MODEL: &str = "vertex/gemini-embedding-2";
const DIMS: usize = 768;

fn home() -> (TempDir, CpdHome) {
    let dir = TempDir::new().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    (dir, home)
}

fn rid(text: &str) -> RecordId {
    RecordId::parse(text).unwrap()
}

fn cid() -> ContextId {
    ContextId::parse("cpd").unwrap()
}

fn embedded() -> Embedded {
    Embedded {
        vector: vec![0.5; DIMS],
        model: MODEL.to_owned(),
        tokens: None,
    }
}

/// Newest first, as the command passes them.
fn records() -> Vec<(RecordId, String)> {
    vec![
        (rid("hs-d3"), "third".to_owned()),
        (rid("hs-n2"), "second".to_owned()),
        (rid("hs-d1"), "first".to_owned()),
    ]
}

fn unreachable() -> CpdError {
    CpdError::new(ErrorCode::EmbeddingUnreachable, "down")
}

#[test]
fn embed_records_embeds_only_missing_and_changed_rows() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    // hs-d1 is current, hs-n2 changed since it was embedded, hs-d3 has no row.
    store
        .put_hashed(&cid(), MODEL, &rid("hs-d1"), &[1.0], &content_hash("first"))
        .unwrap();
    store
        .put_hashed(
            &cid(),
            MODEL,
            &rid("hs-n2"),
            &[1.0],
            &content_hash("old second"),
        )
        .unwrap();
    let fake = FakeEmbedding::answering(embedded());
    let report = embed_records(&store, &fake, &cid(), &records(), 0).unwrap();
    assert_eq!(
        report,
        EmbedReport {
            embedded: 2,
            skipped: 1,
            remaining: 0,
            embed_tokens: 0,
            ms: 0
        }
    );
    let texts: Vec<String> = fake.calls().into_iter().map(|(t, _)| t).collect();
    assert_eq!(texts, vec!["third".to_owned(), "second".to_owned()]);
    let again = embed_records(&store, &fake, &cid(), &records(), 0).unwrap();
    assert_eq!(
        again,
        EmbedReport {
            embedded: 0,
            skipped: 3,
            remaining: 0,
            embed_tokens: 0,
            ms: 0
        }
    );
}

#[test]
fn embed_records_stops_at_the_limit_and_reports_remaining() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    let fake = FakeEmbedding::answering(embedded());
    let report = embed_records(&store, &fake, &cid(), &records(), 2).unwrap();
    assert_eq!(
        report,
        EmbedReport {
            embedded: 2,
            skipped: 0,
            remaining: 1,
            embed_tokens: 0,
            ms: 0
        }
    );
    assert_eq!(fake.calls().len(), 2);
    let have = store.embedded(&cid(), MODEL).unwrap();
    assert!(have.contains(&rid("hs-d3")) && have.contains(&rid("hs-n2")));
    assert!(!have.contains(&rid("hs-d1")));
    let rest = embed_records(&store, &fake, &cid(), &records(), 2).unwrap();
    assert_eq!(
        rest,
        EmbedReport {
            embedded: 1,
            skipped: 2,
            remaining: 0,
            embed_tokens: 0,
            ms: 0
        }
    );
}

#[test]
fn embed_records_keeps_written_rows_when_the_embedder_fails() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    let fake = FakeEmbedding::new(vec![Ok(embedded()), Err(unreachable())]);
    let e = embed_records(&store, &fake, &cid(), &records(), 0).unwrap_err();
    assert_eq!(e.code(), ErrorCode::EmbeddingUnreachable);
    assert_eq!(fake.calls().len(), 2, "the first error stops the call");
    let have = store.embedded(&cid(), MODEL).unwrap();
    assert_eq!(have.len(), 1);
    assert!(have.contains(&rid("hs-d3")));
}

#[test]
fn embed_records_writes_the_hash_of_the_embedded_text() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    let fake = FakeEmbedding::answering(embedded());
    embed_records(&store, &fake, &cid(), &records(), 0).unwrap();
    assert!(fake.calls().iter().all(|(_, dims)| *dims == DIMS));
    let ids: Vec<RecordId> = records().into_iter().map(|(id, _)| id).collect();
    let hashes = store.hashes(&cid(), MODEL, &ids).unwrap();
    assert_eq!(hashes.len(), 3);
    for (id, text) in records() {
        assert_eq!(hashes[&id], content_hash(&text), "{id}");
    }
}

const STEP: std::time::Duration = std::time::Duration::from_millis(5);

fn metered() -> Embedded {
    Embedded {
        tokens: Some(4),
        ..embedded()
    }
}

#[test]
fn embed_report_has_tokens_and_ms() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    let clock = ManualClock::new();
    let fake = FakeEmbedding::answering(metered()).advancing(&clock, STEP);
    let report = embed_run(&home, &clock, &store, &fake, &cid(), &records(), 0).unwrap();
    assert_eq!(
        report,
        EmbedReport {
            embedded: 3,
            skipped: 0,
            remaining: 0,
            embed_tokens: 12,
            ms: 15
        }
    );
}

#[test]
fn embed_appends_one_curate_runs_line() {
    let (_dir, home) = home();
    let store = VectorStore::at(&home);
    let clock = ManualClock::new();
    let fake = FakeEmbedding::answering(metered()).advancing(&clock, STEP);
    let started = crate::clock::timestamp(clock.utc());
    embed_run(&home, &clock, &store, &fake, &cid(), &records(), 2).unwrap();
    let path = home.context_digest_dir(&cid()).join("curate-runs.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 1);
    let finished = crate::clock::timestamp(clock.utc());
    let at = lines[0]["at"].as_str().unwrap().to_owned();
    assert!(started <= at && at <= finished, "{at}");
    assert_eq!(
        lines[0],
        serde_json::json!({
            "kind": "embed", "at": at, "context": "cpd", "ms": 10, "embed_tokens": 8,
            "kev_tokens_in": 0, "kev_tokens_out": 0, "judge_tokens_in": 0, "judge_tokens_out": 0,
            "cost": null, "embedded": 2, "skipped": 0, "remaining": 1
        })
    );
}
