//! Asleep Sessions (contract §2.2 "Asleep", §4.5; commands.md §10.4, §10.10, §10.11, session list §10.x;
//! hs-d515, hs-d520), run in this process (`Fx` on the fake ports: the cockpit is the fake tmux's).

use crate::common::TestEnv;
use crate::common::fx::Fx;
use crate::common::session::{events, setup, show};

use serde_json::{Value, json};

const LEAD1: &str = "lead1";
const LEAD2: &str = "lead2";
const FORK: &str = "forkless";
const ASLEEP: &str = "2020-01-02T00:00:00Z";

fn put_to_sleep(env: &TestEnv, id: &str) {
    env.sql(&format!(
        "UPDATE sessions SET asleep_at = '{ASLEEP}' WHERE id = '{id}'"
    ));
}

#[test]
fn open_clears_asleep_at() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    put_to_sleep(&fx.env, LEAD1);
    assert_eq!(show(&fx, LEAD1)["asleep_at"], json!(ASLEEP));

    fx.cpd(&["session", "open", LEAD1]).ok();
    assert_eq!(
        show(&fx, LEAD1)["asleep_at"],
        Value::Null,
        "session open wakes the Session"
    );
}

#[test]
fn list_shows_asleep_at() {
    let fx = Fx::new();
    setup(&fx.env);
    put_to_sleep(&fx.env, LEAD2);
    let list = fx.cpd(&["session", "list"]).ok();
    let at = |id: &str| {
        list["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap()["asleep_at"]
            .clone()
    };
    assert_eq!(at(LEAD2), json!(ASLEEP));
    assert_eq!(at(LEAD1), Value::Null);
}

#[test]
fn archive_clears_asleep_at() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.env.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from, asleep_at)
           VALUES ('{FORK}', 'lead', 'ctx', 'pay', 'Side', '/', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't', 'lead1', '{ASLEEP}')"
    ));
    assert_eq!(show(&fx, FORK)["asleep_at"], json!(ASLEEP));
    fx.cpd(&["session", "archive", FORK]).ok();
    let row = show(&fx, FORK);
    assert!(row["archived_at"].is_string(), "{row}");
    assert_eq!(
        row["asleep_at"],
        Value::Null,
        "archiving clears asleep_at in the same transaction"
    );
    assert_eq!(events(&fx, "session-archived").len(), 1);
}
