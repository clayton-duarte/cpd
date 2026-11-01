//! Red tests of the pump, the schedule and the clock needs (time is an argument; no sleeps, no real clock waits).
use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::Event;

use crate::pace::Reads;
use crate::pump::{wake, Needs, Pump, ReadKind, Schedule, Signal, FRAME, SAFETY};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn fake() -> (mpsc::Sender<Event>, Pump) {
    let (tx, rx) = mpsc::channel::<Event>();
    let pump = Pump::new(move || rx.recv().map_err(io::Error::other));
    (tx, pump)
}

#[test]
fn a_read_is_wanted_after_a_commit_and_taken_at_once_when_none_ran() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    s.commit();
    assert_eq!(s.take(t0), Some(ReadKind { force: false }));
}

#[test]
fn nothing_is_taken_without_a_commit_before_the_safety_interval() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    assert_eq!(s.take(t0), None);
    assert_eq!(s.take(t0 + ms(1999)), None);
}

#[test]
fn the_safety_interval_takes_a_read_that_does_not_force_a_snapshot() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    assert_eq!(s.take(t0 + SAFETY), Some(ReadKind { force: false }));
}

#[test]
fn a_commit_within_a_frame_of_the_last_read_waits_for_the_frame_to_end() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    s.commit();
    assert!(s.take(t0).is_some());
    s.done();
    s.commit();
    assert_eq!(s.take(t0 + ms(25)), None);
    assert_eq!(s.take(t0 + ms(49)), None);
    assert_eq!(s.take(t0 + FRAME), Some(ReadKind { force: false }));
}

#[test]
fn commits_while_a_read_is_out_make_one_read_after_it_is_done() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    s.commit();
    assert!(s.take(t0).is_some());
    s.commit();
    s.commit();
    s.done();
    assert!(s.take(t0 + ms(200)).is_some());
    s.done();
    assert_eq!(s.take(t0 + ms(400)), None);
}

#[test]
fn no_second_read_is_taken_while_one_is_out() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    s.commit();
    assert!(s.take(t0).is_some());
    s.commit();
    s.force();
    assert_eq!(s.take(t0 + ms(500)), None);
    assert_eq!(s.take(t0 + SAFETY * 2), None);
}

#[test]
fn a_burst_of_a_hundred_commits_in_a_second_takes_twenty_reads_at_most() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    let mut taken = 0;
    for step in 0..1000u64 {
        if step % 10 == 0 {
            s.commit();
        }
        if s.take(t0 + ms(step)).is_some() {
            taken += 1;
            s.done();
        }
    }
    assert!(taken >= 1, "a burst still reads");
    assert!(taken > 10, "took {taken} reads: one per 50 ms frame");
    assert!(taken <= 20, "took {taken} reads in one second");
}

#[test]
fn the_read_pace_and_the_frame_clock_are_one_50_ms_frame() {
    assert_eq!(FRAME, ms(50));
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    s.commit();
    assert!(s.take(t0).is_some());
    s.done();
    s.commit();
    assert_eq!(s.take(t0 + ms(49)), None);
    assert_eq!(s.take(t0 + ms(50)), Some(ReadKind { force: false }));
}

#[test]
fn force_takes_a_read_that_forces_a_snapshot() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    s.force();
    assert_eq!(s.take(t0), Some(ReadKind { force: true }));
    s.done();
    assert_eq!(s.take(t0 + ms(500)), None);
}

#[test]
fn the_deadline_is_none_while_a_read_is_out_and_the_safety_time_when_idle() {
    let t0 = Instant::now();
    let mut s = Schedule::new(t0, SAFETY);
    assert_eq!(s.deadline(t0), Some(t0 + SAFETY));
    s.commit();
    assert!(s.take(t0).is_some());
    assert_eq!(s.deadline(t0), None);
    s.done();
    s.commit();
    assert_eq!(s.deadline(t0 + ms(10)), Some(t0 + FRAME));
}

#[test]
fn the_pump_returns_input_commit_and_read_signals_in_arrival_order() {
    let (tx, pump) = fake();
    let waker = pump.waker();
    tx.send(Event::FocusGained).unwrap();
    assert!(matches!(pump.wait(None), Signal::Input(Event::FocusGained)));
    waker.commit();
    waker.read_done();
    waker.commit();
    assert!(matches!(pump.wait(None), Signal::Commit));
    assert!(matches!(pump.wait(None), Signal::Read));
    assert!(matches!(pump.wait(None), Signal::Commit));
    drop(tx);
}

#[test]
fn wait_returns_due_when_the_deadline_has_passed_and_nothing_is_queued() {
    let (tx, pump) = fake();
    assert!(matches!(pump.wait(Some(Instant::now())), Signal::Due));
    drop(tx);
}

#[test]
fn a_waker_does_nothing_after_the_pump_is_dropped() {
    let (tx, pump) = fake();
    let waker = pump.waker();
    drop(pump);
    waker.commit();
    waker.read_done();
    drop(tx);
}

#[test]
fn a_failing_input_source_posts_input_failed_and_ends_the_thread() {
    let pump = Pump::new(|| Err(io::Error::other("gone")));
    assert!(matches!(pump.wait(None), Signal::InputFailed(_)));
}

#[test]
fn a_finished_read_wakes_the_pump_with_a_read_signal() {
    let (tx, pump) = fake();
    let mut reads: Reads<u32> = Reads::with_waker(pump.waker());
    reads.request(|| 7);
    assert!(matches!(pump.wait(None), Signal::Read));
    assert_eq!(reads.take(), Some(7));
    drop(reads);
    drop(tx);
}

#[test]
fn no_need_and_no_deadline_waits_for_a_signal_only() {
    let needs = Needs { frame: false, second: false, at: None };
    assert_eq!(wake(Instant::now(), 1234, &needs, None), None);
}

#[test]
fn a_frame_need_wakes_at_the_next_50_ms_boundary_of_the_wall_clock() {
    let now = Instant::now();
    let needs = Needs { frame: true, second: false, at: None };
    assert_eq!(wake(now, 1234, &needs, None), Some(now + ms(16)));
}

#[test]
fn a_second_need_wakes_at_the_next_whole_second_of_the_wall_clock() {
    let now = Instant::now();
    let needs = Needs { frame: false, second: true, at: None };
    assert_eq!(wake(now, 1234, &needs, None), Some(now + ms(766)));
}

#[test]
fn the_earliest_of_the_deadline_the_needs_and_the_extra_time_wins() {
    let now = Instant::now();
    let all = Needs { frame: true, second: true, at: Some(now + ms(10)) };
    assert_eq!(wake(now, 1234, &all, Some(now + ms(900))), Some(now + ms(10)));
    let no_at = Needs { frame: true, second: true, at: Some(now + ms(500)) };
    assert_eq!(wake(now, 1234, &no_at, Some(now + ms(900))), Some(now + ms(16)));
    let slow = Needs { frame: false, second: true, at: Some(now + ms(800)) };
    assert_eq!(wake(now, 1234, &slow, Some(now + ms(20))), Some(now + ms(20)));
}
