//! `wake::next_tick`: pure, time is an argument.

use std::time::{Duration, Instant};

use super::wake::{EARLY_GAP, next_tick};

const TICK: Duration = Duration::from_secs(5);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn with_no_commit_the_next_tick_is_a_tick_after_the_last_one_began() {
    let t0 = Instant::now();
    assert_eq!(next_tick(t0, t0 + ms(100), TICK, None, false), t0 + TICK);
}

#[test]
fn a_commit_brings_the_next_tick_to_half_a_second_after_the_last_one_began() {
    let t0 = Instant::now();
    assert_eq!(EARLY_GAP, ms(500));
    assert_eq!(
        next_tick(t0, t0 + ms(100), TICK, None, true),
        t0 + EARLY_GAP
    );
}

#[test]
fn a_commit_after_the_gap_has_passed_ticks_at_once() {
    let t0 = Instant::now();
    let now = t0 + ms(1300);
    assert_eq!(next_tick(t0, now, TICK, None, true), now);
}

#[test]
fn a_poll_due_before_the_tick_brings_the_tick_to_its_due_time() {
    let t0 = Instant::now();
    let due = t0 + ms(2000);
    assert_eq!(next_tick(t0, t0 + ms(100), TICK, Some(due), false), due);
    assert_eq!(
        next_tick(t0, t0 + ms(100), TICK, Some(t0 + TICK + ms(1)), false),
        t0 + TICK
    );
}

#[test]
fn a_running_poll_has_no_due_time_and_brings_no_tick() {
    let t0 = Instant::now();
    assert_eq!(next_tick(t0, t0 + ms(100), TICK, None, false), t0 + TICK);
    assert_eq!(next_tick(t0, t0 + ms(4000), TICK, None, false), t0 + TICK);
}

#[test]
fn a_tick_shorter_than_the_gap_is_never_delayed_by_a_commit() {
    let t0 = Instant::now();
    let tick = ms(200);
    assert_eq!(next_tick(t0, t0 + ms(50), tick, None, true), t0 + tick);
}

#[test]
fn ten_thousand_commits_in_a_minute_start_at_most_two_ticks_a_second() {
    let t0 = Instant::now();
    let end = t0 + Duration::from_secs(60);
    let mut began = t0;
    let mut now = t0;
    let mut starts = 0u32;
    // One commit every 6 ms is ten thousand a minute: `committed` is true at every nap.
    while now < end {
        now += ms(100);
        if now >= next_tick(began, now, TICK, None, true) {
            starts += 1;
            began = now;
        }
    }
    assert!(starts >= 1, "a commit must start a tick");
    assert!(starts <= 120, "{starts} ticks in a minute");
}
