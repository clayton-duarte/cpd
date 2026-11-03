//! `PollRun` (E5's non-blocking poll). Jobs block on a channel the test releases; no sleeps, no clock waits.

use std::sync::mpsc::{Sender, channel};
use std::time::Instant;

use serde_json::{Value, json};

use super::poll_run::PollRun;
use super::report::Failure;

/// Releases the blocked job when dropped, so no thread outlives a test, whatever its outcome.
struct Release(Sender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

fn blocked(result: Result<Value, Failure>) -> (Release, PollRun) {
    let (tx, rx) = channel::<()>();
    let run = PollRun::start(move || {
        let _ = rx.recv();
        result
    });
    (Release(tx), run)
}

#[test]
fn starting_a_poll_does_not_wait_for_it() {
    let (release, mut run) = blocked(Ok(json!({})));
    // `start` returned while the job is still blocked.
    drop(release);
    assert!(run.wait().is_some());
}

#[test]
fn a_running_poll_gives_nothing_to_take() {
    let (release, mut run) = blocked(Ok(json!({})));
    let taken = run.try_take();
    drop(release);
    let none = taken.is_none();
    run.wait();
    assert!(none, "a blocked job has nothing to take");
}

#[test]
fn waiting_returns_the_result_and_end_time_of_the_poll_once() {
    let before = Instant::now();
    let (release, mut run) = blocked(Ok(json!({"polled": 3})));
    drop(release);
    let finished = run.wait().expect("the finished poll");
    let after = Instant::now();
    assert_eq!(
        finished.result.expect("the job's result"),
        json!({"polled": 3})
    );
    assert!(finished.ended >= before && finished.ended <= after);
    assert!(run.wait().is_none(), "a second wait");
    assert!(
        run.try_take().is_none(),
        "try_take after the result was taken"
    );
}

#[test]
fn a_panicking_poll_is_a_failure_and_not_a_dead_engine() {
    let run = PollRun::start(|| {
        // The panic is caught by the poll thread's catch_unwind; resume_unwind prints no message.
        std::panic::resume_unwind(Box::new("poll exploded"))
    });
    let mut run = run;
    let finished = run.wait().expect("the finished poll");
    let failure = finished.result.expect_err("a panic is a failure");
    assert_eq!(failure.code, "spawn_failed");
    assert!(
        failure.message.contains("poll exploded"),
        "{}",
        failure.message
    );
}

#[test]
fn the_poll_output_reaches_the_engine_unchanged_so_the_rebind_is_the_polls_own() {
    // The rebind of a reopened pull request (pro-d362) runs inside `cpd pr poll`: the engine's `PollRun` only carries the
    // poll's JSON, whatever it holds, and does no reopen work of its own.
    let out = json!({"polled": 1, "changed": [], "reopened": [{"plan": "pay", "key": "api"}]});
    let (release, mut run) = blocked(Ok(out.clone()));
    drop(release);
    let finished = run.wait().expect("the finished poll");
    assert_eq!(finished.result.expect("the job's result"), out);
}
