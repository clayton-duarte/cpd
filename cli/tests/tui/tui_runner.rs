//! `run_view`, which every view of `cpd tui view` runs through: whatever way a view ends, by a panic or by an error, the
//! terminal is restored and mouse reporting is off again.
//!
//! The view runs in this process on `run_view_with`, with a writer for the terminal's bytes and a `Tty` that logs raw
//! mode (docs/implementing.md, Tests): what the program asked of the terminal is what it wrote, read from the writer.
//! The raw-mode ioctl and the alternate screen are crossterm's and tmux's; what is ours is that the view writes the
//! `Disable*` and `LeaveAlternateScreen` sequences and turns raw mode off, on every way out.

use crate::common::screen::{Bytes, Display, FakeTty, Keys};

use std::cell::RefCell;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ports::Fakes;
use cpd_tui::view_ports::run_view_with;
use cpd_tui::{Capture, ViewError};
use ratatui::crossterm::Command;
use ratatui::crossterm::event::{
    DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
};
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

/// The bytes of `command`, as crossterm writes them to a terminal.
fn ansi(command: impl Command) -> Vec<u8> {
    let mut text = String::new();
    command.write_ansi(&mut text).unwrap();
    text.into_bytes()
}

fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// What a view that ended in a way left behind.
struct Ended {
    /// What the view had written when its body ran.
    during: Vec<u8>,
    /// Everything it wrote.
    after: Vec<u8>,
    /// The calls of raw mode, in order.
    raw: Vec<&'static str>,
    /// Its result; `None` when its body panicked and the panic went on.
    result: Option<Result<(), CpdError>>,
}

/// Runs a view whose body panics (`"panic"`) or fails (anything else).
fn view_ending(ends: &str) -> Ended {
    let fakes = Fakes::new();
    let (mut tty, out, display) = (FakeTty::new(true), Bytes::default(), Display::new(80, 10));
    let mut input = Keys::script([]);
    let during = Rc::new(RefCell::new(Vec::new()));
    let (mut writer, seen, written) = (out.clone(), out.clone(), during.clone());
    let result = catch_unwind(AssertUnwindSafe(|| {
        run_view_with(
            &fakes.ports,
            &mut tty,
            &mut writer,
            display.terminal(),
            &mut input,
            "test",
            Capture::MouseAndFocus,
            || Ok(()),
            |_, ()| {
                *written.borrow_mut() = seen.bytes();
                match ends {
                    "panic" => panic!("the view panicked"),
                    _ => Err(ViewError::Terminal(io::Error::other("the view failed"))),
                }
            },
        )
        .map(|_| ())
    }));
    let raw = tty.log.borrow().clone();
    let during = during.borrow().clone();
    Ended {
        during,
        after: out.bytes(),
        raw,
        result: result.ok(),
    }
}

/// The view owned the terminal while its body ran (the alternate screen, the mouse and focus reporting on, raw mode
/// on) and, after it ended, each is off again: the reporting before the alternate screen is left, which is the last
/// thing written.
fn assert_owned_then_restored(ended: &Ended, what: &str) {
    let (during, after) = (&ended.during, &ended.after);
    assert!(
        position(during, &ansi(EnterAlternateScreen)).is_some(),
        "the view owns the alternate screen: {during:?}"
    );
    assert!(
        position(during, &ansi(EnableMouseCapture)).is_some(),
        "mouse reporting is on: {during:?}"
    );
    assert!(
        position(during, &ansi(EnableFocusChange)).is_some(),
        "focus reporting is on: {during:?}"
    );
    assert!(
        position(during, &ansi(DisableMouseCapture)).is_none(),
        "nothing is turned off while the view runs: {during:?}"
    );
    assert_eq!(
        ended.raw,
        ["raw_on", "raw_off"],
        "raw mode is on for the body and off after {what}"
    );
    let left = position(after, &ansi(LeaveAlternateScreen)).expect("the alternate screen is left");
    for off in [ansi(DisableMouseCapture), ansi(DisableFocusChange)] {
        let at = position(after, &off)
            .unwrap_or_else(|| panic!("{off:?} is written after {what}: {after:?}"));
        assert!(
            at < left,
            "the reporting is off before the alternate screen is left: {after:?}"
        );
    }
    assert!(
        after.ends_with(&ansi(LeaveAlternateScreen)),
        "the alternate screen is left last after {what}: {after:?}"
    );
}

#[test]
fn a_panic_in_a_view_leaves_the_terminal_restored_and_the_mouse_off() {
    let ended = view_ending("panic");
    assert!(ended.result.is_none(), "the panic goes on");
    assert_owned_then_restored(&ended, "a panic");
}

#[test]
fn an_error_in_a_view_leaves_the_terminal_restored_and_the_mouse_off() {
    let ended = view_ending("error");
    let e = ended
        .result
        .as_ref()
        .expect("an error does not panic")
        .as_ref()
        .unwrap_err();
    assert_eq!(
        (e.code(), e.message()),
        (
            ErrorCode::NotATerminal,
            "The terminal of cpd tui view test failed: the view failed."
        )
    );
    assert_owned_then_restored(&ended, "an error");
}
