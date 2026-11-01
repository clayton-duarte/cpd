//! Red tests of `Background::with_waker`: a finished job wakes the loop (no sleeps, no real-time waits).
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use ratatui::crossterm::event::Event;

use crate::background::Background;
use crate::pump::{Pump, Signal};

fn fake() -> (mpsc::Sender<Event>, Pump) {
    let (tx, rx) = mpsc::channel::<Event>();
    let pump = Pump::new(move || rx.recv().map_err(io::Error::other));
    (tx, pump)
}

#[test]
fn a_finished_job_posts_a_read_signal_through_the_waker() {
    let (input, pump) = fake();
    let mut background = Background::with_waker(pump.waker());
    background.submit(|| 7);
    assert!(matches!(pump.wait(None), Signal::Read));
    assert_eq!(background.finished(), Some(7));
    drop(input);
}

#[test]
fn jobs_run_one_at_a_time_in_order_and_each_posts_one_signal() {
    let (input, pump) = fake();
    let mut background = Background::with_waker(pump.waker());
    let (release, hold) = mpsc::channel::<()>();
    let second_started = Arc::new(AtomicBool::new(false));
    background.submit(move || {
        hold.recv().unwrap();
        1
    });
    let flag = second_started.clone();
    background.submit(move || {
        flag.store(true, Ordering::SeqCst);
        2
    });
    assert_eq!(background.finished(), None);
    assert!(!second_started.load(Ordering::SeqCst));
    release.send(()).unwrap();
    assert!(matches!(pump.wait(None), Signal::Read));
    assert_eq!(background.finished(), Some(1));
    assert!(matches!(pump.wait(None), Signal::Read));
    assert_eq!(background.finished(), Some(2));
    assert!(matches!(pump.wait(Some(Instant::now())), Signal::Due));
    drop(input);
}

#[test]
fn submit_returns_before_the_job_ends() {
    let (input, pump) = fake();
    let mut background = Background::with_waker(pump.waker());
    let (release, hold) = mpsc::channel::<()>();
    background.submit(move || {
        hold.recv().unwrap();
        1
    });
    assert_eq!(background.finished(), None);
    release.send(()).unwrap();
    assert!(matches!(pump.wait(None), Signal::Read));
    assert_eq!(background.finished(), Some(1));
    drop(input);
}
