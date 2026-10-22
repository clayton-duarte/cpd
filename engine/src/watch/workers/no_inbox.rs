//! Red test for hs-d541 (contract §6.1.1): the engine no longer sets `@cpd_inbox` or `@cpd_inbox_tier`, which only the
//! Inbox pill read; the status-line update still sets the workers texts.
//!
//! The tmux is a script that records its arguments, so the test reads what the update sends.

use std::os::unix::fs::PermissionsExt;

use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;

use super::super::engine::Engine;

#[test]
fn the_engine_sets_no_inbox_options() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("config.toml"), "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n\n[tmux]\nsocket = \"no-inbox\"\n").unwrap();
    let calls = dir.path().join("calls");
    let program = dir.path().join("tmux");
    std::fs::write(&program, format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n", calls.display())).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();

    let home = CpdHome::resolve(Some(root.into()), None).unwrap();
    let mut engine = Engine::for_test(home, Tmux::new("no-inbox").with_program(&program));
    super::run(&mut engine);

    let sent = std::fs::read_to_string(&calls).expect("the update called tmux");
    assert!(sent.contains("set -g @cpd_workers "), "the workers text is still set: {sent}");
    assert!(sent.contains("@cpd_workers_short "), "the short text is still set: {sent}");
    assert!(!sent.contains("@cpd_inbox"), "neither `@cpd_inbox` nor `@cpd_inbox_tier` is set: {sent}");
}
