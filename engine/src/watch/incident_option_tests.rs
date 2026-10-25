//! Red test for the engine's `@cpd_incident` option (contract §6.1, hs-d634, hs-d640): the
//! status option step sets it to the elapsed time since the incident row's `started_at` while a
//! row exists and unsets it while none does, like `@cpd_workers`. The tmux is a script that
//! records its arguments, so the test reads what the step sends.

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
                    "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, '2000-01-01T00:00:00.000Z', 'mgr', ?1, ?2)",
                    rusqlite::params![reason, tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
    }
    workers::run(&mut engine);
    std::fs::read_to_string(&calls).expect("the step called tmux")
}

/// While an `incident` row exists the step sets `@cpd_incident` to the elapsed time since the
/// row's `started_at` (a coarse `{n}d {n}h` duration, the exact format unit-tested in core);
/// with none it unsets the option. The reason is no longer written.
#[test]
fn the_status_step_sets_the_incident_option() {
    let on = sent(Some("pager down"));
    assert!(on.contains("@cpd_incident"), "the option is named while incident mode is on: {on}");
    assert!(!on.contains("-gu @cpd_incident"), "it is set, not unset, while on: {on}");
    assert!(!on.contains("pager down"), "the option no longer carries the reason: {on}");
    // The value is the coarse elapsed duration: a `{n}d {n}h` shape, since the started_at is far in the past.
    let value = on.trim_end().split("@cpd_incident ").nth(1).expect("the incident set").trim().to_owned();
    let (days, rest) = value.split_once('d').unwrap_or_else(|| panic!("a days unit in {value:?}"));
    assert!(!days.is_empty() && days.chars().all(|c| c.is_ascii_digit()), "digits before `d`: {value:?}");
    let (hours, tail) = rest.trim_start().split_once('h').unwrap_or_else(|| panic!("an hours unit in {value:?}"));
    assert!(!hours.is_empty() && hours.chars().all(|c| c.is_ascii_digit()), "digits before `h`: {value:?}");
    assert!(tail.trim().is_empty(), "the `{{n}}d {{n}}h` shape only: {value:?}");

    let off = sent(None);
    assert!(off.contains("@cpd_incident"), "the option is named while incident mode is off: {off}");
    assert!(off.contains("-gu @cpd_incident") || off.contains("-u @cpd_incident"), "it is unset while off: {off}");
}
