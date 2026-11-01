//! `Call.stdin` and `Call.stderr_to` (docs/implementing.md, Tests): recorded and readable by a `Handler`; the
//! `Command` of a `Call`; a hung call over `Runner::run`. No process is started.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::deadline::Failed;
use super::runner::{Call, FakeRunner, Handler, Reply, Runner, command};
use crate::clock::ManualClock;
use crate::error::ErrorCode;

const DEADLINE: Duration = Duration::from_secs(10);

fn call(stdin: Option<&[u8]>, stderr_to: Option<&str>) -> Call {
    Call {
        tool: "op".into(),
        program: "op".into(),
        args: vec!["item".into(), "create".into()],
        envs: vec![],
        cwd: None,
        deadline: DEADLINE,
        stdin: stdin.map(<[u8]>::to_vec),
        stderr_to: stderr_to.map(PathBuf::from),
    }
}

fn fake() -> (FakeRunner, ManualClock) {
    let clock = ManualClock::new();
    (FakeRunner::new(clock.clone()), clock)
}

/// What a call read: its stdin bytes and its stdin file.
type Read = (Option<Vec<u8>>, Option<PathBuf>);

#[derive(Default)]
struct Reads(Mutex<Vec<Read>>);

impl Handler for Reads {
    fn handle(&self, call: &Call) -> Reply {
        self.0
            .lock()
            .unwrap()
            .push((call.stdin.clone(), call.stderr_to.clone()));
        Reply::ok("done")
    }
}

#[test]
fn a_call_with_stdin_and_a_stderr_file_is_recorded_by_calls() {
    let (runner, _) = fake();
    let runner = runner.on("op", |_| true, Reply::ok(""));
    let sent = call(Some(b"secret"), Some("/logs/pi.log"));
    runner.run(&sent).unwrap();
    assert_eq!(runner.calls(), vec![sent]);
}

#[test]
fn a_call_without_stdin_or_a_stderr_file_is_recorded_with_none() {
    let (runner, _) = fake();
    let runner = runner.on("op", |_| true, Reply::ok(""));
    runner.run(&call(None, None)).unwrap();
    let recorded = runner.calls();
    assert_eq!(
        (recorded[0].stdin.clone(), recorded[0].stderr_to.clone()),
        (None, None)
    );
}

#[test]
fn a_handler_reads_the_stdin_and_the_stderr_file_of_the_call() {
    let (runner, _) = fake();
    let reads = Arc::new(Reads::default());
    let runner = runner.route("op", reads.clone());
    let out = runner
        .run(&call(Some(b"secret"), Some("/logs/pi.log")))
        .unwrap();
    assert_eq!(out.stdout, b"done");
    assert_eq!(
        *reads.0.lock().unwrap(),
        vec![(
            Some(b"secret".to_vec()),
            Some(PathBuf::from("/logs/pi.log"))
        )]
    );
}

#[test]
fn a_rule_can_match_on_the_stdin_of_the_call() {
    let (runner, _) = fake();
    let runner = runner.on(
        "op",
        |c| c.stdin.as_deref() == Some(b"yes".as_slice()),
        Reply::ok("matched"),
    );
    assert_eq!(
        runner.run(&call(Some(b"yes"), None)).unwrap().stdout,
        b"matched"
    );
    assert!(matches!(
        runner.run(&call(Some(b"no"), None)),
        Err(Failed::Io(_))
    ));
    assert_eq!(runner.unscripted().len(), 1);
}

#[test]
fn a_run_that_takes_the_deadline_is_a_timeout_on_virtual_time() {
    let (runner, clock) = fake();
    let runner = runner.on(
        "op",
        |_| true,
        Reply::ok("").takes(DEADLINE + Duration::from_secs(1)),
    );
    let Err(Failed::Timeout(e)) = runner.run(&call(Some(b"x"), None)) else {
        panic!("not a timeout")
    };
    assert_eq!(e.code(), ErrorCode::ToolTimeout);
    assert_eq!(e.details()["tool"], "op");
    assert_eq!(clock.elapsed(), DEADLINE);
}

#[test]
fn a_run_that_takes_less_than_the_deadline_passes_its_time_and_returns_its_output() {
    let (runner, clock) = fake();
    let runner = runner.on(
        "op",
        |_| true,
        Reply::exit(3, "out", "err").takes(Duration::from_secs(4)),
    );
    let out = runner.run(&call(None, None)).unwrap();
    assert_eq!(
        (out.stdout, out.stderr, out.status.code()),
        (b"out".to_vec(), b"err".to_vec(), Some(3))
    );
    assert_eq!(clock.elapsed(), Duration::from_secs(4));
}

#[test]
fn the_command_of_a_call_has_its_program_args_cwd_and_envs() {
    let mut c = call(Some(b"in"), Some("/logs/pi.log"));
    c.program = "/usr/bin/pi".into();
    c.args = vec!["--print".into(), "--".into(), "go".into()];
    c.envs = vec![
        ("PI_SKIP_VERSION_CHECK".into(), Some("1".into())),
        ("CPD_SESSION".into(), None),
    ];
    c.cwd = Some(PathBuf::from("/work"));
    let cmd = command(&c);
    assert_eq!(cmd.get_program(), "/usr/bin/pi");
    assert_eq!(cmd.get_args().collect::<Vec<_>>(), ["--print", "--", "go"]);
    assert_eq!(cmd.get_current_dir(), Some(std::path::Path::new("/work")));
    // `Command` keeps its environment in a map sorted by key: the test pins the set, not an order.
    let mut envs: Vec<_> = cmd
        .get_envs()
        .map(|(k, v)| (k.to_str().unwrap(), v.map(|v| v.to_str().unwrap())))
        .collect();
    envs.sort();
    assert_eq!(
        envs,
        [("CPD_SESSION", None), ("PI_SKIP_VERSION_CHECK", Some("1"))]
    );
}

#[test]
fn the_command_of_a_call_with_no_cwd_and_no_envs_has_none() {
    let cmd = command(&call(None, None));
    assert_eq!(cmd.get_current_dir(), None);
    assert_eq!(cmd.get_envs().count(), 0);
}
