//! Red test for the engine's `@cpd_incident` option (contract §6.1, hs-d634): the
//! status option step sets it while an `incident` row exists and unsets it while
//! none does, like `@cpd_workers`. The tmux is a script that records its
//! arguments, so the test reads what the step sends.

use std::os::unix::fs::PermissionsExt;

use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;

use super::engine::Engine;
use super::workers;

/// One run of the status option step on a fresh home, with an `incident` row
/// carrying `incident` (None: no row). Returns what the step sent to tmux.
fn sent(incident: Option<&str>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("home");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("config.toml"), "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n\n[tmux]\nsocket = \"incident\"\n").unwrap();
    let calls = dir.path().join("calls");
    let program = dir.path().join("tmux");
    std::fs::write(&program, format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n", calls.display())).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();

    let home = CpdHome::resolve(Some(root.into()), None).unwrap();
    let mut engine = Engine::for_test(home, Tmux::new("incident").with_program(&program));
    if let Some(reason) = incident {
        engine
            .store
            .write("test", |tx| {
                tx.execute(
                    "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', ?1, ?2)",
                    rusqlite::params![reason, tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
    }
    workers::run(&mut engine);
    std::fs::read_to_string(&calls).expect("the step called tmux")
}

/// While an `incident` row exists the step sets `@cpd_incident` to the reason;
/// with none it unsets the option.
#[test]
fn the_status_step_sets_the_incident_option() {
    let on = sent(Some("pager down"));
    assert!(on.contains("@cpd_incident"), "the option is named while incident mode is on: {on}");
    assert!(!on.contains("-gu @cpd_incident"), "it is set, not unset, while on: {on}");
    assert!(on.contains("pager down"), "the option carries the reason while on: {on}");

    let off = sent(None);
    assert!(off.contains("@cpd_incident"), "the option is named while incident mode is off: {off}");
    assert!(off.contains("-gu @cpd_incident") || off.contains("-u @cpd_incident"), "it is unset while off: {off}");
}
