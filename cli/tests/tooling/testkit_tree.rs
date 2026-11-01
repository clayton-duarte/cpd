//! `common::tree`: the tree view in the test process, driven by the test (`Tree`), and the `Gate` of a fake `cpd`.
//!
//! In-process: no terminal, no process, no wall clock.

use std::thread;
use std::time::Duration;

use cpd_core::host::runner::{Call, Handler, Reply};
use ratatui::crossterm::event::{Event, KeyCode};

use crate::common::screen::{ch, key};
use crate::common::tree::{Gate, Tree, engine, fx, named};

fn call() -> Call {
    Call {
        tool: "cpd".into(),
        program: "cpd".into(),
        args: Vec::new(),
        envs: Vec::new(),
        cwd: None,
        deadline: Duration::ZERO,
        stdin: None,
        stderr_to: None,
    }
}

#[test]
fn a_tree_draws_before_start_returns_and_a_nap_moves_the_clock_and_its_drop_ends_the_view() {
    let fx = fx();
    let _engine = engine(&fx);
    let mut tree = Tree::start(&fx, 40, 6);
    assert!(
        tree.shows("no Contexts"),
        "the first frame is drawn: {:?}",
        tree.screen()
    );
    let before = fx.fakes.clock.elapsed();
    tree.idle();
    assert!(
        fx.fakes.clock.elapsed() > before,
        "the loop's poll napped on the ManualClock"
    );
    let written = tree.written();
    assert!(
        !written.is_empty(),
        "the view set the terminal up: {written:?}"
    );
    drop(tree); // joins the loop's thread: it ended by the closed input, not by a panic or an error of its own
}

#[test]
fn a_view_is_run_by_its_entry_and_its_flushes_and_snapshots_are_counted() {
    let fx = fx();
    let _engine = engine(&fx);
    let mut tree = Tree::start(&fx, 40, 6);
    let flushes = tree.flushes();
    assert!(
        flushes > 0 && tree.snapshots() == 0,
        "the first frame is drawn, and the first read is no snapshot of the loop"
    );
    tree.keys(&["j"]);
    assert!(tree.flushes() > flushes, "a key is a frame");
    fx.env.sql(
        "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '2026-02-01T00:00:00.000Z', 'session-state', 'fixture', '{}');",
    );
    tree.until_that("a snapshot", |tree| tree.snapshots() > 0);
    assert_eq!(tree.snapshots(), 1, "one Event, one snapshot");
}

#[test]
fn a_key_is_the_event_the_loop_reads_and_the_names_are_tmuxs() {
    assert_eq!(named("Enter"), key(KeyCode::Enter));
    assert_eq!(named("Escape"), key(KeyCode::Esc));
    assert_eq!(named("Down"), key(KeyCode::Down));
    assert_eq!(named("Space"), ch(' '));
    assert_eq!(named("j"), ch('j'));
    assert!(matches!(named("Tab"), Event::Key(_)));
}

#[test]
fn a_gate_holds_a_call_until_its_key_opens_it_and_a_dropped_key_opens_it_too() {
    let reply = Reply::ok("{}");
    let (gate, key) = Gate::new(reply.clone());
    let waiting = thread::spawn({
        let gate = gate.clone();
        move || gate.handle(&call())
    });
    assert!(!waiting.is_finished(), "the call waits for the key");
    key.open();
    assert_eq!(waiting.join().unwrap(), reply);

    let (gate, key) = Gate::new(reply.clone());
    drop(key);
    assert_eq!(
        gate.handle(&call()),
        reply,
        "a key that is gone opens the gate"
    );
}
