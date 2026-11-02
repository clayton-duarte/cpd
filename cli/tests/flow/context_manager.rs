//! `cpd context manager` (commands.md §3.10, contract §3.1 "The Context's
//! Manager"; B3-A1, B3-A2) and `session open` of a Manager (§10.4), run in this process (`Fx::with_ports`) on the
//! fake tmux: the Manager's window is the one `new-window` call that `context manager` makes, so the tests assert that
//! command (`cpd session run <id>`, its directory and its `-e` variables) and what `context manager` writes itself
//! (the Session row, the Events, the first-message file). What `session run` does with the Session row (pi's argv:
//! `--model`, `--session-id`, `-n`, `-- @<path>`) is the session area's (`launch_pi`,
//! `host/pi/launch.rs`), not asserted here.

use crate::common;

use std::fs;

use common::fx::{Fx, PaneRow};
use common::refused;
use serde_json::{Value, json};

fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// The Context `ctx` with a note, the config, and no cockpit yet.
fn setup(fx: &Fx) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cpd(&["context", "note", "Mind the ledger.", "--context", "ctx"])
        .ok();
}

/// The chat slot: the pane of `main` with a Session or the placeholder.
fn slot(fx: &Fx) -> PaneRow {
    let found: Vec<_> = fx
        .panes()
        .into_iter()
        .filter(|p| p.window_name == "main" && (!p.session.is_empty() || p.view == "chat"))
        .collect();
    assert_eq!(found.len(), 1, "one chat slot: {found:?}");
    found[0].clone()
}

fn window_names(fx: &Fx) -> Vec<String> {
    let state = fx.tmux_state().expect("a server");
    let mut names: Vec<String> = state.windows().into_iter().map(|w| w.name).collect();
    names.sort();
    names
}

/// The `new-window` calls on the Manager's windows `ctx-<context>`: what `context manager` asked tmux to start.
fn new_windows(fx: &Fx) -> Vec<Vec<String>> {
    fx.calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|a| a.iter().any(|x| x == "new-window") && a.iter().any(|x| x == "ctx-ctx"))
        .collect()
}

/// What the fake server started in `pane`: `(argv, cwd, the -e variables)`.
fn started(fx: &Fx, pane: &str) -> (Vec<String>, String, Vec<(String, String)>) {
    let state = fx.tmux_state().expect("a server");
    let p = state
        .panes()
        .into_iter()
        .find(|p| p.id == pane)
        .unwrap_or_else(|| panic!("no pane {pane}"));
    (p.argv, p.cwd, state.pane_env(pane))
}

fn manager_ids(fx: &Fx) -> Vec<String> {
    let out = fx.cpd(&["session", "list", "--role", "manager"]).ok();
    out["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_first_call_registers_the_manager_and_starts_pi_in_its_window() {
    let fx = Fx::with_ports();
    setup(&fx);
    fx.cockpit();
    let placeholder = slot(&fx);

    let out = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(out["started"], true);
    let manager = &out["manager"];
    let id = manager["session"].as_str().unwrap().to_owned();
    assert_eq!(
        (&manager["name"], &manager["state"]),
        (&json!("ctx-manager"), &json!("stopped"))
    );

    // The Session row.
    let row = fx.cpd(&["session", "show", &id]).ok()["session"].clone();
    assert_eq!(
        (
            &row["role"],
            &row["plan"],
            &row["context"],
            &row["tier"],
            &row["forked_from"],
            &row["state"]
        ),
        (
            &json!("manager"),
            &Value::Null,
            &json!("ctx"),
            &json!("O4"),
            &Value::Null,
            &json!("stopped")
        )
    );
    assert_eq!(
        row["cwd"],
        fx.env.home().canonicalize().unwrap().display().to_string()
    );
    assert_eq!(row["tmux_pane"], manager["pane"]);
    assert_eq!(row["place"], "parked");

    // Its window, parked, with @cpd_session.
    let pane = manager["pane"].as_str().unwrap();
    let mine = fx.panes().into_iter().find(|p| p.pane == pane).unwrap();
    assert_eq!(
        (mine.window_name.as_str(), mine.session.as_str()),
        ("ctx-ctx", id.as_str())
    );
    assert_eq!(window_names(&fx), ["ctx-ctx", "main"]);

    // The Events.
    let registered = events(&fx, "session-registered");
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0]["session"], json!(id));
    assert_eq!(
        registered[0]["data"],
        json!({"role": "manager", "name": "ctx-manager", "forked_from": null, "tier": "O4"})
    );
    let moved = events(&fx, "session-moved");
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0]["data"]["place"], "parked");
    assert_eq!(moved[0]["data"]["pane"], json!(pane));

    // The window's command: `cpd session run <id>` (which starts pi) in the Session's directory, with the Manager's
    // environment: its Session and this env's home, and no Plan.
    assert_eq!(new_windows(&fx).len(), 1, "one new-window");
    let (argv, cwd, vars) = started(&fx, pane);
    assert_eq!(argv, ["cpd", "session", "run", id.as_str()]);
    assert_eq!(
        cwd,
        fx.env.home().canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        vars,
        [
            (
                "CPD_HOME".to_owned(),
                fx.env.cpd_home().display().to_string()
            ),
            ("CPD_SESSION".to_owned(), id.clone())
        ],
        "the Manager's environment: a Session, a home and no CPD_PLAN"
    );
    // The first message goes as its file, which `session run` hands to pi as `@<path>` (hs-n111), as a Lead's does.
    let path = fx.env.cpd_home().join("run/ctx/manager-message.md");
    let first = fs::read_to_string(&path).unwrap();
    assert!(
        first.starts_with(
            "You are the Manager of Context ctx \"Payments\". Follow the cpd-manager skill.\n\n"
        ),
        "{first}"
    );
    assert!(
        first.ends_with("Context on demand: cpd context search <query> --context ctx · cpd context show --context ctx --since <seq> · cpd context show --context ctx --record <id>\n"),
        "the pointer line, not the records (#418): {first}"
    );
    assert!(
        !first.contains("Mind the ledger."),
        "no record is listed in the first message: {first}"
    );

    // The chat slot did not move (hs-d103).
    assert_eq!(slot(&fx), placeholder);
}

#[test]
fn the_manager_tier_is_the_config_key() {
    let fx = Fx::with_ports();
    setup(&fx);
    fx.env.write_tmux_config("manager_tier = \"O3\"");
    fx.cockpit();
    let out = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = out["manager"]["session"].as_str().unwrap();
    assert_eq!(
        fx.cpd(&["session", "show", id]).ok()["session"]["tier"],
        "O3"
    );
}

#[test]
fn a_second_call_changes_nothing() {
    let fx = Fx::with_ports();
    setup(&fx);
    fx.cockpit();
    let first = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = first["manager"]["session"].as_str().unwrap().to_owned();
    let slot_before = slot(&fx);
    let events_before: i64 = fx.env.query("SELECT count(*) FROM events");
    let panes_before = fx.panes();

    let second = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(second["started"], false);
    assert_eq!(second["manager"], first["manager"]);
    assert_eq!(manager_ids(&fx), [id]);
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events"),
        events_before
    );
    assert_eq!(fx.panes(), panes_before);
    assert_eq!(slot(&fx), slot_before);
    assert_eq!(new_windows(&fx).len(), 1, "pi is started once");
}

#[test]
fn a_stopped_manager_without_a_pane_is_started_again_on_its_file() {
    let fx = Fx::with_ports();
    setup(&fx);
    fx.cockpit();
    let first = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = first["manager"]["session"].as_str().unwrap().to_owned();
    // The first message is written once, at registration; a restart must not write it again.
    let message = fx.env.cpd_home().join("run/ctx/manager-message.md");
    assert!(message.exists());
    fs::remove_file(&message).unwrap();

    // Close its pane: stopped, no pane.
    fx.cpd(&["session", "close", &id]).ok();
    let row = fx.cpd(&["session", "show", &id]).ok()["session"].clone();
    assert_eq!(
        (&row["state"], &row["tmux_pane"]),
        (&json!("stopped"), &Value::Null)
    );
    assert_eq!(window_names(&fx), ["main"]);
    let slot_before = slot(&fx);

    let again = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(again["started"], true);
    assert_eq!(again["manager"]["session"], json!(id));
    assert_ne!(again["manager"]["pane"], first["manager"]["pane"]);
    assert_eq!(
        manager_ids(&fx),
        std::slice::from_ref(&id),
        "no second Manager"
    );
    assert_eq!(events(&fx, "session-registered").len(), 1);
    // `session-moved` for: parked, its pane closed, parked again.
    assert_eq!(events(&fx, "session-moved").len(), 3);
    assert_eq!(window_names(&fx), ["ctx-ctx", "main"]);
    assert_eq!(slot(&fx), slot_before);

    // The window runs `cpd session run` of the same Session again: `session run` finds its file and resumes it on the
    // row's Tier, with no first message (hs-d506, the session area's); here, no first message is written again.
    assert_eq!(new_windows(&fx).len(), 2);
    let (argv, _, vars) = started(&fx, again["manager"]["pane"].as_str().unwrap());
    assert_eq!(argv, ["cpd", "session", "run", id.as_str()]);
    assert!(
        vars.contains(&("CPD_SESSION".to_owned(), id.clone())),
        "{vars:?}"
    );
    assert!(
        !message.exists(),
        "no first message is written for a restart"
    );

    // And a third call changes nothing.
    assert_eq!(
        fx.cpd(&["context", "manager", "--context", "ctx"]).ok()["started"],
        false
    );
    assert_eq!(new_windows(&fx).len(), 2);
}

#[test]
fn refusals_register_nothing() {
    let fx = Fx::with_ports();
    setup(&fx);

    // No cockpit: refused before the Session is registered.
    let error = refused(
        &fx.cpd(&["context", "manager", "--context", "ctx"]),
        "no_cockpit",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd tui");
    assert!(manager_ids(&fx).is_empty());

    fx.cockpit();
    refused(
        &fx.cpd(&["context", "manager", "--context", "nobody"]),
        "context_not_found",
        4,
    );
    refused(&fx.cpd(&["context", "manager"]), "context_required", 2);

    fx.cpd(&["context", "archive", "ctx"]).ok();
    let error = refused(
        &fx.cpd(&["context", "manager", "--context", "ctx"]),
        "archived",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");

    assert!(manager_ids(&fx).is_empty());
    assert_eq!(window_names(&fx), ["main"]);
    assert!(new_windows(&fx).is_empty(), "no pi was started");
}
