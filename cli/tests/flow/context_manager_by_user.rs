//! Durable Manager first-message and by-user registration, with fake tmux.

use crate::common;

use std::fs;

use common::fx::Fx;

/// The Context `ctx`, the config and the cockpit.
fn setup(fx: &Fx) {
    fx.env.write_tmux_config("");
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Payments', 't', 'm1');",
    );
    fx.cockpit();
}

fn manager(fx: &Fx, by_user: bool) -> String {
    let mut args = vec!["context", "manager", "--context", "ctx"];
    if by_user {
        args.push("--by-user");
    }
    fx.cpd(&args).ok()["manager"]["session"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn context_manager_by_user_starts_the_manager_without_a_first_message() {
    let fx = Fx::with_ports();
    setup(&fx);
    let id = manager(&fx, true);
    assert!(
        fx.calls("tmux")
            .iter()
            .all(|c| !c.args.iter().any(|a| a == "new-window"))
    );
    assert!(
        !fx.env
            .cpd_home()
            .join("run/ctx/manager-message.md")
            .exists(),
        "no first-message file"
    );
    assert_eq!(
        fx.cpd(&["session", "show", &id]).ok()["session"]["by_user"],
        true
    );
}

#[test]
fn context_manager_without_by_user_keeps_the_first_message() {
    let fx = Fx::with_ports();
    setup(&fx);
    let id = manager(&fx, false);
    assert!(
        fx.calls("tmux")
            .iter()
            .all(|c| !c.args.iter().any(|a| a == "new-window"))
    );
    let path = fx.env.cpd_home().join("run/ctx/manager-message.md");
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .starts_with("You are the Manager of Context ctx")
    );
    assert_eq!(
        fx.cpd(&["session", "show", &id]).ok()["session"]["by_user"],
        false
    );
}
