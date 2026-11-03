//! `cpd context search` answered by the holographic engine (docs/research/context-holographic.md
//! `## Search contract` and `## Index and staleness`; contract §4.6; commands.md §3.3), in this
//! process (`Fx`).
//!
//! Every test drives a fake bridge ([`common::FakeBridge`]) through the engine config keys; no test
//! imports or runs holographic. The tests are red until `context search` and the engine client exist.

use crate::common;

use common::FakeBridge;
use common::fx::Fx;
use cpd_core::context::curation::{Curation, propose};
use cpd_core::ids::ContextId;
use serde_json::{Value, json};

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

/// Records a decision and returns its id.
fn decide(fx: &Fx, text: &str, supersedes: Option<&str>) -> String {
    let mut args = vec!["context", "decide", text, "--context", CTX];
    if let Some(id) = supersedes {
        args.extend(["--supersedes", id]);
    }
    fx.cpd(&args).ok()["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A `stats` whose watermark is ahead of every record, so a search sends no `index`.
fn synced(facts: u64) -> String {
    json!({"ok": true, "facts": facts, "indexed_seq": 1_000_000, "curated_at": "2999-01-01T00:00:00.000Z"}).to_string()
}

/// A `search` response with the hits in the given engine order.
fn hits(hits: &[(&str, f64)]) -> String {
    let hits: Vec<Value> = hits
        .iter()
        .map(|(record, score)| json!({"fact_id": 1, "record": record, "score": score}))
        .collect();
    json!({"ok": true, "hits": hits}).to_string()
}

/// [`hits`] over owned ids (for a generated list).
fn hits_owned(hits: &[(String, f64)]) -> String {
    let hits: Vec<Value> = hits
        .iter()
        .map(|(record, score)| json!({"fact_id": 1, "record": record, "score": score}))
        .collect();
    json!({"ok": true, "hits": hits}).to_string()
}

/// Writes a curation (the `short` a search lists) for `id` through the real `propose`, as a distill run does.
fn curate(fx: &Fx, id: &str, short: &str) {
    curate_topic(fx, id, "engine", short);
}

/// [`curate`] with a chosen topic.
fn curate_topic(fx: &Fx, id: &str, topic: &str, short: &str) {
    let curations = [Curation {
        record: id.parse().unwrap(),
        topic: topic.into(),
        short: short.into(),
    }];
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &curations,
        &[],
    )
    .unwrap();
}

/// Inserts `n` notes directly (a fixture), returning their ids in `seq` order.
fn seed_notes(fx: &Fx, n: usize) -> Vec<String> {
    let ids: Vec<String> = (1..=n).map(|i| format!("hs-n{i}")).collect();
    let values: Vec<String> = (1..=n)
        .map(|i| format!("('{CTX}','hs-n{i}','note','2026-01-01T00:00:00.000Z','hs','Note {i}.')"))
        .collect();
    fx.env.sql(&format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text) VALUES {};",
        values.join(",")
    ));
    ids
}

/// Takes an out-of-force `id` out of force with an approved retirement row (a fixture for `--all`).
fn retire(fx: &Fx, id: &str, reason: &str, target: &str) {
    fx.env.sql(&format!(
        "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at, decided_at, decided_by) \
         VALUES ('{CTX}', '{id}', '{reason}', '{target}', 'approved', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z', 'user');"
    ));
}

/// The two search requests a test made, as `(query, limit)` in order.
fn searches(bridge: &FakeBridge) -> Vec<(String, u64)> {
    bridge
        .requests()
        .iter()
        .filter(|request| request["cmd"] == "search")
        .map(|request| {
            (
                request["query"].as_str().unwrap_or_default().to_owned(),
                request["limit"].as_u64().expect("a search names a limit"),
            )
        })
        .collect()
}

#[test]
fn the_result_is_the_design_frame_with_the_short_line() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "The engine ranks the hits.");
    curate(&fx, &n1, "engine ranks the hits");
    bridge.reply("stats", &synced(1));
    bridge.reply("search", &hits(&[(&n1, 0.42)]));

    let out = fx
        .cpd(&["context", "search", "engine", "--context", CTX])
        .ok();
    assert_eq!(out["context"]["id"], CTX, "{out}");
    assert_eq!(out["query"], "engine", "{out}");
    let record = &out["records"][0];
    assert_eq!(record["id"], n1.as_str(), "{out}");
    assert_eq!(record["short"], "engine ranks the hits", "{out}");
    assert_eq!(record["topic"], "engine", "{out}");
    assert_eq!(record["kind"], "note", "{out}");
    assert!(record["at"].is_string(), "{out}");
    assert_eq!(record["seq"], 1, "{out}");
    assert_eq!(
        record["score"], 0.42,
        "score is the engine's, higher better: {out}"
    );
    assert_eq!(record["in_force"], true, "{out}");
    assert_eq!(
        out["markdown"],
        format!("- [{n1}] [engine] engine ranks the hits"),
        "{out}"
    );
}

#[test]
fn a_curated_hit_carries_its_topic() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "The store keeps the rows.");
    curate_topic(&fx, &n1, "store", "store keeps rows");
    bridge.reply("stats", &synced(1));
    bridge.reply("search", &hits(&[(&n1, 0.5)]));

    let out = fx
        .cpd(&["context", "search", "store", "--context", CTX])
        .ok();
    assert_eq!(out["records"][0]["topic"], "store", "{out}");
    assert_eq!(
        out["markdown"],
        format!("- [{n1}] [store] store keeps rows"),
        "{out}"
    );
}

#[test]
fn an_uncurated_hit_has_a_null_topic() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "An uncurated note.");
    bridge.reply("stats", &synced(1));
    bridge.reply("search", &hits(&[(&n1, 0.5)]));

    let out = fx
        .cpd(&["context", "search", "note", "--context", CTX])
        .ok();
    let record = out["records"][0].as_object().expect("a record");
    assert_eq!(record.get("topic"), Some(&Value::Null), "{out}");
    assert_eq!(
        out["markdown"],
        format!("- [{n1}] An uncurated note."),
        "{out}"
    );
}

#[test]
fn out_of_force_records_are_hidden_unless_all() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "Old note.");
    let n2 = note(&fx, "Retired note.");
    let n3 = note(&fx, "Live note.");
    decide(&fx, "The new decision.", Some(&n1));
    retire(&fx, &n2, "duplicate", "hs-n3");
    bridge.reply("stats", &synced(3));
    bridge.reply("search", &hits(&[(&n1, 0.9), (&n2, 0.8), (&n3, 0.1)]));

    let visible = fx
        .cpd(&["context", "search", "note", "--context", CTX])
        .ok();
    let ids: Vec<&str> = visible["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [n3.as_str()],
        "an out-of-force record is hidden by default: {visible}"
    );

    let all = fx
        .cpd(&["context", "search", "note", "--context", CTX, "--all"])
        .ok();
    let ids: Vec<&str> = all["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [n1.as_str(), n2.as_str(), n3.as_str()],
        "--all includes them, in the engine's order: {all}"
    );
    assert_eq!(all["records"][0]["in_force"], false, "{all}");
    assert_eq!(all["records"][1]["in_force"], false, "{all}");
    assert_eq!(all["records"][2]["in_force"], true, "{all}");
}

#[test]
fn all_labels_superseded_and_retired() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "Old note.");
    let n2 = note(&fx, "Documented note.");
    decide(&fx, "The new decision.", Some(&n1));
    retire(&fx, &n2, "documented", "docs/a.md");
    bridge.reply("stats", &synced(3));
    bridge.reply("search", &hits(&[(&n1, 0.9), (&n2, 0.8)]));

    let all = fx
        .cpd(&["context", "search", "note", "--context", CTX, "--all"])
        .ok();
    assert_eq!(
        all["markdown"],
        format!(
            "- [{n1}] Old note. (superseded by hs-d1)\n- [{n2}] Documented note. [retired (documented docs/a.md)]"
        ),
        "{all}"
    );
}

#[test]
fn kind_selects_the_searched_kinds_and_plan_is_selectable() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "A note.");
    let d1 = decide(&fx, "A decision.", None);
    let q1 = fx
        .cpd(&["context", "ask", "A question?", "--context", CTX])
        .ok()["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let a1 = fx
        .cpd(&["context", "answer", &q1, "An answer.", "--context", CTX])
        .ok()["record"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.env.sql(&format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine) \
         VALUES ('{CTX}', 'hs-p1', 'plan', '2026-01-01T00:00:00.000Z', 'hs', 'A Plan record.', 'p1', 'hs');"
    ));
    bridge.reply("stats", &synced(5));
    bridge.reply(
        "search",
        &hits(&[
            (&n1, 0.9),
            (&d1, 0.8),
            (&q1, 0.7),
            (&a1, 0.6),
            ("hs-p1", 0.5),
        ]),
    );

    let default = fx.cpd(&["context", "search", "a", "--context", CTX]).ok();
    let kinds: Vec<&str> = default["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["note", "decision", "question", "answer"],
        "the default kinds, never plan: {default}"
    );

    let plans = fx
        .cpd(&["context", "search", "a", "--context", CTX, "--kind", "plan"])
        .ok();
    let records = plans["records"].as_array().unwrap();
    assert_eq!(records.len(), 1, "--kind plan selects it: {plans}");
    assert_eq!(records[0]["id"], "hs-p1", "{plans}");
    assert_eq!(records[0]["kind"], "plan", "{plans}");
    assert_eq!(
        records[0]["in_force"], false,
        "a plan record is never in force: {plans}"
    );
}

#[test]
fn the_over_fetch_retries_once_and_hidden_hits_do_not_count() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "The one live note.");
    let n2 = note(&fx, "The hidden note.");
    decide(&fx, "The new decision.", Some(&n2));
    // The engine returns more hits than the filtered page needs: the first search returns exactly
    // N = max(limit*5, 80) = 80 hits, none of them in force, so cpd retries once with N*5 = 400.
    let gone = |from: usize, to: usize| -> Vec<(String, f64)> {
        (from..to).map(|i| (format!("hs-n{i}"), 0.1)).collect()
    };
    let mut first = gone(1000, 1079);
    first.push((n2.clone(), 0.2));
    let mut second = gone(2000, 2399);
    second.push((n1.clone(), 0.5));
    bridge.reply("stats", &synced(1000));
    bridge.reply_n("search", 1, &hits_owned(&first));
    bridge.reply_n("search", 2, &hits_owned(&second));

    let out = fx
        .cpd(&[
            "context",
            "search",
            "note",
            "--context",
            CTX,
            "--limit",
            "1",
        ])
        .ok();
    let ids: Vec<&str> = out["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [n1.as_str()],
        "hidden hits never count against the limit: {out}"
    );
    assert_eq!(
        searches(&bridge),
        [("note".to_owned(), 80), ("note".to_owned(), 400)],
        "one retry with a larger N: {:?}",
        bridge.requests()
    );
}

#[test]
fn the_limit_defaults_to_20_and_zero_asks_for_all() {
    let (fx, bridge) = env();
    let ids = seed_notes(&fx, 200);
    let all: Vec<(String, f64)> = ids.iter().map(|id| (id.clone(), 0.5)).collect();
    let first: Vec<(String, f64)> = all[..100].to_vec();
    bridge.reply("stats", &synced(200));
    bridge.reply_n("search", 1, &hits_owned(&first));
    bridge.reply_n("search", 2, &hits_owned(&all));

    let default = fx
        .cpd(&["context", "search", "note", "--context", CTX])
        .ok();
    assert_eq!(
        default["records"].as_array().unwrap().len(),
        20,
        "the default page is 20: {default}"
    );
    assert_eq!(
        default["markdown"].as_str().unwrap().lines().count(),
        20,
        "{default}"
    );

    let every = fx
        .cpd(&[
            "context",
            "search",
            "note",
            "--context",
            CTX,
            "--limit",
            "0",
        ])
        .ok();
    assert_eq!(
        every["records"].as_array().unwrap().len(),
        200,
        "--limit 0 is no limit: {every}"
    );
    assert_eq!(
        searches(&bridge),
        [("note".to_owned(), 100), ("note".to_owned(), 200)],
        "the over-fetch N for the default and for all: {:?}",
        bridge.requests()
    );
}

#[test]
fn exact_ties_break_by_newest_seq() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "First.");
    let n2 = note(&fx, "Second.");
    let n3 = note(&fx, "Third.");
    bridge.reply("stats", &synced(3));
    bridge.reply("search", &hits(&[(&n1, 0.5), (&n2, 0.5), (&n3, 0.5)]));

    let out = fx
        .cpd(&["context", "search", "note", "--context", CTX])
        .ok();
    let ids: Vec<&str> = out["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [n3.as_str(), n2.as_str(), n1.as_str()],
        "exact ties break by seq descending: {out}"
    );
}

#[test]
fn no_hits_is_success() {
    let (fx, bridge) = env();
    note(&fx, "A note.");
    bridge.reply("stats", &synced(1));
    bridge.reply("search", &json!({"ok": true, "hits": []}).to_string());

    let out = fx
        .cpd(&["context", "search", "nothing", "--context", CTX])
        .ok();
    assert_eq!(out["records"], json!([]), "{out}");
    assert_eq!(out["markdown"], "", "{out}");
}

#[test]
fn an_empty_query_and_a_bad_query_are_usage() {
    let (fx, bridge) = env();
    note(&fx, "A note.");

    let empty = fx.cpd(&["context", "search", "   ", "--context", CTX]);
    common::refused(&empty, "usage", 2);
    assert!(
        !bridge.log.exists(),
        "an empty query never reaches the bridge"
    );

    bridge.reply("stats", &synced(1));
    bridge.reply(
        "search",
        &json!({"ok": false, "error": "bad_query", "message": "fts5: syntax error"}).to_string(),
    );
    let bad = fx.cpd(&["context", "search", "\"unterminated", "--context", CTX]);
    let error = common::refused(&bad, "usage", 2);
    assert_eq!(error["details"]["query"], "\"unterminated", "{error}");
    assert_eq!(error["details"]["message"], "fts5: syntax error", "{error}");
}

#[test]
fn a_missing_disabled_or_failed_engine_is_the_designed_code() {
    let (fx, bridge) = env();
    note(&fx, "A note.");

    // Software missing: the bridge path names nothing.
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"hs\"\ncontext_engine_bridge = \"{}/nope.py\"\ncontext_engine_python = \"/bin/sh\"\n",
        fx.env.root().display()
    ));
    let missing = fx.cpd(&["context", "search", "note", "--context", CTX]);
    let error = common::refused(&missing, "engine_missing", 4);
    assert!(
        error["details"]["bridge"]
            .as_str()
            .unwrap()
            .ends_with("nope.py"),
        "{error}"
    );

    // The operator's kill switch: no spawn. The key appears once (TOML forbids duplicates).
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"hs\"\n{}",
        bridge.config().replacen(
            "context_engine_enabled = true",
            "context_engine_enabled = false",
            1
        )
    ));
    let disabled = fx.cpd(&["context", "search", "note", "--context", CTX]);
    common::refused(&disabled, "engine_disabled", 3);
    assert!(!bridge.log.exists(), "a disabled engine spawns nothing");

    // The bridge crashes: a non-zero exit is engine_failed.
    bridge.install("exit 3\n");
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"hs\"\n{}",
        bridge.config()
    ));
    let failed = fx.cpd(&["context", "search", "note", "--context", CTX]);
    common::refused(&failed, "engine_failed", 1);
}

#[test]
fn a_search_syncs_only_the_new_and_changed_facts() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "The curated note.");
    let n2 = note(&fx, "The old note.");
    curate(&fx, &n1, "curated note");
    let n3 = note(&fx, "The new note.");
    bridge.reply("stats", &json!({"ok": true, "facts": 3, "indexed_seq": 2, "curated_at": "2000-01-01T00:00:00.000Z"}).to_string());
    bridge.reply(
        "index",
        &json!({"ok": true, "inserted": 2, "updated": 0}).to_string(),
    );
    bridge.reply("search", &hits(&[(&n3, 0.5)]));

    fx.cpd(&["context", "search", "note", "--context", CTX])
        .ok();
    let commands = bridge.commands();
    assert_eq!(
        commands,
        ["stats", "index", "search"],
        "{:?}",
        bridge.requests()
    );
    let requests = bridge.requests();
    let indexed: Vec<&str> = requests
        .iter()
        .find(|request| request["cmd"] == "index")
        .expect("an index request")["facts"]
        .as_array()
        .expect("facts")
        .iter()
        .map(|fact| fact["record"].as_str().unwrap())
        .collect();
    assert_eq!(
        indexed,
        [n1.as_str(), n3.as_str()],
        "the changed curation and the new record, never the unchanged one: {:?}",
        bridge.requests()
    );
    assert!(!indexed.contains(&n2.as_str()), "{:?}", bridge.requests());
}

/// Runs a search for `typed` against a fake engine that returns one note; returns the answer and the
/// query the engine received.
fn search_typed(typed: &str) -> (Value, String, String) {
    let (fx, bridge) = env();
    let n1 = note(&fx, "A note about retrieval.");
    bridge.reply("stats", &synced(1));
    bridge.reply("search", &hits(&[(&n1, 0.5)]));
    let out = fx.cpd(&["context", "search", typed, "--context", CTX]).ok();
    let sent = searches(&bridge);
    assert_eq!(sent.len(), 1, "one search request: {:?}", bridge.requests());
    (out, sent[0].0.clone(), n1)
}

/// Asserts `typed` is answered ok with the engine's record and reaches the engine as `sent`.
fn assert_reaches_engine(typed: &str, sent: &str) {
    let (out, got, n1) = search_typed(typed);
    assert_eq!(out["records"][0]["id"], n1.as_str(), "{out}");
    assert_eq!(got, sent, "typed {typed:?}: {out}");
}

#[test]
fn a_bare_id_reaches_the_engine_quoted() {
    assert_reaches_engine("hs-d655", "\"hs-d655\"");
}

#[test]
fn a_dotted_token_reaches_the_engine_quoted() {
    assert_reaches_engine("db.migrations", "\"db.migrations\"");
}

#[test]
fn a_slashed_token_reaches_the_engine_quoted() {
    assert_reaches_engine("plan/key", "\"plan/key\"");
}

#[test]
fn an_apostrophe_reaches_the_engine_quoted() {
    assert_reaches_engine("don't", "\"don't\"");
}

#[test]
fn an_id_among_plain_words_reaches_the_engine_quoted() {
    assert_reaches_engine(
        "retrieval of hs-d655 today",
        "retrieval of \"hs-d655\" today",
    );
}

#[test]
fn the_answer_echoes_the_query_as_typed() {
    let (out, got, n1) = search_typed("hs-d655");
    assert_eq!(out["records"][0]["id"], n1.as_str(), "{out}");
    assert_eq!(out["query"], "hs-d655", "{out}");
    assert_eq!(got, "\"hs-d655\"", "{out}");
}

#[test]
fn a_short_page_retry_repeats_the_quoted_query() {
    let (fx, bridge) = env();
    let n1 = note(&fx, "The one live note.");
    let n2 = note(&fx, "The hidden note.");
    decide(&fx, "The new decision.", Some(&n2));
    let gone = |from: usize, to: usize| -> Vec<(String, f64)> {
        (from..to).map(|i| (format!("hs-n{i}"), 0.1)).collect()
    };
    let mut first = gone(1000, 1079);
    first.push((n2.clone(), 0.2));
    let mut second = gone(2000, 2399);
    second.push((n1.clone(), 0.5));
    bridge.reply("stats", &synced(1000));
    bridge.reply_n("search", 1, &hits_owned(&first));
    bridge.reply_n("search", 2, &hits_owned(&second));

    let out = fx
        .cpd(&[
            "context",
            "search",
            "hs-d655",
            "--context",
            CTX,
            "--limit",
            "1",
        ])
        .ok();
    assert_eq!(out["records"][0]["id"], n1.as_str(), "{out}");
    assert_eq!(
        searches(&bridge),
        [
            ("\"hs-d655\"".to_owned(), 80),
            ("\"hs-d655\"".to_owned(), 400)
        ],
        "both requests carry the quoted query: {:?}",
        bridge.requests()
    );
}

#[test]
fn a_prefix_phrase_and_operator_reach_the_engine_as_typed() {
    for typed in ["retriev*", "\"first message\" OR digest"] {
        assert_reaches_engine(typed, typed);
    }
    // The prefix star and the operator stay outside the quotes of an id they follow or join.
    assert_reaches_engine("pro-d217*", "\"pro-d217\"*");
    assert_reaches_engine(
        "\"first message\" OR hs-d655",
        "\"first message\" OR \"hs-d655\"",
    );
}
