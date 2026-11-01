//! Red test for the engine's `@cpd_incident` option (contract §6.1, hs-d634, hs-d640): the
//! status option step sets it to the elapsed time since the incident row's `started_at` while a
//! row exists and unsets it while none does, like `@cpd_workers`. The tmux is the in-process one, whose runner records
//! its arguments, so the test reads what the step sends.

use cpd_core::clock::{Clock, ManualClock};
use time::Duration;
use time::format_description::well_known::Rfc3339;

use super::testkit::Fx;
use super::workers;

/// `now - ago` on `clock` as the RFC 3339 `started_at` the incident row holds.
fn started_before(clock: &ManualClock, ago: Duration) -> String {
    (clock.utc() - ago)
        .format(&Rfc3339)
        .expect("a time formats as RFC 3339")
}

/// One run of the status option step on a fresh home, on a `ManualClock`, with an `incident` row that started `ago` before the
/// clock's now (None: no row). Returns what the step sent to tmux.
fn sent(ago: Option<Duration>) -> String {
    let clock = ManualClock::new();
    let mut fx = Fx::new("");
    fx.engine.ports.clock = std::sync::Arc::new(clock.clone());
    let runner = fx.on_fake_tmux();
    fx.tmux_server();
    let engine = &mut fx.engine;
    if let Some(ago) = ago {
        let started_at = started_before(&clock, ago);
        engine
            .store
            .write("test", |tx| {
                tx.execute(
                    "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, ?1, 'mgr', 'pager down', ?2)",
                    rusqlite::params![&started_at, tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
    }
    let before = runner.calls().len();
    workers::run(engine);
    let sent: Vec<String> = runner
        .calls()
        .into_iter()
        .skip(before)
        .map(|call| call.args.join(" "))
        .collect();
    assert!(!sent.is_empty(), "the step called tmux");
    sent.join("\n")
}

/// The elapsed duration the step set on `@cpd_incident`.
fn written(calls: &str) -> String {
    let (_, rest) = calls
        .trim_end()
        .split_once("@cpd_incident ")
        .expect("the incident is set");
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
        (
            Duration::hours(1) + Duration::minutes(23) + Duration::seconds(30),
            "1h 23m",
        ),
        (Duration::days(1) + Duration::seconds(30), "1d"),
        (
            Duration::days(2) + Duration::hours(4) + Duration::seconds(30),
            "2d 4h",
        ),
    ];
    for (ago, want) in cases {
        let on = sent(Some(ago));
        assert!(
            on.contains("@cpd_incident"),
            "the option is named while incident mode is on: {on}"
        );
        assert!(
            !on.contains("-gu @cpd_incident"),
            "it is set, not unset, while on: {on}"
        );
        assert!(
            !on.contains("pager down"),
            "the option no longer carries the reason: {on}"
        );
        assert_eq!(written(&on), want, "the elapsed duration {ago:?}: {on}");
    }

    let off = sent(None);
    assert!(
        off.contains("@cpd_incident"),
        "the option is named while incident mode is off: {off}"
    );
    assert!(
        off.contains("-gu @cpd_incident") || off.contains("-u @cpd_incident"),
        "it is unset while off: {off}"
    );
}
