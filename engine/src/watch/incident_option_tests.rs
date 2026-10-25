//! Red test for the engine's `@cpd_incident` option (contract §6.1, hs-d634, hs-d640): the
//! status option step sets it to the elapsed time since the incident row's `started_at` while a
//! row exists and unsets it while none does, like `@cpd_workers`. The tmux is a script that
//! records its arguments, so the test reads what the step sends.

use std::os::unix::fs::PermissionsExt;

use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::engine::Engine;
use super::workers;

/// `now - ago` as the RFC 3339 `started_at` the incident row holds.
fn started_before(ago: Duration) -> String {
    (OffsetDateTime::now_utc() - ago).format(&Rfc3339).expect("a time formats as RFC 3339")
}

/// One run of the status option step on a fresh home with an `incident` row whose `started_at`
/// is `started_at` (None: no row). Returns what the step sent to tmux.
fn sent(started_at: Option<&str>) -> String {
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
    if let Some(started_at) = started_at {
        engine
            .store
            .write("test", |tx| {
                tx.execute(
                    "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, ?1, 'mgr', 'pager down', ?2)",
                    rusqlite::params![started_at, tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
    }
    workers::run(&mut engine);
    std::fs::read_to_string(&calls).expect("the step called tmux")
}

/// The elapsed duration the step set on `@cpd_incident`.
fn written(calls: &str) -> String {
    let (_, rest) = calls.trim_end().split_once("@cpd_incident ").expect("the incident is set");
    rest.split(';').next().unwrap().trim().to_owned()
}

/// While an `incident` row exists the step sets `@cpd_incident` to the duration since the row's
/// `started_at` in the format hs-d640 settled: whole minutes, `<m>m` under an hour, `<h>h` plus
/// ` <m>m` when the minutes are non-zero under a day, `<d>d` plus ` <h>h` when the hours are
/// non-zero beyond; the reason is not written. With no row the step unsets the option.
#[test]
fn the_status_step_writes_the_elapsed_duration() {
    let cases = [
        (Duration::seconds(30), "0m"),
        (Duration::minutes(5) + Duration::seconds(30), "5m"),
        (Duration::minutes(59) + Duration::seconds(30), "59m"),
        (Duration::hours(1) + Duration::seconds(30), "1h"),
        (Duration::hours(1) + Duration::minutes(23) + Duration::seconds(30), "1h 23m"),
        (Duration::days(1) + Duration::seconds(30), "1d"),
        (Duration::days(2) + Duration::hours(4) + Duration::seconds(30), "2d 4h"),
    ];
    for (ago, want) in cases {
        let on = sent(Some(&started_before(ago)));
        assert!(on.contains("@cpd_incident"), "the option is named while incident mode is on: {on}");
        assert!(!on.contains("-gu @cpd_incident"), "it is set, not unset, while on: {on}");
        assert!(!on.contains("pager down"), "the option no longer carries the reason: {on}");
        assert_eq!(written(&on), want, "the elapsed duration {ago:?}: {on}");
    }

    let off = sent(None);
    assert!(off.contains("@cpd_incident"), "the option is named while incident mode is off: {off}");
    assert!(off.contains("-gu @cpd_incident") || off.contains("-u @cpd_incident"), "it is unset while off: {off}");
}
