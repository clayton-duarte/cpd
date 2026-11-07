//! `cpd session send` and the `session-message` Event (commands.md §10.13,
//! contract §5.2), the command tree in this process (`Fx`); the help usage runs the built binary.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::session::{events, setup, setup_with_fork, show};
use common::{TestEnv, refused};
use serde_json::{Value, json};

fn messages(fx: &Fx) -> Vec<Value> {
    events(fx, "session-message")
}

/// `events follow` owns its stdout (`Fx` cannot capture it): the built binary, through the env of `fx`.
fn follow(fx: &Fx, session: &str) -> Vec<Value> {
    let run = fx.env.cpd(&[
        "events",
        "follow",
        "--session",
        session,
        "--until-idle",
        "--since",
        "0",
        "--kind",
        "session-message",
    ]);
    assert_eq!(run.exit, 0, "{run:?}");
    run.stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn a_send_appends_one_session_message_from_cpd_session() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let out = fx
        .cpd_with(
            fx.env.root(),
            &["session", "send", "lead1", "hi there", "--unblocks", "u"],
            &[("CPD_SESSION", "lead2")],
        )
        .ok();
    let events = messages(&fx);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(
        out,
        json!({"ok": true, "event": event["id"], "session": "lead1", "from": "lead2", "unblocks": "u", "started": false, "pane": null})
    );
    assert_eq!(event["kind"], "session-message");
    assert_eq!(
        (&event["session"], &event["context"], &event["plan"]),
        (&json!("lead1"), &json!("ctx"), &json!("pay"))
    );
    assert_eq!(
        (&event["job"], &event["attempt"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(
        event["data"],
        json!({"from": "lead2", "text": "hi there", "unblocks": "u"})
    );
    assert!(
        event["text"].as_str().unwrap().contains("(unblocks: u)"),
        "{event}"
    );
}

#[test]
fn from_is_pi_session_id_when_cpd_session_is_unset_and_null_for_the_user() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let pi = fx
        .cpd_with(
            fx.env.root(),
            &["session", "send", "lead1", "from pi", "--unblocks", "u"],
            &[("PI_SESSION_ID", "worker1")],
        )
        .ok();
    assert_eq!(pi["from"], "worker1");
    let both = fx
        .cpd_with(
            fx.env.root(),
            &["session", "send", "lead1", "both", "--unblocks", "u"],
            &[("CPD_SESSION", "lead2"), ("PI_SESSION_ID", "worker1")],
        )
        .ok();
    assert_eq!(both["from"], "lead2");
    let user = fx
        .cpd(&[
            "session",
            "send",
            "lead1",
            "from the user",
            "--unblocks",
            "u",
        ])
        .ok();
    assert_eq!(user["from"], Value::Null);
    let events = messages(&fx);
    assert_eq!(events.len(), 3);
    assert_eq!(
        events[2]["data"],
        json!({"from": null, "text": "from the user", "unblocks": "u"})
    );
    assert!(
        events[2]["text"]
            .as_str()
            .unwrap()
            .contains("(unblocks: u)"),
        "{}",
        events[2]
    );
}

#[test]
fn the_event_text_quotes_the_first_60_characters() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let message = format!("{}{}", "é".repeat(60), "tail");
    fx.cpd(&["session", "send", "lead1", &message, "--unblocks", "u"])
        .ok();
    let event = &messages(&fx)[0];
    assert!(
        event["text"].as_str().unwrap().contains(&"é".repeat(60)),
        "{event}"
    );
    assert!(!event["text"].as_str().unwrap().contains("tail"), "{event}");
    assert_eq!(event["data"]["text"], message);
}

#[test]
fn a_message_can_come_from_a_file_as_written() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let path = fx.env.root().join("message.md");
    fs::write(&path, "line one\nline two\n").unwrap();
    let out = fx
        .cpd(&[
            "session",
            "send",
            "lead1",
            "--file",
            path.to_str().unwrap(),
            "--unblocks",
            "u",
        ])
        .ok();
    assert_eq!(out["session"], "lead1");
    assert_eq!(messages(&fx)[0]["data"]["text"], "line one\nline two\n");
}

#[test]
fn each_call_is_one_more_message() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let a = fx
        .cpd(&["session", "send", "lead1", "same", "--unblocks", "u"])
        .ok();
    let b = fx
        .cpd(&["session", "send", "lead1", "same", "--unblocks", "u"])
        .ok();
    assert_ne!(a["event"], b["event"]);
    assert_eq!(messages(&fx).len(), 2);
}

#[test]
fn every_kind_of_lead_is_accepted_a_stopped_one_too() {
    let fx = Fx::with_ports();
    setup_with_fork(&fx.env);
    fx.env.write_tmux_config("");
    fx.cockpit();
    for lead in ["lead1", "lead2", "fork1"] {
        let out = fx
            .cpd(&["session", "send", lead, "hello", "--unblocks", "u"])
            .ok();
        assert_eq!(out["session"], lead);
    }
    let events = messages(&fx);
    assert_eq!(
        events
            .iter()
            .map(|e| e["session"].clone())
            .collect::<Vec<_>>(),
        ["lead1", "lead2", "fork1"]
    );
    assert_eq!(
        (&events[2]["plan"], &events[2]["context"]),
        (&json!("pay"), &json!("ctx"))
    );
}

#[test]
fn a_manager_is_a_target_and_an_archived_one_is_refused() {
    let fx = Fx::with_ports();
    setup_with_fork(&fx.env);
    fx.env.write_tmux_config("");
    fx.cockpit();
    fx.env.sql("INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
         VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx-manager', '/', 'O4', 'stopped', 't5', 't5', NULL)",
    );
    let out = fx
        .cpd_with(
            fx.env.root(),
            &["session", "send", "mgr1", "done", "--unblocks", "u"],
            &[("CPD_SESSION", "lead1")],
        )
        .ok();
    assert_eq!(out["session"], "mgr1");
    let events = messages(&fx);
    assert_eq!(events.len(), 1);
    assert_eq!(
        (
            &events[0]["session"],
            &events[0]["context"],
            &events[0]["plan"]
        ),
        (&json!("mgr1"), &json!("ctx"), &Value::Null)
    );
    assert_eq!(
        events[0]["data"],
        json!({"from": "lead1", "text": "done", "unblocks": "u"})
    );
    fx.env
        .sql("UPDATE sessions SET archived_at = 't' WHERE id = 'mgr1'");
    refused(
        &fx.cpd(&["session", "send", "mgr1", "again", "--unblocks", "u"]),
        "archived",
        3,
    );
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn the_refusals_write_nothing() {
    let fx = Fx::with_ports();
    setup_with_fork(&fx.env);
    fx.env.write_tmux_config("");
    fx.cockpit();
    let error = refused(
        &fx.cpd(&["session", "send", "nope", "hi", "--unblocks", "u"]),
        "session_not_found",
        4,
    );
    assert_eq!(error["details"]["session"], "nope");
    let error = refused(
        &fx.cpd(&["session", "send", "worker1", "hi", "--unblocks", "u"]),
        "not_a_lead",
        3,
    );
    assert_eq!(error["details"]["role"], "worker");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .ends_with("is a worker, not a Lead or a Manager."),
        "{error}"
    );
    fx.env
        .sql("UPDATE sessions SET archived_at = 't' WHERE id = 'fork1'");
    let error = refused(
        &fx.cpd(&["session", "send", "fork1", "hi", "--unblocks", "u"]),
        "archived",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd session unarchive fork1");
    assert!(messages(&fx).is_empty());
    // Unarchived, it is accepted again.
    fx.cpd(&["session", "unarchive", "fork1"]).ok();
    fx.cpd(&["session", "send", "fork1", "hi", "--unblocks", "u"])
        .ok();
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn exactly_one_of_text_and_file_and_the_text_limits() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let path = fx.env.root().join("m.md");
    fs::write(&path, "x").unwrap();
    let file = path.to_str().unwrap();
    refused(
        &fx.cpd(&["session", "send", "lead1", "--unblocks", "u"]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "session",
            "send",
            "lead1",
            "hi",
            "--file",
            file,
            "--unblocks",
            "u",
        ]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&["session", "send", "lead1", "", "--unblocks", "u"]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "session",
            "send",
            "lead1",
            &"a".repeat(20_001),
            "--unblocks",
            "u",
        ]),
        "usage",
        2,
    );
    fs::write(&path, "").unwrap();
    refused(
        &fx.cpd(&[
            "session",
            "send",
            "lead1",
            "--file",
            file,
            "--unblocks",
            "u",
        ]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "session",
            "send",
            "lead1",
            "--file",
            "/nonexistent/m.md",
            "--unblocks",
            "u",
        ]),
        "file_unreadable",
        2,
    );
    assert!(messages(&fx).is_empty());
    fx.cpd(&[
        "session",
        "send",
        "lead1",
        &"a".repeat(20_000),
        "--unblocks",
        "u",
    ])
    .ok();
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn the_targets_follower_sees_the_message_and_the_senders_does_not() {
    let fx = Fx::with_ports();
    setup_with_fork(&fx.env);
    fx.env.write_tmux_config("");
    fx.cockpit();
    fx.cpd_with(
        fx.env.root(),
        &["session", "send", "lead1", "ping", "--unblocks", "u"],
        &[("CPD_SESSION", "lead2")],
    )
    .ok();
    let seen = follow(&fx, "lead1");
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(
        (
            &seen[0]["kind"],
            &seen[0]["session"],
            &seen[0]["data"]["text"]
        ),
        (&json!("session-message"), &json!("lead1"), &json!("ping"))
    );
    assert!(follow(&fx, "lead2").is_empty());
    // A side fork of the same Plan receives only what is addressed to it.
    assert!(follow(&fx, "fork1").is_empty());
    fx.cpd_with(
        fx.env.root(),
        &["session", "send", "fork1", "pong", "--unblocks", "u"],
        &[("CPD_SESSION", "lead2")],
    )
    .ok();
    assert_eq!(follow(&fx, "fork1").len(), 1);
    assert_eq!(follow(&fx, "lead1").len(), 2); // the Plan's Lead sees the Plan's Events
}

const AGENT: &[(&str, &str)] = &[("CPD_SESSION", "lead2")];

#[test]
fn a_send_without_unblocks_is_a_usage_error() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.cpd(&["session", "send", "lead1", "ok", "--unblocks", "u"])
        .ok();
    refused(&fx.cpd(&["session", "send", "lead1", "hi"]), "usage", 2);
    refused(
        &fx.cpd(&["session", "send", "lead1", "hi", "--unblocks", ""]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "session",
            "send",
            "lead1",
            "hi",
            "--unblocks",
            &"a".repeat(201),
        ]),
        "usage",
        2,
    );
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn every_send_wakes_and_carries_its_unblocks() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    let out = fx
        .cpd(&[
            "session",
            "send",
            "lead1",
            "x",
            "--unblocks",
            "PR #210 opened",
        ])
        .ok();
    assert_eq!(out["unblocks"], "PR #210 opened");
    assert!(out.get("wake").is_none(), "{out}");
    let event = &messages(&fx)[0];
    assert_eq!(
        event["data"],
        json!({"from": null, "text": "x", "unblocks": "PR #210 opened"})
    );
    assert!(
        event["text"]
            .as_str()
            .unwrap()
            .contains("(unblocks: PR #210 opened)"),
        "{event}"
    );
}

#[test]
fn the_wake_flag_is_gone() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.cpd(&["session", "send", "lead1", "x", "--unblocks", "u"])
        .ok();
    refused(
        &fx.cpd(&["session", "send", "lead1", "y", "--unblocks", "u", "--wake"]),
        "usage",
        2,
    );
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn a_send_to_the_lead_of_a_done_plan_is_refused_plan_inactive() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.env.sql(
        "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
         UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
         UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'pay'",
    );
    let error = refused(
        &fx.cpd_with(
            fx.env.root(),
            &["session", "send", "lead1", "hi", "--unblocks", "u"],
            AGENT,
        ),
        "plan_inactive",
        3,
    );
    assert_eq!(error["details"]["plan"], "pay");
    assert_eq!(error["details"]["phase"], "done");
    assert!(messages(&fx).is_empty());
}

#[test]
fn the_same_text_to_a_second_session_is_refused_broadcast() {
    let fx = Fx::with_ports();
    setup_with_fork(&fx.env);
    fx.env.write_tmux_config("");
    fx.cockpit();
    fx.cpd_with(
        fx.env.root(),
        &["session", "send", "lead1", "news", "--unblocks", "u"],
        AGENT,
    )
    .ok();
    let error = refused(
        &fx.cpd_with(
            fx.env.root(),
            &["session", "send", "fork1", " news ", "--unblocks", "u"],
            AGENT,
        ),
        "broadcast",
        3,
    );
    assert_eq!(error["details"]["session"], "lead1");
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn the_same_text_to_the_same_session_within_an_hour_is_refused_duplicate() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.cpd_with(
        fx.env.root(),
        &["session", "send", "lead1", "news", "--unblocks", "u"],
        AGENT,
    )
    .ok();
    let error = refused(
        &fx.cpd_with(
            fx.env.root(),
            &["session", "send", "lead1", "news", "--unblocks", "u"],
            AGENT,
        ),
        "duplicate",
        3,
    );
    assert!(error["details"]["at"].is_string(), "{error}");
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn the_user_without_a_session_is_not_guarded() {
    let fx = Fx::with_ports();
    setup_with_fork(&fx.env);
    fx.env.write_tmux_config("");
    fx.cockpit();
    fx.env.sql(
        "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
         UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
         UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'pay'",
    );
    fx.cpd(&["session", "send", "lead1", "same", "--unblocks", "u"])
        .ok();
    fx.cpd(&["session", "send", "lead1", "same", "--unblocks", "u"])
        .ok();
    fx.cpd(&["session", "send", "fork1", "same", "--unblocks", "u"])
        .ok();
    assert_eq!(messages(&fx).len(), 3);
}

#[test]
fn the_help_usage_puts_the_id_first() {
    let env = TestEnv::new();
    let run = env.cpd(&["session", "send", "--help"]);
    let text = format!("{}{}", run.stdout, run.stderr);
    let usage = text
        .lines()
        .find(|l| l.contains("Usage:"))
        .unwrap_or_else(|| panic!("no usage line: {text}"));
    let id = usage
        .find("<id>")
        .unwrap_or_else(|| panic!("no <id> in: {usage}"));
    let unblocks = usage
        .find("--unblocks")
        .unwrap_or_else(|| panic!("no --unblocks in: {usage}"));
    assert!(
        id < unblocks,
        "usage must put the id first, as the parser does: {usage}"
    );
}

#[test]
fn the_send_help_names_a_lead_or_a_manager() {
    let env = TestEnv::new();
    let run = env.cpd(&["session", "send", "--help"]);
    let text = format!("{}{}", run.stdout, run.stderr);
    for want in [
        "Send a message to a Lead or Manager",
        "The Lead or Manager Session to send to",
    ] {
        assert!(text.contains(want), "help must contain {want:?}: {text}");
    }
}

// ---------- a send starts a stopped Lead or Manager that has no pane (commands.md §10.13, hs-d725) ----------

/// `(pane, window name, @cpd_session)` of every pane of the fake server.
fn panes(fx: &Fx) -> Vec<(String, String, String)> {
    let out = fx.tmux(&[
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{window_name}\t#{@cpd_session}",
    ]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[0].to_owned(), f[1].to_owned(), f[2].to_owned())
        })
        .collect()
}

/// The chat slot: the pane of `main` that holds a Session.
fn slot(fx: &Fx) -> (String, String) {
    let found: Vec<_> = panes(fx)
        .into_iter()
        .filter(|(_, window, session)| window == "main" && !session.is_empty())
        .collect();
    assert_eq!(found.len(), 1, "one Session in the chat slot: {found:?}");
    (found[0].0.clone(), found[0].2.clone())
}

/// The `new-window` calls the mocked tmux got, as their argv.
fn new_windows(fx: &Fx) -> Vec<Vec<String>> {
    fx.calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|a| a.iter().any(|x| x == "new-window"))
        .collect()
}

fn swaps(fx: &Fx) -> usize {
    fx.calls("tmux")
        .iter()
        .filter(|c| c.args.iter().any(|x| x == "swap-pane"))
        .count()
}

/// A cockpit whose chat slot holds `lead1` (idle, opened); `lead2` and `manager1` are stopped with no pane.
fn cockpit_with_lead1_in_the_chat_slot() -> Fx {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    fx.cpd(&["session", "open", "lead1"]).ok();
    fx
}

fn parked_moves(fx: &Fx) -> Vec<Value> {
    events(fx, "session-moved")
        .into_iter()
        .filter(|e| e["data"]["place"] == "parked")
        .collect()
}

/// Sends to `target` and asserts what every start has: one `new-window -d` in the cockpit named `window` running
/// `cpd session run <target>`, the row's pane the new one, one parked `session-moved`, the chat slot and its mark as
/// they were, no swap, and `started: true` with the pane in the JSON.
fn assert_a_send_starts_it_in(fx: &Fx, target: &str, window: &str) {
    let slot_before = slot(fx);
    let windows_before = new_windows(fx).len();
    let swaps_before = swaps(fx);
    let parked_before = parked_moves(fx).len();
    let out = fx
        .cpd(&["session", "send", target, "wake up", "--unblocks", "u"])
        .ok();

    let started = new_windows(fx).split_off(windows_before);
    assert_eq!(started.len(), 1, "one new-window: {started:?}");
    let argv = &started[0];
    assert!(
        argv.windows(2).any(|w| w == ["-t", "=cpd:"]) && argv.contains(&"-d".to_owned()),
        "-d in the cockpit: {argv:?}"
    );
    assert!(
        argv.windows(2).any(|w| w == ["-n", window]),
        "window {window}: {argv:?}"
    );
    assert_eq!(
        argv[argv.len() - 3..],
        ["session", "run", target],
        "{argv:?}"
    );

    let pane = out["pane"]
        .as_str()
        .unwrap_or_else(|| panic!("a pane in the JSON: {out}"))
        .to_owned();
    assert_eq!(out["started"], true, "{out}");
    assert_eq!(show(fx, target)["tmux_pane"], json!(pane));
    assert!(
        panes(fx)
            .iter()
            .any(|(p, w, s)| *p == pane && w == window && s == target),
        "{:?}",
        panes(fx)
    );

    let moved = parked_moves(fx);
    assert_eq!(
        moved.len(),
        parked_before + 1,
        "one parked session-moved: {moved:?}"
    );
    assert_eq!(
        (
            &moved.last().unwrap()["session"],
            &moved.last().unwrap()["data"]["pane"]
        ),
        (&json!(target), &json!(pane))
    );
    assert_eq!(slot(fx), slot_before, "the chat slot did not move");
    assert_eq!(swaps(fx), swaps_before, "no swap-pane");
    assert_eq!(messages(fx).last().unwrap()["session"], target);
}

#[test]
fn a_send_to_a_stopped_lead_with_no_pane_starts_it_in_a_parked_window_running_session_run() {
    let fx = cockpit_with_lead1_in_the_chat_slot();
    assert!(show(&fx, "lead2")["tmux_pane"].is_null());
    assert_a_send_starts_it_in(&fx, "lead2", "park-other");
}

#[test]
fn a_send_to_a_stopped_lead_records_its_pane_and_leaves_the_chat_slot_alone() {
    let fx = cockpit_with_lead1_in_the_chat_slot();
    let (pane, session) = slot(&fx);
    let swaps_before = swaps(&fx);
    let out = fx
        .cpd(&["session", "send", "lead2", "wake up", "--unblocks", "u"])
        .ok();
    let started = out["pane"]
        .as_str()
        .unwrap_or_else(|| panic!("a pane in the JSON: {out}"))
        .to_owned();
    assert_ne!(started, pane);
    assert_eq!(show(&fx, "lead2")["tmux_pane"], json!(started));
    assert_eq!(show(&fx, "lead2")["place"], "parked");
    assert_eq!(slot(&fx), (pane, session));
    assert_eq!(show(&fx, "lead1")["place"], "chat");
    assert_eq!(swaps(&fx), swaps_before);
}

#[test]
fn a_send_to_a_stopped_manager_with_no_pane_starts_it_in_its_context_window() {
    let fx = cockpit_with_lead1_in_the_chat_slot();
    assert!(show(&fx, "manager1")["tmux_pane"].is_null());
    assert_a_send_starts_it_in(&fx, "manager1", "ctx-ctx");
}

#[test]
fn a_send_to_a_lead_with_a_live_pane_starts_no_window() {
    let fx = cockpit_with_lead1_in_the_chat_slot();
    // lead2 gets a live parked pane (open, then park); its state is still `stopped`.
    fx.cpd(&["session", "open", "lead2"]).ok();
    fx.cpd(&["session", "park"]).ok();
    let pane = show(&fx, "lead2")["tmux_pane"].clone();
    assert!(pane.is_string(), "lead2 has a pane");
    let windows = new_windows(&fx).len();
    let moved = events(&fx, "session-moved").len();
    let out = fx
        .cpd(&["session", "send", "lead2", "hi", "--unblocks", "u"])
        .ok();
    assert_eq!(out["started"], false, "{out}");
    assert_eq!(out["pane"], Value::Null, "{out}");
    assert_eq!(new_windows(&fx).len(), windows, "no new window");
    assert_eq!(events(&fx, "session-moved").len(), moved);
    assert_eq!(show(&fx, "lead2")["tmux_pane"], pane);
    assert_eq!(messages(&fx).len(), 1);
}

#[test]
fn a_send_to_a_stopped_lead_with_no_cockpit_is_refused_no_cockpit_and_writes_no_event() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    let error = refused(
        &fx.cpd(&["session", "send", "lead2", "wake up", "--unblocks", "u"]),
        "no_cockpit",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd tui");
    assert!(messages(&fx).is_empty());
    assert!(show(&fx, "lead2")["tmux_pane"].is_null());
}

#[test]
fn session_send_origin_flag_sets_event_data() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.cpd(&[
        "session",
        "send",
        "lead1",
        "from the web",
        "--unblocks",
        "u",
        "--origin",
        "webui",
    ])
    .ok();
    fx.cpd(&[
        "session",
        "send",
        "lead1",
        "from the tui",
        "--unblocks",
        "u",
        "--origin",
        "tui",
    ])
    .ok();
    let events = messages(&fx);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["data"]["origin"], json!("webui"), "{}", events[0]);
    assert_eq!(events[1]["data"]["origin"], json!("tui"), "{}", events[1]);
}

#[test]
fn session_send_without_origin_omits_it() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.cpd(&["session", "send", "lead1", "hi", "--unblocks", "u"])
        .ok();
    let event = &messages(&fx)[0];
    assert!(
        event["data"].as_object().unwrap().get("origin").is_none(),
        "no --origin leaves the field absent: {event}"
    );
}

#[test]
fn session_send_refuses_invalid_origin() {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    refused(
        &fx.cpd(&[
            "session",
            "send",
            "lead1",
            "hi",
            "--unblocks",
            "u",
            "--origin",
            "bogus",
        ]),
        "usage",
        2,
    );
    assert!(messages(&fx).is_empty(), "a refused send writes no Event");
}
