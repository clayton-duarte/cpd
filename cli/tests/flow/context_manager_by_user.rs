//! `context manager --by-user` (commands.md §3.10, §10.8 step 3, hs-d542), run in this process (`Fx::with_ports`) on the
//! fake tmux. `context manager` writes the first-message file only without `--by-user`, and records the flag on the
//! Session row, which `session run` reads to start pi with `-- @<path>` or without a first message (the session area's,
//! `start` in `session/run.rs`): here the file, the row and the window's command are asserted.

use crate::common;

use std::fs;

use common::fx::Fx;

/// The Context `ctx`, the config and the cockpit.
fn setup(fx: &Fx) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cockpit();
}

/// The argv the fake server started in the Manager's window: `new-window` made it once, and it is `cpd session run <id>`.
fn window_argv(fx: &Fx, id: &str) -> Vec<String> {
    let made: Vec<_> = fx
        .calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|a| a.iter().any(|x| x == "new-window") && a.iter().any(|x| x == "ctx-ctx"))
        .collect();
    assert_eq!(made.len(), 1, "one new-window: {made:?}");
    let argv: Vec<String> = made[0]
        .iter()
        .skip_while(|a| *a != "--")
        .skip(1)
        .cloned()
        .collect();
    assert_eq!(argv, ["cpd", "session", "run", id]);
    argv
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
    let argv = window_argv(&fx, &id);
    assert!(
        !argv.iter().any(|a| a == "--" || a.starts_with('@')),
        "no first message file: {argv:?}"
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
    window_argv(&fx, &id);
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
