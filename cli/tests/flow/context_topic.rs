//! `context show --topic` (contract §4.6 "Consumers"; commands.md §3.3; hs-d549, hs-d551): the topic view stays
//! when the digest goes. The tests run the command tree in this process (`Fx`); curations are written through
//! `curation::propose`, as the distiller will.

use crate::common;

use crate::context_on_demand::{CTX, curate, fast};
use common::TestEnv;
use common::fx::Fx;
use cpd_core::context::curation::{Approval, Proposal, Reason, propose};
use cpd_core::ids::ContextId;
use serde_json::Value;

/// Inserts a record of `kind` written `minutes` minutes ago (the CLI cannot backdate a record).
fn seed(env: &TestEnv, id: &str, kind: &str, minutes: u32, text: &str) {
    env.sql(&format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text)
         VALUES ('{CTX}', '{id}', '{kind}', strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{minutes} minutes'), 'm1', '{text}')"
    ));
}

fn topic(env: &Fx, t: &str) -> Value {
    env.cpd(&["context", "show", "--context", CTX, "--topic", t])
        .ok()
}

fn markdown(out: &Value) -> String {
    out["markdown"]
        .as_str()
        .unwrap_or_else(|| panic!("a markdown in {out}"))
        .to_owned()
}

/// Every curated record in force of a topic, oldest first.
#[test]
fn show_topic_prints_every_curated_record_of_the_topic() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 40 * 24 * 60, "a");
    seed(&fx.env, "m1-n2", "note", 30 * 24 * 60, "b");
    seed(&fx.env, "m1-n3", "note", 20 * 24 * 60, "c");
    seed(&fx.env, "m1-n4", "note", 10 * 24 * 60, "d");
    seed(&fx.env, "m1-n5", "note", 5 * 24 * 60, "e");
    curate(
        &fx.env,
        &[
            ("m1-n1", "engine", "first"),
            ("m1-n2", "store", "other topic"),
            ("m1-n3", "engine", "second"),
            ("m1-n4", "engine", "superseded one"),
        ],
    );
    // m1-n5 is uncurated; a later note supersedes m1-n4
    fx.cpd(&[
        "context",
        "note",
        "Newer.",
        "--supersedes",
        "m1-n4",
        "--context",
        CTX,
    ])
    .ok();

    let out = topic(&fx, "engine");
    assert_eq!(out["topic"], "engine", "{out}");
    assert_eq!(
        out["records"],
        serde_json::json!([{"id": "m1-n1", "short": "first"}, {"id": "m1-n3", "short": "second"}]),
        "{out}"
    );
    assert_eq!(
        markdown(&out),
        "- [m1-n1] first\n- [m1-n3] second",
        "oldest first, as the docs say"
    );
}

/// hs-d558 FX5: a topic no curated record has ever used is refused, with the topics in use.
#[test]
fn show_topic_refuses_an_unknown_topic_with_the_topics_in_use() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "a");
    seed(&fx.env, "m1-n2", "note", 50, "b");
    curate(
        &fx.env,
        &[("m1-n1", "engine", "one"), ("m1-n2", "store", "two")],
    );

    let run = fx.cpd(&["context", "show", "--context", CTX, "--topic", "nothing"]);
    let err = run.error();
    assert_eq!(run.exit, 4, "{err}");
    assert_eq!(err["code"], "topic_not_found", "{err}");
    assert_eq!(
        err["details"],
        serde_json::json!({"context": CTX, "topic": "nothing", "topics": ["engine", "store"]}),
        "{err}"
    );
    // a topic in use is still answered
    assert_eq!(markdown(&topic(&fx, "engine")), "- [m1-n1] one");
}

/// An approved retirement removes a record from its topic while the other records of the topic stay.
#[test]
fn a_retired_record_is_gone_from_its_topic() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "a");
    seed(&fx.env, "m1-n2", "note", 50, "b");
    curate(
        &fx.env,
        &[
            ("m1-n1", "engine", "retired short"),
            ("m1-n2", "engine", "kept short"),
        ],
    );
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &[],
        &[Proposal {
            record: "m1-n1".parse().unwrap(),
            reason: Reason::Duplicate,
            target: "m1-n2".into(),
            approval: Approval::Pending,
        }],
    )
    .unwrap();
    // pending: still in force, so still in its topic
    assert_eq!(
        markdown(&topic(&fx, "engine")),
        "- [m1-n1] retired short\n- [m1-n2] kept short"
    );

    fx.cpd(&["context", "retire", "--context", CTX, "--approve", "m1-n1"])
        .ok();
    assert_eq!(markdown(&topic(&fx, "engine")), "- [m1-n2] kept short");
}

fn topics(fx: &Fx) -> Value {
    fx.cpd(&["context", "topics", "--context", CTX]).ok()
}

#[test]
fn topics_lists_each_topic_in_force_with_its_count() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "a");
    seed(&fx.env, "m1-n2", "note", 50, "b");
    seed(&fx.env, "m1-n3", "note", 40, "c");
    seed(&fx.env, "m1-n4", "note", 30, "d");
    curate(
        &fx.env,
        &[
            ("m1-n1", "store", "one"),
            ("m1-n2", "engine", "two"),
            ("m1-n3", "engine", "three"),
        ],
    );

    let out = topics(&fx);
    assert_eq!(out["context"]["id"], CTX, "{out}");
    assert_eq!(
        out["topics"],
        serde_json::json!([{"topic": "engine", "count": 2}, {"topic": "store", "count": 1}]),
        "{out}"
    );
    assert_eq!(markdown(&out), "engine (2), store (1)", "{out}");
}

#[test]
fn topics_counts_only_records_in_force() {
    let fx = fast();
    for (i, id) in ["m1-n1", "m1-n2", "m1-n3", "m1-n4", "m1-n5"]
        .iter()
        .enumerate()
    {
        seed(&fx.env, id, "note", 60 - i as u32, "x");
    }
    curate(
        &fx.env,
        &[
            ("m1-n1", "engine", "kept"),
            ("m1-n2", "engine", "superseded"),
            ("m1-n3", "engine", "retired"),
            ("m1-n4", "store", "superseded alone"),
            ("m1-n5", "store", "retired alone"),
        ],
    );
    fx.cpd(&[
        "context",
        "note",
        "Newer.",
        "--supersedes",
        "m1-n2",
        "--context",
        CTX,
    ])
    .ok();
    fx.cpd(&[
        "context",
        "note",
        "Newer.",
        "--supersedes",
        "m1-n4",
        "--context",
        CTX,
    ])
    .ok();
    propose(
        &mut fx.env.store(),
        &ContextId::parse(CTX).unwrap(),
        &[],
        &[
            Proposal {
                record: "m1-n3".parse().unwrap(),
                reason: Reason::Duplicate,
                target: "m1-n1".into(),
                approval: Approval::Pending,
            },
            Proposal {
                record: "m1-n5".parse().unwrap(),
                reason: Reason::Duplicate,
                target: "m1-n1".into(),
                approval: Approval::Pending,
            },
        ],
    )
    .unwrap();
    fx.cpd(&["context", "retire", "--context", CTX, "--approve", "m1-n3"])
        .ok();
    fx.cpd(&["context", "retire", "--context", CTX, "--approve", "m1-n5"])
        .ok();

    let out = topics(&fx);
    assert_eq!(
        out["topics"],
        serde_json::json!([{"topic": "engine", "count": 1}]),
        "{out}"
    );
    assert_eq!(markdown(&out), "engine (1)", "{out}");
}

#[test]
fn topics_of_a_context_without_curations_is_empty() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "a");

    let out = topics(&fx);
    assert_eq!(out["topics"], serde_json::json!([]), "{out}");
    assert_eq!(out["markdown"], "", "{out}");
}

#[test]
fn topics_without_a_context_is_refused() {
    let fx = fast();
    common::refused(&fx.cpd(&["context", "topics"]), "context_required", 2);
}

#[test]
fn topics_of_an_unknown_context_is_refused() {
    let fx = fast();
    common::refused(
        &fx.cpd(&["context", "topics", "--context", "nothing-here"]),
        "context_not_found",
        4,
    );
}

/// A provisional topic (`record_topics`, decision pro-d11) counts a record once; a curation's topic wins.
#[test]
fn topics_counts_a_provisional_topic_once() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "a");
    seed(&fx.env, "m1-n2", "note", 50, "b");
    seed(&fx.env, "m1-n3", "note", 40, "c");
    curate(
        &fx.env,
        &[("m1-n1", "engine", "one"), ("m1-n3", "store", "three")],
    );
    fx.env.sql(&format!(
        "INSERT INTO record_topics (context, record, topic, model, at) VALUES
           ('{CTX}', 'm1-n2', 'engine', 'kev-4b', '2026-11-03T00:00:00.000Z'),
           ('{CTX}', 'm1-n3', 'engine', 'kev-4b', '2026-11-03T00:00:00.000Z');"
    ));
    let out = topics(&fx);
    assert_eq!(
        out["topics"],
        serde_json::json!([{"topic": "engine", "count": 2}, {"topic": "store", "count": 1}]),
        "{out}"
    );
    assert_eq!(markdown(&out), "engine (2), store (1)", "{out}");
}

/// `show --topic` stays curated-only: a provisional topic lists nothing and is not a topic of the Context.
#[test]
fn show_topic_lists_only_curated_records() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "a");
    seed(&fx.env, "m1-n2", "note", 50, "b");
    curate(&fx.env, &[("m1-n1", "engine", "one")]);
    fx.env.sql(&format!(
        "INSERT INTO record_topics (context, record, topic, model, at) VALUES
           ('{CTX}', 'm1-n2', 'engine', 'kev-4b', '2026-11-03T00:00:00.000Z');"
    ));
    assert_eq!(markdown(&topic(&fx, "engine")), "- [m1-n1] one");
}
