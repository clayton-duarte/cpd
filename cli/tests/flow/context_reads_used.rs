//! Citation marking (decision pro-d10 item 5): a Session that cites a record id in what it writes (`context decide`,
//! `context note`, `plan new --brief-file`, `job add --spec`, `session send`) sets `used = 1` on its own read rows of
//! that record. Marking is telemetry: the output never changes. Red until the marking exists.

use crate::common;

use std::fs;

use crate::job;
use common::fx::Fx;
use serde_json::{Value, json};

const S1: &str = "s1";
const S2: &str = "s2";

/// `(context, record, session, channel, used)`.
type Row = (String, String, String, String, Option<i64>);

/// Machine `pro`, Contexts `ctx` and `other`, Sessions `s1` (of `ctx`) and `s2` (of `other`).
fn base(fx: &Fx) {
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"pro\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
    for id in ["ctx", "other"] {
        fx.cpd(&["context", "new", id, "--title", id, "--owner", "acme"])
            .ok();
    }
    sessions(fx, ("ctx", S1), ("other", S2));
    records(fx);
}

/// A Manager Session row each.
fn sessions(fx: &Fx, a: (&str, &str), b: (&str, &str)) {
    fx.env.sql(&format!(
        "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at) VALUES
           ('{}', 'manager', '{}', 'One', '/', 'O3', 'idle', 't', 't'),
           ('{}', 'manager', '{}', 'Two', '/', 'O3', 'idle', 't', 't');",
        a.1, a.0, b.1, b.0
    ));
}

/// Records `pro-d1`, `pro-d2` of `ctx`, `pro-d5` of `other`; `s1` read `pro-d1` (show) and `pro-d5` of `other`, `s2`
/// read `pro-d2` (search); every `used` is NULL.
fn records(fx: &Fx) {
    fx.env.sql(&format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('ctx', 'pro-d1', 'decision', '2026-01-01T00:00:00.000Z', 'pro', 'one'),
           ('ctx', 'pro-d2', 'decision', '2026-01-01T00:00:01.000Z', 'pro', 'two'),
           ('other', 'pro-d5', 'decision', '2026-01-01T00:00:02.000Z', 'pro', 'five');
         INSERT INTO context_reads (context, record, session, channel, read_at) VALUES
           ('ctx', 'pro-d1', '{S1}', 'show', 't'),
           ('other', 'pro-d5', '{S1}', 'show', 't'),
           ('ctx', 'pro-d2', '{S2}', 'search', 't');"
    ));
}

fn rows(fx: &Fx) -> Vec<Row> {
    fx.env.read(|c| {
        let mut st = c.prepare(
            "SELECT context, record, session, channel, used FROM context_reads ORDER BY context, record, session, channel",
        )?;
        let rows = st.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect::<Result<Vec<Row>, _>>()
    })
}

/// `used` of the one row `(context, record, session)`.
fn used(fx: &Fx, context: &str, record: &str, session: &str) -> Option<i64> {
    let all: Vec<Row> = rows(fx)
        .into_iter()
        .filter(|r| r.0 == context && r.1 == record && r.2 == session)
        .collect();
    assert_eq!(
        all.len(),
        1,
        "no read row of {context} {record} {session}: {:?}",
        rows(fx)
    );
    all[0].4
}

fn as_session(session: &str) -> [(&str, &str); 1] {
    [("CPD_SESSION", session)]
}

fn write(fx: &Fx, verb: &str, session: Option<&str>, text: &str, extra: &[&str]) -> Value {
    let mut args = vec!["context", verb, text, "--context", "ctx"];
    args.extend(extra);
    match session {
        Some(s) => fx.cpd_with(fx.env.root(), &args, &as_session(s)).ok(),
        None => fx.cpd(&args).ok(),
    }
}

#[test]
fn decide_marks_the_read_and_cited_record_used_for_that_session_only() {
    let fx = Fx::new();
    base(&fx);
    // No Session: nothing is marked.
    write(&fx, "decide", None, "Follows pro-d1.", &[]);
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), None);
    // s1 cites pro-d2, which only s2 read: s1's rows stay alone.
    write(&fx, "decide", Some(S1), "Follows pro-d2.", &[]);
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), None);
    assert_eq!(used(&fx, "ctx", "pro-d2", S2), None);
    // s1 cites pro-d1. Not the text of the session-less write above: an identical record is a retry (`already`).
    let out = write(&fx, "decide", Some(S1), "Follows pro-d1 again.", &[]);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), Some(1));
    assert_eq!(used(&fx, "ctx", "pro-d2", S2), None, "s2's row is s2's");
}

#[test]
fn own_id_and_supersedes_targets_are_not_marked() {
    let fx = Fx::new();
    base(&fx);
    // s1 read the id the next decision takes (`pro-d3`): its read row is seeded before the record exists, with the
    // foreign keys off for the seed connection.
    let db = fx.env.cpd_home().join("cpd.db");
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    // The history triggers call `cpd_txn()`: on this fixture connection it is the id of the latest txn.
    let txn: i64 = conn
        .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
        .unwrap();
    conn.create_scalar_function(
        "cpd_txn",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |_| Ok(txn),
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO context_reads (context, record, session, channel, read_at)
           VALUES ('ctx', 'pro-d3', 's1', 'show', 't');",
    )
    .unwrap();
    drop(conn);
    let out = write(&fx, "decide", Some(S1), "This is pro-d3 itself.", &[]);
    assert_eq!(out["record"]["id"], "pro-d3", "{out}");
    assert_eq!(
        used(&fx, "ctx", "pro-d3", S1),
        None,
        "a record never cites itself"
    );
    // The Supersedes target named in the text is not marked.
    let out = write(
        &fx,
        "decide",
        Some(S1),
        "Replaces pro-d1.",
        &["--supersedes", "pro-d1"],
    );
    assert_eq!(out["record"]["supersedes"], json!(["pro-d1"]), "{out}");
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), None);
    // A later write that cites it marks it.
    write(&fx, "note", Some(S1), "Still on pro-d1.", &[]);
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), Some(1));
}

#[test]
fn a_record_of_another_context_marks_nothing() {
    let fx = Fx::new();
    base(&fx);
    let out = write(&fx, "decide", Some(S1), "Builds on pro-d5.", &[]);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(used(&fx, "other", "pro-d5", S1), None);
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), None);
    // The same write, citing both, marks only the record of the Context written to.
    write(&fx, "decide", Some(S1), "Builds on pro-d5 and pro-d1.", &[]);
    assert_eq!(used(&fx, "other", "pro-d5", S1), None);
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), Some(1));
}

#[test]
fn note_marks_like_decide() {
    let fx = Fx::new();
    base(&fx);
    write(&fx, "note", None, "About pro-d1.", &[]);
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), None);
    write(&fx, "note", Some(S1), "About pro-d2.", &[]);
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), None);
    let out = write(&fx, "note", Some(S1), "About pro-d1 again.", &[]);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), Some(1));
    assert_eq!(used(&fx, "ctx", "pro-d2", S2), None);
}

#[test]
fn session_send_marks_for_the_sender_and_the_user_marks_nothing() {
    let fx = Fx::new();
    common::session::setup_with_fork(&fx.env);
    // `lead2` (sender) read pro-d1; `worker1` read pro-d2.
    fx.env.sql(
        "INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('ctx', 'pro-d1', 'decision', '2026-01-01T00:00:00.000Z', 'pro', 'one'),
           ('ctx', 'pro-d2', 'decision', '2026-01-01T00:00:01.000Z', 'pro', 'two');
         INSERT INTO context_reads (context, record, session, channel, read_at) VALUES
           ('ctx', 'pro-d1', 'lead2', 'show', 't'),
           ('ctx', 'pro-d2', 'worker1', 'search', 't');",
    );
    let send = |vars: &[(&str, &str)]| {
        fx.cpd_with(
            fx.env.root(),
            &["session", "send", "lead1", "see pro-d1", "--unblocks", "u"],
            vars,
        )
        .ok()
    };
    let user = send(&[]);
    assert_eq!(user["from"], Value::Null, "{user}");
    assert_eq!(
        used(&fx, "ctx", "pro-d1", "lead2"),
        None,
        "the User has no Session"
    );
    let sent = send(&[("CPD_SESSION", "lead2")]);
    assert_eq!(sent["from"], "lead2", "{sent}");
    assert_eq!(sent["ok"], true);
    assert_eq!(used(&fx, "ctx", "pro-d1", "lead2"), Some(1));
    assert_eq!(used(&fx, "ctx", "pro-d2", "worker1"), None);
    // The Events are the send's own: one per send, the text unchanged.
    let events = fx
        .cpd(&["events", "list", "--kind", "session-message"])
        .ok();
    let texts: Vec<&Value> = events["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["data"]["text"])
        .collect();
    assert_eq!(texts, [&json!("see pro-d1"), &json!("see pro-d1")]);
}

#[test]
fn job_add_scans_goal_context_acceptance_and_out_of_scope() {
    let fx = Fx::new();
    job::setup(&fx.env);
    job::plan(&fx.env, "pl", "plan");
    // The caller is the Plan's Lead (`lead-pl`): a Manager may add a Job only to a Lead-less shaping Plan of its own
    // Context (commands.md §5.1; air-d19, air-d22), and `job::plan` makes a Plan that has a Lead.
    fx.env.sql(
        "INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('ctx', 'pro-d1', 'decision', '2026-01-01T00:00:00.000Z', 'pro', 'one'),
           ('ctx', 'pro-d2', 'decision', '2026-01-01T00:00:01.000Z', 'pro', 'two'),
           ('ctx', 'pro-d3', 'decision', '2026-01-01T00:00:02.000Z', 'pro', 'three'),
           ('ctx', 'pro-d4', 'decision', '2026-01-01T00:00:03.000Z', 'pro', 'four'),
           ('ctx', 'pro-d6', 'decision', '2026-01-01T00:00:04.000Z', 'pro', 'six');
         INSERT INTO context_reads (context, record, session, channel, read_at) VALUES
           ('ctx', 'pro-d1', 'lead-pl', 'show', 't'),
           ('ctx', 'pro-d2', 'lead-pl', 'show', 't'),
           ('ctx', 'pro-d3', 'lead-pl', 'show', 't'),
           ('ctx', 'pro-d4', 'lead-pl', 'show', 't'),
           ('ctx', 'pro-d6', 'lead-pl', 'show', 't');",
    );
    // An explore Job has no `touches`, so this is an ordinary Job. pro-d6 is cited in `touches` only; pro-d7 is cited and was never read.
    let spec = json!({
        "goal": "Do pro-d1.",
        "context": "Because pro-d2 and pro-d7.",
        "touches": ["src/pro-d6.rs"],
        "acceptance": ["pro-d3 holds"],
        "out_of_scope": ["Not pro-d4."]
    });
    let file = job::write_spec(&fx.env, "scan.json", &spec.to_string());
    let run = fx.cpd_with(
        fx.env.root(),
        &[
            "job", "add", "k", "--title", "Look", "--tier", "S2", "--spec", &file, "--plan", "pl",
        ],
        &as_session("lead-pl"),
    );
    assert_eq!(run.ok()["ok"], true);
    for id in ["pro-d1", "pro-d2", "pro-d3", "pro-d4"] {
        assert_eq!(used(&fx, "ctx", id, "lead-pl"), Some(1), "{id}");
    }
    assert_eq!(
        used(&fx, "ctx", "pro-d6", "lead-pl"),
        None,
        "touches is not scanned"
    );
}

#[test]
fn plan_new_brief_marks_the_reading_session() {
    let fx = Fx::with_ports();
    base(&fx);
    fx.keyed_repo("pay");
    let brief = fx.env.root().join("brief.md");
    fs::write(&brief, "Retry payouts, as pro-d1 says.\n").unwrap();
    let out = fx
        .cpd_with(
            fx.env.root(),
            &[
                "plan",
                "new",
                "pay",
                "--context",
                "ctx",
                "--title",
                "Pay",
                "--brief-file",
                brief.to_str().unwrap(),
            ],
            &as_session(S1),
        )
        .ok();
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(used(&fx, "ctx", "pro-d1", S1), Some(1));
    assert_eq!(used(&fx, "ctx", "pro-d2", S2), None);
}
