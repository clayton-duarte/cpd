//! `cpd tui action host` (commands.md §9.12, hs-d618): a click on the red version pill reads the
//! install state file and opens the machine's host Manager, waking it with the incident. The built
//! `fake-tmux` is the only tmux, wrapped so its calls are recorded; no real tmux, git, pi or gh runs.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{Run, SLEEP_SECONDS, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::store::Store;
use serde_json::{Value, json};

/// The machine's host Context and its Manager (contract §3.7 E6, commands.md §9.12).
const CONTEXT: &str = "host";
const MANAGER: &str = "host-manager";
/// The checkout the incident message names, literally.
const CHECKOUT: &str = "~/projects/cpd";
/// The tmux client the StatusBar's click names.
const CLIENT: &str = "c1";
/// The report of a click with no host Manager.
const NO_MANAGER: &str = "φ: no host Manager: apply dotfiles";

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

/// The env: config on its private fake tmux, a recording `tmux` that runs the built `fake-tmux`, a
/// `cpd` shim, a cockpit with the chat placeholder, and — when `manager` — Context `host` with its
/// stopped Manager, registered as `cpd context manager --by-user` would (no pane, no first turn).
fn fixture(manager: bool) -> TestEnv {
    let env = TestEnv::new();
    env.write_tmux_config("");
    recording_fake_tmux(&env);
    env.fake_cpd_views();
    if manager {
        register_manager(&env);
    }
    cockpit(&env);
    env
}

/// A `tmux` on `PATH` that records its argv (env.calls) and then runs the built fake tmux, so no real
/// tmux server ever starts.
fn recording_fake_tmux(env: &TestEnv) {
    env.fake(
        "tmux",
        &format!("exec '{}' \"$@\"", env!("CARGO_BIN_EXE_fake-tmux")),
    );
}

/// Context `host` and its stopped, by-user Manager: the row the install script creates.
fn register_manager(env: &TestEnv) {
    let cwd = env.root().display().to_string();
    let mut store = Store::open(&home(env), 5000).unwrap();
    common::sql(
        &mut store,
        &format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('{CONTEXT}', 'Host', '2026-01-01T00:00:00Z', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, by_user)
             VALUES ('{MANAGER}', 'manager', '{CONTEXT}', NULL, NULL, '{MANAGER}', '{cwd}', 'O4', 'stopped',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 1);"
        ),
    );
}

/// The cockpit: session `cpd`, window `main`, one tree pane and the chat placeholder (§13.1).
fn cockpit(env: &TestEnv) {
    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(
            out.status.success(),
            "tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let tree = ok(&[
        "new-session",
        "-d",
        "-s",
        "cpd",
        "-n",
        MAIN_WINDOW,
        "-x",
        "215",
        "-y",
        "62",
        "-P",
        "-F",
        "#{pane_id}",
        &format!("sleep {SLEEP_SECONDS}"),
    ]);
    ok(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
    let chat = ok(&[
        "split-window",
        "-d",
        "-t",
        &tree,
        "-P",
        "-F",
        "#{pane_id}",
        &format!("sleep {SLEEP_SECONDS}"),
    ]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
}

/// Writes `$CPD_HOME/run/cpd-install.json`, one JSON object on one line (contract §1.4).
fn write_state(env: &TestEnv, status: &str, reason: Option<&str>, since: &str) {
    let mut state = json!({"status": status, "since": since});
    if let Some(reason) = reason {
        state["reason"] = json!(reason);
    }
    let path = state_path(env);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, state.to_string()).unwrap();
}

fn state_path(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("run/cpd-install.json")
}

fn marker_path(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("run/cpd-install-notified")
}

/// `cpd tui action host --client <client>`.
fn action(env: &TestEnv) -> Run {
    env.cpd(&["tui", "action", "host", "--client", CLIENT])
}

/// The `session-message` Events, in order.
fn messages(env: &TestEnv) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", "session-message"])
        .ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// The `@cpd_session` of the chat slot of `cpd:main` (empty while the placeholder holds it).
fn chat_slot_session(env: &TestEnv) -> String {
    let out = env.tmux(&[
        "list-panes",
        "-t",
        "=cpd:main",
        "-F",
        "#{@cpd_session}\t#{@cpd_view}",
    ]);
    let text = String::from_utf8(out.stdout).unwrap();
    for line in text.lines() {
        let (session, view) = line.split_once('\t').expect("session and view");
        if !session.is_empty() || view == "chat" {
            return session.to_owned();
        }
    }
    panic!("no chat slot in {text:?}");
}

/// The recorded `display-message` calls.
fn display_messages(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux")
        .into_iter()
        .filter(|c| c.iter().any(|a| a == "display-message"))
        .collect()
}

#[test]
fn a_click_opens_the_host_manager_and_sends_the_incident_once() {
    let env = fixture(true);
    write_state(&env, "skipped", Some("dirty"), "2026-10-24T10:00:00Z");

    let run = action(&env);
    assert_eq!(
        (run.exit, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", ""),
        "{run:?}"
    );

    let events = messages(&env);
    assert_eq!(events.len(), 1, "{events:?}");
    let event = &events[0];
    assert_eq!(
        (&event["kind"], &event["session"]),
        (&json!("session-message"), &json!(MANAGER))
    );
    assert_eq!(
        event["data"]["from"],
        Value::Null,
        "the User's side, not an agent: {event}"
    );
    assert_eq!(event["data"]["unblocks"], "cpd install skipped", "{event}");
    let text = event["data"]["text"].as_str().expect("a message text");
    assert!(
        text.contains("dirty") && text.contains(CHECKOUT) && text.contains("2026-10-24T10:00:00Z"),
        "{text}"
    );

    assert_eq!(
        chat_slot_session(&env),
        MANAGER,
        "the Manager is in the chat slot"
    );
    assert_eq!(
        fs::read_to_string(marker_path(&env)).unwrap(),
        "2026-10-24T10:00:00Z\n"
    );
}

#[test]
fn a_click_when_the_state_is_ok_does_nothing() {
    let env = fixture(true);
    write_state(&env, "ok", None, "2026-10-24T10:00:00Z");

    let run = action(&env);
    assert_eq!(
        (run.exit, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", ""),
        "{run:?}"
    );

    assert!(messages(&env).is_empty(), "{:?}", messages(&env));
    assert!(!marker_path(&env).exists(), "no incident, no marker");
    assert_eq!(chat_slot_session(&env), "", "the Manager is not opened");
    assert!(
        display_messages(&env).is_empty(),
        "{:?}",
        display_messages(&env)
    );
}

#[test]
fn a_click_without_a_host_manager_reports_on_the_client_and_exits_0() {
    let env = fixture(false);
    write_state(&env, "skipped", Some("dirty"), "2026-10-24T10:00:00Z");

    let run = action(&env);
    assert_eq!(
        (run.exit, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", ""),
        "{run:?}"
    );

    assert!(messages(&env).is_empty(), "no Manager to wake");
    let displays = display_messages(&env);
    let call = displays
        .iter()
        .find(|c| c.iter().any(|a| a == NO_MANAGER))
        .unwrap_or_else(|| panic!("no {NO_MANAGER:?}: {displays:?}"));
    assert!(call.windows(2).any(|w| w == ["-c", CLIENT]), "{call:?}");
}

#[test]
fn a_new_incident_is_announced_again() {
    let env = fixture(true);
    write_state(&env, "skipped", Some("dirty"), "2026-10-24T10:00:00Z");
    action(&env);
    assert_eq!(messages(&env).len(), 1, "{:?}", messages(&env));
    assert_eq!(
        fs::read_to_string(marker_path(&env)).unwrap(),
        "2026-10-24T10:00:00Z\n"
    );

    // The same incident again: the click only opens the Manager.
    action(&env);
    assert_eq!(
        messages(&env).len(),
        1,
        "the same incident is announced once"
    );
    assert_eq!(chat_slot_session(&env), MANAGER);

    // A new incident is a new `since`: it is announced again.
    let next = "2026-10-24T11:30:00Z";
    write_state(&env, "skipped", Some("branch"), next);
    action(&env);
    let events = messages(&env);
    assert_eq!(events.len(), 2, "{events:?}");
    let text = events[1]["data"]["text"].as_str().expect("a message text");
    assert!(text.contains("branch") && text.contains(next), "{text}");
    assert_eq!(
        fs::read_to_string(marker_path(&env)).unwrap(),
        format!("{next}\n")
    );
}
