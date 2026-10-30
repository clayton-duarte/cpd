//! hs-d738, hs-d739: a Lead's and a Manager's pi launch carries no `PLANNOTATOR_*` variable, set or removed, whatever
//! the cpd that runs `session run` holds. In this process (`Fx`), on the fake tmux and the fake spawner, as
//! `run_gives_the_lead_remote_and_the_review_host` of session.rs.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::session::{home, setup};
use common::wrapper::{Pane, installed_cpd};
use cpd_core::host::spawn::Launch;

const LEAD1: &str = "lead1";
const MANAGER: &str = "manager1";

fn fixture(fx: &Fx, session: &str) {
    setup(&fx.env);
    fx.cockpit();
    let sessions = fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join(format!("2026_{session}.jsonl")), "{\"type\":\"session\"}\n").unwrap();
}

fn export_plannotator(fx: &Fx) {
    fx.export("PLANNOTATOR_URL_HOST", "review.test");
    fx.export("PLANNOTATOR_REMOTE", "0");
    fx.export("PLANNOTATOR_PORT", "19432-19456");
}

fn assert_no_plannotator(launch: &Launch) {
    let set: Vec<&String> = launch.env_set.iter().map(|(k, _)| k).filter(|k| k.starts_with("PLANNOTATOR_")).collect();
    let removed: Vec<&String> = launch.env_remove.iter().filter(|k| k.starts_with("PLANNOTATOR_")).collect();
    assert!(set.is_empty(), "env_set holds {set:?}");
    assert!(removed.is_empty(), "env_remove holds {removed:?}");
}

#[test]
fn drop_plannotator_lead_launch_sets_and_removes_no_plannotator_variable() {
    let fx = Fx::with_ports();
    fixture(&fx, LEAD1);
    let pane = Pane::new(&fx, LEAD1, &installed_cpd(&fx));
    export_plannotator(&fx);
    let mut wrapper = pane.run(&["session", "run", LEAD1]);
    wrapper.until("the Lead's pi", || pane.spawn.pis().len() == 1);
    assert_no_plannotator(&pane.spawn.pis()[0]);
}

#[test]
fn drop_plannotator_manager_launch_sets_and_removes_no_plannotator_variable() {
    let fx = Fx::with_ports();
    fixture(&fx, MANAGER);
    let message = home(&fx.env).manager_message(&cpd_core::ids::ContextId::parse("ctx").unwrap());
    fs::create_dir_all(message.parent().unwrap()).unwrap();
    fs::write(&message, "You are the Manager.").unwrap();
    let pane = Pane::new(&fx, MANAGER, &installed_cpd(&fx));
    export_plannotator(&fx);
    let mut wrapper = pane.run(&["session", "run", MANAGER]);
    wrapper.until("the Manager's pi", || pane.spawn.pis().len() == 1);
    assert_no_plannotator(&pane.spawn.pis()[0]);
}
