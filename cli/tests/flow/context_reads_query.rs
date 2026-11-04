//! `cpd context reads` (commands.md §3.18): the counts per record, the filters, the order.

use crate::common;

use common::fx::Fx;
use serde_json::{Value, json};

/// A machine `hs` Context `rd` with records d1 (decision), d2 (decision), n3 (note) and no reads.
fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.env.write_config("version = 1\nmachine = \"hs\"\n");
    fx.cpd(&[
        "context", "new", "rd", "--title", "Reads", "--owner", "acme",
    ])
    .ok();
    fx.env.sql(
        "INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('rd', 'hs-d1', 'decision', 't', 'hs', 'x'),
           ('rd', 'hs-d2', 'decision', 't', 'hs', 'x'),
           ('rd', 'hs-n3', 'note', 't', 'hs', 'x');",
    );
    fx
}

fn read(fx: &Fx, record: &str, session: &str, channel: &str, at: &str, used: Option<u8>) {
    let used = used.map_or("NULL".to_string(), |u| u.to_string());
    fx.env.sql(&format!(
        "INSERT INTO context_reads (context, record, session, channel, read_at, used)
           VALUES ('rd', '{record}', '{session}', '{channel}', '{at}', {used});"
    ));
}

fn reads(fx: &Fx, extra: &[&str]) -> Value {
    let mut args = vec!["context", "reads", "--context", "rd"];
    args.extend_from_slice(extra);
    fx.cpd(&args).ok()
}

fn ids(out: &Value) -> Vec<&str> {
    out["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect()
}

const T1: &str = "2026-01-01T00:00:00.000Z";
const T2: &str = "2026-02-01T00:00:00.000Z";

#[test]
fn counts_reads_sessions_and_used_per_record() {
    let fx = fx();
    read(&fx, "hs-d1", "s1", "show", T1, Some(1));
    read(&fx, "hs-d1", "s1", "search", T1, None);
    read(&fx, "hs-d1", "s2", "pushed", T1, None);
    read(&fx, "hs-n3", "s1", "show", T1, Some(0));
    let out = reads(&fx, &[]);
    assert_eq!(
        out["context"],
        json!({"id": "rd", "title": "Reads", "jira": null})
    );
    assert_eq!(out["since"], Value::Null);
    assert_eq!(out["channels"], json!(["pushed", "search", "show"]));
    assert_eq!(out["min_sessions"], 1);
    assert_eq!(
        out["records"],
        json!([
            {"id": "hs-d1", "kind": "decision", "reads": 3, "sessions": 2, "used": 1},
            {"id": "hs-n3", "kind": "note", "reads": 1, "sessions": 1, "used": 0},
        ])
    );
    assert_eq!(
        out["markdown"],
        "hs-d1 decision reads=3 sessions=2 used=1\nhs-n3 note reads=1 sessions=1 used=0"
    );
}

#[test]
fn shadow_rows_are_never_counted() {
    let fx = fx();
    read(&fx, "hs-d1", "s1", "shadow", T1, Some(1));
    read(&fx, "hs-d2", "s1", "shadow", T1, None);
    read(&fx, "hs-d2", "s1", "show", T1, None);
    let out = reads(&fx, &[]);
    assert_eq!(ids(&out), vec!["hs-d2"]);
    assert_eq!(out["records"][0]["reads"], 1);
}

#[test]
fn channel_since_and_min_sessions_filter() {
    let fx = fx();
    read(&fx, "hs-d1", "s1", "show", T1, None);
    read(&fx, "hs-d1", "s2", "search", T2, None);
    read(&fx, "hs-d2", "s1", "pushed", T2, None);
    read(&fx, "hs-n3", "s1", "show", T2, None);

    let out = reads(&fx, &["--channel", "show,search"]);
    assert_eq!(out["channels"], json!(["search", "show"]));
    assert_eq!(ids(&out), vec!["hs-d1", "hs-n3"]);

    let out = reads(&fx, &["--since", "2026-01-15T00:00:00Z"]);
    assert_eq!(out["since"], "2026-01-15T00:00:00.000Z");
    assert_eq!(ids(&out), vec!["hs-d1", "hs-d2", "hs-n3"]);
    assert_eq!(out["records"][0]["reads"], 1);
    // a zone offset is compared as an instant; the row at T2 is at the bound
    let out = reads(&fx, &["--since", "2026-01-31T19:00:00-05:00"]);
    assert_eq!(out["since"], "2026-02-01T00:00:00.000Z");
    assert_eq!(ids(&out), vec!["hs-d1", "hs-d2", "hs-n3"]);
    let out = reads(&fx, &["--since", "2026-02-01T00:00:00.001Z"]);
    assert_eq!(out["records"], json!([]));

    let out = reads(&fx, &["--min-sessions", "2"]);
    assert_eq!(out["min_sessions"], 2);
    assert_eq!(ids(&out), vec!["hs-d1"]);
}

#[test]
fn bad_flag_values_are_usage_errors() {
    let fx = fx();
    for extra in [
        &["--channel", "shadow"][..],
        &["--channel", "show,shadow"],
        &["--channel", "bogus"],
        &["--since", "yesterday"],
        &["--since", "2026-01-01"],
        &["--since", "2026-01-01T00:00:00"],
        &["--min-sessions", "0"],
        &["--min-sessions", "x"],
    ] {
        let mut args = vec!["context", "reads", "--context", "rd"];
        args.extend_from_slice(extra);
        let run = fx.cpd(&args);
        let err = run.error();
        assert_eq!(err["code"], "usage", "{extra:?}: {run:?}");
        assert_eq!(run.exit, 2, "{extra:?}");
    }
}

#[test]
fn orders_by_reads_then_sessions_then_id() {
    let fx = fx();
    fx.env.sql(
        "INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('rd', 'hs-d4', 'decision', 't', 'hs', 'x');",
    );
    // d1: 2 reads 1 session; d2: 2 reads 2 sessions; d4: 2 reads 2 sessions; n3: 3 reads 1 session
    read(&fx, "hs-d1", "s1", "show", T1, None);
    read(&fx, "hs-d1", "s1", "search", T1, None);
    read(&fx, "hs-d2", "s1", "show", T1, None);
    read(&fx, "hs-d2", "s2", "show", T1, None);
    read(&fx, "hs-d4", "s1", "show", T1, None);
    read(&fx, "hs-d4", "s2", "show", T1, None);
    read(&fx, "hs-n3", "s1", "show", T1, None);
    read(&fx, "hs-n3", "s1", "search", T1, None);
    read(&fx, "hs-n3", "s1", "pushed", T1, None);
    let out = reads(&fx, &[]);
    assert_eq!(ids(&out), vec!["hs-n3", "hs-d2", "hs-d4", "hs-d1"]);
}

#[test]
fn an_empty_context_has_no_records() {
    let fx = fx();
    read(&fx, "hs-d1", "s1", "show", T1, None);
    let before = fx.env.query::<i64>("SELECT count(*) FROM context_reads");
    fx.cpd(&[
        "context", "new", "empty", "--title", "Empty", "--owner", "acme",
    ])
    .ok();
    let out = fx.cpd(&["context", "reads", "--context", "empty"]).ok();
    assert_eq!(out["records"], json!([]));
    assert_eq!(out["markdown"], "");
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM context_reads"),
        before,
        "the command writes nothing"
    );
}
