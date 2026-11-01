//! The scripted pump, the inline reads and the three loops that run on them (plan, widgets and history): one thread, the
//! `ManualClock` and a `TestBackend`; no sleep, no wall clock, no watcher, no process.

use std::ffi::OsString;
use std::io;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
};

use crate::background::Background;
use crate::pace::Reads;
use crate::pump::{FRAME, Feed, Pump, SAFETY, Schedule, Signal};
use crate::view_ports::{Ctx, Input};
use crate::{ViewError, history, plan, widgets};
use cpd_core::clock::Clock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::Fakes;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn press(code: KeyCode) -> Signal {
    Signal::Input(Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }))
}

fn gone() -> Signal {
    Signal::InputFailed(io::Error::other("gone"))
}

/// The input of a loop that does not read it (a scripted pump carries the events): any read is a bug of the test.
struct NoInput;

impl Input for NoInput {
    fn poll(&mut self, _: Duration) -> io::Result<bool> {
        panic!("the loop polls ctx.input: its events come from the pump");
    }
    fn read(&mut self) -> io::Result<Event> {
        panic!("the loop reads ctx.input: its events come from the pump");
    }
}

fn kind_of(signal: &Signal) -> &'static str {
    match signal {
        Signal::Input(_) => "input",
        Signal::InputFailed(_) => "failed",
        Signal::Commit => "commit",
        Signal::Read => "read",
        Signal::Due => "due",
    }
}

// ---- the pump ----------------------------------------------------------------------------------------------------------

#[test]
fn the_scripted_pump_yields_the_scripted_signals_in_order_without_a_nap() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(
        fakes.ports.clock.clone(),
        vec![
            press(KeyCode::Char('a')),
            Signal::Commit,
            Signal::Read,
            gone(),
        ],
    );
    let until = Some(fakes.clock.now() + ms(500));
    let seen: Vec<_> = (0..4).map(|_| kind_of(&pump.wait(until))).collect();
    assert_eq!(seen, ["input", "commit", "read", "failed"]);
    assert_eq!(fakes.clock.naps(), Vec::<Duration>::new());
}

#[test]
fn a_scripted_pump_with_nothing_left_advances_the_clock_to_the_time_asked_and_never_past_it() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let start = fakes.clock.now();
    assert!(matches!(pump.wait(Some(start + ms(500))), Signal::Due));
    assert_eq!(fakes.clock.now(), start + ms(500));
    // the time has passed: due at once, the clock stays
    assert!(matches!(pump.wait(Some(start + ms(300))), Signal::Due));
    assert_eq!(fakes.clock.now(), start + ms(500));
    // a signal first: the clock does not move
    let pump = Pump::scripted(fakes.ports.clock.clone(), vec![Signal::Commit]);
    assert!(matches!(pump.wait(Some(start + ms(900))), Signal::Commit));
    assert_eq!(fakes.clock.now(), start + ms(500));
}

#[test]
fn a_commit_the_test_announces_wakes_the_scripted_pump_before_the_script() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), vec![press(KeyCode::Char('a'))]);
    pump.waker().commit();
    assert!(matches!(pump.wait(None), Signal::Commit));
    assert!(matches!(pump.wait(None), Signal::Input(_)));
}

#[test]
#[should_panic(expected = "the script is over and the loop waits for a signal")]
fn a_scripted_pump_with_nothing_left_and_no_time_to_wait_for_says_the_script_did_not_end_the_loop()
{
    let fakes = Fakes::new();
    Pump::scripted(fakes.ports.clock.clone(), Vec::new()).wait(None);
}

#[test]
#[should_panic(expected = "the script is over and the loop still waits")]
fn a_scripted_pump_that_is_due_for_ever_says_the_script_did_not_end_the_loop() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    loop {
        pump.wait(Some(fakes.clock.now() + FRAME));
    }
}

#[test]
fn an_idle_scripted_pump_makes_one_read_per_safety_interval_of_virtual_time() {
    let fakes = Fakes::new();
    let feed = Feed::scripted(fakes.ports.clock.clone(), Vec::new());
    let mut schedule = Schedule::new(fakes.clock.now(), SAFETY);
    let start = fakes.clock.now();
    for round in 1..=3u32 {
        assert!(matches!(
            feed.pump.wait(schedule.deadline(fakes.clock.now())),
            Signal::Due
        ));
        assert!(schedule.take(fakes.clock.now()).is_some());
        schedule.done();
        assert_eq!(fakes.clock.now(), start + SAFETY * round);
    }
}

#[test]
fn a_scripted_feed_has_no_watcher() {
    let fakes = Fakes::new();
    assert!(matches!(
        Feed::scripted(fakes.ports.clock.clone(), Vec::new()).watch,
        Ok(None)
    ));
}

// ---- the inline reads and background jobs ---------------------------------------------------------------------------

#[test]
fn an_inline_read_is_finished_when_request_returns_and_wakes_the_pump() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let mut reads: Reads<u32> = Reads::on(&pump);
    assert_eq!(reads.take(), None);
    reads.request(|| 7);
    assert_eq!(reads.take(), Some(7));
    assert!(matches!(pump.wait(None), Signal::Read));
}

#[test]
fn inline_reads_run_in_the_caller_and_the_newest_result_is_the_last() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let mut reads: Reads<std::thread::ThreadId> = Reads::on(&pump);
    reads.request(|| std::thread::current().id());
    reads.request(|| std::thread::current().id());
    assert_eq!(
        (reads.take(), reads.take(), reads.take()),
        (
            Some(std::thread::current().id()),
            Some(std::thread::current().id()),
            None
        )
    );
    let mut counted: Reads<u32> = Reads::on(&pump);
    for n in 1..=3 {
        counted.request(move || n);
    }
    let taken: Vec<u32> = std::iter::from_fn(|| counted.take()).collect();
    assert_eq!(
        taken.last(),
        Some(&3),
        "what the loop ends on is the newest read: {taken:?}"
    );
}

#[test]
fn the_production_pump_is_not_inline() {
    // the production pump has an input thread; its reads are the reader thread's, so `request` returns before the job ran
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let pump = Pump::new(move || rx.recv().map_err(io::Error::other));
    assert!(!pump.inline());
    drop(tx);
}

#[test]
fn an_inline_background_job_is_finished_when_submit_returns_and_the_next_starts_after_it_is_taken()
{
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let mut background: Background<u32> = Background::on(&pump);
    background.submit(|| 1);
    background.submit(|| 2);
    assert_eq!(
        (
            background.finished(),
            background.finished(),
            background.finished()
        ),
        (Some(1), Some(2), None)
    );
    assert!(matches!(pump.wait(None), Signal::Read));
}

// ---- the loops ----------------------------------------------------------------------------------------------------------

fn home() -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    // the schema, as `cpd` makes it at the first open
    drop(cpd_core::store::Store::open(&home, 5000).unwrap());
    (dir, home)
}

fn no_command(_: Vec<OsString>) -> Result<Output, CpdError> {
    Err(CpdError::new(
        ErrorCode::StoreBusy,
        "the smoke test runs no command",
    ))
}

fn ctx<'a>(fakes: &'a Fakes, input: &'a mut NoInput) -> Ctx<'a, TestBackend> {
    Ctx::new(
        &fakes.ports,
        Terminal::new(TestBackend::new(60, 14)).unwrap(),
        input,
    )
}

/// The loop ends with the terminal's failure the script ends it with, after the keys: neither view has a key that ends it.
fn assert_ended_by_the_script(result: Result<(), ViewError>) {
    match result {
        Err(ViewError::Terminal(e)) => assert_eq!(e.to_string(), "gone"),
        Err(ViewError::Cpd(e)) => panic!("the view's own work failed: {}", e.message()),
        Ok(()) => panic!("the loop ended with no terminal failure: no key ends it"),
    }
}

#[test]
fn plan_show_with_draws_runs_its_reads_inline_and_ends_when_the_terminal_fails() {
    let fakes = Fakes::new();
    let (_dir, home) = home();
    let prepared = plan::prepare(&fakes.ports, home, no_command, || Ok(60)).unwrap();
    let feed = Feed::scripted(
        fakes.ports.clock.clone(),
        vec![press(KeyCode::Char('q')), Signal::Commit, gone()],
    );
    let mut input = NoInput;
    let mut ctx = ctx(&fakes, &mut input);
    assert_ended_by_the_script(plan::show_with(&mut ctx, feed, prepared));
    assert!(ctx.draws() >= 1, "drew {} frames", ctx.draws());
}

#[test]
fn widgets_show_with_draws_runs_its_reads_inline_and_ends_when_the_terminal_fails() {
    let fakes = Fakes::new();
    let (_dir, home) = home();
    let prepared = widgets::prepare(&fakes.ports, home).unwrap();
    let feed = Feed::scripted(
        fakes.ports.clock.clone(),
        vec![press(KeyCode::Char('q')), Signal::Commit, gone()],
    );
    let mut input = NoInput;
    let mut ctx = ctx(&fakes, &mut input);
    assert_ended_by_the_script(widgets::show_with(&mut ctx, feed, prepared, no_command));
    assert!(ctx.draws() >= 1, "drew {} frames", ctx.draws());
}

#[test]
fn history_show_with_leaves_on_q_and_drew_at_least_once() {
    struct Keys(Vec<Event>);
    impl Input for Keys {
        fn poll(&mut self, _: Duration) -> io::Result<bool> {
            Ok(true)
        }
        fn read(&mut self) -> io::Result<Event> {
            Ok(self.0.remove(0))
        }
    }
    let fakes = Fakes::new();
    let Signal::Input(q) = press(KeyCode::Char('q')) else {
        unreachable!()
    };
    let mut input = Keys(vec![q]);
    let mut ctx = Ctx::new(
        &fakes.ports,
        Terminal::new(TestBackend::new(60, 14)).unwrap(),
        &mut input,
    );
    history::show_with(&mut ctx, "old lead", true, "plain text").unwrap();
    assert!(ctx.draws() >= 1);
}
