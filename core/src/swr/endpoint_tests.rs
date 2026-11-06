//! Endpoint health (swr.md §7): rate-limit pause, one report per outage after ten minutes, a permanent
//! failure at once. A temp store; time is passed in from a `ManualClock`.

use std::time::Duration;

use time::OffsetDateTime;

use super::endpoint::{self, Endpoint, Kind};
use crate::clock::{Clock, ManualClock};
use crate::store::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = crate::home::CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, store)
}

fn mins(_clock: &ManualClock, base: OffsetDateTime, secs: u64) -> OffsetDateTime {
    base + Duration::from_secs(secs)
}

fn row(store: &mut Store, name: &str) -> Endpoint {
    endpoint::get(store, name)
        .unwrap()
        .expect("the endpoint row")
}

#[test]
fn swr_endpoint_rate_limit_pauses_until_reset() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    let reset = t0 + Duration::from_secs(600);
    endpoint::record_rate(&mut store, "github", 5, reset).unwrap();
    let healthy = row(&mut store, "github");
    assert_eq!(healthy.rate_remaining, Some(5));
    assert_eq!(healthy.rate_reset_at, Some(reset));
    assert!(
        !endpoint::is_paused(&healthy, t0),
        "a healthy limit does not pause"
    );

    endpoint::record_rate(&mut store, "github", 0, reset).unwrap();
    let spent = row(&mut store, "github");
    assert_eq!(spent.fetch_status, "paused");
    assert_eq!(spent.paused_until, Some(reset));
    assert!(endpoint::is_paused(&spent, t0));
    assert!(endpoint::is_paused(&spent, reset - Duration::from_secs(1)));
    assert!(!endpoint::is_paused(&spent, reset));

    endpoint::record_ok(&mut store, "github", t0 + Duration::from_secs(60)).unwrap();
    let ok = row(&mut store, "github");
    assert!(
        endpoint::is_paused(&ok, t0 + Duration::from_secs(61)),
        "a success never lifts a rate-limit pause early"
    );

    endpoint::pause_until(&mut store, "slack", reset).unwrap();
    let paused = row(&mut store, "slack");
    assert_eq!(paused.fetch_status, "paused");
    assert_eq!(paused.paused_until, Some(reset));
}

#[test]
fn swr_endpoint_outage_reports_once_after_ten_minutes() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    endpoint::record_failure(&mut store, "jira", t0, "503", Kind::Transient).unwrap();
    let r = row(&mut store, "jira");
    assert_eq!(r.failing_since, Some(t0));
    assert_eq!(r.error_count, 1);
    assert_eq!(r.last_error.as_deref(), Some("503"));
    assert!(!endpoint::due_report(&mut store, "jira", t0).unwrap());

    let t1 = mins(&clock, t0, 300);
    endpoint::record_failure(&mut store, "jira", t1, "503", Kind::Transient).unwrap();
    let r = row(&mut store, "jira");
    assert_eq!(
        r.failing_since,
        Some(t0),
        "the outage keeps its first failure"
    );
    assert_eq!(r.error_count, 2);
    assert!(!endpoint::due_report(&mut store, "jira", mins(&clock, t0, 599)).unwrap());

    let ten = mins(&clock, t0, 600);
    assert!(endpoint::due_report(&mut store, "jira", ten).unwrap());
    assert_eq!(row(&mut store, "jira").reported_at, Some(ten));
    assert!(!endpoint::due_report(&mut store, "jira", mins(&clock, t0, 601)).unwrap());
    endpoint::record_failure(
        &mut store,
        "jira",
        mins(&clock, t0, 900),
        "503",
        Kind::Transient,
    )
    .unwrap();
    assert!(!endpoint::due_report(&mut store, "jira", mins(&clock, t0, 1200)).unwrap());
}

#[test]
fn swr_endpoint_success_clears_outage_and_allows_new_report() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    endpoint::record_failure(&mut store, "bifrost", t0, "down", Kind::Transient).unwrap();
    assert!(endpoint::due_report(&mut store, "bifrost", mins(&clock, t0, 600)).unwrap());

    let ok_at = mins(&clock, t0, 660);
    endpoint::record_ok(&mut store, "bifrost", ok_at).unwrap();
    let r = row(&mut store, "bifrost");
    assert_eq!(r.failing_since, None);
    assert_eq!(r.reported_at, None);
    assert_eq!(r.last_error, None);
    assert_eq!(r.error_count, 0);
    assert_eq!(r.last_ok_at, Some(ok_at));
    assert_eq!(r.status.as_deref(), Some("success"));
    assert_eq!(r.fetch_status, "idle");

    let t2 = mins(&clock, t0, 1200);
    endpoint::record_failure(&mut store, "bifrost", t2, "down", Kind::Transient).unwrap();
    assert_eq!(row(&mut store, "bifrost").failing_since, Some(t2));
    assert!(!endpoint::due_report(&mut store, "bifrost", mins(&clock, t0, 1799)).unwrap());
    assert!(endpoint::due_report(&mut store, "bifrost", mins(&clock, t0, 1800)).unwrap());
}

#[test]
fn swr_endpoint_permanent_failure_is_reported_at_once() {
    let (_dir, mut store) = store();
    let t0 = ManualClock::new().utc();
    endpoint::record_failure(&mut store, "ollama.lab", t0, "401", Kind::Permanent).unwrap();
    assert!(endpoint::due_report(&mut store, "ollama.lab", t0).unwrap());
    assert_eq!(row(&mut store, "ollama.lab").reported_at, Some(t0));
    assert!(!endpoint::due_report(&mut store, "ollama.lab", t0).unwrap());
    assert!(
        !endpoint::due_report(&mut store, "ollama.lab", t0 + Duration::from_secs(3600)).unwrap(),
        "once only"
    );
}
