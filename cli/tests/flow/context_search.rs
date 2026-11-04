//! `cpd context search` on an in-memory FTS5 table built per call (the Plan ctx-fts-remove-holo; commands.md §3.3), in
//! this process (`Fx`). No engine, no bridge, no engine config: a test config is just the version and the machine.

use crate::common;

use common::fx::Fx;
use cpd_core::context::curation::{Curation, propose};
use cpd_core::ids::ContextId;
use serde_json::json;

const CTX: &str = "building-cpd";

/// An `Fx` on machine `hs` with Context `building-cpd`.
fn env() -> Fx {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"hs\"\n");
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

/// Writes a curation (the `short` a search lists) for `id` through the real `propose`, as a distill run does.
fn curate(fx: &Fx, id: &str, topic: &str, short: &str) {
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

/// Fixture: a retirement row `id` with `status` ('pending' has no decision).
fn retire_as(fx: &Fx, id: &str, reason: &str, target: &str, status: &str) {
    let decided = if status == "pending" {
        "NULL, NULL".to_owned()
    } else {
        "'2026-01-01T00:00:00.000Z', 'user'".to_owned()
    };
    fx.env.sql(&format!(
        "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at, decided_at, decided_by) \
         VALUES ('{CTX}', '{id}', '{reason}', '{target}', '{status}', '2026-01-01T00:00:00.000Z', {decided});"
    ));
}

/// Runs `cpd context search <args>` and returns the ids of the records, in order.
fn search_ids(fx: &Fx, query: &str, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["context", "search", query, "--context", CTX];
    args.extend(extra);
    let out = fx.cpd(&args).ok();
    out["records"]
        .as_array()
        .unwrap_or_else(|| panic!("{out}"))
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_result_keeps_the_design_frame_and_short_line() {
    let fx = env();
    let n1 = note(&fx, "The engine ranks the hits.");
    curate(&fx, &n1, "ranking", "engine ranks the hits");

    let out = fx
        .cpd(&["context", "search", "engine", "--context", CTX])
        .ok();
    assert_eq!(out["context"]["id"], CTX, "{out}");
    assert_eq!(out["query"], "engine", "{out}");
    let record = &out["records"][0];
    assert_eq!(record["id"], n1.as_str(), "{out}");
    assert_eq!(record["short"], "engine ranks the hits", "{out}");
    assert_eq!(record["kind"], "note", "{out}");
    assert!(record["at"].is_string(), "{out}");
    assert_eq!(record["seq"], 1, "{out}");
    assert!(record["score"].as_f64().unwrap() > 0.0, "{out}");
    assert_eq!(record["in_force"], true, "{out}");
    assert_eq!(
        out["markdown"],
        format!("- [{n1}] engine ranks the hits"),
        "{out}"
    );
}

#[test]
fn a_search_for_a_record_id_returns_that_record_first() {
    let fx = env();
    let n1 = note(&fx, "The first thing.");
    let n2 = note(&fx, &format!("Later I quote {n1} and {n1} again."));
    assert_eq!(n1, "hs-n1");
    assert_eq!(search_ids(&fx, &n1, &[])[0], n1);
    assert!(search_ids(&fx, &n1, &[]).contains(&n2));
}

#[test]
fn a_query_matches_curated_topic_and_short() {
    let fx = env();
    let n1 = note(&fx, "Plain words only.");
    let n2 = note(&fx, "Other plain words.");
    curate(&fx, &n1, "zebrafish", "a giraffe appears");
    assert_eq!(search_ids(&fx, "zebrafish", &[]), std::slice::from_ref(&n1));
    assert_eq!(search_ids(&fx, "giraffe", &[]), std::slice::from_ref(&n1));
    assert!(!search_ids(&fx, "plain", &[]).is_empty());
    assert!(!search_ids(&fx, "giraffe", &[]).contains(&n2));
}

#[test]
fn porter_stemming_matches_word_forms() {
    let fx = env();
    let n1 = note(&fx, "The retrieval of facts is fast.");
    note(&fx, "Nothing relevant.");
    assert_eq!(search_ids(&fx, "retrieving", &[]), [n1]);
}

#[test]
fn a_record_written_after_a_search_is_found_by_the_next_search() {
    let fx = env();
    note(&fx, "An early note.");
    assert!(search_ids(&fx, "quokka", &[]).is_empty());
    let n2 = note(&fx, "A quokka note.");
    assert_eq!(search_ids(&fx, "quokka", &[]), [n2]);
}

#[test]
fn out_of_force_records_are_hidden_unless_all() {
    let fx = env();
    let n1 = note(&fx, "Old note.");
    let n2 = note(&fx, "Retired note.");
    let n3 = note(&fx, "Live note.");
    decide(&fx, "The new decision.", Some(&n1));
    retire(&fx, &n2, "duplicate", "hs-n3");

    assert_eq!(search_ids(&fx, "note", &[]), std::slice::from_ref(&n3));

    let all = fx
        .cpd(&["context", "search", "note", "--context", CTX, "--all"])
        .ok();
    let mut got: Vec<(String, bool)> = all["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_owned(),
                r["in_force"].as_bool().unwrap(),
            )
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        [(n1, false), (n2, false), (n3, true)],
        "--all includes them: {all}"
    );
}

#[test]
fn all_labels_superseded_and_retired() {
    let fx = env();
    let n1 = note(&fx, "Old note.");
    let n2 = note(&fx, "Documented note.");
    decide(&fx, "The new decision.", Some(&n1));
    retire(&fx, &n2, "documented", "docs/a.md");

    let all = fx
        .cpd(&["context", "search", "note", "--context", CTX, "--all"])
        .ok();
    let markdown = all["markdown"].as_str().unwrap();
    let lines: Vec<&str> = markdown.lines().collect();
    assert!(
        lines.contains(&format!("- [{n1}] Old note. (superseded by hs-d1)").as_str()),
        "{all}"
    );
    assert!(
        lines.contains(
            &format!("- [{n2}] Documented note. [retired (documented docs/a.md)]").as_str()
        ),
        "{all}"
    );
}

#[test]
fn kind_selects_the_searched_kinds_and_plan_is_selectable() {
    let fx = env();
    let n1 = note(&fx, "A note about gizmos.");
    let d1 = decide(&fx, "A decision about gizmos.", None);
    fx.env.sql(&format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine) \
         VALUES ('{CTX}', 'hs-p1', 'plan', '2026-01-01T00:00:00.000Z', 'hs', 'A Plan about gizmos.', 'p1', 'hs');"
    ));

    let mut default = search_ids(&fx, "gizmos", &[]);
    default.sort();
    let mut want = vec![n1.clone(), d1.clone()];
    want.sort();
    assert_eq!(default, want, "the default kinds, never plan");

    let plans = fx
        .cpd(&[
            "context",
            "search",
            "gizmos",
            "--context",
            CTX,
            "--kind",
            "plan",
        ])
        .ok();
    let records = plans["records"].as_array().unwrap();
    assert_eq!(records.len(), 1, "--kind plan selects it: {plans}");
    assert_eq!(records[0]["id"], "hs-p1", "{plans}");
    assert_eq!(records[0]["kind"], "plan", "{plans}");
    assert_eq!(records[0]["in_force"], false, "{plans}");

    assert_eq!(search_ids(&fx, "gizmos", &["--kind", "decision"]), [d1]);
}

#[test]
fn the_limit_defaults_to_20_and_zero_asks_for_all() {
    let fx = env();
    seed_notes(&fx, 200);

    let default = fx
        .cpd(&["context", "search", "note", "--context", CTX])
        .ok();
    assert_eq!(
        default["records"].as_array().unwrap().len(),
        20,
        "{default}"
    );
    assert_eq!(
        default["markdown"].as_str().unwrap().lines().count(),
        20,
        "{default}"
    );

    assert_eq!(search_ids(&fx, "note", &["--limit", "0"]).len(), 200);
}

#[test]
fn exact_ties_break_by_newest_seq() {
    let fx = env();
    let n1 = note(&fx, "Alpha one.");
    let n2 = note(&fx, "Alpha two.");
    let n3 = note(&fx, "Alpha six.");
    assert_eq!(search_ids(&fx, "alpha", &[]), [n3, n2, n1]);
}

#[test]
fn a_pending_duplicate_or_superseded_proposal_hides_the_record_unless_all() {
    let fx = env();
    let n1 = note(&fx, "Pending duplicate note.");
    let n2 = note(&fx, "Pending superseded note.");
    let n3 = note(&fx, "Plain note.");
    retire_as(&fx, &n1, "duplicate", "hs-n3", "pending");
    retire_as(&fx, &n2, "superseded", "hs-n3", "pending");

    assert_eq!(search_ids(&fx, "note", &[]), std::slice::from_ref(&n3));
    let mut all = search_ids(&fx, "note", &["--all"]);
    all.sort();
    assert_eq!(all, [n1, n2, n3]);
}

#[test]
fn a_pending_documented_proposal_does_not_hide_the_record() {
    let fx = env();
    let n1 = note(&fx, "Pending documented note.");
    retire_as(&fx, &n1, "documented", "docs/a.md", "pending");
    assert_eq!(search_ids(&fx, "note", &[]), [n1]);
}

#[test]
fn a_record_named_by_id_is_shown_even_with_a_pending_duplicate_proposal() {
    let fx = env();
    let n1 = note(&fx, "Pending duplicate note.");
    note(&fx, "Plain note.");
    retire_as(&fx, &n1, "duplicate", "hs-n2", "pending");
    assert_eq!(search_ids(&fx, &n1, &[])[0], n1);
}

#[test]
fn weak_tail_hits_are_dropped_by_the_relative_floor() {
    let fx = env();
    let strong = note(&fx, &"zebra ".repeat(40));
    let filler =
        "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor ".repeat(6);
    for _ in 0..8 {
        note(&fx, &format!("{filler} zebra {filler}"));
    }
    let ids = search_ids(&fx, "zebra", &["--limit", "0"]);
    assert_eq!(ids[0], strong);
    assert!(ids.len() < 9, "the weak tail is dropped: {ids:?}");
}

#[test]
fn no_hits_is_success() {
    let fx = env();
    note(&fx, "A note.");
    let out = fx
        .cpd(&["context", "search", "nothing", "--context", CTX])
        .ok();
    assert_eq!(out["records"], json!([]), "{out}");
    assert_eq!(out["markdown"], "", "{out}");
}

#[test]
fn an_empty_query_and_a_bad_query_are_usage() {
    let fx = env();
    note(&fx, "A note.");
    let empty = fx.cpd(&["context", "search", "   ", "--context", CTX]);
    common::refused(&empty, "usage", 2);

    let bad = fx.cpd(&["context", "search", "\"unterminated", "--context", CTX]);
    let error = common::refused(&bad, "usage", 2);
    assert_eq!(error["details"]["query"], "\"unterminated", "{error}");
    assert!(error["details"]["message"].is_string(), "{error}");
}

#[test]
fn punctuation_in_a_query_is_text() {
    let fx = env();
    let n1 = note(&fx, "See db.migrations and plan/key, don't panic.");
    note(&fx, "Unrelated words.");
    for typed in ["db.migrations", "plan/key", "don't"] {
        assert_eq!(
            search_ids(&fx, typed, &[]),
            std::slice::from_ref(&n1),
            "{typed}"
        );
    }
    let out = fx
        .cpd(&["context", "search", "hs-n1", "--context", CTX])
        .ok();
    assert_eq!(out["records"][0]["id"], "hs-n1", "{out}");
}

#[test]
fn search_needs_no_engine_config() {
    let fx = env();
    fx.env.write_config("version = 1\nmachine = \"hs\"\n");
    let n1 = note(&fx, "A note about quokkas.");
    let out = fx
        .cpd(&["context", "search", "quokkas", "--context", CTX])
        .ok();
    assert_eq!(out["records"][0]["id"], n1.as_str(), "{out}");
}

#[test]
fn context_reindex_is_no_longer_a_command() {
    let fx = env();
    let run = fx.cpd(&["context", "reindex", "--context", CTX]);
    assert_eq!(run.exit, 2, "{run:?}");
}
