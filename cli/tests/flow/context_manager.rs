//! Manager creation and legacy Manager resumption using fake tmux.

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
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Payments', 't', 'm1');",
    );
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
fn the_first_call_registers_a_durable_manager_without_a_window() {
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
        (&json!("ctx"), &json!("stopped"))
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
    assert_eq!(row["place"], "none");

    assert_eq!(row["runner"], "durable");
    assert!(manager["pane"].is_null());
    assert_eq!(window_names(&fx), ["main"]);
    assert!(new_windows(&fx).is_empty());

    // The Events.
    let registered = events(&fx, "session-registered");
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0]["session"], json!(id));
    assert_eq!(
        registered[0]["data"],
        json!({"role": "manager", "name": "ctx", "forked_from": null, "tier": "O4"})
    );
    assert!(events(&fx, "session-moved").is_empty());

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
    assert!(
        new_windows(&fx).is_empty(),
        "durable creation opens no window"
    );
}

#[test]
fn a_stopped_durable_manager_is_not_recreated_or_given_a_pane() {
    let fx = Fx::with_ports();
    setup(&fx);
    fx.cockpit();
    let first = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    let id = first["manager"]["session"].as_str().unwrap().to_owned();
    let message = fx.env.cpd_home().join("run/ctx/manager-message.md");
    assert!(message.exists());
    fs::remove_file(&message).unwrap();
    let before = slot(&fx);
    let again = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(again["started"], false);
    assert_eq!(again["manager"]["session"], id);
    assert!(again["manager"]["pane"].is_null());
    assert_eq!(manager_ids(&fx), [id]);
    assert_eq!(events(&fx, "session-registered").len(), 1);
    assert!(events(&fx, "session-moved").is_empty());
    assert!(new_windows(&fx).is_empty());
    assert_eq!(slot(&fx), before);
    assert!(!message.exists());
}

#[test]
fn missing_cockpit_does_not_prevent_durable_manager_creation() {
    let fx = Fx::with_ports();
    setup(&fx);
    let out = fx.cpd(&["context", "manager", "--context", "ctx"]).ok();
    assert_eq!(out["started"], true);
    assert_eq!(
        manager_ids(&fx),
        [out["manager"]["session"].as_str().unwrap()]
    );
    assert!(out["manager"]["pane"].is_null());
    assert!(fx.calls("tmux").is_empty());
    assert!(fx.tmux_state().is_none());
}

#[test]
fn refusals_register_nothing() {
    let fx = Fx::with_ports();
    setup(&fx);

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
