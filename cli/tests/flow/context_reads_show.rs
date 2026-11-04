//! Read logging of `context show` (record, topic, pushed) and `context search` (decision pro-d10 item 5): a Session
//! that reads leaves `context_reads` rows; the output never changes.

use crate::common;

use crate::context_on_demand::{CTX, curate, fast};
use common::fx::Fx;
use serde_json::Value;

const SESSION: &str = "sess-1";

/// Context `ctx`, a Session row of it, and three notes `m1-n1..3`.
fn fixture() -> Fx {
    let fx = fast();
    fx.env.sql(&format!(
        "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('{SESSION}', 'manager', '{CTX}', 'Manager', '/', 'O3', 'idle', 't', 't');
         INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('{CTX}', 'm1-n1', 'note', '2026-01-01T00:00:00.000Z', 'm1', 'one'),
           ('{CTX}', 'm1-n2', 'note', '2026-01-01T00:00:01.000Z', 'm1', 'two'),
           ('{CTX}', 'm1-n3', 'note', '2026-01-01T00:00:02.000Z', 'm1', 'three');"
    ));
    fx
}

fn show(fx: &Fx, vars: &[(&str, &str)], args: &[&str]) -> common::Run {
    let mut argv = vec!["context", "show", "--context", CTX];
    argv.extend(args);
    fx.cpd_with(fx.env.root(), &argv, vars)
}

type Row = (String, String, Option<i64>, Option<f64>, Option<i64>);

/// The `(record, channel, rank, score, used)` rows, ordered.
fn rows(fx: &Fx) -> Vec<Row> {
    fx.env.read(|c| {
        let mut st = c.prepare(
            "SELECT record, channel, rank, score, used FROM context_reads ORDER BY record, channel",
        )?;
        let rows = st.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect::<Result<Vec<Row>, _>>()
    })
}

fn row(record: &str, channel: &str) -> Row {
    (record.into(), channel.into(), None, None, None)
}

fn as_session() -> [(&'static str, &'static str); 1] {
    [("CPD_SESSION", SESSION)]
}

#[test]
fn show_record_logs_one_show_read_for_a_session() {
    let fx = fixture();
    let plain = show(&fx, &[], &["--record", "m1-n2"]).ok();
    assert_eq!(rows(&fx), []);
    let logged = show(&fx, &as_session(), &["--record", "m1-n2"]).ok();
    assert_eq!(logged, plain, "the output is unchanged");
    assert_eq!(rows(&fx), [row("m1-n2", "show")]);
}

#[test]
fn show_record_twice_keeps_one_row() {
    let fx = fixture();
    show(&fx, &as_session(), &["--record", "m1-n2"]).ok();
    show(&fx, &as_session(), &["--record", "m1-n2"]).ok();
    assert_eq!(rows(&fx), [row("m1-n2", "show")]);
}

#[test]
fn show_topic_logs_a_read_per_record() {
    let fx = fixture();
    curate(
        &fx.env,
        &[
            ("m1-n1", "engine", "first"),
            ("m1-n3", "engine", "third"),
            ("m1-n2", "store", "other"),
        ],
    );
    let plain = show(&fx, &[], &["--topic", "engine"]).ok();
    assert_eq!(rows(&fx), []);
    let logged = show(&fx, &as_session(), &["--topic", "engine"]).ok();
    assert_eq!(logged, plain, "the output is unchanged");
    assert_eq!(rows(&fx), [row("m1-n1", "show"), row("m1-n3", "show")]);
}

#[test]
fn since_with_pushed_logs_pushed_reads_and_without_it_none() {
    let fx = fixture();
    let bare = show(&fx, &as_session(), &["--since", "0"]).ok();
    assert_eq!(bare["records"].as_array().unwrap().len(), 3, "{bare}");
    assert_eq!(rows(&fx), [], "no --pushed, no read");
    let pushed = show(&fx, &as_session(), &["--since", "1", "--pushed"]).ok();
    assert_eq!(pushed["records"].as_array().unwrap().len(), 2, "{pushed}");
    assert_eq!(rows(&fx), [row("m1-n2", "pushed"), row("m1-n3", "pushed")]);
}

#[test]
fn pushed_needs_since_and_excludes_record_and_topic() {
    let fx = fixture();
    for args in [
        &["--pushed"][..],
        &["--since", "0", "--pushed", "--record", "m1-n1"],
        &["--since", "0", "--pushed", "--topic", "engine"],
    ] {
        let run = show(&fx, &as_session(), args);
        common::refused(&run, "usage", 2);
    }
    assert_eq!(rows(&fx), []);
}

#[test]
fn no_usable_session_logs_nothing_and_changes_nothing() {
    let fx = fixture();
    let plain = show(&fx, &[], &["--record", "m1-n2"]).ok();
    for id in ["nobody", "bad id!", ""] {
        let run = show(&fx, &[("CPD_SESSION", id)], &["--record", "m1-n2"]);
        assert_eq!(run.exit, 0, "{run:?}");
        assert_eq!(run.ok(), plain);
    }
    assert_eq!(rows(&fx), []);
    // the other Channel, with a usable Session only through the unknown id: still nothing
    let run = show(
        &fx,
        &[("CPD_SESSION", "nobody")],
        &["--since", "0", "--pushed"],
    );
    assert_eq!(run.exit, 0, "{run:?}");
    assert_eq!(rows(&fx), []);
    // control: the same call with the real Session does log
    show(&fx, &as_session(), &["--record", "m1-n2"]).ok();
    assert_eq!(rows(&fx), [row("m1-n2", "show")]);
}

#[test]
fn pi_session_id_alone_is_honoured() {
    let fx = fixture();
    show(&fx, &[("PI_SESSION_ID", SESSION)], &["--record", "m1-n3"]).ok();
    assert_eq!(rows(&fx), [row("m1-n3", "show")]);
}

/// A search logs one `search` read per returned hit: rank its 1-based place in the page, score the hit's own.
#[test]
fn a_search_logs_ranked_search_reads() {
    let fx = fixture();
    let args = ["context", "search", "one OR two OR three", "--context", CTX];
    let plain: Value = fx.cpd(&args).ok();
    assert_eq!(rows(&fx), []);
    let logged = fx.cpd_with(fx.env.root(), &args, &as_session()).ok();
    assert_eq!(logged, plain, "the output is unchanged");
    let hits = logged["records"].as_array().expect("records");
    assert!(hits.len() >= 2, "{logged}");
    let mut expected: Vec<Row> = hits
        .iter()
        .enumerate()
        .map(|(place, hit)| {
            (
                hit["id"].as_str().expect("id").into(),
                "search".into(),
                Some(place as i64 + 1),
                Some(hit["score"].as_f64().expect("score")),
                None,
            )
        })
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(rows(&fx), expected);
}
