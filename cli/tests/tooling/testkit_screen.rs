//! `common::screen`: `Screen` over a `TestBackend` buffer and `Keys`, the scripted input of a view.
//!
//! In-process: no terminal, no process, no wall clock.

use std::cell::Cell;
use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cpd_core::clock::ManualClock;
use cpd_tui::view_ports::{Input, Tty};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event, KeyCode};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::common::screen::{
    Bytes, Display, FakeTty, Keys, Screen, Script, SharedBackend, Step, Steps, ch, ctrl, idle, key,
    left_click, right_click, then, typed,
};

/// The screen of a 12x3 terminal after it drew `lines`.
fn drawn(lines: Vec<Line<'static>>) -> Screen {
    let mut terminal = Terminal::new(TestBackend::new(12, 3)).expect("a test terminal");
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new(lines), frame.area()))
        .expect("draw");
    Screen::from_buffer(terminal.backend().buffer())
}

fn two_lines() -> Screen {
    drawn(vec![
        Line::from(vec![
            Span::raw("plan "),
            Span::styled("pay", Style::new().fg(Color::Green).bg(Color::Black).bold()),
        ]),
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
    assert_eq!(
        (plain.fg, plain.bg, plain.modifier),
        (Color::Reset, Color::Reset, Modifier::empty())
    );
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
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("one"), frame.area()))
        .expect("draw");
    let before = Screen::from_buffer(terminal.backend().buffer());
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("two"), frame.area()))
        .expect("draw");

    assert_eq!(before.rows(), ["one"]);
    assert_eq!(
        Screen::from_buffer(terminal.backend().buffer()).rows(),
        ["two"]
    );
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

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        keys.poll(std::time::Duration::ZERO)
    }));

    assert!(
        result.is_err(),
        "a loop that polls past its script must not spin"
    );
}

// ---- SharedBackend and Script

#[test]
fn shared_backend_shows_the_test_what_the_terminal_drew() {
    let backend = SharedBackend::new(8, 2);
    let mut terminal = backend.terminal();

    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("one"), frame.area()))
        .expect("draw");
    assert_eq!(backend.screen().rows(), ["one", ""]);
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("two"), frame.area()))
        .expect("draw");

    assert_eq!(backend.screen().rows(), ["two", ""]);
}

#[test]
fn script_runs_what_the_world_does_and_the_looks_up_to_the_next_event_and_an_idle_poll_naps_the_clock()
 {
    use cpd_core::ports::Fakes;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    let fakes = Fakes::new();
    let backend = SharedBackend::new(8, 1);
    let mut terminal = backend.terminal();
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("first"), frame.area()))
        .expect("draw");
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (done, looked) = (Rc::clone(&seen), Rc::clone(&seen));
    let mut script = Script::on_screen(
        fakes.clock.clone(),
        &backend,
        [
            Step::then(move || done.borrow_mut().push("world".to_owned())),
            Step::look(move |screen| looked.borrow_mut().push(screen.row(0))),
            Step::Idle,
            Step::ch('q'),
        ],
    );

    assert!(
        !script.poll(Duration::from_millis(50)).unwrap(),
        "the idle poll finds nothing"
    );
    assert_eq!(*seen.borrow(), ["world", "first"]);
    assert_eq!(fakes.clock.naps(), [Duration::from_millis(50)]);
    assert!(script.poll(Duration::from_millis(50)).unwrap());
    assert_eq!(script.read().unwrap(), ch('q'));
    assert_eq!(script.remaining(), 0);
}

// ---- Script

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new())
}

#[test]
fn script_idle_poll_naps_its_wait_on_the_clock_and_says_no() {
    let clock = clock();
    let mut script = Script::new(clock.clone(), idle(2));

    assert!(!script.poll(Duration::from_millis(100)).unwrap());
    assert!(!script.poll(Duration::from_millis(250)).unwrap());

    assert_eq!(
        clock.naps(),
        [Duration::from_millis(100), Duration::from_millis(250)]
    );
    assert_eq!(script.remaining(), 0);
}

#[test]
fn script_poll_with_nothing_left_fails_loudly() {
    use cpd_core::ports::Fakes;

    let fakes = Fakes::new();
    let mut script = Script::on_screen(fakes.clock.clone(), &SharedBackend::new(1, 1), []);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        script.poll(std::time::Duration::ZERO)
    }));

    assert!(
        result.is_err(),
        "a loop that polls past its script must not spin"
    );
}

// ---- Steps: what the world does while a view draws

/// Draws `text` on `terminal`, one frame.
fn frame(terminal: &mut Terminal<SharedBackend>, text: &'static str) {
    terminal
        .draw(|f| f.render_widget(Paragraph::new(text), f.area()))
        .expect("draw");
}

#[test]
fn steps_run_in_order_one_at_most_per_frame_each_at_the_first_frame_that_satisfies_its_condition() {
    let backend = SharedBackend::new(8, 2);
    let mut terminal = backend.terminal();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let (a, b, c) = (seen.clone(), seen.clone(), seen.clone());
    let steps = Steps::new()
        .at(
            "two",
            |s| s.contains("two"),
            move |s| a.borrow_mut().push(format!("two: {}", s.row(0))),
        )
        .next("the next", move |s| {
            b.borrow_mut().push(format!("next: {}", s.row(0)))
        })
        .at(
            "four",
            |s| s.contains("four"),
            move |_| c.borrow_mut().push("four".to_owned()),
        );
    steps.on(&backend);

    frame(&mut terminal, "one");
    assert_eq!(
        steps.pending(),
        ["two", "the next", "four"],
        "no frame satisfies the first condition yet"
    );
    frame(&mut terminal, "two");
    frame(&mut terminal, "three");
    frame(&mut terminal, "four");
    assert_eq!(*seen.borrow(), ["two: two", "next: three", "four"]);
    steps.finish();
}

#[test]
fn steps_finish_fails_naming_the_step_that_no_frame_reached() {
    let backend = SharedBackend::new(8, 2);
    let steps = Steps::new().at("never", |s| s.contains("never"), |_| {});
    steps.on(&backend);
    frame(&mut backend.terminal(), "one");

    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| steps.finish()))
        .expect_err("a step is pending");
    let message = failure
        .downcast_ref::<String>()
        .expect("a formatted message");
    assert!(message.contains("never"), "{message}");
}

#[test]
fn a_frame_hook_may_replace_itself_and_the_next_one_replaces_the_one_before() {
    let backend = SharedBackend::new(8, 2);
    let mut terminal = backend.terminal();
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let (first, second) = (count.clone(), count.clone());
    let again = backend.clone();
    backend.on_frame(move |_| {
        first.set(first.get() + 1);
        let second = second.clone();
        again.on_frame(move |_| second.set(second.get() + 10));
    });

    frame(&mut terminal, "one");
    frame(&mut terminal, "two");
    frame(&mut terminal, "three");
    assert_eq!(
        count.get(),
        21,
        "the first hook once, then the one it set twice"
    );
}

// ---- shown rows and clicks

#[test]
fn shown_rows_leave_out_the_escapes_of_a_link_and_the_cells_it_covers() {
    let backend = SharedBackend::new(12, 1);
    let mut terminal = backend.terminal();
    // a link piece: one cell that holds the text of the two it covers, which the terminal does not diff
    let linked = "\x1b]8;;https://x/1\x07ab\x1b]8;;\x07";
    terminal
        .draw(|f| {
            let buf = f.buffer_mut();
            buf.set_string(0, 0, "go ", Style::new());
            buf[(3, 0)].set_symbol(linked).set_diff_option(
                ratatui::buffer::CellDiffOption::ForcedWidth(std::num::NonZeroU16::new(2).unwrap()),
            );
            buf.set_string(5, 0, " end", Style::new());
        })
        .expect("draw");

    assert_eq!(backend.screen().shown_rows(), ["go ab end"]);
}

#[test]
fn left_and_right_click_are_a_press_of_that_button_at_a_cell() {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    for (event, button) in [
        (left_click(3, 6), MouseButton::Left),
        (right_click(3, 6), MouseButton::Right),
    ] {
        let Event::Mouse(m) = event else {
            panic!("{event:?}")
        };
        assert_eq!(
            (m.kind, m.column, m.row),
            (MouseEventKind::Down(button), 3, 6)
        );
    }
}

#[test]
fn script_event_is_there_at_the_poll_and_read_hands_it_out() {
    let mut script = Script::new(clock(), [Step::Event(ch('q'))]);

    assert!(script.poll(Duration::ZERO).unwrap());
    assert_eq!(script.read().unwrap(), ch('q'));
}

#[test]
fn script_do_runs_between_two_polls_and_takes_no_time() {
    let (clock, ran) = (clock(), Rc::new(Cell::new(0)));
    let seen = ran.clone();
    let mut script = Script::new(
        clock.clone(),
        [
            then(move || seen.set(seen.get() + 1)),
            Step::Idle,
            Step::Event(ch('x')),
        ],
    );

    assert_eq!(ran.get(), 0, "nothing runs before the first poll");
    assert!(!script.poll(Duration::from_millis(100)).unwrap());
    assert_eq!(ran.get(), 1);
    assert!(script.poll(Duration::ZERO).unwrap());
    assert_eq!(clock.naps(), [Duration::from_millis(100)]);
}

#[test]
fn script_that_is_over_fails_loudly_instead_of_spinning() {
    let mut script = Script::new(clock(), []);

    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| script.poll(Duration::ZERO)));

    assert!(result.is_err());
}

// ---- Display, FakeTty and Bytes

#[test]
fn display_shows_what_a_terminal_made_of_it_drew_while_the_test_holds_a_handle() {
    let display = Display::new(8, 2);
    let mut terminal = display.terminal();

    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("one"), frame.area()))
        .expect("draw");
    assert_eq!(display.screen().rows(), ["one", ""]);
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("two"), frame.area()))
        .expect("draw");
    assert_eq!(display.screen().rows(), ["two", ""]);
}

#[test]
fn display_counts_a_flush_for_every_draw_even_one_that_changes_nothing() {
    let display = Display::new(8, 2);
    let mut terminal = display.terminal();
    assert_eq!(display.flushes(), 0);

    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("one"), frame.area()))
        .expect("draw");
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new("one"), frame.area()))
        .expect("draw");

    assert_eq!(display.flushes(), 2);
}

#[test]
fn fake_tty_says_whether_there_is_a_terminal_and_logs_raw_mode_in_order() {
    let mut tty = FakeTty::new(true);
    assert!(tty.is_terminal() && !FakeTty::new(false).is_terminal());

    tty.raw_on().unwrap();
    tty.raw_off().unwrap();

    assert_eq!(*tty.log.borrow(), ["raw_on", "raw_off"]);
}

#[test]
fn bytes_are_read_by_the_test_while_a_clone_of_it_is_written() {
    let bytes = Bytes::default();
    let mut writer = bytes.clone();

    writer.write_all(b"ab").unwrap();
    writer.write_all(b"c").unwrap();

    assert_eq!(bytes.bytes(), b"abc");
}
