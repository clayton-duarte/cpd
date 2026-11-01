//! `plan new --by-user` (commands.md §4.1, §10.8 step 3, hs-d542), run in this process (`Fx::with_ports`) on the fake tmux.
//! `plan new` registers the Lead and opens its window, whose program is `cpd session run <id>`; the test runs that
//! command itself in the window's pane (`Pane`, on virtual time) and reads what pi was launched with from the `FakeSpawn`
//! instead of from a pi that records its calls.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::wrapper::{Pane, installed_cpd};
use cpd_core::host::spawn::Launch;

/// The Context `ctx` and the cockpit.
fn setup(fx: &Fx) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cockpit();
}

/// `plan new pay` and the `session run` of its Lead's window, stepped until pi was launched: the id of the Lead and
/// pi's launch, the one start. The pane is returned so its wrapper's environment stays until the test ends.
fn plan_new(fx: &Fx, by_user: bool) -> (String, Launch, Pane) {
    let mut args = vec!["plan", "new", "pay", "--context", "ctx", "--title", "Pay"];
    if by_user {
        args.push("--by-user");
    }
    let out = fx.cpd(&args).ok();
    let id = out["lead"]["session"].as_str().unwrap().to_owned();
    let pane = Pane::at(
        fx,
        out["lead"]["pane"].as_str().unwrap(),
        &installed_cpd(fx),
    );
    let mut wrapper = pane.run(&["session", "run", &id]);
    wrapper.until("pi", || !pane.spawn.pis().is_empty());
    let pis = pane.spawn.pis();
    assert_eq!(pis.len(), 1, "one pi start: {pis:?}");
    (id, pis[0].clone(), pane)
}

#[test]
fn plan_new_by_user_starts_pi_without_a_first_message() {
    let fx = Fx::with_ports();
    setup(&fx);
    let (id, pi, _pane) = plan_new(&fx, true);
    let argv = &pi.args;
    assert!(
        !argv.iter().any(|a| a == "--" || a.starts_with('@')),
        "no first message file: {argv:?}"
    );
    assert!(
        !fx.env.cpd_home().join("run/pay/lead-message.md").exists(),
        "no first-message file"
    );
    assert_eq!(
        fx.cpd(&["session", "show", &id]).ok()["session"]["by_user"],
        true
    );
}

#[test]
fn plan_new_without_by_user_keeps_the_first_message() {
    let fx = Fx::with_ports();
    setup(&fx);
    let (id, pi, _pane) = plan_new(&fx, false);
    let argv = &pi.args;
    let path = fx.env.cpd_home().join("run/pay/lead-message.md");
    assert_eq!(
        argv[argv.len() - 2..],
        ["--".to_owned(), format!("@{}", path.display())],
        "{argv:?}"
    );
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .starts_with("You are the Lead of Plan pay")
    );
    assert_eq!(
        fx.cpd(&["session", "show", &id]).ok()["session"]["by_user"],
        false
    );
}
