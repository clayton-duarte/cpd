//! Red tests for the `@cpd_runner` feed (pro-d199; contract §6.1): the workers step reads `durable_workers` from
//! config.toml afresh and sets `@cpd_runner` (`pi` or `durable`) in the same tmux call as the workers texts, remembers it
//! and sets it again only when it differs; a config that does not parse is an `error` and sets nothing.
//!
//! The tmux is the in-process one, whose runner records its arguments, so the tests read what the step sends.

use super::super::testkit::Fx;
use cpd_core::host::runner::FakeRunner;

/// A config.toml with `durable_workers` for `<runner>`, the rest as the kit writes it.
fn write_runner(fx: &Fx, runner: &str) {
    let text = format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n\n[experimental]\ndurable_workers = {}\n\n[tmux]\nsocket = \"testkit\"\n",
        runner == "durable"
    );
    std::fs::write(fx.engine.home.config(), text).unwrap();
}

/// One run of the step: the tmux calls it made that set an option, each as one line.
fn sets(fx: &mut Fx, runner: &FakeRunner) -> Vec<String> {
    let before = runner.calls().len();
    super::run(&mut fx.engine);
    runner
        .calls()
        .into_iter()
        .skip(before)
        .map(|call| call.args.join(" "))
        .filter(|line| line.contains("set "))
        .collect()
}

#[test]
fn the_workers_step_sets_the_runner_option_in_the_same_call_as_the_workers_texts() {
    let mut fx = Fx::new("");
    let runner = fx.on_fake_tmux();
    fx.tmux_server();
    write_runner(&fx, "durable");
    let sent = sets(&mut fx, &runner);
    assert_eq!(sent.len(), 1, "one call: {sent:?}");
    assert!(sent[0].contains("@cpd_workers "), "{sent:?}");
    assert!(
        sent[0].contains("set -g @cpd_runner durable"),
        "the runner option is set in that call: {sent:?}"
    );
}

#[test]
fn a_changed_config_line_changes_the_option_at_the_next_run() {
    let mut fx = Fx::new("");
    let runner = fx.on_fake_tmux();
    fx.tmux_server();
    write_runner(&fx, "pi");
    let first = sets(&mut fx, &runner);
    assert!(
        first.iter().any(|l| l.contains("set -g @cpd_runner pi")),
        "{first:?}"
    );
    assert!(
        sets(&mut fx, &runner).is_empty(),
        "nothing differs: nothing is set again"
    );

    write_runner(&fx, "durable");
    let next = sets(&mut fx, &runner);
    assert_eq!(next.len(), 1, "{next:?}");
    assert!(next[0].contains("set -g @cpd_runner durable"), "{next:?}");
    assert!(sets(&mut fx, &runner).is_empty(), "remembered again");
}

#[test]
fn a_config_that_does_not_parse_reports_an_error_and_sets_no_option() {
    let mut fx = Fx::new("");
    let runner = fx.on_fake_tmux();
    fx.tmux_server();
    std::fs::write(
        fx.engine.home.config(),
        "version = 1\n[experimental]\ndurable_workers = [\n",
    )
    .unwrap();
    let sent = sets(&mut fx, &runner);
    assert!(
        sent.is_empty(),
        "no option is set, no stale fallback: {sent:?}"
    );
    assert_eq!(fx.count("error"), 1, "the step reports it");
}
