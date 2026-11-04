//! The question and answer Context record kinds are gone (pro-d8): `cpd context ask` and `cpd context answer` are
//! refused as usage errors, `list` and `show` carry no question counts or sections, and the letters q and a no longer
//! parse as record ids.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::refused;

const CTX: &str = "building-cpd";

/// A machine `hs` with Context `building-cpd` holding decision hs-d1 and note hs-n1.
fn fx() -> Fx {
    let fx = Fx::new();
    fs::create_dir_all(fx.env.cpd_home()).unwrap();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"hs\"\n",
    )
    .unwrap();
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
    fx.cpd(&["context", "decide", "A decision.", "--context", CTX])
        .ok();
    fx.cpd(&["context", "note", "A note.", "--context", CTX])
        .ok();
    fx
}

#[test]
fn ask_is_refused_as_a_usage_error() {
    let fx = fx();
    let error = refused(
        &fx.cpd(&["context", "ask", "Why?", "--context", CTX]),
        "usage",
        2,
    );
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("unrecognized subcommand 'ask'"), "{error}");
}

#[test]
fn answer_is_refused_as_a_usage_error() {
    let fx = fx();
    let error = refused(
        &fx.cpd(&["context", "answer", "hs-q1", "Because.", "--context", CTX]),
        "usage",
        2,
    );
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("unrecognized subcommand 'answer'"),
        "{error}"
    );
}

#[test]
fn list_has_no_open_questions_key() {
    let fx = fx();
    let out = fx.cpd(&["context", "list"]).ok();
    let context = &out["contexts"][0];
    assert_eq!(context["id"], CTX, "{out}");
    assert!(
        context.get("open_questions").is_none(),
        "open_questions in {out}"
    );
}

#[test]
fn show_renders_no_question_sections() {
    let fx = fx();
    // An `ask` is refused once the kinds are gone; its outcome is not this test's concern, only what `show` renders.
    let _ = fx.cpd(&["context", "ask", "Why?", "--context", CTX]);
    let out = fx.cpd(&["context", "show", "--context", CTX]).ok();
    let md = out["markdown"].as_str().unwrap();
    assert!(md.contains("## Decisions"), "{md}");
    for heading in ["Open questions", "Answered questions"] {
        assert!(!md.contains(heading), "{heading} in {md}");
    }
}

#[test]
fn record_id_with_letter_q_or_a_is_refused() {
    let fx = fx();
    for id in ["pro-q1", "pro-a1"] {
        let run = fx.cpd(&["context", "show", "--record", id, "--context", CTX]);
        let error = run.error();
        assert_eq!(run.exit, 2, "{run:?}");
        let message = error["message"].as_str().unwrap();
        let (_, tail) = message
            .split_once("letter one of ")
            .unwrap_or_else(|| panic!("no letter list in {error}"));
        let list = tail.split(',').next().unwrap();
        let letters: Vec<&str> = list.split_whitespace().collect();
        assert!(letters.contains(&"d"), "{error}");
        assert!(
            !letters.contains(&"q") && !letters.contains(&"a"),
            "letters {letters:?} in {error}"
        );
    }
}
