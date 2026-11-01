//! `context manager --runner <pi|durable>` (commands.md §3.11, contract §6.19; hs-d820, hs-d754 Q10): the runner
//! is chosen at creation, stored on `sessions.runner`, and never changed. With the flag omitted the config key
//! `manager_runner` is the default (default `pi`). Run in this process (`Fx::with_ports`) on the fake tmux: a
//! durable Manager starts no tmux and no pane; a pi Manager needs the cockpit.

use crate::common;

use common::fx::Fx;

/// The Context `ctx`, with no cockpit yet.
fn setup(fx: &Fx, extra: &str) {
    fx.env.write_tmux_config(extra);
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
}

/// `context manager --context ctx` with an optional `--runner`.
fn manager(fx: &Fx, runner: Option<&str>) -> common::Run {
    let mut args = vec!["context", "manager", "--context", "ctx"];
    if let Some(runner) = runner {
        args.extend(["--runner", runner]);
    }
    fx.cpd(&args)
}

/// The stored runner of `session`, read from `session show`.
fn runner_of(fx: &Fx, session: &str) -> String {
    fx.cpd(&["session", "show", session]).ok()["session"]["runner"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_manager_is_created_with_the_runner_it_was_asked_for() {
    let fx = Fx::with_ports();
    setup(&fx, "manager_runner = \"pi\"");
    let out = manager(&fx, Some("durable")).ok();
    let id = out["manager"]["session"].as_str().unwrap().to_owned();
    assert!(out["manager"]["pane"].is_null(), "{out}");
    assert_eq!(runner_of(&fx, &id), "durable");
}

#[test]
fn a_runner_flag_overrides_the_config_default() {
    let fx = Fx::with_ports();
    setup(&fx, "manager_runner = \"durable\"");
    fx.cockpit();
    let out = manager(&fx, Some("pi")).ok();
    let id = out["manager"]["session"].as_str().unwrap().to_owned();
    assert!(!out["manager"]["pane"].is_null(), "{out}");
    assert_eq!(runner_of(&fx, &id), "pi");
}

#[test]
fn a_resumed_manager_keeps_its_stored_runner() {
    let fx = Fx::with_ports();
    setup(&fx, "");
    let first = manager(&fx, Some("durable")).ok();
    let id = first["manager"]["session"].as_str().unwrap().to_owned();

    // The config default changes; the stored choice does not.
    fx.env.write_tmux_config("manager_runner = \"pi\"");
    let again = manager(&fx, None).ok();
    assert_eq!(again["manager"]["session"], id.as_str(), "{again}");
    assert_eq!(runner_of(&fx, &id), "durable");
}

#[test]
fn a_resume_with_a_different_runner_is_refused() {
    let fx = Fx::with_ports();
    setup(&fx, "");
    fx.cockpit();
    let first = manager(&fx, Some("pi")).ok();
    let id = first["manager"]["session"].as_str().unwrap().to_owned();
    common::refused(&manager(&fx, Some("durable")), "usage", 2);
    assert_eq!(runner_of(&fx, &id), "pi");
}
