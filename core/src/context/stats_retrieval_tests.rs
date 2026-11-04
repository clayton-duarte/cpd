//! Red tests of the `retrieval` section of `context::stats::compute` (commands.md §3.20, contract §4.6; Plan
//! `ctx-observability`, pro-d13): `context_reads` and `records` seeded by SQL, `search_log` by raw rusqlite DDL.

use serde_json::{Value, json};

use super::stats::compute;
use crate::home::CpdHome;
use crate::ids::ContextId;
use crate::store::Store;

const CTX: &str = "c1";

/// `(at, context, query, mode, hits, fts_ms, embed_ms)`.
type Row<'a> = (&'a str, &'a str, &'a str, &'a str, i64, i64, Option<i64>);

struct Fx {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c1', 'C1', 't', 'm1');
                 INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c2', 'C2', 't', 'm1');
                 INSERT INTO context_records (context, id, kind, at, machine, text, supersedes) VALUES
                   ('c1', 'm1-n1', 'note', '2026-01-01T12:00:00.000Z', 'm1', 'a', NULL),
                   ('c1', 'm1-n2', 'note', '2026-01-02T12:00:00.000Z', 'm1', 'b', NULL),
                   ('c1', 'm1-n3', 'note', '2026-01-03T12:00:00.000Z', 'm1', 'c', NULL),
                   ('c1', 'm1-n4', 'note', '2026-01-04T12:00:00.000Z', 'm1', 'd', NULL),
                   ('c1', 'm1-n5', 'note', '2026-01-05T12:00:00.000Z', 'm1', 'e', NULL),
                   ('c2', 'm1-d9', 'decision', '2026-01-06T12:00:00.000Z', 'm1', 'other', NULL);",
            )?;
            Ok(())
        })
        .unwrap();
    Fx {
        _dir: dir,
        home,
        store,
    }
}

impl Fx {
    fn sql(&mut self, sql: &'static str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    fn retrieval(&mut self, since: Option<&str>) -> Value {
        let (home, ctx) = (self.home.clone(), ContextId::parse(CTX).unwrap());
        let mut v = self
            .store
            .read(|conn| compute(conn, &home, &ctx, since))
            .unwrap();
        v["retrieval"].take()
    }

    /// A `vectors.db` with a v3 `search_log`; `rows` are `(at, context, query, mode, hits, fts_ms, embed_ms)`.
    fn search_log(&self, rows: &[Row]) {
        let conn = rusqlite::Connection::open(self.home.vectors_db()).unwrap();
        conn.execute_batch(
            "CREATE TABLE context_embeddings (context TEXT NOT NULL, model TEXT NOT NULL, record TEXT NOT NULL, dims INTEGER NOT NULL, vector BLOB NOT NULL, at TEXT NOT NULL, content_hash TEXT NOT NULL DEFAULT '', PRIMARY KEY (context, model, record)) STRICT, WITHOUT ROWID;
             CREATE TABLE context_checked (context TEXT NOT NULL, record TEXT NOT NULL, at TEXT NOT NULL, PRIMARY KEY (context, record)) STRICT, WITHOUT ROWID;
             CREATE TABLE search_log (id INTEGER PRIMARY KEY, at TEXT NOT NULL, context TEXT NOT NULL, session TEXT, query TEXT NOT NULL,
               mode TEXT NOT NULL CHECK (mode IN ('fts','hybrid')), candidates INTEGER NOT NULL, floor_dropped INTEGER NOT NULL,
               pending_hidden INTEGER NOT NULL, hits INTEGER NOT NULL, fts_ms INTEGER NOT NULL, embed_ms INTEGER, embed_tokens INTEGER) STRICT;
             CREATE INDEX search_log_context_at ON search_log(context, at);
             PRAGMA user_version = 3;",
        )
        .unwrap();
        for (at, ctx, query, mode, hits, fts_ms, embed_ms) in rows {
            conn.execute(
                "INSERT INTO search_log (at, context, session, query, mode, candidates, floor_dropped, pending_hidden, hits, fts_ms, embed_ms, embed_tokens)
                 VALUES (?1, ?2, NULL, ?3, ?4, 0, 0, 0, ?5, ?6, ?7, NULL)",
                rusqlite::params![at, ctx, query, mode, hits, fts_ms, embed_ms],
            )
            .unwrap();
        }
    }
}

const READS: &str = "INSERT INTO context_reads (context, record, session, channel, read_at, rank, score, used) VALUES
   ('c1', 'm1-n1', 's1', 'pushed', '2026-03-01T12:00:00.000Z', NULL, NULL, 1),
   ('c1', 'm1-n2', 's1', 'pushed', '2026-03-01T12:00:00.000Z', NULL, NULL, 0),
   ('c1', 'm1-n1', 's1', 'search', '2026-03-01T12:00:01.000Z', 1, 0.5, 1),
   ('c1', 'm1-n3', 's2', 'search', '2026-06-01T12:00:00.000Z', 2, 0.4, NULL),
   ('c1', 'm1-n1', 's3', 'show', '2026-06-02T12:00:00.000Z', NULL, NULL, 0),
   ('c2', 'm1-d9', 's9', 'pushed', '2026-03-01T12:00:00.000Z', NULL, NULL, 1);";

#[test]
fn stats_retrieval_counts_reads_by_channel_and_sessions() {
    let mut f = fixture();
    f.sql(READS);
    let r = f.retrieval(None);
    assert_eq!(r["reads"], json!({"pushed": 2, "search": 2, "show": 1}));
    assert_eq!(r["reading_sessions"], 3);
}

#[test]
fn stats_retrieval_counts_cited_and_conversion() {
    let mut f = fixture();
    f.sql(READS);
    let r = f.retrieval(None);
    assert_eq!(r["cited"]["used"], 2);
    let conv = r["cited"]["conversion"].as_f64().unwrap();
    assert!((conv - 0.4).abs() < 1e-9, "conversion {conv}");
}

#[test]
fn stats_retrieval_counts_unread_in_force_records() {
    let mut f = fixture();
    f.sql(READS);
    // n4 is retired_pending (still counted), n5 is retired_approved (not counted); n1..n3 were read.
    f.sql(
        "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at, decided_at, decided_by) VALUES
           ('c1', 'm1-n4', 'duplicate', 'm1-n1', 'pending', '2026-04-01T12:00:00.000Z', NULL, NULL),
           ('c1', 'm1-n5', 'duplicate', 'm1-n1', 'approved', '2026-04-01T12:00:00.000Z', '2026-04-02T12:00:00.000Z', 'u');",
    );
    assert_eq!(f.retrieval(None)["unread_in_force"], 1);
}

#[test]
fn stats_retrieval_search_log_modes_and_zero_hits() {
    let mut f = fixture();
    f.search_log(&[
        ("2026-03-01T12:00:00.000Z", "c1", "a", "fts", 3, 1, None),
        (
            "2026-03-02T12:00:00.000Z",
            "c1",
            "b",
            "hybrid",
            0,
            1,
            Some(5),
        ),
        (
            "2026-03-03T12:00:00.000Z",
            "c1",
            "c",
            "hybrid",
            2,
            1,
            Some(7),
        ),
        ("2026-03-04T12:00:00.000Z", "c1", "d", "fts", 0, 1, None),
        ("2026-03-05T12:00:00.000Z", "c2", "other", "fts", 0, 1, None),
    ]);
    let s = &f.retrieval(None)["searches"];
    assert_eq!(s["total"], 4);
    assert_eq!(s["hybrid"], 2);
    assert_eq!(s["fts"], 2);
    assert_eq!(s["zero_hit"], 2);
}

#[test]
fn stats_retrieval_recent_zero_hit_queries_are_the_newest_ten() {
    let mut f = fixture();
    let names: Vec<String> = (1..=12).map(|i| format!("q{i:02}")).collect();
    let ats: Vec<String> = (1..=12)
        .map(|i| format!("2026-03-{i:02}T12:00:00.000Z"))
        .collect();
    let mut rows: Vec<Row> = names
        .iter()
        .zip(&ats)
        .map(|(n, a)| (a.as_str(), "c1", n.as_str(), "fts", 0, 1, None))
        .collect();
    rows.push(("2026-03-20T12:00:00.000Z", "c1", "hit", "fts", 4, 1, None));
    f.search_log(&rows);
    let q = f.retrieval(None)["searches"]["recent_zero_hit_queries"].clone();
    let want: Vec<String> = (3..=12).rev().map(|i| format!("q{i:02}")).collect();
    assert_eq!(q, json!(want));
}

#[test]
fn stats_retrieval_search_latency_mean_and_max() {
    let mut f = fixture();
    f.search_log(&[
        ("2026-03-01T12:00:00.000Z", "c1", "a", "fts", 1, 2, None),
        (
            "2026-03-02T12:00:00.000Z",
            "c1",
            "b",
            "hybrid",
            1,
            4,
            Some(10),
        ),
        (
            "2026-03-03T12:00:00.000Z",
            "c1",
            "c",
            "hybrid",
            1,
            9,
            Some(30),
        ),
    ]);
    let s = &f.retrieval(None)["searches"];
    assert_eq!(s["fts_ms"]["mean"].as_f64().unwrap(), 5.0);
    assert_eq!(s["fts_ms"]["max"], 9);
    // embed_ms means over the rows that have one.
    assert_eq!(s["embed_ms"]["mean"].as_f64().unwrap(), 20.0);
    assert_eq!(s["embed_ms"]["max"], 30);
}

#[test]
fn stats_retrieval_since_filters_reads_and_searches() {
    let mut f = fixture();
    f.sql(READS);
    f.search_log(&[
        ("2026-03-01T12:00:00.000Z", "c1", "old", "fts", 0, 1, None),
        ("2026-07-01T12:00:00.000Z", "c1", "new", "fts", 0, 1, None),
    ]);
    let r = f.retrieval(Some("2026-05-01T00:00:00Z"));
    assert_eq!(r["reads"], json!({"pushed": 0, "search": 1, "show": 1}));
    assert_eq!(r["reading_sessions"], 2);
    assert_eq!(r["searches"]["total"], 1);
    assert_eq!(r["searches"]["recent_zero_hit_queries"], json!(["new"]));
}

#[test]
fn stats_retrieval_without_vectors_db_has_zero_searches() {
    let mut f = fixture();
    f.sql(READS);
    let r = f.retrieval(None);
    // Reads are still counted; only the searches are zero (absent vectors.db is not an error).
    assert_eq!(r["reads"]["pushed"], 2);
    assert_eq!(r["searches"]["total"], 0);
    assert_eq!(r["searches"]["recent_zero_hit_queries"], json!([]));
    assert!(r["searches"]["fts_ms"]["mean"].is_null());
}
