//! `Clock`, `SystemClock` and `ManualClock` (docs/implementing.md, "Time in tests").

use std::time::Duration;

use crate::clock::{Clock, ManualClock, SystemClock};

const MS: fn(u64) -> Duration = Duration::from_millis;

#[test]
fn a_manual_clock_sleep_advances_now_by_exactly_the_nap() {
    let clock = ManualClock::new();
    let before = clock.now();
    clock.sleep(MS(250));
    assert_eq!(clock.now() - before, MS(250));
}

#[test]
fn a_manual_clock_sleep_records_the_nap() {
    let clock = ManualClock::new();
    clock.sleep(MS(20));
    assert_eq!(clock.naps(), vec![MS(20)]);
}

#[test]
fn a_manual_clock_sleep_counts_in_elapsed() {
    let clock = ManualClock::new();
    clock.sleep(MS(500));
    clock.sleep(MS(1000));
    assert_eq!(clock.elapsed(), MS(1500));
}

#[test]
fn a_new_manual_clock_has_no_elapsed_time_and_no_naps() {
    let clock = ManualClock::new();
    assert_eq!(clock.elapsed(), Duration::ZERO);
    assert!(clock.naps().is_empty());
}

#[test]
fn a_manual_clock_advance_moves_now_and_elapsed() {
    let clock = ManualClock::new();
    let before = clock.now();
    clock.advance(Duration::from_secs(3));
    assert_eq!(clock.now() - before, Duration::from_secs(3));
    assert_eq!(clock.elapsed(), Duration::from_secs(3));
}

#[test]
fn a_manual_clock_advance_records_no_nap() {
    let clock = ManualClock::new();
    clock.advance(Duration::from_secs(3));
    assert!(clock.naps().is_empty());
}

#[test]
fn a_manual_clock_keeps_the_naps_in_order() {
    let clock = ManualClock::new();
    for ms in [500, 1000, 20, 2000] {
        clock.sleep(MS(ms));
    }
    assert_eq!(clock.naps(), vec![MS(500), MS(1000), MS(20), MS(2000)]);
}

#[test]
fn a_manual_clock_clone_shares_the_state() {
    let clock = ManualClock::new();
    let other = clock.clone();
    let before = clock.now();
    other.sleep(MS(40));
    other.advance(MS(60));
    assert_eq!(clock.naps(), vec![MS(40)]);
    assert_eq!(clock.elapsed(), MS(100));
    assert_eq!(clock.now() - before, MS(100));
}

#[test]
fn the_system_clock_now_is_monotonic() {
    let first = SystemClock.now();
    let second = SystemClock.now();
    assert!(second >= first);
}

#[test]
fn the_system_clock_sleeps_for_zero_and_returns() {
    SystemClock.sleep(Duration::ZERO);
}
