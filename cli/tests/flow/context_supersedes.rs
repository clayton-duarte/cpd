//! Multi-target `--supersedes` of `cpd context decide|note`: repeatable (`--supersedes a --supersedes b`) or a comma
//! list (`a,b`); every target is validated and retired, and the record JSON `supersedes` is an array. In this process (`Fx`).

use crate::common;

use common::fx::Fx;
use common::{Run, TestEnv};
use serde_json::{Value, json};

const CTX: &str = "building-cpd";

/// Machine `hs`, with Context `building-cpd`.
fn hs() -> Fx {
    let fx = Fx::new();
    write_hs(&fx.env);
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

fn write_hs(env: &TestEnv) {
    env.write_config("version = 1\nmachine = \"hs\"\n");
}

fn refused(run: Run, exit: i32, expected: &str) -> Value {
    let error = common::refused(&run, expected, exit);
    assert_eq!(run.stderr, "", "{run:?}");
    error
}

/// Writes a record: `kind` is `decide` or `note`.
fn write(fx: &Fx, kind: &str, text: &str) {
    fx.cpd(&["context", kind, text, "--context", CTX]).ok();
}

fn show(fx: &Fx) -> Value {
    fx.cpd(&["context", "show", "--context", CTX, "--all"]).ok()
}

fn record_ids(fx: &Fx) -> Vec<String> {
    show(fx)["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

/// Whether the markdown lists `id` as an item line (`- <id> (`).
fn lists(md: &str, id: &str) -> bool {
    md.lines().any(|l| l.starts_with(&format!("- {id} (")))
}

#[test]
fn decide_accepts_a_repeated_supersedes_and_stores_every_target() {
    let fx = hs();
    write(&fx, "decide", "Old one.");
    write(&fx, "decide", "Old two.");
    let out = fx
        .cpd(&[
            "context",
            "decide",
            "New.",
            "--supersedes",
            "hs-d1",
            "--supersedes",
            "hs-d2",
            "--context",
            CTX,
        ])
        .ok();
    assert_eq!(
        out["record"]["supersedes"],
        json!(["hs-d1", "hs-d2"]),
        "{out}"
    );
    let shown = show(&fx);
    assert_eq!(
        shown["records"][2]["supersedes"],
        json!(["hs-d1", "hs-d2"]),
        "the index keeps every target: {shown}"
    );
}

#[test]
fn decide_accepts_a_comma_list_of_supersedes_targets() {
    let fx = hs();
    write(&fx, "decide", "Old one.");
    write(&fx, "note", "Old note.");
    let out = fx
        .cpd(&[
            "context",
            "decide",
            "New.",
            "--supersedes",
            "hs-d1,hs-n1",
            "--context",
            CTX,
        ])
        .ok();
    assert_eq!(
        out["record"]["supersedes"],
        json!(["hs-d1", "hs-n1"]),
        "{out}"
    );
    let shown = show(&fx);
    assert_eq!(
        shown["records"][2]["supersedes"],
        json!(["hs-d1", "hs-n1"]),
        "{shown}"
    );
}

#[test]
fn note_accepts_a_repeated_supersedes() {
    let fx = hs();
    write(&fx, "note", "Old one.");
    write(&fx, "note", "Old two.");
    let out = fx
        .cpd(&[
            "context",
            "note",
            "New fact.",
            "--supersedes",
            "hs-n1",
            "--supersedes",
            "hs-n2",
            "--context",
            CTX,
        ])
        .ok();
    assert_eq!(
        out["record"]["supersedes"],
        json!(["hs-n1", "hs-n2"]),
        "{out}"
    );
    let shown = show(&fx);
    assert_eq!(
        shown["records"][2]["supersedes"],
        json!(["hs-n1", "hs-n2"]),
        "{shown}"
    );
}

#[test]
fn a_decision_naming_a_target_twice_is_supersedes_invalid_and_writes_nothing() {
    let fx = hs();
    write(&fx, "decide", "Old.");
    let run = fx.cpd(&[
        "context",
        "decide",
        "New.",
        "--supersedes",
        "hs-d1",
        "--supersedes",
        "hs-d1",
        "--context",
        CTX,
    ]);
    refused(run, 3, "supersedes_invalid");
    assert_eq!(record_ids(&fx), ["hs-d1"]);
}

#[test]
fn a_repeated_supersedes_naming_an_unknown_record_is_record_not_found_and_writes_nothing() {
    let fx = hs();
    write(&fx, "decide", "Old.");
    let run = fx.cpd(&[
        "context",
        "decide",
        "New.",
        "--supersedes",
        "hs-d1",
        "--supersedes",
        "hs-d9",
        "--context",
        CTX,
    ]);
    let error = refused(run, 3, "record_not_found");
    assert_eq!(error["details"]["record"], "hs-d9", "{error}");
    assert_eq!(record_ids(&fx), ["hs-d1"]);
}

#[test]
fn a_repeated_supersedes_naming_a_decision_from_a_note_is_supersedes_invalid_and_writes_nothing() {
    let fx = hs();
    write(&fx, "decide", "Old.");
    write(&fx, "note", "Fact.");
    let run = fx.cpd(&[
        "context",
        "note",
        "New.",
        "--supersedes",
        "hs-n1",
        "--supersedes",
        "hs-d1",
        "--context",
        CTX,
    ]);
    let error = refused(run, 3, "supersedes_invalid");
    assert_eq!(error["details"]["record"], "hs-d1", "{error}");
    assert_eq!(error["details"]["kind"], "decision", "{error}");
    assert_eq!(record_ids(&fx), ["hs-d1", "hs-n1"]);
}

#[test]
fn show_leaves_every_superseded_target_out_of_force() {
    let fx = hs();
    write(&fx, "decide", "Old one.");
    write(&fx, "decide", "Old two.");
    fx.cpd(&[
        "context",
        "decide",
        "New.",
        "--supersedes",
        "hs-d1",
        "--supersedes",
        "hs-d2",
        "--context",
        CTX,
    ])
    .ok();
    let shown = show(&fx);
    let md = shown["markdown"].as_str().unwrap();
    let (in_force, superseded) = md
        .split_once("## Superseded")
        .expect("a Superseded section");
    assert!(lists(in_force, "hs-d3"), "{md}");
    assert!(!lists(in_force, "hs-d1"), "hs-d1 is superseded: {md}");
    assert!(!lists(in_force, "hs-d2"), "hs-d2 is superseded: {md}");
    for old in ["hs-d1", "hs-d2"] {
        assert!(
            superseded.lines().any(
                |l| l.starts_with(&format!("- {old} (")) && l.contains("(superseded by hs-d3)")
            ),
            "{old} is listed as superseded: {md}"
        );
    }
}
