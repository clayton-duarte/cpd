//! `common::screen`: `Screen` over a `TestBackend` buffer and `Keys`, the scripted input of a view.
//!
//! In-process: no terminal, no process, no clock.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use cpd_tui::view_ports::Input;

use crate::common::screen::{Keys, Screen, Script, SharedBackend, Step, ch, ctrl, key, typed};

/// The screen of a 12x3 terminal after it drew `lines`.
fn drawn(lines: Vec<Line<'static>>) -> Screen {
    let mut terminal = Terminal::new(TestBackend::new(12, 3)).expect("a test terminal");
    terminal.draw(|frame| frame.render_widget(Paragraph::new(lines), frame.area())).expect("draw");
    Screen::from_buffer(terminal.backend().buffer())
}

fn two_lines() -> Screen {
    drawn(vec![
        Line::from(vec![Span::raw("plan "), Span::styled("pay", Style::new().fg(Color::Green).bg(Color::Black).bold())]),
        Line::from("  idle"),
    ])
}

// ---- Screen

#[test]
fn screen_rows_are_the_text_of_each_row_without_trailing_spaces() {
    let screen = two_lines();

    assert_eq!(screen.rows(), ["plan pay", "  idle", ""]);
    assert_eq!((screen.width(), screen.height()), (12, 3));
    assert_eq!(screen.row(1), "  idle");
    assert_eq!(screen.text(), "plan pay\n  idle\n");
}

#[test]
fn screen_cell_reads_the_style_of_a_cell() {
    let screen = two_lines();

    let pay = screen.cell(5, 0);
    assert_eq!(pay.symbol, "p");
    assert_eq!((pay.fg, pay.bg), (Color::Green, Color::Black));
    assert!(pay.modifier.contains(Modifier::BOLD));
    let plain = screen.cell(0, 0);
    assert_eq!((plain.fg, plain.bg, plain.modifier), (Color::Reset, Color::Reset, Modifier::empty()));
}

#[test]
fn screen_find_gives_the_position_of_a_text_and_cell_of_its_style() {
    let screen = two_lines();

    assert_eq!(screen.find("pay"), Some((5, 0)));
    assert_eq!(screen.find("idle"), Some((2, 1)));
    assert_eq!(screen.find("nope"), None);
    assert!(screen.contains("plan pay") && !screen.contains("pay plan"));
    assert_eq!(screen.cell_of("pay").fg, Color::Green);
}

#[test]
fn screen_cell_outside_the_screen_is_refused() {
    let screen = two_lines();

    let result = std::panic::catch_unwind(|| screen.cell(12, 0).clone());

    assert!(result.is_err());
}

#[test]
fn screen_is_a_copy_of_the_buffer_it_was_read_from() {
    let mut terminal = Terminal::new(TestBackend::new(6, 1)).expect("a test terminal");
    terminal.draw(|frame| frame.render_widget(Paragraph::new("one"), frame.area())).expect("draw");
    let before = Screen::from_buffer(terminal.backend().buffer());
    terminal.draw(|frame| frame.render_widget(Paragraph::new("two"), frame.area())).expect("draw");

    assert_eq!(before.rows(), ["one"]);
    assert_eq!(Screen::from_buffer(terminal.backend().buffer()).rows(), ["two"]);
}

// ---- Keys

#[test]
fn keys_hand_out_the_scripted_events_in_order() {
    let mut keys = Keys::script([ch('a'), key(KeyCode::Enter), ctrl('c')]);

    assert_eq!(keys.remaining(), 3);
    assert!(keys.poll(std::time::Duration::from_millis(100)).unwrap());
    let read: Vec<Event> = (0..3).map(|_| keys.read().unwrap()).collect();

    assert_eq!(read, [ch('a'), key(KeyCode::Enter), ctrl('c')]);
    assert_eq!(keys.remaining(), 0);
}

#[test]
fn typed_is_one_key_press_per_character() {
    assert_eq!(typed("a b"), [ch('a'), ch(' '), ch('b')]);
}

#[test]
fn keys_poll_with_nothing_left_fails_loudly() {
    let mut keys = Keys::script([]);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| keys.poll(std::time::Duration::ZERO)));

    assert!(result.is_err(), "a loop that polls past its script must not spin");
}

// ---- SharedBackend and Script

#[test]
fn shared_backend_shows_the_test_what_the_terminal_drew() {
    let backend = SharedBackend::new(8, 2);
    let mut terminal = backend.terminal();

    terminal.draw(|frame| frame.render_widget(Paragraph::new("one"), frame.area())).expect("draw");
    assert_eq!(backend.screen().rows(), ["one", ""]);
    terminal.draw(|frame| frame.render_widget(Paragraph::new("two"), frame.area())).expect("draw");

    assert_eq!(backend.screen().rows(), ["two", ""]);
}

#[test]
fn script_runs_what_the_world_does_and_the_looks_up_to_the_next_event_and_an_idle_poll_naps_the_clock() {
    use cpd_core::ports::Fakes;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    let fakes = Fakes::new();
    let backend = SharedBackend::new(8, 1);
    let mut terminal = backend.terminal();
    terminal.draw(|frame| frame.render_widget(Paragraph::new("first"), frame.area())).expect("draw");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (done, looked) = (Rc::clone(&seen), Rc::clone(&seen));
    let mut script = Script::new(
        &fakes.clock,
        &backend,
        [
            Step::then(move || done.borrow_mut().push("world".to_owned())),
            Step::look(move |screen| looked.borrow_mut().push(screen.row(0))),
            Step::Idle,
            Step::ch('q'),
        ],
    );

    assert!(!script.poll(Duration::from_millis(50)).unwrap(), "the idle poll finds nothing");
    assert_eq!(*seen.borrow(), ["world", "first"]);
    assert_eq!(fakes.clock.naps(), [Duration::from_millis(50)]);
    assert!(script.poll(Duration::from_millis(50)).unwrap());
    assert_eq!(script.read().unwrap(), ch('q'));
    assert_eq!(script.remaining(), 0);
}

#[test]
fn script_poll_with_nothing_left_fails_loudly() {
    use cpd_core::ports::Fakes;

    let fakes = Fakes::new();
    let mut script = Script::new(&fakes.clock, &SharedBackend::new(1, 1), []);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| script.poll(std::time::Duration::ZERO)));

    assert!(result.is_err(), "a loop that polls past its script must not spin");
}
