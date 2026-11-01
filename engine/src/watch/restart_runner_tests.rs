//! `restart::rebuild_cockpit_with` on a [`FakeRunner`]: the `cpd tui` of each restart step, with the env the code sets today.

use std::io::ErrorKind;
use std::path::Path;

use super::*;
use cpd_core::clock::ManualClock;
use cpd_core::host::runner::{Call, FakeRunner, Reply};

const EXE: &str = "/opt/cpd/bin/cpd";
const ROOT: &str = "/cpd-home";

fn home() -> CpdHome {
    CpdHome::resolve(Some(ROOT.into()), None).unwrap()
}

fn fake(reply: Reply) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).on("cpd", |_| true, reply)
}

fn rebuild(runner: &FakeRunner) -> Result<(), CpdError> {
    rebuild_cockpit_with(runner, Path::new(EXE), &home())
}

fn the_call(runner: &FakeRunner) -> Call {
    let calls = runner.calls();
    assert_eq!(calls.len(), 1, "one process per rebuild");
    calls[0].clone()
}

#[test]
fn rebuild_cockpit_with_runs_the_recorded_executable_with_tui_once() {
    let runner = fake(Reply::ok(""));
    rebuild(&runner).unwrap();
    let call = the_call(&runner);
    assert_eq!(
        (call.program.as_str(), call.args.as_slice()),
        (EXE, ["tui".to_owned()].as_slice())
    );
    assert_eq!(call.tool, "cpd");
}

#[test]
fn rebuild_cockpit_with_runs_in_the_home_with_the_home_in_the_environment() {
    let runner = fake(Reply::ok(""));
    rebuild(&runner).unwrap();
    let call = the_call(&runner);
    assert_eq!(call.cwd.as_deref(), Some(Path::new(ROOT)));
    assert!(
        call.envs
            .contains(&("CPD_HOME".to_owned(), Some(ROOT.to_owned()))),
        "{:?}",
        call.envs
    );
}

#[test]
fn rebuild_cockpit_with_removes_the_selection_and_the_tmux_variables() {
    let runner = fake(Reply::ok(""));
    rebuild(&runner).unwrap();
    let envs = the_call(&runner).envs;
    for name in [
        "CPD_PLAN",
        "CPD_SESSION",
        "CPD_ATTEMPT",
        "TMUX",
        "TMUX_PANE",
    ] {
        assert!(
            envs.contains(&(name.to_owned(), None)),
            "{name} is removed: {envs:?}"
        );
    }
}

#[test]
fn rebuild_cockpit_with_gives_the_child_a_null_stdin_and_captures_its_stderr() {
    let runner = fake(Reply::ok(""));
    rebuild(&runner).unwrap();
    let call = the_call(&runner);
    assert_eq!((call.stdin, call.stderr_to), (None, None));
}

#[test]
fn rebuild_cockpit_with_a_failing_tui_is_tmux_failed_with_the_tail_of_its_output() {
    let runner = fake(Reply::exit(1, "no session\n", " and no server"));
    let e = rebuild(&runner).unwrap_err();
    assert_eq!(e.code(), ErrorCode::TmuxFailed);
    assert_eq!(
        e.message(),
        "cpd tui failed after the server was killed: no session and no server"
    );
}

#[test]
fn rebuild_cockpit_with_a_program_that_cannot_start_is_spawn_failed() {
    let runner = fake(Reply::io(ErrorKind::NotFound));
    let e = rebuild(&runner).unwrap_err();
    assert_eq!(e.code(), ErrorCode::SpawnFailed);
    assert!(
        e.message().starts_with("Cannot run cpd tui: "),
        "{}",
        e.message()
    );
}

#[test]
fn rebuild_cockpit_with_a_second_restart_step_runs_tui_again() {
    let runner = fake(Reply::ok(""));
    rebuild(&runner).unwrap();
    rebuild(&runner).unwrap();
    assert_eq!(runner.calls().len(), 2);
    assert!(runner.unscripted().is_empty());
}
