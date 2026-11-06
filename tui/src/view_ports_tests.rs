//! Red tests of the view seams (`view_ports`, and the `_with` forms in each view family): a loop runs N frames from
//! scripted events on a `ManualClock`, a `TestBackend` and a `FakeSpawn`; no terminal, no sleep, no process, no wall clock.
//!
//! The doubles are here and are real code, not stubs: [`FakeInput`] (what `event::poll` and `event::read` give, a nap on
//! the clock for each poll that finds nothing), [`FakeTty`] (the raw-mode facts) and [`Shared`] (the injected writer).
//! Ported assertions (`old -> new`; the old tests stay until the implementation Job):
//! `tree::folds::tests::a_write_waits_a_second_after_the_last_and_unchanged_folds_write_nothing`
//!   -> `folds_a_change_within_a_second_of_the_last_write_waits_and_one_a_second_later_writes`,
//! `plan::tests::a_write_waits_a_second_after_the_last_unless_the_plan_changed_and_unchanged_offsets_write_nothing`
//!   -> `offsets_a_change_within_a_second_of_the_last_write_waits_unless_the_plan_changed`,
//! `merge::marker::tests::the_marker_is_one_file_per_pull_request_with_this_pid_and_an_rfc_3339_time_and_no_temporary_file_is_left`
//!   -> `a_merge_marker_is_stamped_by_the_clock`,
//! `merge::marker::tests::*` (`DEAD_PID` = a pid no process has) -> `a_marker_is_live_while_its_pid_is_alive_on_the_spawn`.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::Command;
use ratatui::crossterm::event::{
    DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event, KeyCode,
    KeyEvent, KeyEventKind, KeyEventState, KeyModifiers,
};
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::widgets::Paragraph;
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::dialog::{Dialog, Mode, Outcome};
use crate::merge::{self, Pull};
use crate::plan::Offsets;
use crate::pump::{Pump, Schedule, Signal};
use crate::read;
use crate::test_text::rows as buffer_rows;
use crate::tree::folds::{Folds, FoldsFile};
use crate::view_ports::{
    Ctx, Input, Restore, RunFn, Tty, backed_off_with, frame_now, run_view_with, snooze_over,
    wall_ms,
};
use crate::viewlog::ViewLog;
use crate::{Capture, Restart, RestartOutcome, ViewError, logs, palette, widgets};
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::host::runner::{FakeRunner, Reply};
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;

// ---- the doubles ---------------------------------------------------------------------------------------------------

/// One thing the scripted terminal does at a poll or a read.
enum Step {
    /// An event is there.
    Event(Event),
    /// Nothing comes before the poll's wait is over: the poll naps its wait on the clock and says no.
    Idle,
    /// The terminal fails with `kind`.
    Fail(io::ErrorKind),
    /// The world changes (a commit by another process) before the next step; takes no time.
    Do(Box<dyn FnMut()>),
}

/// The scripted [`Input`]. A script that is over is a loop that did not end: it panics, so the test fails loudly.
struct FakeInput {
    clock: Arc<ManualClock>,
    steps: VecDeque<Step>,
    /// The snapshots the loop told it of ([`Input::snapshot_read`]).
    snapshots: Rc<std::cell::Cell<usize>>,
}

impl FakeInput {
    fn new(fakes: &Fakes, steps: Vec<Step>) -> FakeInput {
        FakeInput {
            clock: Arc::clone(&fakes.clock),
            steps: steps.into(),
            snapshots: Rc::default(),
        }
    }
}

impl Input for FakeInput {
    fn snapshot_read(&mut self) {
        self.snapshots.set(self.snapshots.get() + 1);
    }

    fn poll(&mut self, wait: Duration) -> io::Result<bool> {
        loop {
            match self.steps.front_mut() {
                None => panic!("the script is over and the loop still polls"),
                Some(Step::Do(change)) => {
                    change();
                    self.steps.pop_front();
                }
                Some(Step::Idle) => {
                    self.steps.pop_front();
                    self.clock.sleep(wait);
                    return Ok(false);
                }
                Some(Step::Event(_)) => return Ok(true),
                Some(Step::Fail(kind)) => {
                    let kind = *kind;
                    self.steps.pop_front();
                    return Err(io::Error::new(kind, "scripted"));
                }
            }
        }
    }

    fn read(&mut self) -> io::Result<Event> {
        loop {
            match self.steps.pop_front() {
                None => panic!("the script is over and the loop still reads"),
                Some(Step::Do(mut change)) => change(),
                Some(Step::Event(event)) => return Ok(event),
                Some(Step::Fail(kind)) => return Err(io::Error::new(kind, "scripted")),
                Some(Step::Idle) => panic!(
                    "a read cannot be idle: the loop reads only what a poll said is there, or blocks"
                ),
            }
        }
    }
}

/// The facts of a terminal, scripted; each call is logged, shared with the test.
struct FakeTty {
    terminal: bool,
    log: Rc<RefCell<Vec<&'static str>>>,
}

impl FakeTty {
    fn new(terminal: bool) -> FakeTty {
        FakeTty {
            terminal,
            log: Rc::default(),
        }
    }
}

impl Tty for FakeTty {
    fn is_terminal(&self) -> bool {
        self.terminal
    }
    fn raw_on(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("raw_on");
        Ok(())
    }
    fn raw_off(&mut self) -> io::Result<()> {
        self.log.borrow_mut().push("raw_off");
        Ok(())
    }
}

/// The injected writer, readable by the test while the code under test holds it.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Shared {
    fn bytes(&self) -> Vec<u8> {
        self.0.borrow().clone()
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }
}

/// A writer that fails every write.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("broken"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("broken"))
    }
}

fn ansi(command: impl Command) -> Vec<u8> {
    let mut text = String::new();
    command.write_ansi(&mut text).unwrap();
    text.into_bytes()
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn key(code: KeyCode) -> Step {
    Step::Event(Event::Key(KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }))
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn idle(n: usize) -> Vec<Step> {
    (0..n).map(|_| Step::Idle).collect()
}

fn terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(60, 14)).unwrap()
}

fn screen(ctx: &Ctx<TestBackend>) -> String {
    buffer_rows(ctx.terminal.backend().buffer()).join("\n")
}

/// A temporary home with its config and its store's schema, which lives as long as the value.
struct Home {
    _dir: tempfile::TempDir,
    home: CpdHome,
}

fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    Home { _dir: dir, home }
}

impl Home {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }
}

fn sql(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// A second connection commits an Event: what another process does while a view polls.
fn commit_event(home: &CpdHome, session: &str) -> Step {
    let home = home.clone();
    let text = format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:02:00.000Z', 'session-moved', NULL, NULL, {session}, 'moved', '{{}}');"
    );
    Step::Do(Box::new(move || {
        sql(&mut Store::open(&home, 5000).unwrap(), &text)
    }))
}

fn strings(argv: &[OsString]) -> Vec<String> {
    argv.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

fn stamp(clock: &ManualClock) -> String {
    clock.utc().format(&Rfc3339).unwrap()
}

// ---- run_view_with: the terminal's set-up and its restoring -----------------------------------------------------------

/// What `run_view_with` runs on: the fakes, a terminal fact, the writer and an empty script.
struct Rig {
    fakes: Fakes,
    tty: FakeTty,
    out: Shared,
    input: FakeInput,
}

fn rig(terminal: bool) -> Rig {
    let fakes = Fakes::new();
    let input = FakeInput::new(&fakes, Vec::new());
    Rig {
        fakes,
        tty: FakeTty::new(terminal),
        out: Shared::default(),
        input,
    }
}

impl Rig {
    fn run<P>(
        &mut self,
        capture: Capture,
        prepare: impl FnOnce() -> Result<P, CpdError>,
        body: impl FnOnce(&mut Ctx<TestBackend>, P) -> Result<(), ViewError>,
    ) -> Result<Output, CpdError> {
        run_view_with(
            &self.fakes.ports,
            &mut self.tty,
            &mut self.out,
            terminal(),
            &mut self.input,
            "tree",
            capture,
            prepare,
            body,
        )
    }
}

/// The terminal is back as it was: raw mode on then off, the reporting `capture` turned on is turned off before the
/// alternate screen is left, which is the last thing written.
fn assert_restored(out: &Shared, tty: &FakeTty, capture: Capture) {
    let bytes = out.bytes();
    assert_eq!(*tty.log.borrow(), ["raw_on", "raw_off"]);
    assert!(
        bytes.ends_with(&ansi(LeaveAlternateScreen)),
        "the alternate screen is left last: {:?}",
        out.text()
    );
    let left = position(&bytes, &ansi(LeaveAlternateScreen)).unwrap();
    let off = |command: Vec<u8>| position(&bytes, &command);
    match capture {
        Capture::Keys => assert_eq!(
            (
                off(ansi(DisableMouseCapture)),
                off(ansi(DisableFocusChange))
            ),
            (None, None)
        ),
        Capture::Mouse => {
            assert!(
                off(ansi(DisableMouseCapture)).is_some_and(|at| at < left),
                "{:?}",
                out.text()
            );
            assert_eq!(off(ansi(DisableFocusChange)), None);
        }
        Capture::MouseAndFocus => {
            assert!(
                off(ansi(DisableMouseCapture)).is_some_and(|at| at < left),
                "{:?}",
                out.text()
            );
            assert!(
                off(ansi(DisableFocusChange)).is_some_and(|at| at < left),
                "{:?}",
                out.text()
            );
        }
    }
}

#[test]
fn a_view_without_a_terminal_is_refused_not_a_terminal_naming_its_command() {
    let mut rig = rig(false);
    let Err(e) = rig.run(Capture::Mouse, || Ok(()), |_, ()| Ok(())) else {
        panic!("refused")
    };
    assert_eq!(e.code(), ErrorCode::NotATerminal);
    assert_eq!(
        e.message(),
        "cpd tui view tree needs a terminal on stdin and stdout."
    );
    assert_eq!(e.details()["command"], "tui view tree");
}

#[test]
fn a_refused_view_prepares_nothing_and_writes_nothing() {
    let mut rig = rig(false);
    let prepared = RefCell::new(false);
    let _ = rig.run(
        Capture::Mouse,
        || {
            *prepared.borrow_mut() = true;
            Ok(())
        },
        |_, ()| Ok(()),
    );
    assert!(!*prepared.borrow(), "prepare ran");
    assert_eq!(
        (rig.out.bytes(), rig.tty.log.borrow().clone()),
        (Vec::new(), Vec::new())
    );
}

#[test]
fn a_failed_prepare_is_the_commands_error_and_the_screen_stays_untouched() {
    let mut rig = rig(true);
    let Err(e) = rig.run(
        Capture::Mouse,
        || Err::<(), _>(CpdError::new(ErrorCode::StoreBusy, "busy")),
        |_, ()| Ok(()),
    ) else {
        panic!("failed")
    };
    assert_eq!((e.code(), e.message()), (ErrorCode::StoreBusy, "busy"));
    assert_eq!(
        (rig.out.bytes(), rig.tty.log.borrow().clone()),
        (Vec::new(), Vec::new())
    );
}

#[test]
fn the_body_runs_in_raw_mode_on_the_alternate_screen() {
    let mut rig = rig(true);
    let (log, out) = (Rc::clone(&rig.tty.log), rig.out.clone());
    rig.run(
        Capture::Keys,
        || Ok(()),
        |_, ()| {
            assert_eq!(*log.borrow(), ["raw_on"]);
            assert!(
                out.bytes().starts_with(&ansi(EnterAlternateScreen)),
                "{:?}",
                out.text()
            );
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn capture_keys_turns_no_reporting_on_and_none_off() {
    let mut rig = rig(true);
    let out = rig.out.clone();
    rig.run(
        Capture::Keys,
        || Ok(()),
        |_, ()| {
            assert_eq!(
                (
                    position(&out.bytes(), &ansi(EnableMouseCapture)),
                    position(&out.bytes(), &ansi(EnableFocusChange))
                ),
                (None, None)
            );
            Ok(())
        },
    )
    .unwrap();
    assert_restored(&rig.out, &rig.tty, Capture::Keys);
}

#[test]
fn capture_mouse_turns_the_mouse_on_for_the_body_and_off_after() {
    let mut rig = rig(true);
    let out = rig.out.clone();
    rig.run(
        Capture::Mouse,
        || Ok(()),
        |_, ()| {
            assert!(
                position(&out.bytes(), &ansi(EnableMouseCapture)).is_some(),
                "{:?}",
                out.text()
            );
            assert_eq!(position(&out.bytes(), &ansi(EnableFocusChange)), None);
            Ok(())
        },
    )
    .unwrap();
    assert_restored(&rig.out, &rig.tty, Capture::Mouse);
}

#[test]
fn capture_mouse_and_focus_turns_both_on_for_the_body_and_off_after() {
    let mut rig = rig(true);
    let out = rig.out.clone();
    rig.run(
        Capture::MouseAndFocus,
        || Ok(()),
        |_, ()| {
            let bytes = out.bytes();
            assert!(
                position(&bytes, &ansi(EnableMouseCapture)).is_some()
                    && position(&bytes, &ansi(EnableFocusChange)).is_some(),
                "{:?}",
                out.text()
            );
            Ok(())
        },
    )
    .unwrap();
    assert_restored(&rig.out, &rig.tty, Capture::MouseAndFocus);
}

#[test]
fn a_body_that_ends_ok_is_owned_and_the_terminal_is_restored() {
    let mut rig = rig(true);
    let output = rig.run(Capture::Mouse, || Ok(()), |_, ()| Ok(())).unwrap();
    assert!(matches!(output, Output::Owned));
    assert_restored(&rig.out, &rig.tty, Capture::Mouse);
}

#[test]
fn a_body_that_fails_with_its_own_error_returns_it_as_it_is_and_the_terminal_is_restored() {
    let mut rig = rig(true);
    let Err(e) = rig.run(
        Capture::Mouse,
        || Ok(()),
        |_, ()| Err(CpdError::new(ErrorCode::StoreBusy, "the store is busy").into()),
    ) else {
        panic!("failed")
    };
    assert_eq!(
        (e.code(), e.message()),
        (ErrorCode::StoreBusy, "the store is busy")
    );
    assert_restored(&rig.out, &rig.tty, Capture::Mouse);
}

#[test]
fn a_terminal_failure_of_the_body_is_not_a_terminal_naming_the_view_and_the_terminal_is_restored() {
    let mut rig = rig(true);
    let Err(e) = rig.run(
        Capture::Mouse,
        || Ok(()),
        |_, ()| Err(io::Error::other("boom").into()),
    ) else {
        panic!("failed")
    };
    assert_eq!(e.code(), ErrorCode::NotATerminal);
    assert_eq!(
        e.message(),
        "The terminal of cpd tui view tree failed: boom."
    );
    assert_restored(&rig.out, &rig.tty, Capture::Mouse);
}

#[test]
fn a_body_that_panics_still_restores_the_terminal() {
    let mut rig = rig(true);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        let _ = rig.run(
            Capture::MouseAndFocus,
            || Ok(()),
            |_, ()| panic!("the view broke"),
        );
    }));
    assert!(caught.is_err(), "the panic goes on");
    assert_restored(&rig.out, &rig.tty, Capture::MouseAndFocus);
}

#[test]
fn a_restore_writes_the_resets_of_its_capture_to_the_injected_writer_and_turns_raw_mode_off() {
    let (mut tty, mut out) = (FakeTty::new(true), Shared::default());
    drop(Restore::new(Capture::Mouse, &mut tty, &mut out));
    assert_eq!(*tty.log.borrow(), ["raw_off"]);
    let mut want = ansi(DisableMouseCapture);
    want.extend(ansi(LeaveAlternateScreen));
    assert_eq!(out.bytes(), want);
}

#[test]
fn a_restore_whose_writer_fails_does_not_panic_and_still_turns_raw_mode_off() {
    let (mut tty, mut out) = (FakeTty::new(true), Broken);
    drop(Restore::new(Capture::MouseAndFocus, &mut tty, &mut out));
    assert_eq!(*tty.log.borrow(), ["raw_off"]);
}

// ---- Ctx: the frames a loop draws -----------------------------------------------------------------------------------

#[test]
fn a_context_counts_the_frames_it_draws() {
    let fakes = Fakes::new();
    let mut input = FakeInput::new(&fakes, Vec::new());
    let mut ctx = Ctx::new(&fakes.ports, terminal(), &mut input);
    assert_eq!(ctx.draws(), 0);
    ctx.draw(|f| f.render_widget(Paragraph::new("one"), f.area()))
        .unwrap();
    ctx.draw(|f| f.render_widget(Paragraph::new("two"), f.area()))
        .unwrap();
    assert_eq!(ctx.draws(), 2);
}

#[test]
fn a_context_draws_on_its_terminal() {
    let fakes = Fakes::new();
    let mut input = FakeInput::new(&fakes, Vec::new());
    let mut ctx = Ctx::new(&fakes.ports, terminal(), &mut input);
    ctx.draw(|f| f.render_widget(Paragraph::new("hello"), f.area()))
        .unwrap();
    assert!(screen(&ctx).starts_with("hello"), "{}", screen(&ctx));
}

// ---- the back-off of a failed view (3 s) ----------------------------------------------------------------------------

fn failed() -> Result<Output, CpdError> {
    Err(CpdError::new(ErrorCode::StoreBusy, "the store is busy"))
}

#[test]
fn a_failed_view_in_a_pane_says_why_and_waits_three_seconds_on_the_clock() {
    let fakes = Fakes::new();
    let mut err = Vec::new();
    let result = backed_off_with(&fakes.ports, true, &mut err, failed());
    assert_eq!(
        result.unwrap_err().code(),
        ErrorCode::StoreBusy,
        "the error is returned as it was"
    );
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "the store is busy\nStarting again in 3 s.\n"
    );
    assert_eq!(fakes.clock.naps(), [Duration::from_secs(3)]);
}

#[test]
fn a_failed_view_without_a_terminal_returns_at_once_and_says_nothing() {
    let fakes = Fakes::new();
    let mut err = Vec::new();
    assert!(backed_off_with(&fakes.ports, false, &mut err, failed()).is_err());
    assert_eq!((err, fakes.clock.naps()), (Vec::new(), Vec::new()));
}

#[test]
fn a_view_that_ended_well_does_not_wait() {
    let fakes = Fakes::new();
    let mut err = Vec::new();
    assert!(matches!(
        backed_off_with(&fakes.ports, true, &mut err, Ok(Output::Owned)),
        Ok(Output::Owned)
    ));
    assert_eq!((err, fakes.clock.naps()), (Vec::new(), Vec::new()));
}

// ---- the pump's wait (the interval) -----------------------------------------------------------------------------------

#[test]
fn a_pump_with_nothing_signalled_naps_to_its_deadline_on_the_clock_and_is_due() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let signal = pump.wait(Some(fakes.clock.now() + ms(500)));
    assert!(matches!(signal, Signal::Due));
    assert_eq!(fakes.clock.naps(), [ms(500)]);
}

#[test]
fn a_pump_naps_only_what_is_left_of_the_interval() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let until = fakes.clock.now() + ms(500);
    fakes.clock.advance(ms(200));
    pump.wait(Some(until));
    assert_eq!(fakes.clock.naps(), [ms(300)]);
}

#[test]
fn a_pump_whose_deadline_has_passed_does_not_nap() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let until = fakes.clock.now() + ms(100);
    fakes.clock.advance(ms(150));
    assert!(matches!(pump.wait(Some(until)), Signal::Due));
    assert_eq!(fakes.clock.naps(), Vec::<Duration>::new());
}

#[test]
fn a_signalled_pump_returns_its_signals_in_order_without_a_nap() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(
        fakes.ports.clock.clone(),
        vec![Signal::Commit, Signal::Read],
    );
    let until = Some(fakes.clock.now() + ms(500));
    assert!(matches!(pump.wait(until), Signal::Commit));
    assert!(matches!(pump.wait(until), Signal::Read));
    assert_eq!(fakes.clock.naps(), Vec::<Duration>::new());
}

#[test]
fn a_failed_input_reaches_the_loop_as_a_signal() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(
        fakes.ports.clock.clone(),
        vec![Signal::InputFailed(io::Error::other("gone"))],
    );
    let Signal::InputFailed(e) = pump.wait(None) else {
        panic!("the failure")
    };
    assert_eq!(e.to_string(), "gone");
}

#[test]
fn an_idle_pump_reads_once_per_safety_interval_of_virtual_time() {
    let fakes = Fakes::new();
    let pump = Pump::scripted(fakes.ports.clock.clone(), Vec::new());
    let mut schedule = Schedule::new(fakes.clock.now(), crate::pump::SAFETY);
    let mut reads = 0;
    for _ in 0..3 {
        let until = schedule.deadline(fakes.clock.now());
        assert!(matches!(pump.wait(until), Signal::Due));
        if schedule.take(fakes.clock.now()).is_some() {
            reads += 1;
            schedule.done();
        }
    }
    assert_eq!(
        (reads, fakes.clock.naps()),
        (3, vec![crate::pump::SAFETY; 3])
    );
}

// ---- the clock facts: the spinner frame, the wall time, a snooze --------------------------------------------------------

#[test]
fn the_spinner_turns_one_frame_per_frame_of_virtual_time() {
    let fakes = Fakes::new();
    let first = frame_now(&fakes.ports);
    fakes.clock.advance(ms(palette::FRAME_MS));
    assert_eq!(
        frame_now(&fakes.ports),
        (first + 1) % palette::SPINNER.len()
    );
}

#[test]
fn the_spinner_is_back_after_a_whole_turn_of_virtual_time() {
    let fakes = Fakes::new();
    let first = frame_now(&fakes.ports);
    fakes
        .clock
        .advance(ms(palette::FRAME_MS * palette::SPINNER.len() as u64));
    assert_eq!(frame_now(&fakes.ports), first);
}

#[test]
fn the_wall_time_in_milliseconds_moves_with_the_clock() {
    let fakes = Fakes::new();
    let start = wall_ms(&fakes.ports);
    assert_eq!(
        start,
        (fakes.clock.utc().unix_timestamp_nanos() / 1_000_000) as u64
    );
    fakes.clock.advance(ms(1500));
    assert_eq!(wall_ms(&fakes.ports), start + 1500);
}

#[test]
fn a_snooze_is_over_when_the_clock_reaches_its_end() {
    let fakes = Fakes::new();
    let end = Some(fakes.clock.utc() + Duration::from_secs(3600));
    assert!(!snooze_over(&fakes.ports, end));
    fakes.clock.advance(Duration::from_secs(3599));
    assert!(!snooze_over(&fakes.ports, end));
    fakes.clock.advance(Duration::from_secs(1));
    assert!(snooze_over(&fakes.ports, end));
}

#[test]
fn no_snooze_is_never_over() {
    let fakes = Fakes::new();
    fakes.clock.advance(Duration::from_secs(86_400));
    assert!(!snooze_over(&fakes.ports, None::<OffsetDateTime>));
}

// ---- the view's own files: stamps and the one-second write ----------------------------------------------------------

#[test]
fn a_view_log_line_is_stamped_by_the_clock() {
    let (fakes, home) = (Fakes::new(), home());
    let log = ViewLog::of(&home.home, "tree");
    log.append_with(&fakes.ports, "first").unwrap();
    let first = stamp(&fakes.clock);
    fakes.clock.advance(Duration::from_secs(90));
    log.append_with(&fakes.ports, "second").unwrap();
    let text = std::fs::read_to_string(home.home.run_dir().join("tui/tree.log")).unwrap();
    assert_eq!(
        text,
        format!("{first} first\n{} second\n", stamp(&fakes.clock))
    );
}

fn folds(collapsed: &[&str]) -> Folds {
    serde_json::from_value(json!({"collapsed": collapsed, "expanded": []})).unwrap()
}

fn saved_folds(home: &Home) -> Folds {
    FoldsFile::at(&home.home).load().unwrap()
}

#[test]
fn folds_the_first_change_is_written_at_once() {
    let (fakes, home) = (Fakes::new(), home());
    let mut file = FoldsFile::at(&home.home);
    file.save_with(&fakes.ports, &folds(&["alpha"])).unwrap();
    assert_eq!(saved_folds(&home), folds(&["alpha"]));
}

#[test]
fn folds_a_change_within_a_second_of_the_last_write_waits_and_one_a_second_later_writes() {
    let (fakes, home) = (Fakes::new(), home());
    let mut file = FoldsFile::at(&home.home);
    file.save_with(&fakes.ports, &folds(&["alpha"])).unwrap();
    fakes.clock.advance(ms(999));
    file.save_with(&fakes.ports, &folds(&["alpha", "beta"]))
        .unwrap();
    assert_eq!(
        saved_folds(&home),
        folds(&["alpha"]),
        "the second change waits"
    );
    fakes.clock.advance(ms(1));
    file.save_with(&fakes.ports, &folds(&["alpha", "beta"]))
        .unwrap();
    assert_eq!(
        saved_folds(&home),
        folds(&["alpha", "beta"]),
        "a second after the last write it writes"
    );
}

#[test]
fn folds_unchanged_write_nothing_whatever_the_time() {
    let (fakes, home) = (Fakes::new(), home());
    let mut file = FoldsFile::at(&home.home);
    file.save_with(&fakes.ports, &Folds::default()).unwrap();
    fakes.clock.advance(Duration::from_secs(60));
    file.save_with(&fakes.ports, &Folds::default()).unwrap();
    assert!(!home.home.run_dir().join("tui/tree-folds.json").exists());
}

fn offsets(entries: &[(&str, (u16, u16))]) -> HashMap<PlanId, (u16, u16)> {
    entries
        .iter()
        .map(|(id, at)| (PlanId::parse(id).unwrap(), *at))
        .collect()
}

fn saved_offsets(home: &Home) -> HashMap<PlanId, (u16, u16)> {
    Offsets::at(&home.home).load().unwrap()
}

#[test]
fn offsets_a_change_within_a_second_of_the_last_write_waits_unless_the_plan_changed() {
    let (fakes, home) = (Fakes::new(), home());
    let mut file = Offsets::at(&home.home);
    file.save_with(&fakes.ports, &offsets(&[("plan-a", (0, 1))]), false)
        .unwrap();
    file.save_with(&fakes.ports, &offsets(&[("plan-a", (0, 2))]), false)
        .unwrap();
    assert_eq!(
        saved_offsets(&home),
        offsets(&[("plan-a", (0, 1))]),
        "the second change waits"
    );
    file.save_with(&fakes.ports, &offsets(&[("plan-a", (0, 2))]), true)
        .unwrap();
    assert_eq!(
        saved_offsets(&home),
        offsets(&[("plan-a", (0, 2))]),
        "a Plan change writes at once"
    );
}

#[test]
fn offsets_a_change_a_second_after_the_last_write_is_written() {
    let (fakes, home) = (Fakes::new(), home());
    let mut file = Offsets::at(&home.home);
    file.save_with(&fakes.ports, &offsets(&[("plan-a", (0, 1))]), false)
        .unwrap();
    fakes.clock.advance(ms(1000));
    file.save_with(&fakes.ports, &offsets(&[("plan-a", (0, 3))]), false)
        .unwrap();
    assert_eq!(saved_offsets(&home), offsets(&[("plan-a", (0, 3))]));
}

#[test]
fn offsets_unchanged_write_nothing_whatever_the_time() {
    let (fakes, home) = (Fakes::new(), home());
    let mut file = Offsets::at(&home.home);
    file.save_with(&fakes.ports, &HashMap::new(), false)
        .unwrap();
    fakes.clock.advance(Duration::from_secs(60));
    file.save_with(&fakes.ports, &HashMap::new(), false)
        .unwrap();
    assert!(!home.home.run_dir().join("tui/plan-scroll.json").exists());
}

/// Plan `p1` of one Workflow `api`, its Lead `lead` in the chat slot.
const PLAN_FIXTURE: &str = "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('lead', 'lead', 'ctx', 'p1', NULL, 'n', '/s/lead.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('p1', 'ctx', 'P', 'dispatch', 'lead', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('p1', 'api', '/api-repo', 'main', 'dispatch', 'cpd/p1', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         UPDATE sessions SET plan = 'p1' WHERE id = 'lead';
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-10-18T15:00:00.000Z', 'session-moved', 'ctx', NULL, 'lead', 'f',
                 '{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}');";

#[test]
fn the_plan_chart_is_drawn_at_the_spinner_frame_of_the_clock() {
    let (fakes, home) = (Fakes::new(), home());
    let mut store = home.store();
    sql(&mut store, PLAN_FIXTURE);
    let first = crate::plan::snapshot_with(&fakes.ports, &mut store, 40)
        .unwrap()
        .expect("the Lead is in the slot");
    assert_eq!(first.frame, frame_now(&fakes.ports));
    fakes.clock.advance(ms(palette::FRAME_MS));
    let next = crate::plan::snapshot_with(&fakes.ports, &mut store, 40)
        .unwrap()
        .expect("the Lead is in the slot");
    assert_eq!(next.frame, (first.frame + 1) % palette::SPINNER.len());
}

#[test]
fn the_plan_views_snapshot_reads_the_elapsed_time_at_its_ports_clock() {
    let (fakes, home) = (Fakes::new(), home());
    let mut store = home.store();
    sql(&mut store, PLAN_FIXTURE);
    // an Attempt that started 100 whole seconds before the fake clock's wall time and has not ended
    let started = OffsetDateTime::from_unix_timestamp(
        i64::try_from(wall_ms(&fakes.ports) / 1000).unwrap() - 100,
    )
    .unwrap();
    let started = started.format(&Rfc3339).unwrap().replace('Z', ".000Z");
    sql(
        &mut store,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               created_txn, created_at)
             VALUES ('p1', 'api', 'a', 'Job', 'S3', 'G.', 'W.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0,
                     '0000000000000000000000000000000000000000000000000000000000000000', cpd_txn(), '2026-01-02T00:00:00.000Z');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
             VALUES ('p1', 'a-a1', 'a', 1, 'S3', 'ladder', 'running', NULL, NULL, 'cpd/p1--a-a1', '/w/a', '{started}', NULL);"
        ),
    );
    let elapsed = |fakes: &Fakes, store: &mut Store| {
        let centre = crate::plan::snapshot_with(&fakes.ports, store, 40)
            .unwrap()
            .expect("the Lead is in the slot");
        centre.plan.workflows[0].elapsed_seconds
    };
    assert_eq!(elapsed(&fakes, &mut store), Some(100)); // the fake clock is 100 s after the start
    fakes.clock.advance(Duration::from_secs(90));
    assert_eq!(elapsed(&fakes, &mut store), Some(190)); // 90 s more
}

// ---- the logs view ----------------------------------------------------------------------------------------------------

const POLL: Duration = Duration::from_millis(250);

/// The logs view over an empty store, run on `steps`; the context it ran on and what it returned.
fn run_logs(
    fakes: &Fakes,
    home: &Home,
    steps: Vec<Step>,
) -> (Result<(), ViewError>, usize, Vec<Duration>) {
    let mut input = FakeInput::new(fakes, steps);
    let mut store = home.store();
    let first = read::logs(&mut store, 100).unwrap();
    let follow = read::Follow::new(&mut store, &[], first.events_cursor).unwrap();
    let mut ctx = Ctx::new(&fakes.ports, terminal(), &mut input);
    let result = logs::show_with(
        &mut ctx,
        &mut store,
        POLL,
        follow,
        logs::Log::new(first.events, ManualClock::new().zone()),
    );
    (result, ctx.draws(), fakes.clock.naps())
}

#[test]
fn logs_idle_polls_nap_the_poll_interval_on_the_clock_and_draw_nothing_more() {
    let (fakes, home) = (Fakes::new(), home());
    let mut steps = idle(3);
    steps.push(key(KeyCode::Char('q')));
    let (result, draws, naps) = run_logs(&fakes, &home, steps);
    result.unwrap();
    assert_eq!((draws, naps), (1, vec![POLL; 3]));
}

#[test]
fn logs_q_closes_the_view_after_its_first_frame() {
    let (fakes, home) = (Fakes::new(), home());
    let (result, draws, _) = run_logs(&fakes, &home, vec![key(KeyCode::Char('q'))]);
    result.unwrap();
    assert_eq!(draws, 1);
}

#[test]
fn logs_escape_closes_the_view() {
    let (fakes, home) = (Fakes::new(), home());
    let (result, draws, _) = run_logs(&fakes, &home, vec![key(KeyCode::Esc)]);
    result.unwrap();
    assert_eq!(draws, 1);
}

#[test]
fn logs_a_key_that_scrolls_redraws() {
    let (fakes, home) = (Fakes::new(), home());
    let (result, draws, _) = run_logs(
        &fakes,
        &home,
        vec![key(KeyCode::Down), key(KeyCode::Char('q'))],
    );
    result.unwrap();
    assert_eq!(draws, 2);
}

#[test]
fn logs_an_event_another_process_commits_is_followed_at_the_next_poll_and_redraws() {
    let (fakes, home) = (Fakes::new(), home());
    let mut steps = vec![commit_event(&home.home, "NULL")];
    steps.extend(idle(2));
    steps.push(key(KeyCode::Char('q')));
    let (result, draws, naps) = run_logs(&fakes, &home, steps);
    result.unwrap();
    assert_eq!(
        (draws, naps),
        (2, vec![POLL; 2]),
        "drawn at the start and once for the Event, not at every poll"
    );
}

#[test]
fn logs_a_failed_input_is_a_terminal_failure() {
    let (fakes, home) = (Fakes::new(), home());
    let (result, draws, _) = run_logs(&fakes, &home, vec![Step::Fail(io::ErrorKind::BrokenPipe)]);
    assert!(matches!(result, Err(ViewError::Terminal(e)) if e.kind() == io::ErrorKind::BrokenPipe));
    assert_eq!(draws, 1);
}

#[test]
fn a_context_tells_its_input_of_each_snapshot_and_a_real_input_ignores_it() {
    let fakes = Fakes::new();
    let mut input = FakeInput::new(&fakes, Vec::new());
    let snapshots = Rc::clone(&input.snapshots);
    let mut ctx = Ctx::new(&fakes.ports, terminal(), &mut input);
    ctx.snapshot_read();
    ctx.snapshot_read();
    assert_eq!(
        (snapshots.get(), ctx.draws()),
        (2, 0),
        "a snapshot is told, and is no frame"
    );
    crate::view_ports::CrosstermInput.snapshot_read(); // the default: nothing to count, nothing to fail
}

// ---- the dialog view ----------------------------------------------------------------------------------------------------

fn confirm() -> Dialog {
    Dialog::new(
        "Delete".into(),
        "Sure?".into(),
        Mode::Confirm {
            yes: "Delete".into(),
        },
    )
}

fn run_dialog(dialog: &mut Dialog, steps: Vec<Step>) -> (io::Result<Outcome>, usize) {
    let fakes = Fakes::new();
    let mut input = FakeInput::new(&fakes, steps);
    let mut ctx = Ctx::new(&fakes.ports, terminal(), &mut input);
    let result = crate::dialog::show_with(&mut ctx, dialog);
    (result, ctx.draws())
}

#[test]
fn dialog_enter_confirms_after_one_frame_per_event_taken() {
    let (result, draws) = run_dialog(
        &mut confirm(),
        vec![key(KeyCode::Char('x')), key(KeyCode::Enter)],
    );
    assert_eq!((result.unwrap(), draws), (Outcome::confirmed(), 2));
}

#[test]
fn dialog_escape_cancels() {
    let (result, draws) = run_dialog(&mut confirm(), vec![key(KeyCode::Esc)]);
    assert_eq!((result.unwrap(), draws), (Outcome::cancelled(), 1));
}

#[test]
fn dialog_a_select_returns_the_option_the_cursor_reached() {
    let mut dialog = Dialog::new(
        "Pick".into(),
        "One of:".into(),
        Mode::Select {
            options: vec!["a".into(), "b".into()],
            cursor: 0,
        },
    );
    let (result, draws) = run_dialog(&mut dialog, vec![key(KeyCode::Down), key(KeyCode::Enter)]);
    assert_eq!((result.unwrap(), draws), (Outcome::value("b".into()), 2));
}

#[test]
fn dialog_a_failed_input_is_returned_as_the_terminal_failure() {
    let (result, draws) = run_dialog(&mut confirm(), vec![Step::Fail(io::ErrorKind::BrokenPipe)]);
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(draws, 1);
}

// ---- the merge view and its action ---------------------------------------------------------------------------------------

/// The commands `merge::action_with` ran through `reply`, in order, each as its words.
fn merge_action(
    reply: impl Fn(&[String]) -> Result<Output, CpdError>,
    home: &Home,
    fakes: &Fakes,
) -> (Result<(), CpdError>, Vec<Vec<String>>) {
    let calls = RefCell::new(Vec::new());
    let run = |argv: Vec<OsString>| {
        let words = strings(&argv);
        calls.borrow_mut().push(words.clone());
        reply(&words)
    };
    let tmux = Tmux::new("cpd-view-ports").with_runner(fakes.runner.clone());
    let result = merge::action_with(
        &fakes.ports,
        &home.home,
        &tmux,
        &Pull::parse("acme/api#7").unwrap(),
        "client",
        &run as RunFn,
    );
    (result, calls.into_inner())
}

fn ok_output(_: &[String]) -> Result<Output, CpdError> {
    Ok(Output::Owned)
}

#[test]
fn merge_the_action_runs_pr_merge_with_the_argv_it_builds() {
    let (fakes, home) = (Fakes::new(), home());
    let (result, calls) = merge_action(ok_output, &home, &fakes);
    result.unwrap();
    assert_eq!(calls, vec![words(&["cpd", "pr", "merge", "acme/api#7"])]);
}

#[test]
fn merge_a_failed_merge_opens_no_further_command() {
    let (fakes, home) = (Fakes::new(), home());
    let (_, calls) = merge_action(
        |_| Err(CpdError::new(ErrorCode::StoreBusy, "busy")),
        &home,
        &fakes,
    );
    assert_eq!(calls, vec![words(&["cpd", "pr", "merge", "acme/api#7"])]);
}

// ---- the legend of the Dispatch column ---------------------------------------------------------------------------------

fn run_legend(steps: Vec<Step>) -> (io::Result<()>, usize) {
    let fakes = Fakes::new();
    let mut input = FakeInput::new(&fakes, steps);
    let mut ctx = Ctx::new(&fakes.ports, terminal(), &mut input);
    let result = widgets::legend::show_with(&mut ctx);
    (result, ctx.draws())
}

#[test]
fn legend_any_key_that_does_not_scroll_closes_it_after_one_frame() {
    let (result, draws) = run_legend(vec![key(KeyCode::Char('q'))]);
    result.unwrap();
    assert_eq!(draws, 1);
}

#[test]
fn legend_a_scrolling_key_redraws_and_does_not_close() {
    let (result, draws) = run_legend(vec![key(KeyCode::Down), key(KeyCode::Char('q'))]);
    result.unwrap();
    assert_eq!(draws, 2);
}

// ---- the cockpit: RunCommand, Restart and the exec of tmux -----------------------------------------------------------------

#[test]
fn the_cockpit_opens_the_lead_with_the_argv_it_builds() {
    let calls = RefCell::new(Vec::new());
    let run = |argv: Vec<OsString>| {
        calls.borrow_mut().push(strings(&argv));
        Ok(Output::success(json!({})))
    };
    crate::cockpit::open_lead_with(&run as RunFn, &SessionId::parse("lead-1").unwrap()).unwrap();
    assert_eq!(
        calls.into_inner(),
        vec![words(&["cpd", "session", "open", "lead-1"])]
    );
}

#[test]
fn the_cockpit_attaches_by_exec_of_tmux_on_the_spawn_with_the_argv_it_builds() {
    let fakes = Fakes::new();
    let tmux = Tmux::new("cpd");
    let e = crate::cockpit::exec_attach_with(&fakes.ports, &tmux, "cockpit", None);
    assert_eq!(e.code(), ErrorCode::TmuxFailed);
    assert!(e.message().contains("faked exec"), "{}", e.message());
    let launches = fakes.spawn.launches();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].program, "tmux");
    assert_eq!(launches[0].args[..3], ["-u", "-L", "cpd"]);
    assert_eq!(
        launches[0].args[launches[0].args.len() - 3..],
        ["attach", "-t", "cockpit"]
    );
    assert!(
        launches[0].env_remove.contains(&"TMUX".to_owned()),
        "{:?}",
        launches[0].env_remove
    );
}

#[test]
fn the_cockpit_attaches_with_the_servers_own_tmux_when_it_is_given() {
    let fakes = Fakes::new();
    let program = std::path::Path::new("/opt/tmux-3.5/bin/tmux");
    crate::cockpit::exec_attach_with(&fakes.ports, &Tmux::new("cpd"), "cockpit", Some(program));
    assert_eq!(fakes.spawn.launches()[0].program, "/opt/tmux-3.5/bin/tmux");
}

thread_local! {
    static CALLED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static OUTCOME: RefCell<Option<RestartOutcome>> = const { RefCell::new(None) };
}

fn restart_failed(_: &CpdHome, _: &cpd_core::clock::TimeZone) -> Result<Option<String>, CpdError> {
    CALLED.with_borrow_mut(|c| c.push("failed".into()));
    Ok(None)
}

fn restart_clear(_: &CpdHome) -> Result<(), CpdError> {
    CALLED.with_borrow_mut(|c| c.push("clear".into()));
    Ok(())
}

fn restart_mark(_: &CpdHome, server: &str, installed: &str) -> Result<bool, CpdError> {
    CALLED.with_borrow_mut(|c| c.push(format!("mark {server} {installed}")));
    Ok(true)
}

fn restart_now(
    _: &CpdHome,
    _: &cpd_core::config::Config,
    _: &mut Store,
) -> Result<RestartOutcome, CpdError> {
    CALLED.with_borrow_mut(|c| c.push("now".into()));
    Ok(OUTCOME
        .with_borrow_mut(Option::take)
        .expect("an outcome was set"))
}

/// `tmux_version_with` where the server runs tmux `server` and the installed tmux is `installed`, the restart `outcome`.
fn tmux_version(server: &str, installed: &str, outcome: RestartOutcome) -> (String, Vec<String>) {
    let fakes = Fakes::new();
    let server_call =
        |call: &cpd_core::host::runner::Call| call.args.iter().any(|a| a == "display");
    let version_call = |call: &cpd_core::host::runner::Call| call.args.iter().any(|a| a == "-V");
    let runner = Arc::new(
        FakeRunner::new((*fakes.clock).clone())
            .on("tmux", server_call, Reply::ok(&format!("{server}\n")))
            .on(
                "tmux",
                version_call,
                Reply::ok(&format!("tmux {installed}\n")),
            ),
    );
    let ports = Ports {
        runner: runner.clone(),
        ..fakes.ports.clone()
    };
    let home = home();
    let config = cpd_core::config::Config::load(&home.home).unwrap();
    let mut store = home.store();
    let tmux = Tmux::new("cpd").with_runner(runner);
    CALLED.with_borrow_mut(Vec::clear);
    OUTCOME.with_borrow_mut(|o| *o = Some(outcome));
    let restart = Restart {
        failed: restart_failed,
        clear: restart_clear,
        mark: restart_mark,
        now: restart_now,
    };
    let said =
        crate::cockpit::tmux_version_with(&ports, &home.home, &config, &mut store, &tmux, restart);
    (said, CALLED.with_borrow(Clone::clone))
}

#[test]
fn the_cockpit_marks_a_restart_pending_with_the_two_versions_when_it_is_busy() {
    let (said, called) = tmux_version(
        "3.5",
        "3.6",
        RestartOutcome::Busy(vec!["an Attempt runs".into()]),
    );
    assert_eq!(called, ["now", "mark 3.5 3.6"]);
    assert_eq!(
        said,
        "; the restart happens once nothing is in the way (an Attempt runs)"
    );
}

#[test]
fn the_cockpit_marks_nothing_when_the_restart_ran() {
    let (said, called) = tmux_version("3.5", "3.6", RestartOutcome::Restarted { reopened: 2 });
    assert_eq!((said.as_str(), called), ("", vec!["now".to_owned()]));
}

#[test]
fn the_cockpit_clears_the_pending_restart_when_the_server_runs_the_installed_tmux() {
    let (said, called) = tmux_version("3.6", "3.6", RestartOutcome::NotNeeded);
    assert_eq!(
        (said.as_str(), called),
        ("", vec!["failed".to_owned(), "clear".to_owned()])
    );
}
