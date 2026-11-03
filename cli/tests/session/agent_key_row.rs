//! The key of a Session row and the pane pill (contract §2.2 "Agent key", §6.1; commands.md §10.1, §10.2; pro-d345,
//! pro-d346): `session show` and `session list` carry `key` next to `name` (the name for a Manager, a Lead that is not
//! a fork and a Worker, null for a fork); a Lead's pill is its key, not its Plan title.

use crate::common;

use common::fx::Fx;
use common::session::{setup_with_fork, show};
use serde_json::{Value, json};

/// `setup_with_fork` with Manager `mgr1` (`ctx`), Leads `lead1` (`ctx:pay`) and `lead2` (`ctx:other`), Worker `worker1`
/// (`ctx:pay:api`) and the fork `fork1` of `lead1` (`ctx:pay (fork)`).
fn world() -> Fx {
    let fx = Fx::new();
    setup_with_fork(&fx.env);
    fx.env.sql(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from)
           VALUES ('mgr1', 'manager', 'ctx', NULL, NULL, 'ctx', '/', 'O4', 'idle', 't5', 't5', NULL);
         UPDATE sessions SET name = 'ctx:pay' WHERE id = 'lead1';
         UPDATE sessions SET name = 'ctx:other' WHERE id = 'lead2';
         UPDATE sessions SET name = 'ctx:pay:api' WHERE id = 'worker1';
         UPDATE sessions SET name = 'ctx:pay (fork)' WHERE id = 'fork1';",
    );
    fx
}

fn assert_key_is_name(id: &str, name: &str) {
    let fx = world();
    let session = show(&fx, id);
    assert_eq!(session["name"], name);
    assert_eq!(session["key"], name);
}

#[test]
fn session_show_carries_the_key_of_a_manager() {
    assert_key_is_name("mgr1", "ctx");
}

#[test]
fn session_show_carries_the_key_of_a_lead() {
    assert_key_is_name("lead1", "ctx:pay");
}

#[test]
fn session_show_carries_the_key_of_a_worker() {
    assert_key_is_name("worker1", "ctx:pay:api");
}

#[test]
fn session_show_carries_a_null_key_for_a_fork() {
    let fx = world();
    let fork = show(&fx, "fork1");
    assert_eq!(fork["name"], "ctx:pay (fork)");
    assert_eq!(fork.get("key"), Some(&Value::Null));
}

#[test]
fn session_list_carries_the_key_of_each_row() {
    let fx = world();
    let out = fx.cpd(&["session", "list"]).ok();
    let rows = out["sessions"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    for row in rows {
        let expected = if row["id"] == "fork1" {
            Value::Null
        } else {
            row["name"].clone()
        };
        assert_eq!(row.get("key"), Some(&expected), "row {}", row["id"]);
    }
}

/// `@cpd_title` of `pane`.
fn pane_title(fx: &Fx, pane: &str) -> String {
    let out = fx.tmux(&["show", "-pqv", "-t", pane, "@cpd_title"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .to_owned()
}

/// `plan new pay` in Context `ctx` on fake ports: the Lead's session id and pane.
fn new_plan(fx: &Fx) -> (String, String) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.keyed_repo("pay");
    fx.cockpit();
    let out = fx
        .cpd(&[
            "plan",
            "new",
            "pay",
            "--context",
            "ctx",
            "--title",
            "Retry payouts",
        ])
        .ok();
    (
        out["lead"]["session"].as_str().unwrap().to_owned(),
        out["lead"]["pane"].as_str().unwrap().to_owned(),
    )
}

#[test]
fn a_new_lead_pane_pill_is_its_key() {
    let fx = Fx::with_ports();
    let (_, pane) = new_plan(&fx);
    assert_eq!(pane_title(&fx, &pane), "ctx:pay");
}

#[test]
fn plan_rename_leaves_the_pill_at_the_key() {
    let fx = Fx::with_ports();
    let (lead, pane) = new_plan(&fx);
    fx.cpd(&[
        "plan",
        "rename",
        "--plan",
        "pay",
        "--title",
        "Retry the payouts",
    ])
    .ok();
    assert_eq!(pane_title(&fx, &pane), "ctx:pay");
    assert_eq!(show(&fx, &lead)["name"], json!("ctx:pay"));
}
