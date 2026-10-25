//! `cpd session first-message` (commands.md §10.15, hs-d542 A3), run in this process (`Fx`).

use crate::common;
use crate::common::session::setup;

use std::fs;

use common::fx::Fx;
use common::{refused, TestEnv};
use serde_json::Value;

/// The machine's config on the env's socket; no `pi` or `cpd` is run (the fake tmux starts no program).
fn config(env: &TestEnv) {
    env.write_tmux_config("");
}

#[test]
fn first_message_of_a_lead_holds_header_brief_and_records_written_after_registration() {
    let fx = Fx::with_ports();
    config(&fx.env);
    fx.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let brief = fx.env.root().join("brief.md");
    fs::write(&brief, "Retry the failed payouts.\n").unwrap();
    let out = fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay", "--by-user", "--brief-file", brief.to_str().unwrap()]).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    fx.cpd(&["context", "note", "Written after registration.", "--context", "ctx"]).ok();

    let out = fx.cpd(&["session", "first-message", "--session", &lead]).ok();
    let text = out["text"].as_str().unwrap();
    assert!(text.starts_with("You are the Lead of Plan pay \"Pay\" in Context ctx."), "{text}");
    assert!(text.contains("Retry the failed payouts."), "{text}");
    assert!(text.contains("Written after registration."), "{text}");
    assert_eq!(out["session"], lead.as_str());
    assert_eq!(out["role"], "lead");
    let file = out["file"].as_str().unwrap();
    assert!(file.ends_with("run/pay/lead-message.md"), "{file}");
    assert_eq!(fs::read_to_string(file).unwrap(), text);
    assert!(out["event"].is_number(), "{out}");
}

#[test]
fn first_message_of_a_manager_holds_header_and_context() {
    let fx = Fx::with_ports();
    config(&fx.env);
    fx.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    fx.cpd(&["context", "note", "Mind the ledger.", "--context", "ctx"]).ok();
    fx.cockpit();
    let out = fx.cpd(&["context", "manager", "--context", "ctx", "--by-user"]).ok();
    let manager = out["manager"]["session"].as_str().unwrap().to_owned();

    let out = fx.cpd(&["session", "first-message", "--session", &manager]).ok();
    let text = out["text"].as_str().unwrap();
    assert!(text.starts_with("You are the Manager of Context ctx \"Payments\". Follow the cpd-manager skill.\n\n"), "{text}");
    assert!(text.contains("Mind the ledger."), "{text}");
    assert_eq!(out["role"], "manager");
    let file = out["file"].as_str().unwrap();
    assert!(file.ends_with("run/ctx/manager-message.md"), "{file}");
    assert_eq!(fs::read_to_string(file).unwrap(), text);
    assert!(out["event"].is_number(), "{out}");
}

#[test]
fn first_message_refuses_a_worker_session() {
    let fx = Fx::new();
    setup(&fx.env);
    let run = fx.cpd(&["session", "first-message", "--session", "worker1"]);
    let _: Value = refused(&run, "not_a_lead", 3);
}
