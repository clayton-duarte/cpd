//! Red test for hs-d541 (contract §6.1.1): the engine no longer sets `@cpd_inbox` or `@cpd_inbox_tier`, which only the
//! Inbox pill read; the status-line update still sets the workers texts.
//!
//! The tmux is the in-process one, whose runner records its arguments, so the test reads what the update sends.

use super::super::testkit::Fx;

#[test]
fn the_engine_sets_no_inbox_options() {
    let mut fx = Fx::new("");
    let runner = fx.on_fake_tmux();
    fx.tmux_server();
    let before = runner.calls().len();
    super::run(&mut fx.engine);

    let sent = runner
        .calls()
        .into_iter()
        .skip(before)
        .map(|call| call.args.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        sent.contains("set -g @cpd_workers "),
        "the workers text is still set: {sent}"
    );
    assert!(
        sent.contains("@cpd_workers_short "),
        "the short text is still set: {sent}"
    );
    assert!(
        !sent.contains("@cpd_inbox"),
        "neither `@cpd_inbox` nor `@cpd_inbox_tier` is set: {sent}"
    );
}
