//! The engine's sentences name times in the machine's local zone (PLAN local-time, pro-d41, pro-d42). The zone is
//! `ManualClock`'s (EST5EDT: EST and EDT); no test reads the system zone.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::engine::Downtime;
use super::pr::rate_guard;
use super::restart::{Failed, Pending};
use cpd_core::clock::{Clock, ManualClock};

fn at(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

#[test]
fn downtime_sentence_names_the_local_stop_time() {
    let down = Downtime { since: at("2026-10-25T22:29:00.000Z"), said: true, down: time::Duration::milliseconds(3200), planned: false, overdue: false };
    assert_eq!(down.sentence(&ManualClock::new().zone()), "The engine was down for 3.2 s: it stopped at 2026-10-25 18:29 EDT.");
}

fn pending(failed: Option<Failed>) -> Pending {
    Pending { at: "2026-10-25T21:10:00.000Z".into(), server: "3.4".into(), installed: "3.5".into(), failed }
}

#[test]
fn tmux_restart_pending_names_the_local_time() {
    assert_eq!(
        pending(None).describe(&ManualClock::new().zone()),
        "A tmux restart is pending since 2026-10-25 17:10 EDT: the server runs tmux 3.4, the installed tmux is 3.5. No Attempt starts until it has run."
    );
}

#[test]
fn tmux_restart_failed_names_the_local_time() {
    let failed = Failed { at: "2026-10-25T21:10:00.000Z".into(), code: "spawn_failed".into(), reason: "<reason>".into(), reported: false };
    assert_eq!(
        pending(Some(failed)).describe(&ManualClock::new().zone()),
        "The tmux restart failed at 2026-10-25 17:10 EDT: <reason> See run/tmux-restart.log."
    );
}

fn limited() -> super::report::Failure {
    let clock = ManualClock::new();
    let out = serde_json::json!({"rate_limit": {"remaining": 12, "reset_at": "2026-10-20T11:00:00Z"}});
    rate_guard(&clock, &out, &clock.zone()).unwrap().expect("12 requests left is below the floor").1
}

#[test]
fn rate_limit_message_names_the_local_reset_time() {
    assert_eq!(limited().message, "GitHub's rate limit has 12 requests left: pull request polling pauses until 2026-10-20 07:00 EDT.");
}

#[test]
fn rate_limit_field_stays_utc() {
    let failure = limited();
    assert_eq!(failure.details["reset_at"], "2026-10-20T11:00:00Z");
    // the field is the UTC string, and only the prose is local: the message does not repeat it
    assert!(!failure.message.contains("2026-10-20T11:00:00Z"), "{}", failure.message);
}
