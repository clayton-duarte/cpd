//! Red tests of `context::stats::compute` (commands.md §3.20, contract §4.6; Plan `ctx-observability`, pro-d13): the
//! `sizes`, `writing_and_curation` and `cost` sections over a tempdir store seeded by SQL, run files written by the
//! test and a `vectors.db` made with raw rusqlite DDL. Nothing external runs.
//! Assumed run-line keys of `runs.jsonl` for the counters: `curations` and `retire` (0 when a line has none).

use std::fs;

use serde_json::{Value, json};

use super::stats::compute;
use crate::home::CpdHome;
use crate::ids::ContextId;
use crate::store::Store;

const CTX: &str = "c1";

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
                   ('c1', 'm1-n1', 'note', '2026-01-01T12:00:00.000Z', 'm1', 'old', NULL),
                   ('c1', 'm1-n2', 'note', '2026-02-01T12:00:00.000Z', 'm1', 'dup note', NULL),
                   ('c1', 'm1-n3', 'note', '2026-03-01T12:00:00.000Z', 'm1', 'pend', NULL),
                   ('c1', 'm1-n4', 'note', '2026-03-01T12:00:01.000Z', 'm1', 'both', NULL),
                   ('c1', 'm1-d1', 'decision', '2026-04-01T12:00:00.000Z', 'm1', 'first', NULL),
                   ('c1', 'm1-d2', 'decision', '2026-05-01T12:00:00.000Z', 'm1', 'second', 'm1-n1,m1-n4'),
                   ('c2', 'm1-d9', 'decision', '2026-05-01T12:00:00.000Z', 'm1', 'other context', NULL);
                 INSERT INTO record_retirements (context, record, reason, target, status, proposed_at, decided_at, decided_by) VALUES
                   ('c1', 'm1-n2', 'duplicate', 'm1-d1', 'approved', '2026-03-05T12:00:00.000Z', '2026-03-06T12:00:00.000Z', 'u'),
                   ('c1', 'm1-n3', 'documented', 'docs/x.md', 'pending', '2026-04-05T12:00:00.000Z', NULL, NULL),
                   ('c1', 'm1-n4', 'superseded', 'm1-d2', 'approved', '2026-04-06T12:00:00.000Z', '2026-04-07T12:00:00.000Z', 'u'),
                   ('c1', 'm1-d1', 'duplicate', 'm1-n2', 'rejected', '2026-05-05T12:00:00.000Z', '2026-05-06T12:00:00.000Z', 'u');",
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

    fn stats(&mut self, since: Option<&str>) -> Value {
        let (home, ctx) = (self.home.clone(), ContextId::parse(CTX).unwrap());
        self.store
            .read(|conn| compute(conn, &home, &ctx, since))
            .unwrap()
    }

    fn write_lines(&self, file: &str, lines: &[Value]) {
        let dir = self
            .home
            .context_digest_dir(&ContextId::parse(CTX).unwrap());
        fs::create_dir_all(&dir).unwrap();
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        // A line that is not JSON is skipped (commands.md §3.20).
        fs::write(dir.join(file), format!("not json\n{body}")).unwrap();
    }

    fn vectors(&self) {
        let conn = rusqlite::Connection::open(self.home.vectors_db()).unwrap();
        conn.execute_batch(
            "CREATE TABLE context_embeddings (context TEXT NOT NULL, model TEXT NOT NULL, record TEXT NOT NULL, dims INTEGER NOT NULL, vector BLOB NOT NULL, at TEXT NOT NULL, content_hash TEXT NOT NULL DEFAULT '', PRIMARY KEY (context, model, record)) STRICT, WITHOUT ROWID;
             CREATE TABLE context_checked (context TEXT NOT NULL, record TEXT NOT NULL, at TEXT NOT NULL, PRIMARY KEY (context, record)) STRICT, WITHOUT ROWID;
             INSERT INTO context_embeddings (context, model, record, dims, vector, at) VALUES
               ('c1', 'm', 'm1-n1', 2, zeroblob(8), 't'), ('c1', 'm', 'm1-d1', 2, zeroblob(8), 't'),
               ('c2', 'm', 'm1-d9', 2, zeroblob(8), 't');
             INSERT INTO context_checked (context, record, at) VALUES
               ('c1', 'm1-n1', 't'), ('c1', 'm1-d1', 't'), ('c1', 'm1-n2', 't'), ('c2', 'm1-d9', 't');
             PRAGMA user_version = 3;",
        )
        .unwrap();
    }

    fn runs(&self) {
        self.write_lines(
            "runs.jsonl",
            &[
                json!({"started_at": "2026-02-10T12:00:00Z", "seconds": 10.5, "model": "m", "tokens_in": 1000, "tokens_out": 100,
                       "cost": 0.25, "context": CTX, "upto_seq": 3, "result": "valid", "rejected": [],
                       "kind": "distill", "tier": "H0", "curations": 3, "retire": 1}),
                json!({"started_at": "2026-04-10T12:00:00Z", "seconds": 20, "model": "m", "tokens_in": 2000, "tokens_out": 200,
                       "cost": 0.5, "context": CTX, "upto_seq": 5, "result": "invalid",
                       "rejected": [{"id": "m1-n1", "line": 1, "what": "a"}, {"id": "m1-n2", "line": 2, "what": "b"}],
                       "kind": "distill", "tier": "H0"}),
                json!({"started_at": "2026-06-10T12:00:00Z", "seconds": 1, "model": "m", "tokens_in": null, "tokens_out": null,
                       "cost": null, "context": CTX, "upto_seq": 6, "result": "failed", "rejected": [],
                       "kind": "distill", "tier": "H0"}),
            ],
        );
        let curate = |at: &str,
                      ms: u64,
                      kev: (u64, u64),
                      judge: (u64, u64),
                      cost: Value,
                      n: [u64; 7]| {
            json!({"kind": "curate", "at": at, "context": CTX, "ms": ms, "embed_tokens": 0,
                   "kev_tokens_in": kev.0, "kev_tokens_out": kev.1, "judge_tokens_in": judge.0, "judge_tokens_out": judge.1,
                   "cost": cost, "free": n[0], "judged": n[1], "retire": n[2], "contradicts": n[3], "disagreed": n[4],
                   "quote_failed": n[5], "kev_unreachable": n[6]})
        };
        self.write_lines(
            "curate-runs.jsonl",
            &[
                curate("2026-03-10T12:00:00.000Z", 4000, (100, 10), (50, 5), json!(0.01), [5, 2, 1, 1, 0, 0, 0]),
                curate("2026-05-10T12:00:00.000Z", 6000, (200, 20), (70, 7), Value::Null, [3, 1, 0, 0, 1, 1, 2]),
                json!({"kind": "embed", "at": "2026-05-11T12:00:00.000Z", "context": CTX, "ms": 2000, "embed_tokens": 300,
                       "kev_tokens_in": 0, "kev_tokens_out": 0, "judge_tokens_in": 0, "judge_tokens_out": 0, "cost": null,
                       "embedded": 4, "skipped": 0, "remaining": 0}),
            ],
        );
    }
}

fn close(got: &Value, want: f64) {
    let got = got
        .as_f64()
        .unwrap_or_else(|| panic!("not a number: {got}"));
    assert!((got - want).abs() < 1e-9, "{got} != {want}");
}

#[test]
fn stats_sizes_counts_records_by_kind_and_status() {
    let mut fx = fixture();
    let sizes = fx.stats(None)["sizes"]["records"].clone();
    // Kind: bytes are the UTF-8 length of the text.
    assert_eq!(
        sizes["by_kind"]["decision"],
        json!({"count": 2, "bytes": 11}),
        "{sizes}"
    );
    assert_eq!(
        sizes["by_kind"]["note"],
        json!({"count": 4, "bytes": 19}),
        "{sizes}"
    );
    assert_eq!(
        sizes["by_kind"]["plan"],
        json!({"count": 0, "bytes": 0}),
        "{sizes}"
    );
    assert_eq!(
        sizes["by_kind"]["brief"],
        json!({"count": 0, "bytes": 0}),
        "{sizes}"
    );
    // Status is exclusive: m1-n4 is superseded and approved-retired, and counts as retired_approved.
    assert_eq!(
        sizes["by_status"]["in_force"],
        json!({"count": 2, "bytes": 11}),
        "{sizes}"
    );
    assert_eq!(
        sizes["by_status"]["superseded"],
        json!({"count": 1, "bytes": 3}),
        "{sizes}"
    );
    assert_eq!(
        sizes["by_status"]["retired_approved"],
        json!({"count": 2, "bytes": 12}),
        "{sizes}"
    );
    assert_eq!(
        sizes["by_status"]["retired_pending"],
        json!({"count": 1, "bytes": 4}),
        "{sizes}"
    );
}

#[test]
fn stats_sizes_counts_curated_topics_and_vectors() {
    let mut fx = fixture();
    fx.sql(
        "INSERT INTO record_curations (context, record, topic, short, curated_at) VALUES
           ('c1', 'm1-n1', 'a', 'abc', 't'), ('c1', 'm1-n2', 'b', 'é', 't');
         INSERT INTO record_topics (context, record, topic, model, at) VALUES
           ('c1', 'm1-n1', 'x', 'k', 't'), ('c1', 'm1-n3', 'y', 'k', 't'), ('c2', 'm1-d9', 'z', 'k', 't');",
    );
    fx.vectors();
    let sizes = fx.stats(None)["sizes"].clone();
    assert_eq!(
        sizes["curated"],
        json!({"count": 2, "short_bytes": 5}),
        "{sizes}"
    );
    // m1-n1 has a curated topic, which wins: only m1-n3 is provisional (c2's row is another Context's).
    assert_eq!(sizes["provisional_topics"], 1, "{sizes}");
    assert_eq!(
        sizes["vectors"],
        json!({"count": 2, "checked": 3, "blob_bytes": 16}),
        "{sizes}"
    );
}

#[test]
fn stats_without_vectors_db_reports_zero_vectors() {
    let mut fx = fixture();
    let report = fx.stats(None);
    assert!(!fx.home.vectors_db().exists(), "stats creates no file");
    assert_eq!(
        report["sizes"]["vectors"],
        json!({"count": 0, "checked": 0, "blob_bytes": 0})
    );
    // The same call is the proof the stub is not a report: the records are counted.
    assert_eq!(
        report["sizes"]["records"]["by_kind"]["note"]["count"], 4,
        "{report}"
    );
}

#[test]
fn stats_writing_counts_records_runs_and_retirements() {
    let mut fx = fixture();
    fx.runs();
    let w = fx.stats(None)["writing_and_curation"].clone();
    assert_eq!(w["records_written"], 6, "{w}");
    assert_eq!(
        w["distill"],
        json!({"runs": 3, "curations": 3, "retire": 1, "rejected": 2, "failed": 1}),
        "{w}"
    );
    assert_eq!(
        w["curate"],
        json!({"runs": 2, "free": 8, "judged": 3, "retire": 1, "contradicts": 1, "disagreed": 1,
               "quote_failed": 1, "kev_unreachable": 2}),
        "{w}"
    );
    assert_eq!(
        w["retirements"]["duplicate"],
        json!({"approved": 1, "pending": 0, "rejected": 1}),
        "{w}"
    );
    assert_eq!(
        w["retirements"]["superseded"],
        json!({"approved": 1, "pending": 0, "rejected": 0}),
        "{w}"
    );
    assert_eq!(
        w["retirements"]["documented"],
        json!({"approved": 0, "pending": 1, "rejected": 0}),
        "{w}"
    );
}

#[test]
fn stats_cost_sums_distill_curate_embed() {
    let mut fx = fixture();
    fx.runs();
    let cost = fx.stats(None)["cost"].clone();
    assert_eq!(cost["distill"]["runs"], 3, "{cost}");
    assert_eq!(cost["distill"]["tokens_in"], 3000, "{cost}");
    assert_eq!(cost["distill"]["tokens_out"], 300, "{cost}");
    assert_eq!(cost["distill"]["ms"], 31500, "{cost}");
    close(&cost["distill"]["cost"], 0.75);
    assert_eq!(cost["curate"]["runs"], 2, "{cost}");
    assert_eq!(cost["curate"]["kev_tokens_in"], 300, "{cost}");
    assert_eq!(cost["curate"]["kev_tokens_out"], 30, "{cost}");
    assert_eq!(cost["curate"]["judge_tokens_in"], 120, "{cost}");
    assert_eq!(cost["curate"]["judge_tokens_out"], 12, "{cost}");
    assert_eq!(cost["curate"]["ms"], 10000, "{cost}");
    close(&cost["curate"]["cost"], 0.01);
    assert_eq!(
        cost["embed"],
        json!({"runs": 1, "embed_tokens": 300, "ms": 2000}),
        "{cost}"
    );
    assert_eq!(
        cost["search"],
        json!({"searches": 0, "embed_tokens": 0, "embed_ms": 0, "fts_ms": 0}),
        "{cost}"
    );
    let total = &cost["total"];
    assert_eq!(total["tokens_in"], 3420, "{cost}");
    assert_eq!(total["tokens_out"], 342, "{cost}");
    assert_eq!(total["embed_tokens"], 300, "{cost}");
    assert_eq!(total["ms"], 43500, "{cost}");
    close(&total["cost"], 0.76);
}

#[test]
fn stats_since_filters_windowed_figures() {
    let mut fx = fixture();
    fx.runs();
    fx.vectors();
    let report = fx.stats(Some("2026-05-01T00:00:00Z"));
    let w = &report["writing_and_curation"];
    assert_eq!(w["records_written"], 1, "{w}");
    assert_eq!(
        w["distill"],
        json!({"runs": 1, "curations": 0, "retire": 0, "rejected": 0, "failed": 1}),
        "{w}"
    );
    assert_eq!(w["curate"]["runs"], 1, "{w}");
    assert_eq!(w["curate"]["free"], 3, "{w}");
    assert_eq!(
        w["retirements"]["duplicate"],
        json!({"approved": 0, "pending": 0, "rejected": 1}),
        "{w}"
    );
    assert_eq!(
        w["retirements"]["superseded"],
        json!({"approved": 0, "pending": 0, "rejected": 0}),
        "{w}"
    );
    let cost = &report["cost"];
    assert_eq!(cost["distill"]["ms"], 1000, "{cost}");
    assert_eq!(cost["curate"]["ms"], 6000, "{cost}");
    assert_eq!(cost["embed"]["runs"], 1, "{cost}");
    // `sizes` is a snapshot of now: the window does not touch it.
    assert_eq!(
        report["sizes"]["records"]["by_status"]["in_force"]["count"], 2,
        "{report}"
    );
    assert_eq!(report["sizes"]["vectors"]["count"], 2, "{report}");
}
