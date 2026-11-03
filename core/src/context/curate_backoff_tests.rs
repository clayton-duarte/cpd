//! Tests of write-time curation: the backoff.

use std::time::Duration;

use super::curate_backoff::Backoff;
use crate::clock::{Clock, ManualClock};

const MIN: Duration = Duration::from_secs(60);

#[test]
fn backoff_starts_at_one_minute() {
    let clock = ManualClock::new();
    let mut b = Backoff::new();
    assert_eq!(b.delay(), Duration::ZERO);
    b.failed(clock.now());
    assert_eq!(b.delay(), MIN);
}

#[test]
fn backoff_doubles_each_failure_and_caps_at_sixty_minutes() {
    let clock = ManualClock::new();
    let mut b = Backoff::new();
    let mut seen = Vec::new();
    for _ in 0..9 {
        b.failed(clock.now());
        seen.push(b.delay());
    }
    let want: Vec<Duration> = [1, 2, 4, 8, 16, 32, 60, 60, 60]
        .iter()
        .map(|m| MIN * *m)
        .collect();
    assert_eq!(seen, want);
}

#[test]
fn backoff_is_not_due_before_the_delay_on_the_manual_clock() {
    let clock = ManualClock::new();
    let mut b = Backoff::new();
    assert!(b.due(clock.now()));
    b.failed(clock.now());
    assert!(!b.due(clock.now()));
    clock.advance(MIN - Duration::from_secs(1));
    assert!(!b.due(clock.now()));
    clock.advance(Duration::from_secs(1));
    assert!(b.due(clock.now()));
}

#[test]
fn backoff_success_resets_to_due_now() {
    let clock = ManualClock::new();
    let mut b = Backoff::new();
    b.failed(clock.now());
    b.failed(clock.now());
    b.succeeded();
    assert!(b.due(clock.now()));
    assert_eq!(b.delay(), Duration::ZERO);
    b.failed(clock.now());
    assert_eq!(b.delay(), MIN);
}
