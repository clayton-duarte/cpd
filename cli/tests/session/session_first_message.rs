//! `cpd session first-message` (commands.md §10.15, hs-d542 A3), run as the built binary.

use crate::common;
use crate::session::setup;

use std::fs;

use common::{TestEnv, refused, SLEEP_SECONDS};
use serde_json::Value;

fn fakes(env: &TestEnv) {
    env.write_tmux_config("");
    env.fake_cpd_views();
    env.fake_pi(&format!("exec sleep {SLEEP_SECONDS}"));
}

#[test]
fn first_message_of_a_lead_holds_header_brief_and_records_written_after_registration() {
    let env = TestEnv::new();
    fakes(&env);
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let brief = env.root().join("brief.md");
    fs::write(&brief, "Retry the failed payouts.\n").unwrap();
    let out = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay", "--by-user", "--brief-file", brief.to_str().unwrap()]).ok();
    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    env.cpd(&["context", "note", "Written after registration.", "--context", "ctx"]).ok();

    let out = env.cpd(&["session", "first-message", "--session", &lead]).ok();
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
    let env = TestEnv::new();
    fakes(&env);
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    env.cpd(&["context", "note", "Mind the ledger.", "--context", "ctx"]).ok();
    let out = env.cpd(&["context", "manager", "--context", "ctx", "--by-user"]).ok();
    let manager = out["manager"]["session"].as_str().unwrap().to_owned();

    let out = env.cpd(&["session", "first-message", "--session", &manager]).ok();
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
    let env = TestEnv::new();
    setup(&env);
    let run = env.cpd(&["session", "first-message", "--session", "worker1"]);
    let _: Value = refused(&run, "not_a_lead", 3);
}
