//! Red tests for `cpd session tail` (contract §6.20, P1.11): the shared Rust socket tail. The tests
//! play the host end of a `FakeHostConn` (welcome, attached, snapshot, events, bye) and assert the plain
//! transcript lines the client prints and the frames it sends. No socket opens, no process starts and no
//! model runs: `Fakes` is the `ManualClock`, the `FakeHostConn` and nothing real. They are red until the
//! implementation Job writes the fold and the client: the stub answers `not implemented`.

use super::tail;
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

/// The base `snapshot` frame: one user entry that already prints.
fn snapshot() -> String {
    r#"{"type":"snapshot","epoch":"e1","seq":1,"snapshot":{"type":"snapshot","entries":[{"id":1,"conversationId":1,"kind":"pi.user","model":[{"role":"user","content":"run the checks","timestamp":1}]}],"tools":[],"nestedTools":[],"compactions":[],"inbox":[],"agent":{},"usage":{"models":{},"tools":{}}}}"#.to_owned()
}

/// An `events` frame whose `message_end` completes one assistant message.
fn assistant(seq: u64, text: &str) -> String {
    let message = format!(
        r#"{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}],"api":"faux","provider":"faux","model":"faux-1","usage":{{}},"stopReason":"stop","timestamp":{seq}}}"#
    );
    format!(
        r#"{{"type":"events","epoch":"e1","seq":{seq},"events":[{{"type":"message_start","message":{message}}},{{"type":"message_end","entry":{{"id":{seq},"conversationId":1,"kind":"pi.assistant","model":[{message}]}}}}]}}"#
    )
}

fn welcome() -> String {
    r#"{"type":"welcome","protocol":1,"host_version":"2.7.0","pi_durable":"1.3.0","session":"lead1","role":"lead","epoch":"e1","client_id":"c1","cwd":"/","limits":{"client_line":1048576}}"#.to_owned()
}

fn bye(reconnect: bool) -> String {
    r#"{"type":"bye","reason":"shutdown","reconnect":RECONNECT}"#
        .replace("RECONNECT", if reconnect { "true" } else { "false" })
}

#[test]
fn tail_folds_the_transcript_and_streams() {
    let (_dir, home, session) = world();
    let fakes = Fakes::new();
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(r#"{"type":"attached","req":"r1","mode":"snapshot","epoch":"e1","from_seq":1,"head_seq":2,"client_id":"c1"}"#),
        line(&snapshot()),
        line(&assistant(2, "Running them now.")),
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
        line(r#"{"type":"attached","req":"r1","mode":"snapshot","epoch":"e1","from_seq":1,"head_seq":3,"client_id":"c1"}"#),
        line(&snapshot()),
        line(&assistant(2, "First answer.")),
        line(&bye(true)),
    ]);
    // Connection 1: the reconnect resumes with the saved cursor and completes a second message.
    fakes.host_conn.serve(vec![
        line(&welcome()),
        line(r#"{"type":"attached","req":"r2","mode":"resume","epoch":"e1","from_seq":3,"head_seq":3,"client_id":"c2"}"#),
        line(&assistant(3, "Second answer.")),
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
        "the snapshot is printed once, not repeated: {text:?}"
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
