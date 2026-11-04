//! Red tests for `cpd session tail` (contract §6.20, P1.11): the shared Rust socket tail. The tests
//! play the host end of a `FakeHostConn` (welcome, attached, view, ops, bye) and assert the plain
//! transcript lines the client prints and the frames it sends. No socket opens, no process starts and no
//! model runs: `Fakes` is the `ManualClock`, the `FakeHostConn` and nothing real. They are red until the
//! implementation Job writes the fold and the client: the stub answers `not implemented`.

use super::tail::{self, Fold, Frame};
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::ports::{Fakes, Ports};
use crate::store::Store;

/// A `$CPD_HOME` with the context `ctx` and one durable Lead `lead1`, and the parsed id.
fn world() -> (tempfile::TempDir, CpdHome, SessionId) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("home").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(
        home.config(),
        "version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"tail\"\n",
    )
    .unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            Ok(tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't', 'durable');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');",
            )?)
        })
        .unwrap();
    let session = SessionId::parse("lead1").unwrap();
    (dir, home, session)
}

fn line(s: &str) -> Option<String> {
    Some(s.to_owned())
}

/// One entry of the window: `role` is `user` or `assistant`, `text` its one text block.
fn entry(id: u64, role: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id, "conversationId": 1, "kind": format!("pi.{role}"),
        "model": [{"role": role, "content": [{"type": "text", "text": text}], "timestamp": id}]
    })
}

/// A `ConversationView` of `entries`, with the window starting after `before`.
fn view_of(entries: &[serde_json::Value], before: Option<u64>) -> serde_json::Value {
    let mut view = serde_json::json!({"conversation": {}, "entries": entries, "docs": {}});
    if let Some(id) = before {
        view["before"] = serde_json::json!(id);
    }
    view
}

fn view_frame(seq: u64, view: &serde_json::Value) -> String {
    serde_json::json!({"type": "view", "epoch": "e1", "seq": seq, "view": view}).to_string()
}

fn ops_frame(seq: u64, ops: serde_json::Value) -> String {
    serde_json::json!({"type": "ops", "epoch": "e1", "seq": seq, "ops": ops}).to_string()
}

/// The base `view` frame: one user entry that already prints.
fn base_view() -> String {
    view_frame(1, &view_of(&[entry(1, "user", "run the checks")], None))
}

/// An `ops` frame appending one assistant entry (id `seq`) after `len` entries.
fn assistant(seq: u64, len: u64, text: &str) -> String {
    ops_frame(
        seq,
        serde_json::json!([["p", ["entries"], len, 0, [entry(seq, "assistant", text)]]]),
    )
}

fn welcome() -> String {
    r#"{"type":"welcome","protocol":2,"host_version":"2.7.0","pi_durable":"1.5.0","session":"lead1","role":"lead","epoch":"e1","client_id":"c1","cwd":"/","limits":{"client_line":1048576}}"#.to_owned()
}

fn attached(req: u64, mode: &str, from: u64, head: u64) -> String {
    format!(
        r#"{{"type":"attached","req":"r{req}","mode":"{mode}","epoch":"e1","from_seq":{from},"head_seq":{head},"client_id":"c1"}}"#
    )
}

fn bye(reconnect: bool) -> String {
    r#"{"type":"bye","reason":"shutdown","reconnect":RECONNECT}"#
        .replace("RECONNECT", if reconnect { "true" } else { "false" })
}

/// A fold frame decoded from its wire line, as the client decodes it.
fn frame(line: &str) -> Frame {
    serde_json::from_str(line).unwrap()
}

/// A fold that has applied the base view of entries 1 (user).
fn started() -> Fold {
    let mut fold = Fold::new();
    fold.apply(&frame(&base_view())).unwrap();
    fold
}

#[test]
fn tail_prints_the_entries_of_the_first_view() {
    let view = view_of(
        &[entry(1, "user", "one"), entry(2, "assistant", "two")],
        None,
    );
    let mut fold = Fold::new();
    let lines = fold.apply(&frame(&view_frame(1, &view))).unwrap();
    assert_eq!(lines, vec!["user: one", "assistant: two"]);
}

#[test]
fn tail_prints_an_appended_entry_of_an_ops_frame() {
    let mut fold = started();
    let lines = fold.apply(&frame(&assistant(2, 1, "Done."))).unwrap();
    assert_eq!(lines, vec!["assistant: Done."]);
}

#[test]
fn tail_prints_nothing_for_an_ops_frame_that_changes_only_docs() {
    let mut fold = started();
    let ops = ops_frame(
        2,
        serde_json::json!([
            ["s", ["docs", "pi.live"], {"generation": {"message": {"role": "assistant", "content": []}}}],
            ["s", ["docs", "pi.live", "generation", "message", "content"], [{"type": "text", "text": "par"}]]
        ]),
    );
    assert_eq!(
        fold.apply(&frame(&ops)).unwrap(),
        Vec::<String>::new(),
        "in-flight text is not printed"
    );
}

#[test]
fn tail_prints_only_new_entries_after_a_root_replacement() {
    let mut fold = started();
    let larger = view_of(
        &[
            entry(1, "user", "run the checks"),
            entry(2, "assistant", "two"),
            entry(3, "user", "three"),
        ],
        None,
    );
    let ops = ops_frame(2, serde_json::json!([["r", larger]]));
    let lines = fold.apply(&frame(&ops)).unwrap();
    assert_eq!(lines, vec!["assistant: two", "user: three"]);
}

#[test]
fn tail_prints_no_entry_twice_over_a_window_overflow() {
    let mut fold = Fold::new();
    let view = view_of(
        &[entry(1, "user", "one"), entry(2, "assistant", "two")],
        None,
    );
    fold.apply(&frame(&view_frame(1, &view))).unwrap();
    let appended = fold.apply(&frame(&assistant(3, 2, "three"))).unwrap();
    assert_eq!(appended, vec!["assistant: three"]);
    let overflow = ops_frame(
        4,
        serde_json::json!([["p", ["entries"], 0, 2, []], ["s", ["before"], 2]]),
    );
    assert_eq!(
        fold.apply(&frame(&overflow)).unwrap(),
        Vec::<String>::new(),
        "the dropped entries are not printed again"
    );
    assert!(fold.gap_notices().is_empty(), "no entry was missed");
}

#[test]
fn tail_notes_a_gap_and_goes_on() {
    let mut fold = started();
    // The window moved past entries 2..=5: `before` is 5, the last printed id is 1.
    let moved = view_of(&[entry(6, "assistant", "six")], Some(5));
    let ops = ops_frame(2, serde_json::json!([["r", moved]]));
    let lines = fold.apply(&frame(&ops)).unwrap();
    assert_eq!(lines, vec!["assistant: six"], "it goes on");
    let notices = fold.gap_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].contains('1') && notices[0].contains('5'),
        "names the last printed id and `before`: {notices:?}"
    );
}

#[test]
fn tail_forgets_its_cursor_after_a_refused_op() {
    let (_dir, home, session) = world();
    let fakes = Fakes::new();
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(&attached(1, "view", 1, 2)),
        line(&base_view()),
        line(&ops_frame(
            2,
            serde_json::json!([["d", ["nope", "deeper"]]]),
        )),
        None,
    ]);
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(&attached(2, "view", 1, 1)),
        line(&base_view()),
        line(&bye(false)),
    ]);

    let mut out = Vec::new();
    Ports::scope(&fakes.ports, || tail::tail(&home, &session, &mut out)).unwrap();

    let sent = fakes.host_conn.sent(1);
    let attach: serde_json::Value =
        serde_json::from_str(sent.get(1).expect("hello then attach")).unwrap();
    assert!(
        attach.get("cursor").is_none() || attach["cursor"].is_null(),
        "the attach after a refusal has no cursor: {sent:?}"
    );
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.matches("user: run the checks").count(), 1, "{text:?}");

    // The fold itself: an `Err`, and the next attach starts clean.
    let mut fold = started();
    let refused = ops_frame(2, serde_json::json!([["d", ["nope", "deeper"]]]));
    assert!(fold.apply(&frame(&refused)).is_err());
}

#[test]
fn tail_folds_the_transcript_and_streams() {
    let (_dir, home, session) = world();
    let fakes = Fakes::new();
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(r#"{"type":"attached","req":"r1","mode":"view","epoch":"e1","from_seq":1,"head_seq":2,"client_id":"c1"}"#),
        line(&base_view()),
        line(&assistant(2, 1, "Running them now.")),
        line(&bye(false)),
    ]);

    let mut out = Vec::new();
    Ports::scope(&fakes.ports, || tail::tail(&home, &session, &mut out)).unwrap();

    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("user: run the checks"), "{text:?}");
    assert!(text.contains("assistant: Running them now."), "{text:?}");

    // The client connected to the Session's socket, sent `hello` and then an `attach` with no cursor.
    let path = home.run_dir().join("host").join("lead1.sock");
    assert_eq!(fakes.host_conn.paths(), vec![path], "the socket of §6.14");
    let sent = fakes.host_conn.sent(0);
    let hello: serde_json::Value =
        serde_json::from_str(sent.first().expect("hello first")).unwrap();
    assert_eq!(
        (hello["type"].as_str(), hello["client"].as_str()),
        (Some("hello"), Some("cpd session tail")),
        "{sent:?}"
    );
    let attach: serde_json::Value =
        serde_json::from_str(sent.get(1).expect("attach second")).unwrap();
    assert_eq!(attach["type"].as_str(), Some("attach"), "{sent:?}");
    assert!(
        attach.get("cursor").is_none() || attach["cursor"].is_null(),
        "the first attach has no cursor: {sent:?}"
    );
}

#[test]
fn tail_reconnects_with_the_cursor_after_bye() {
    let (_dir, home, session) = world();
    let fakes = Fakes::new();
    // Connection 0: the host says goodbye with `reconnect:true` after two frames.
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(r#"{"type":"attached","req":"r1","mode":"view","epoch":"e1","from_seq":1,"head_seq":3,"client_id":"c1"}"#),
        line(&base_view()),
        line(&assistant(2, 1, "First answer.")),
        line(&bye(true)),
    ]);
    // Connection 1: the reconnect resumes with the saved cursor and completes a second message.
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(r#"{"type":"attached","req":"r2","mode":"resume","epoch":"e1","from_seq":3,"head_seq":3,"client_id":"c2"}"#),
        line(&assistant(3, 2, "Second answer.")),
        line(&bye(false)),
    ]);

    let mut out = Vec::new();
    Ports::scope(&fakes.ports, || tail::tail(&home, &session, &mut out)).unwrap();

    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("assistant: First answer."), "{text:?}");
    assert!(text.contains("assistant: Second answer."), "{text:?}");
    assert_eq!(
        text.matches("user: run the checks").count(),
        1,
        "the view is printed once, not repeated: {text:?}"
    );

    // The second attach carries the cursor of the last applied frame.
    assert_eq!(
        fakes.host_conn.sent(0).len(),
        2,
        "hello and attach before the bye"
    );
    let sent = fakes.host_conn.sent(1);
    let attach: serde_json::Value =
        serde_json::from_str(sent.get(1).expect("hello then attach")).unwrap();
    assert_eq!(attach["type"].as_str(), Some("attach"), "{sent:?}");
    assert_eq!(
        attach["cursor"],
        serde_json::json!({"epoch": "e1", "seq": 2}),
        "{sent:?}"
    );
}

#[test]
fn tail_ends_on_protocol_mismatch() {
    let (_dir, home, session) = world();
    let fakes = Fakes::new();
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(
            r#"{"type":"error","code":"protocol_mismatch","message":"the host speaks protocol 2"}"#,
        ),
        None,
    ]);

    let mut out = Vec::new();
    let err = Ports::scope(&fakes.ports, || tail::tail(&home, &session, &mut out)).unwrap_err();
    assert!(
        err.to_string().contains("this host speaks protocol 2"),
        "{err}"
    );
    let sent = fakes.host_conn.sent(0);
    let hello: serde_json::Value = serde_json::from_str(sent.first().unwrap()).unwrap();
    assert_eq!(
        (hello["protocol"].as_u64(), hello["min_protocol"].as_u64()),
        (Some(2), Some(2)),
        "{sent:?}"
    );
}
