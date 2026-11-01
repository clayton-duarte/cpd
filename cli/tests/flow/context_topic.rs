//! `context show --topic` (contract §4.6 "Consumers"; commands.md §3.3; hs-d549, hs-d551): the topic view stays
//! when the digest goes. The tests run the command tree in this process (`Fx`); curations are written through
//! `curation::propose`, as the distiller will.

use crate::common;

use crate::context_on_demand::{CTX, curate, fast};
use common::TestEnv;
use common::fx::Fx;
use cpd_core::context::curation::{Proposal, Reason, propose};
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
