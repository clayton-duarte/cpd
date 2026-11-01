//! `distill::wait_limited_with`, `distill::run_pi_with` and `session::retitle::run_pi_with` on a `FakeRunner` and a
//! `ManualClock`: the `pi` call and how the answer is mapped. No process.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::distill::{Waited, run_pi_with, wait_limited_with};
use crate::clock::ManualClock;
use crate::error::ErrorCode;
use crate::host::pi::Launch;
use crate::host::runner::{Call, FakeRunner, Reply};
use crate::ids::SessionId;
use crate::session::retitle::{NO_DEADLINE, run_pi_with as retitle_pi};

const LIMIT: Duration = Duration::from_secs(15 * 60);
const DIR: &str = "/home/context/digest";
const LOG: &str = "/home/context/digest/distill.log";

fn fake(reply: Reply) -> (FakeRunner, ManualClock) {
    let clock = ManualClock::new();
    (
        FakeRunner::new(clock.clone()).on("pi", |_| true, reply),
        clock,
    )
}

fn launch() -> Launch {
    Launch {
        argv: ["pi", "--mode", "json", "--model", "m:high", "@prompt.md"]
            .map(str::to_owned)
            .to_vec(),
        env: BTreeMap::from([("PI_SKIP_VERSION_CHECK".to_owned(), "1".to_owned())]),
    }
}

fn environ() -> Vec<String> {
    ["HOME", "CPD_HOME", "CPD_SESSION", "PATH", "CPD_ATTEMPT"]
        .map(str::to_owned)
        .to_vec()
}

fn pi_call() -> Call {
    Call {
        tool: "pi".into(),
        program: "pi".into(),
        args: vec!["--mode".into(), "json".into()],
        envs: vec![],
        cwd: Some(PathBuf::from(DIR)),
        deadline: LIMIT,
        stdin: None,
        stderr_to: Some(PathBuf::from(LOG)),
    }
}

#[test]
fn wait_limited_with_runs_the_call_under_the_limit_and_returns_its_status_and_stdout() {
    let (runner, _) = fake(Reply::exit(3, "{}\n", "boom"));
    let Ok(Waited::Exited { status, stdout }) = wait_limited_with(&runner, &pi_call(), LIMIT)
    else {
        panic!("did not exit")
    };
    assert_eq!(status.code(), Some(3));
    assert_eq!(stdout.unwrap(), b"{}\n");
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].deadline, LIMIT);
    assert_eq!(calls[0].stderr_to.as_deref(), Some(Path::new(LOG)));
}

#[test]
fn a_hung_pi_is_timed_out_at_the_limit_on_virtual_time() {
    let (runner, clock) = fake(Reply::ok("").takes(LIMIT + Duration::from_secs(1)));
    assert!(matches!(
        wait_limited_with(&runner, &pi_call(), LIMIT),
        Ok(Waited::TimedOut)
    ));
    assert_eq!(clock.elapsed(), LIMIT);
}

#[test]
fn a_pi_that_cannot_start_is_an_io_error() {
    let (runner, _) = fake(Reply::io(ErrorKind::NotFound));
    let e = wait_limited_with(&runner, &pi_call(), LIMIT).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound);
}

#[test]
fn run_pi_with_calls_pi_with_the_launch_argv_and_env_in_the_digest_dir() {
    let (runner, _) = fake(Reply::ok("{}"));
    run_pi_with(
        &runner,
        &launch(),
        Path::new(DIR),
        Path::new(LOG),
        LIMIT,
        &environ(),
    )
    .unwrap();
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "pi");
    assert_eq!(
        calls[0].args,
        ["--mode", "json", "--model", "m:high", "@prompt.md"]
    );
    assert_eq!(calls[0].cwd.as_deref(), Some(Path::new(DIR)));
    assert_eq!(calls[0].deadline, LIMIT);
    assert_eq!(calls[0].stdin, None);
    assert!(
        calls[0]
            .envs
            .contains(&("PI_SKIP_VERSION_CHECK".to_owned(), Some("1".to_owned())))
    );
}

#[test]
fn run_pi_with_sends_pis_stderr_to_the_log_file() {
    let (runner, _) = fake(Reply::ok("{}"));
    run_pi_with(
        &runner,
        &launch(),
        Path::new(DIR),
        Path::new(LOG),
        LIMIT,
        &environ(),
    )
    .unwrap();
    assert_eq!(runner.calls()[0].stderr_to.as_deref(), Some(Path::new(LOG)));
}

#[test]
fn run_pi_with_removes_every_cpd_variable_of_the_environment_and_no_other() {
    let (runner, _) = fake(Reply::ok("{}"));
    run_pi_with(
        &runner,
        &launch(),
        Path::new(DIR),
        Path::new(LOG),
        LIMIT,
        &environ(),
    )
    .unwrap();
    let envs = runner.calls()[0].envs.clone();
    for name in ["CPD_HOME", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert!(
            envs.contains(&(name.to_owned(), None)),
            "{name} is removed: {envs:?}"
        );
    }
    for name in ["HOME", "PATH"] {
        assert!(
            envs.iter().all(|(k, _)| k != name),
            "{name} is left alone: {envs:?}"
        );
    }
}

#[test]
fn run_pi_with_a_hung_pi_is_timed_out_at_the_distill_limit() {
    let (runner, clock) = fake(Reply::ok("").takes(LIMIT));
    let waited = run_pi_with(
        &runner,
        &launch(),
        Path::new(DIR),
        Path::new(LOG),
        LIMIT,
        &environ(),
    )
    .unwrap();
    assert!(matches!(waited, Waited::TimedOut), "{waited:?}");
    assert_eq!(clock.elapsed(), LIMIT);
}

#[test]
fn run_pi_with_returns_the_exit_status_and_the_stdout_of_pi() {
    let (runner, _) = fake(Reply::exit(1, "out", "err"));
    let Waited::Exited { status, stdout } = run_pi_with(
        &runner,
        &launch(),
        Path::new(DIR),
        Path::new(LOG),
        LIMIT,
        &environ(),
    )
    .unwrap() else {
        panic!("did not exit")
    };
    assert_eq!((status.code(), stdout.unwrap()), (Some(1), b"out".to_vec()));
}

fn session() -> SessionId {
    SessionId::parse("4965bfc8").unwrap()
}

const TITLES: &str = "/home/run/titles";
const TEXT: &str = "user: fix the build\nassistant: done";

fn retitle(
    reply: Reply,
) -> (
    Result<crate::args::Title, crate::error::CpdError>,
    FakeRunner,
) {
    let (runner, _) = fake(reply);
    let result = retitle_pi(
        &runner,
        &session(),
        "g-model",
        Path::new(TITLES),
        TEXT,
        60,
        &environ(),
    );
    (result, runner)
}

#[test]
fn a_retitle_sends_the_text_as_stdin_to_pi() {
    let (_, runner) = retitle(Reply::ok("TITLE: Fix The Build\n"));
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "pi");
    assert_eq!(calls[0].stdin.as_deref(), Some(TEXT.as_bytes()));
}

#[test]
fn a_retitle_has_no_deadline_as_today() {
    let (_, runner) = retitle(Reply::ok("TITLE: Fix The Build\n"));
    assert_eq!(runner.calls()[0].deadline, NO_DEADLINE);
}

#[test]
fn a_retitle_runs_pi_print_on_the_model_in_the_titles_dir_without_cpd_variables() {
    let (_, runner) = retitle(Reply::ok("TITLE: Fix The Build\n"));
    let call = &runner.calls()[0];
    assert_eq!(
        &call.args[..6],
        [
            "--print",
            "--no-session",
            "--no-extensions",
            "--model",
            "g-model",
            "--"
        ]
    );
    assert_eq!(call.cwd.as_deref(), Some(Path::new(TITLES)));
    assert!(
        call.envs
            .contains(&("PI_SKIP_VERSION_CHECK".to_owned(), Some("1".to_owned())))
    );
    assert!(call.envs.contains(&("CPD_SESSION".to_owned(), None)));
    assert!(call.envs.iter().all(|(k, _)| k != "HOME"));
}

#[test]
fn a_retitle_returns_the_title_pi_printed() {
    let (title, _) = retitle(Reply::ok("thinking\nTITLE: Fix The Build\n"));
    assert_eq!(title.unwrap().as_str(), "Fix The Build");
}

#[test]
fn a_pi_that_exits_non_zero_is_title_failed_with_its_stderr_tail() {
    let (result, _) = retitle(Reply::exit(3, "", "one\ntwo\n"));
    let e = result.unwrap_err();
    assert_eq!(e.code(), ErrorCode::TitleFailed);
    assert_eq!(e.details()["reason"], "exit");
    assert_eq!(e.details()["exit"], 3);
    assert_eq!(e.details()["tail"], "one\ntwo");
}

#[test]
fn a_pi_that_cannot_start_is_title_failed_spawn() {
    let (result, _) = retitle(Reply::io(ErrorKind::NotFound));
    let e = result.unwrap_err();
    assert_eq!(e.code(), ErrorCode::TitleFailed);
    assert_eq!(e.details()["reason"], "spawn");
}

#[test]
fn an_empty_answer_is_title_failed_empty() {
    let (result, _) = retitle(Reply::ok("  \n"));
    assert_eq!(result.unwrap_err().details()["reason"], "empty");
}
