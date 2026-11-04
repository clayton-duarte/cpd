//! The `supersedes_unlinked` refusal of `cpd context decide|note` and `--no-supersedes`: a text that names, after
//! `supersedes`, `overrules` or `replaces`, an in-force record the flag does not list is refused. In this process (`Fx`).
//!
//! Each acceptance test first checks, on the same Context, that an unlinked in-force target is refused: a test of
//! "not refused" alone would pass before the refusal exists.

use crate::common;

use common::Run;
use common::fx::Fx;
use serde_json::Value;

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

fn refused(run: Run, exit: i32, expected: &str) -> Value {
    let error = common::refused(&run, expected, exit);
    assert_eq!(run.stderr, "", "{run:?}");
    error
}

fn write(fx: &Fx, kind: &str, text: &str) {
    fx.cpd(&["context", kind, text, "--context", CTX]).ok();
}

fn decide(fx: &Fx, text: &str, extra: &[&str]) -> Run {
    let mut args = vec!["context", "decide", text];
    args.extend_from_slice(extra);
    args.extend_from_slice(&["--context", CTX]);
    fx.cpd(&args)
}

fn record_ids(fx: &Fx) -> Vec<String> {
    fx.cpd(&["context", "show", "--all", "--context", CTX]).ok()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

/// Two decisions `hs-d1`, `hs-d2`, in force.
fn two() -> Fx {
    let fx = hs();
    write(&fx, "decide", "Old one.");
    write(&fx, "decide", "Old two.");
    fx
}

/// The refusal of an unlinked in-force target, the control of the acceptance tests.
fn assert_unlinked_refused(fx: &Fx) {
    let error = refused(
        decide(fx, "Control: supersedes hs-d1.", &[]),
        3,
        "supersedes_unlinked",
    );
    assert_eq!(error["details"]["ids"], serde_json::json!(["hs-d1"]));
}

#[test]
fn a_decision_whose_text_supersedes_an_in_force_record_without_the_flag_is_refused() {
    let fx = two();
    let error = refused(
        decide(&fx, "New. This supersedes hs-d1.", &[]),
        3,
        "supersedes_unlinked",
    );
    assert_eq!(error["details"]["ids"], serde_json::json!(["hs-d1"]));
}

#[test]
fn an_overrules_and_a_replaces_phrase_are_refused_the_same_way() {
    let fx = two();
    let error = refused(
        decide(&fx, "New. It overrules hs-d1.", &[]),
        3,
        "supersedes_unlinked",
    );
    assert_eq!(error["details"]["ids"], serde_json::json!(["hs-d1"]));
    write(&fx, "note", "A fact.");
    let run = fx.cpd(&[
        "context",
        "note",
        "A note that REPLACES hs-n1.",
        "--context",
        CTX,
    ]);
    let error = refused(run, 3, "supersedes_unlinked");
    assert_eq!(error["details"]["ids"], serde_json::json!(["hs-n1"]));
    fx.cpd(&[
        "context",
        "note",
        "A note that replaces hs-d2.",
        "--context",
        CTX,
    ])
    .ok();
}

#[test]
fn a_phrase_naming_two_records_lists_both_ids_in_the_refusal_details() {
    let fx = two();
    let error = refused(
        decide(&fx, "New: supersedes hs-d2 and hs-d1.", &[]),
        3,
        "supersedes_unlinked",
    );
    assert_eq!(
        error["details"]["ids"],
        serde_json::json!(["hs-d2", "hs-d1"])
    );
}

#[test]
fn the_refusal_writes_nothing() {
    let fx = two();
    refused(
        decide(&fx, "New. Supersedes hs-d1.", &[]),
        3,
        "supersedes_unlinked",
    );
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-d2"]);
}

#[test]
fn no_supersedes_accepts_the_prose_and_writes_no_supersedes() {
    let fx = two();
    assert_unlinked_refused(&fx);
    let out = decide(&fx, "New. Supersedes hs-d1.", &["--no-supersedes"]).ok();
    assert!(out["record"].get("supersedes").is_none(), "{out}");
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-d2", "hs-d3"]);
}

#[test]
fn a_supersedes_flag_listing_every_prose_target_is_accepted() {
    let fx = two();
    assert_unlinked_refused(&fx);
    let out = decide(
        &fx,
        "New: supersedes hs-d1 and hs-d2.",
        &["--supersedes", "hs-d1", "--supersedes", "hs-d2"],
    )
    .ok();
    assert_eq!(
        out["record"]["supersedes"],
        serde_json::json!(["hs-d1", "hs-d2"]),
        "{out}"
    );
}

#[test]
fn a_supersedes_flag_missing_one_prose_target_is_refused() {
    let fx = two();
    let error = refused(
        decide(
            &fx,
            "New: supersedes hs-d1 and hs-d2.",
            &["--supersedes", "hs-d1"],
        ),
        3,
        "supersedes_unlinked",
    );
    assert_eq!(error["details"]["ids"], serde_json::json!(["hs-d2"]));
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-d2"]);
}

#[test]
fn a_prose_target_already_superseded_is_not_refused() {
    let fx = two();
    assert_unlinked_refused(&fx);
    decide(&fx, "Third.", &["--supersedes", "hs-d1"]).ok();
    // hs-d1 is no longer in force: naming it is a mention.
    decide(&fx, "Fourth. Supersedes hs-d1.", &[]).ok();
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-d2", "hs-d3", "hs-d4"]);
}

#[test]
fn a_prose_target_of_an_unsupersedable_kind_or_unknown_id_is_not_refused() {
    let fx = two();
    assert_unlinked_refused(&fx);
    // hs-d1 is a decision: a note may not supersede it; hs-d9 does not exist.
    fx.cpd(&[
        "context",
        "note",
        "New. Supersedes hs-d1 and hs-d9.",
        "--context",
        CTX,
    ])
    .ok();
}

#[test]
fn a_partial_amendment_phrase_is_not_refused() {
    let fx = two();
    assert_unlinked_refused(&fx);
    decide(&fx, "New. Supersedes A1 of hs-d2.", &[]).ok();
}

#[test]
fn no_supersedes_conflicts_with_supersedes() {
    let fx = two();
    assert_unlinked_refused(&fx);
    let run = decide(&fx, "New.", &["--no-supersedes", "--supersedes", "hs-d1"]);
    refused(run, 2, "args_invalid");
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-d2"]);
}
