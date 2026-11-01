//! The holographic engine rail (contract §4.6; docs/research/context-holographic.md,
//! `## Bridge` and `## Config and failure`).
//!
//! Two halves, one per Job that answers them:
//!
//! - the real bridge, `contrib/holographic/bridge.py`, driven over its JSON-lines
//!   protocol with `python3` and a stub `holographic_memory` package on `PYTHONPATH`
//!   (the external engine is mocked; the interpreter is real, as the machine has it);
//! - the Rust spawning client, driven with fake bridge scripts written with
//!   [`common::write_executable`].
//!
//! These tests are red against the stubs of `cpd_core::context::engine` and the
//! missing `contrib/holographic/bridge.py`; Job `bridge-impl` implements them.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{TestEnv, output_with_stdin, write_executable};
use cpd_core::context::engine::{Bridge, ContextEngine};
use serde_json::{Value, json};

// ------------------------------------------------------------ the stub engine

/// The stub `holographic_memory` package the bridge tests put on `PYTHONPATH` in
/// place of the external engine. It holds **only** what `contrib/holographic/bridge.py`
/// calls: `MemoryStore(db_path)` with a `_conn`, `add_facts_batch`, `update_fact` and
/// `FactRetriever(store).search`; the engine is never imported.
const HOLOGRAPHIC_STUB: &str = r##"
"""A stub of the external holographic engine; the bridge tests only."""
import sqlite3

__version__ = "0.16.0"

_SCHEMA = """
CREATE TABLE IF NOT EXISTS facts (
    fact_id     INTEGER PRIMARY KEY AUTOINCREMENT,
    content     TEXT NOT NULL UNIQUE,
    category    TEXT DEFAULT 'general',
    tags        TEXT DEFAULT '',
    trust_score REAL DEFAULT 0.5,
    updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts
    USING fts5(content, tags, content=facts, content_rowid=fact_id);
CREATE TRIGGER IF NOT EXISTS facts_ai AFTER INSERT ON facts BEGIN
    INSERT INTO facts_fts(rowid, content, tags) VALUES (new.fact_id, new.content, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS facts_au AFTER UPDATE ON facts BEGIN
    INSERT INTO facts_fts(facts_fts, rowid, content, tags) VALUES ('delete', old.fact_id, old.content, old.tags);
    INSERT INTO facts_fts(rowid, content, tags) VALUES (new.fact_id, new.content, new.tags);
END;
CREATE TRIGGER IF NOT EXISTS facts_ad AFTER DELETE ON facts BEGIN
    INSERT INTO facts_fts(facts_fts, rowid, content, tags) VALUES ('delete', old.fact_id, old.content, old.tags);
END;
"""


class MemoryStore:
    def __init__(self, db_path):
        self.db_path = str(db_path)
        self._conn = sqlite3.connect(self.db_path)
        self._conn.row_factory = sqlite3.Row
        self._conn.executescript(_SCHEMA)
        self._conn.commit()

    def add_facts_batch(self, facts):
        inserted = 0
        for content, category, tags in facts:
            try:
                self._conn.execute(
                    "INSERT INTO facts (content, category, tags) VALUES (?, ?, ?)",
                    (content, category, tags),
                )
                inserted += 1
            except sqlite3.IntegrityError:
                pass
        self._conn.commit()
        return inserted

    def update_fact(self, fact_id, content=None, category=None, tags=None):
        if self._conn.execute("SELECT 1 FROM facts WHERE fact_id = ?", (fact_id,)).fetchone() is None:
            return False
        self._conn.execute(
            "UPDATE facts SET content = COALESCE(?, content), category = COALESCE(?, category),"
            " tags = COALESCE(?, tags) WHERE fact_id = ?",
            (content, category, tags, fact_id),
        )
        self._conn.commit()
        return True

    def close(self):
        self._conn.close()


class FactRetriever:
    def __init__(self, store):
        self.store = store

    def search(self, query, category=None, limit=10):
        rows = self.store._conn.execute(
            "SELECT f.fact_id, f.content, f.category, facts_fts.rank AS rank "
            "FROM facts_fts JOIN facts f ON f.fact_id = facts_fts.rowid "
            "WHERE facts_fts MATCH ? ORDER BY facts_fts.rank LIMIT ?",
            (query, limit),
        ).fetchall()
        return [
            {
                "fact_id": row["fact_id"],
                "content": row["content"],
                "category": row["category"],
                "tags": "",
                "trust_score": 0.5,
                "score": 1.0 / (1.0 + abs(row["rank"])),
            }
            for row in rows
        ]
"##;

/// `contrib/holographic/bridge.py`, resolved from the test crate.
fn bridge_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../contrib/holographic/bridge.py")
}

/// Writes the stub engine package under `env` and returns the directory to put on `PYTHONPATH`.
fn stub_engine(env: &TestEnv) -> PathBuf {
    let root = env.root().join("holographic-stub");
    let package = root.join("holographic_memory");
    fs::create_dir_all(&package).expect("create the stub package");
    fs::write(package.join("__init__.py"), HOLOGRAPHIC_STUB).expect("write the stub package");
    root
}

/// Runs `python3 <bridge> --db <db>` over `input`, returning its response lines
/// (parsed), its stderr and its exit code.
fn run_bridge_raw(env: &TestEnv, stub: &Path, db: &Path, input: &str) -> (Vec<Value>, String, i32) {
    let mut cmd = env.command("python3");
    cmd.arg(bridge_path())
        .arg("--db")
        .arg(db)
        .env("PYTHONPATH", stub);
    let out = output_with_stdin(&mut cmd, input.as_bytes(), Duration::from_secs(30))
        .expect("run the bridge");
    let stdout = String::from_utf8(out.stdout).expect("bridge stdout is UTF-8");
    let lines = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("bridge line {line:?} is not JSON: {e}"))
        })
        .collect();
    (
        lines,
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().expect("the bridge has an exit code"),
    )
}

/// [`run_bridge_raw`] with JSON requests, asserting the bridge exited 0.
fn run_bridge(env: &TestEnv, stub: &Path, db: &Path, requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let (lines, stderr, exit) = run_bridge_raw(env, stub, db, &input);
    assert_eq!(exit, 0, "the bridge exited {exit}: {stderr}");
    assert_eq!(
        lines.len(),
        requests.len(),
        "one response per request: {lines:?}"
    );
    lines
}

/// A `facts` array for `index`: one fact per `(record, text)`.
fn facts(pairs: &[(&str, &str)]) -> Value {
    Value::Array(
        pairs
            .iter()
            .map(|(record, text)| json!({"record": record, "text": text, "topic": "", "short": ""}))
            .collect(),
    )
}

fn hits(response: &Value) -> &Vec<Value> {
    response["hits"]
        .as_array()
        .unwrap_or_else(|| panic!("no hits in {response}"))
}

/// The record id of the first hit.
fn first_record(response: &Value) -> &str {
    hits(response)[0]["record"]
        .as_str()
        .unwrap_or_else(|| panic!("no record in {response}"))
}

// ------------------------------------------------------------- the bridge

/// docs/research/context-holographic.md, `## Bridge`: one response per request, in order,
/// for `ping`, `stats`, `reset`, `index` and `search`.
#[test]
fn the_bridge_answers_ping_stats_reset_index_and_search() {
    let env = TestEnv::new();
    let stub = stub_engine(&env);
    let db = env.root().join("context.db");

    let responses = run_bridge(
        &env,
        &stub,
        &db,
        &[
            json!({"cmd": "ping"}),
            json!({"cmd": "stats"}),
            json!({"cmd": "reset"}),
            json!({"cmd": "index", "facts": facts(&[("hs-d1", "the bridge spawn is quick"), ("hs-d2", "a decision about ranking")])}),
            json!({"cmd": "stats"}),
            json!({"cmd": "search", "query": "bridge", "limit": 10}),
        ],
    );

    let ping = &responses[0];
    assert_eq!(ping["ok"], true, "{ping}");
    assert_eq!(ping["engine"], "holographic", "{ping}");
    assert_eq!(ping["version"], "0.16.0", "{ping}");
    assert!(ping["pid"].is_u64(), "{ping}");

    let before = &responses[1];
    assert_eq!(before["ok"], true, "{before}");
    assert_eq!(before["facts"], 0, "{before}");

    let reset = &responses[2];
    assert_eq!(reset["ok"], true, "{reset}");

    let indexed = &responses[3];
    assert_eq!(indexed["ok"], true, "{indexed}");
    assert_eq!(indexed["inserted"], 2, "{indexed}");

    let after = &responses[4];
    assert_eq!(after["ok"], true, "{after}");
    assert_eq!(after["facts"], 2, "{after}");

    let search = &responses[5];
    assert_eq!(search["ok"], true, "{search}");
    assert_eq!(first_record(search), "hs-d1", "{search}");
    let hit = &hits(search)[0];
    assert_eq!(hit["record"], "hs-d1", "{hit}");
    assert!(hit["score"].is_number(), "{hit}");
}

/// docs/research/context-holographic.md, `## Bridge`: the `facts_fts` probe maps a
/// malformed `MATCH` to `bad_query`, not to a silent empty result.
#[test]
fn the_bridge_maps_a_malformed_match_to_bad_query() {
    let env = TestEnv::new();
    let stub = stub_engine(&env);
    let db = env.root().join("context.db");

    let responses = run_bridge(
        &env,
        &stub,
        &db,
        &[
            json!({"cmd": "index", "facts": facts(&[("hs-d1", "one decision")])}),
            json!({"cmd": "search", "query": "AND", "limit": 10}),
            json!({"cmd": "search", "query": "decision", "limit": 10}),
        ],
    );

    let malformed = &responses[1];
    assert_eq!(malformed["ok"], false, "{malformed}");
    assert_eq!(malformed["error"], "bad_query", "{malformed}");

    let valid = &responses[2];
    assert_eq!(valid["ok"], true, "{valid}");
    assert_eq!(first_record(valid), "hs-d1", "{valid}");
}

/// docs/research/context-holographic.md, `## Bridge`: the two protocol errors,
/// `bad_json` and `bad_cmd`.
#[test]
fn the_bridge_reports_bad_json_and_bad_cmd() {
    let env = TestEnv::new();
    let stub = stub_engine(&env);
    let db = env.root().join("context.db");

    let input = "this is not json\n{\"cmd\":\"nope\"}\n{\"cmd\":\"ping\"}\n";
    let (lines, stderr, exit) = run_bridge_raw(&env, &stub, &db, input);
    assert_eq!(
        exit, 0,
        "the bridge surfaces errors in the protocol, not its exit: {stderr}"
    );
    assert_eq!(lines.len(), 3, "one response per line: {lines:?}");

    assert_eq!(lines[0]["ok"], false, "{}", lines[0]);
    assert_eq!(lines[0]["error"], "bad_json", "{}", lines[0]);
    assert!(lines[0]["message"].is_string(), "{}", lines[0]);

    assert_eq!(lines[1]["ok"], false, "{}", lines[1]);
    assert_eq!(lines[1]["error"], "bad_cmd", "{}", lines[1]);
    assert!(lines[1]["message"].is_string(), "{}", lines[1]);

    // The bridge answers the next valid request after the two errors.
    assert_eq!(lines[2]["ok"], true, "{}", lines[2]);
    assert_eq!(lines[2]["engine"], "holographic", "{}", lines[2]);
}

/// docs/research/context-holographic.md, `## Bridge`: `index` upserts by `category`
/// (the cpd record id), so a re-index of an existing record updates its one fact.
#[test]
fn an_index_upserts_a_fact_by_record_id() {
    let env = TestEnv::new();
    let stub = stub_engine(&env);
    let db = env.root().join("context.db");

    let responses = run_bridge(
        &env,
        &stub,
        &db,
        &[
            json!({"cmd": "index", "facts": facts(&[("hs-d7", "the old wording")])}),
            json!({"cmd": "index", "facts": facts(&[("hs-d7", "the new wording")])}),
            json!({"cmd": "stats"}),
            json!({"cmd": "search", "query": "wording", "limit": 10}),
        ],
    );

    assert_eq!(responses[1]["ok"], true, "{}", responses[1]);
    assert_eq!(
        responses[2]["facts"], 1,
        "one fact, updated, not a second: {}",
        responses[2]
    );
    let search = &responses[3];
    assert_eq!(hits(search).len(), 1, "{search}");
    assert_eq!(first_record(search), "hs-d7", "{search}");
    assert!(hits(search)[0]["score"].is_number(), "{search}");
}

// ------------------------------------------------------------- the client

/// The client writes every request and reads one response per request, in order,
/// over one spawn.
#[test]
fn the_client_reads_the_responses_in_order() {
    let env = TestEnv::new();
    let script = env.root().join("bridge.sh");
    write_executable(
        &script,
        "#!/bin/sh\nn=0\nwhile IFS= read -r _line; do\n  printf '{\"ok\":true,\"n\":%s}\\n' \"$n\"\n  n=$((n+1))\ndone\n",
    );
    let engine = Bridge::new(
        true,
        script,
        PathBuf::from("/bin/sh"),
        Duration::from_secs(10),
    )
    .expect("a client");
    let responses = engine
        .run(
            &env.root().join("engine.db"),
            &[
                json!({"cmd": "a"}),
                json!({"cmd": "b"}),
                json!({"cmd": "c"}),
            ],
        )
        .expect("the responses");
    assert_eq!(responses.len(), 3, "{responses:?}");
    for (n, response) in responses.iter().enumerate() {
        assert_eq!(response["n"], n as u64, "{response}");
    }
}

/// docs/research/context-holographic.md, `## Config and failure`: a non-zero exit, a
/// hang past the deadline and bad JSON are each `engine_failed` (exit 1), and the child
/// is gone.
#[test]
fn a_nonzero_exit_a_timeout_and_bad_json_are_engine_failed() {
    let env = TestEnv::new();
    let db = env.root().join("engine.db");

    let exit = env.root().join("exit.sh");
    write_executable(&exit, "#!/bin/sh\necho boom >&2\nexit 3\n");
    let engine = Bridge::new(
        true,
        exit,
        PathBuf::from("/bin/sh"),
        Duration::from_secs(10),
    )
    .expect("a client");
    let err = engine
        .run(&db, &[json!({"cmd": "ping"})])
        .expect_err("a non-zero exit");
    assert_eq!(err.code().as_str(), "engine_failed", "{err:?}");
    assert_eq!(err.exit().code(), 1, "{err:?}");

    let bad = env.root().join("bad.sh");
    write_executable(
        &bad,
        "#!/bin/sh\nwhile IFS= read -r _line; do echo 'not json'; done\n",
    );
    let engine = Bridge::new(true, bad, PathBuf::from("/bin/sh"), Duration::from_secs(10))
        .expect("a client");
    let err = engine
        .run(&db, &[json!({"cmd": "ping"})])
        .expect_err("bad JSON");
    assert_eq!(err.code().as_str(), "engine_failed", "{err:?}");
    assert_eq!(err.exit().code(), 1, "{err:?}");

    let hang = env.root().join("hang.sh");
    // `exec` so the timeout kills one process, not a `sh` that leaves a `sleep` orphan.
    write_executable(
        &hang,
        "#!/bin/sh\nwhile IFS= read -r _line; do :; done\nexec sleep 60\n",
    );
    let engine = Bridge::new(
        true,
        hang,
        PathBuf::from("/bin/sh"),
        Duration::from_millis(200),
    )
    .expect("a client");
    let err = engine
        .run(&db, &[json!({"cmd": "ping"})])
        .expect_err("a timeout");
    assert_eq!(err.code().as_str(), "engine_failed", "{err:?}");
    assert_eq!(err.exit().code(), 1, "{err:?}");
    assert!(err.details().contains_key("timeout_ms"), "{err:?}");
}

/// docs/research/context-holographic.md, `## Config and failure`: a missing bridge or
/// interpreter is `engine_missing` (exit 4).
#[test]
fn a_missing_bridge_or_interpreter_is_engine_missing() {
    let env = TestEnv::new();
    let script = env.root().join("bridge.sh");
    write_executable(&script, "#!/bin/sh\nexit 0\n");

    let missing_bridge = Bridge::new(
        true,
        env.root().join("no-such-bridge.py"),
        PathBuf::from("/bin/sh"),
        Duration::from_secs(5),
    )
    .err()
    .expect("a missing bridge");
    assert_eq!(
        missing_bridge.code().as_str(),
        "engine_missing",
        "{missing_bridge:?}"
    );
    assert_eq!(missing_bridge.exit().code(), 4, "{missing_bridge:?}");

    let missing_python = Bridge::new(
        true,
        script,
        env.root().join("no-such-python"),
        Duration::from_secs(5),
    )
    .err()
    .expect("a missing interpreter");
    assert_eq!(
        missing_python.code().as_str(),
        "engine_missing",
        "{missing_python:?}"
    );
    assert_eq!(missing_python.exit().code(), 4, "{missing_python:?}");
}

/// docs/research/context-holographic.md, `## Config and failure`:
/// `context_engine_enabled = false` is `engine_disabled` (exit 3) with no spawn.
#[test]
fn a_disabled_engine_is_engine_disabled_without_a_spawn() {
    let env = TestEnv::new();
    let marker = env.root().join("spawned");
    let script = env.root().join("bridge.sh");
    write_executable(
        &script,
        &format!("#!/bin/sh\ntouch '{}'\nexit 0\n", marker.display()),
    );

    let err = Bridge::new(
        false,
        script,
        PathBuf::from("/bin/sh"),
        Duration::from_secs(5),
    )
    .err()
    .expect("a disabled engine");
    assert_eq!(err.code().as_str(), "engine_disabled", "{err:?}");
    assert_eq!(err.exit().code(), 3, "{err:?}");
    assert!(!marker.exists(), "a disabled engine spawns nothing");
}

// -------------------------------------------------------------- the config

/// contract §8.2: the six engine keys with their built-in defaults and sources
/// (`docs/research/context-holographic.md`, `## Config and failure`).
#[test]
fn the_config_has_the_engine_keys_with_defaults_and_sources() {
    let env = TestEnv::new();
    env.write_config("version = 1\nmachine = \"hs\"\n");
    let out = env.cpd(&["config"]).ok();
    let config = &out["config"];
    assert_eq!(config["context_engine_enabled"], true, "{config}");
    assert_eq!(config["context_engine_bridge"], "", "{config}");
    assert_eq!(config["context_engine_python"], "", "{config}");
    assert_eq!(config["context_engine_dir"], "", "{config}");
    assert_eq!(config["context_engine_timeout_ms"], 30000, "{config}");
    assert_eq!(
        config["context_engine_reindex_timeout_ms"], 600000,
        "{config}"
    );
    for key in [
        "context_engine_enabled",
        "context_engine_bridge",
        "context_engine_python",
        "context_engine_dir",
        "context_engine_timeout_ms",
        "context_engine_reindex_timeout_ms",
    ] {
        assert_eq!(out["sources"][key], "default", "{key}");
    }

    env.write_config(
        "version = 1\nmachine = \"hs\"\ncontext_engine_enabled = false\ncontext_engine_bridge = \"/opt/bridge.py\"\ncontext_engine_timeout_ms = 1234\n",
    );
    let out = env.cpd(&["config"]).ok();
    let config = &out["config"];
    assert_eq!(config["context_engine_enabled"], false, "{config}");
    assert_eq!(
        config["context_engine_bridge"], "/opt/bridge.py",
        "{config}"
    );
    assert_eq!(config["context_engine_timeout_ms"], 1234, "{config}");
    assert_eq!(out["sources"]["context_engine_enabled"], "file");
    assert_eq!(out["sources"]["context_engine_bridge"], "file");
    assert_eq!(out["sources"]["context_engine_timeout_ms"], "file");
}
