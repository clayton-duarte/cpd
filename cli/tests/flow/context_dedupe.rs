//! In-force exact-text dedupe of `Contexts::write` (decision pro-d10 item 3): a write whose text (trimmed) and
//! supersedes list equal an in-force record of the same (context, kind) writes nothing and returns that record with
//! `already: true`, whatever machine or session wrote it. In this process (`Fx`).
//! Each "does not match" test first proves a control write does dedupe, so it is red until the dedupe exists.

use crate::common;

use common::fx::Fx;
use common::{Run, refused};
use cpd_core::context::curation::{Approval, Curation, Proposal, Reason, propose};
use cpd_core::ids::ContextId;
use serde_json::{Value, json};

const CTX: &str = "building-cpd";

/// Machine `hs`, with Context `building-cpd`.
fn hs() -> Fx {
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

/// Writes a record: `kind` is `decide` or `note`; `extra` are more arguments.
fn write(fx: &Fx, kind: &str, text: &str, extra: &[&str]) -> Run {
    let mut args = vec!["context", kind, text, "--context", CTX];
    args.extend_from_slice(extra);
    fx.cpd(&args)
}

fn as_session(fx: &Fx, kind: &str, text: &str, session: &str) -> Value {
    let run = fx.cpd_with(
        fx.env.root(),
        &["context", kind, text, "--context", CTX],
        &[("CPD_SESSION", session)],
    );
    serde_json::from_str(&run.stdout).unwrap()
}

fn record_ids(fx: &Fx) -> Vec<String> {
    let shown = fx.cpd(&["context", "show", "--all", "--context", CTX]).ok();
    shown["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

fn recorded_events(fx: &Fx) -> usize {
    fx.cpd(&["events", "list", "--kind", "context-record"]).ok()["events"]
        .as_array()
        .unwrap()
        .len()
}

/// Writes `text` twice, with a record between: the second is the dedupe of the first.
fn assert_deduped(fx: &Fx, kind: &str, text: &str, first: &Value) {
    let again = write(fx, kind, text, &[]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(again["record"], first["record"], "{again}");
}

#[test]
fn a_decision_with_the_text_of_an_older_in_force_decision_returns_that_record_with_already_true() {
    let fx = hs();
    let first = write(&fx, "decide", "Use sqlite.", &[]).ok();
    write(&fx, "decide", "Use rust.", &[]).ok();
    let again = write(&fx, "decide", "Use sqlite.", &[]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(again["record"], first["record"]);
    assert_eq!(again["seq"], first["seq"]);
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-d2"]);
}

#[test]
fn the_dedupe_ignores_surrounding_whitespace() {
    let fx = hs();
    let first = write(&fx, "note", "A fact.", &[]).ok();
    write(&fx, "note", "Other fact.", &[]).ok();
    let again = write(&fx, "note", "  \tA fact.\n ", &[]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(again["record"], first["record"]);
    assert_eq!(record_ids(&fx), ["hs-n1", "hs-n2"]);
}

#[test]
fn the_dedupe_matches_a_record_written_by_another_session() {
    let fx = hs();
    let first = as_session(&fx, "decide", "Shared text.", "session-one");
    write(&fx, "decide", "Other.", &[]).ok();
    let again = as_session(&fx, "decide", "Shared text.", "session-two");
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(again["record"], first["record"]);
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-d2"]);
}

#[test]
fn the_dedupe_needs_an_identical_supersedes_list() {
    let fx = hs();
    write(&fx, "decide", "Old one.", &[]).ok();
    write(&fx, "decide", "Old two.", &[]).ok();
    let first = write(&fx, "decide", "New.", &["--supersedes", "hs-d1"]).ok();
    write(&fx, "decide", "Filler.", &[]).ok();
    // control: the same text and the same list
    let same = write(&fx, "decide", "New.", &["--supersedes", "hs-d1"]).ok();
    assert_eq!(same["already"], true, "{same}");
    assert_eq!(same["record"], first["record"]);
    // no list, another list: new records
    let none = write(&fx, "decide", "New.", &[]).ok();
    assert!(none.get("already").is_none(), "{none}");
    let other = write(&fx, "decide", "New.", &["--supersedes", "hs-d2"]).ok();
    assert!(other.get("already").is_none(), "{other}");
    assert_eq!(other["record"]["id"], "hs-d6");
}

#[test]
fn the_dedupe_does_not_match_a_record_of_another_kind() {
    let fx = hs();
    let note = write(&fx, "note", "Same words.", &[]).ok();
    write(&fx, "note", "Other.", &[]).ok();
    assert_deduped(&fx, "note", "Same words.", &note);
    let decision = write(&fx, "decide", "Same words.", &[]).ok();
    assert!(decision.get("already").is_none(), "{decision}");
    assert_eq!(decision["record"]["id"], "hs-d1");
}

#[test]
fn the_dedupe_does_not_match_a_superseded_record() {
    let fx = hs();
    let old = write(&fx, "decide", "Old.", &[]).ok();
    let control = write(&fx, "decide", "Control.", &[]).ok();
    write(&fx, "decide", "Filler.", &[]).ok();
    assert_deduped(&fx, "decide", "Control.", &control);
    write(&fx, "decide", "New.", &["--supersedes", "hs-d1"]).ok();
    write(&fx, "decide", "Filler two.", &[]).ok();
    let again = write(&fx, "decide", "Old.", &[]).ok();
    assert!(again.get("already").is_none(), "{again}");
    assert_ne!(again["record"]["id"], old["record"]["id"]);
}

#[test]
fn the_dedupe_does_not_match_a_retired_record() {
    let fx = hs();
    let control = write(&fx, "note", "Control.", &[]).ok();
    write(&fx, "note", "Retired text.", &[]).ok();
    write(&fx, "note", "Kept text.", &[]).ok();
    write(&fx, "note", "Filler.", &[]).ok();
    assert_deduped(&fx, "note", "Control.", &control);
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &[Curation {
            record: "hs-n2".parse().unwrap(),
            topic: "engine".into(),
            short: "old".into(),
        }],
        &[Proposal {
            record: "hs-n2".parse().unwrap(),
            reason: Reason::Duplicate,
            target: "hs-n3".into(),
            approval: Approval::Pending,
        }],
    )
    .unwrap();
    fx.cpd(&["context", "retire", "--context", CTX, "--approve", "hs-n2"])
        .ok();
    let again = write(&fx, "note", "Retired text.", &[]).ok();
    assert!(again.get("already").is_none(), "{again}");
    assert_eq!(again["record"]["id"], "hs-n5");
}

#[test]
fn a_deduped_write_adds_no_row_and_no_event() {
    let fx = hs();
    write(&fx, "decide", "Use sqlite.", &[]).ok();
    write(&fx, "decide", "Use rust.", &[]).ok();
    let (rows, events) = (record_ids(&fx), recorded_events(&fx));
    let again = write(&fx, "decide", "Use sqlite.", &[]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(record_ids(&fx), rows);
    assert_eq!(recorded_events(&fx), events);
}

#[test]
fn a_note_superseding_a_valid_note_and_a_decision_is_supersedes_invalid_and_writes_nothing() {
    let fx = hs();
    write(&fx, "note", "A fact.", &[]).ok();
    write(&fx, "decide", "A decision.", &[]).ok();
    let control = write(&fx, "note", "Control.", &[]).ok();
    write(&fx, "note", "Filler.", &[]).ok();
    // the dedupe runs before the links: a repeated write is the existing record (red until the dedupe exists)
    assert_deduped(&fx, "note", "Control.", &control);
    let (rows, events) = (record_ids(&fx), recorded_events(&fx));
    let run = write(
        &fx,
        "note",
        "Replaces both.",
        &["--supersedes", "hs-n1", "--supersedes", "hs-d1"],
    );
    refused(&run, "supersedes_invalid", 3);
    assert_eq!(record_ids(&fx), rows);
    assert_eq!(recorded_events(&fx), events);
    let shown = fx.cpd(&["context", "show", "--all", "--context", CTX]).ok();
    assert_eq!(shown["records"][0]["id"], json!("hs-n1"));
}
