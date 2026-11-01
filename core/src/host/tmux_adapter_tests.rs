//! `Tmux` on a `FakeRunner` where its tests used to run a shell script as `tmux`: a server that dies is a scripted
//! exit 1 and then answers, time is the `ManualClock`, and `installed_version` and `display-popup` go through the runner.

use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use super::runner::{FakeRunner, Reply};
use super::tmux::{Tmux, installed_version_with};
use crate::clock::ManualClock;
use crate::error::ErrorCode;

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

fn lists_sessions(call: &super::runner::Call) -> bool {
    call.args.iter().any(|a| a == "list-sessions")
}

fn kills_server(call: &super::runner::Call) -> bool {
    call.args.iter().any(|a| a == "kill-server")
}

/// A `tmux` as one is while its server dies after `kill-server` (hs-n285): `kill-server` exits 1 with `server exited
/// unexpectedly`; the first answers to `list-sessions` are `server exited unexpectedly`, then `lost server`, then a
/// refused connection, then `no server running`.
fn dying_server() -> (FakeRunner, ManualClock, Tmux) {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone())
        .once(
            "tmux",
            kills_server,
            Reply::exit(1, "", "server exited unexpectedly\n"),
        )
        .once(
            "tmux",
            lists_sessions,
            Reply::exit(1, "", "server exited unexpectedly\n"),
        )
        .once(
            "tmux",
            lists_sessions,
            Reply::exit(1, "", "server exited unexpectedly\n"),
        )
        .once("tmux", lists_sessions, Reply::exit(1, "", "lost server\n"))
        .once(
            "tmux",
            lists_sessions,
            Reply::exit(1, "", "error connecting to /tmp/x (Connection refused)\n"),
        )
        .on(
            "tmux",
            lists_sessions,
            Reply::exit(1, "", "no server running on /tmp/x\n"),
        );
    let tmux = Tmux::new("dying")
        .with_runner(Arc::new(runner.clone()))
        .with_clock(Arc::new(clock.clone()));
    (runner, clock, tmux)
}

#[test]
fn a_dying_server_answering_server_exited_unexpectedly_does_not_fail_kill_server() {
    let (runner, _clock, tmux) = dying_server();
    tmux.kill_server().unwrap();
    let calls = runner.calls();
    let listed = calls.iter().filter(|c| lists_sessions(c)).count();
    assert_eq!(
        listed, 4,
        "it waited until the connection was refused: {calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|c| c.args == ["-u", "-L", "dying", "-f", "/dev/null", "kill-server"]),
        "{calls:?}"
    );
}

#[test]
fn kill_server_naps_fifty_milliseconds_on_the_clock_between_the_answers_of_a_dying_server() {
    let (_runner, clock, tmux) = dying_server();
    tmux.kill_server().unwrap();
    assert_eq!(
        clock.naps(),
        [Duration::from_millis(50), Duration::from_millis(50)]
    );
}

#[test]
fn kill_server_of_a_server_that_keeps_answering_fails_after_ten_seconds_of_virtual_time() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("tmux", |_| true, Reply::ok(""));
    let tmux = Tmux::new("stuck")
        .with_runner(Arc::new(runner))
        .with_clock(Arc::new(clock.clone()));
    let err = tmux.kill_server().unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert!(
        err.message().contains("did not exit within 10 s"),
        "{}",
        err.message()
    );
    assert!(
        clock.elapsed() >= Duration::from_secs(10),
        "{:?}",
        clock.elapsed()
    );
}

#[test]
fn installed_version_runs_tmux_u_dash_v_through_the_runner_and_drops_the_prefix() {
    let runner = FakeRunner::new(ManualClock::new()).on("tmux", |_| true, Reply::ok("tmux 3.6a\n"));
    assert_eq!(installed_version_with(&runner).unwrap(), "3.6a");
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].program.as_str(), calls[0].args.clone()),
        ("tmux", strings(&["-u", "-V"]))
    );
}

#[test]
fn installed_version_that_prints_no_tmux_version_is_tmux_failed_with_the_argv() {
    let runner = FakeRunner::new(ManualClock::new()).on("tmux", |_| true, Reply::ok("not tmux\n"));
    let err = installed_version_with(&runner).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert!(err.message().contains("did not print"), "{}", err.message());
}

#[test]
fn installed_version_without_a_tmux_is_tmux_failed_cannot_run() {
    let runner =
        FakeRunner::new(ManualClock::new()).on("tmux", |_| true, Reply::io(ErrorKind::NotFound));
    let err = installed_version_with(&runner).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert!(
        err.message().contains("Cannot run tmux"),
        "{}",
        err.message()
    );
}

#[test]
fn a_display_popup_has_no_deadline_but_runs_through_the_runner_too() {
    // `/nonexistent/bin/tmux` is not a file: only a runner that answers for it can make the call succeed.
    let dir = tempfile::tempdir().unwrap();
    let runner = FakeRunner::new(ManualClock::new()).on("tmux", |_| true, Reply::ok(""));
    let tmux = Tmux::with_tmpdir("t", dir.path())
        .with_program("/nonexistent/bin/tmux")
        .with_runner(Arc::new(runner.clone()));
    tmux.run(&strings(&[
        "display-popup",
        "-E",
        "-T",
        "t",
        "cpd tui view legend",
    ]))
    .unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "/nonexistent/bin/tmux");
    assert_eq!(
        calls[0].args,
        strings(&[
            "-u",
            "-L",
            "t",
            "-f",
            "/dev/null",
            "display-popup",
            "-E",
            "-T",
            "t",
            "cpd tui view legend"
        ])
    );
}
