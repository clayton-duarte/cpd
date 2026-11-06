//! The contract of [`FakeRunner`] (docs/implementing.md, Tests): scripted answers, the record, virtual time.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::deadline::{self, Failed};
use super::runner::{Call, FakeRunner, Handler, Reply, Runner};
use crate::clock::ManualClock;
use crate::error::ErrorCode;

const DEADLINE: Duration = Duration::from_secs(10);

fn fake() -> (FakeRunner, ManualClock) {
    let clock = ManualClock::new();
    (FakeRunner::new(clock.clone()), clock)
}

fn run(runner: &dyn Runner, program: &str, args: &[&str]) -> Result<std::process::Output, Failed> {
    runner.output(Command::new(program).args(args), "tool", DEADLINE)
}

fn any(_: &Call) -> bool {
    true
}

#[test]
fn a_scripted_call_gets_its_scripted_stdout_stderr_and_exit_code() {
    let (runner, _) = fake();
    let runner = runner.on("git", any, Reply::exit(3, "out", "err"));
    let out = run(&runner, "git", &["status"]).unwrap();
    assert_eq!(out.stdout, b"out");
    assert_eq!(out.stderr, b"err");
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn an_ok_reply_exits_zero_with_its_stdout() {
    let (runner, _) = fake();
    let runner = runner.on("git", any, Reply::ok("fine\n"));
    let out = run(&runner, "git", &[]).unwrap();
    assert_eq!(out.stdout, b"fine\n");
    assert!(out.stderr.is_empty());
    assert!(out.status.success());
}

#[test]
fn every_call_is_recorded_in_order_with_what_the_command_carries() {
    let (runner, _) = fake();
    let runner = runner
        .on("git", any, Reply::ok(""))
        .on("tmux", any, Reply::ok(""));
    let mut first = Command::new("git");
    first
        .args(["-C", "/repo", "status"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("TMUX")
        .current_dir("/repo");
    runner
        .output(&mut first, "git", Duration::from_secs(60))
        .unwrap();
    runner
        .output(
            Command::new("tmux").arg("-V"),
            "tmux",
            Duration::from_secs(10),
        )
        .unwrap();

    assert_eq!(
        runner.calls(),
        vec![
            Call {
                tool: "git".into(),
                program: "git".into(),
                args: vec!["-C".into(), "/repo".into(), "status".into()],
                envs: vec![
                    ("GIT_TERMINAL_PROMPT".into(), Some("0".into())),
                    ("TMUX".into(), None)
                ],
                cwd: Some(PathBuf::from("/repo")),
                deadline: Duration::from_secs(60),
                stdin: None,
                stderr_to: None,
            },
            Call {
                tool: "tmux".into(),
                program: "tmux".into(),
                args: vec!["-V".into()],
                envs: vec![],
                cwd: None,
                deadline: Duration::from_secs(10),
                stdin: None,
                stderr_to: None,
            },
        ]
    );
}

#[test]
fn a_call_no_rule_matches_is_not_found_and_is_unscripted_and_recorded() {
    let (runner, _) = fake();
    let runner = runner.on("git", any, Reply::ok(""));
    let err = run(&runner, "tmux", &["-V"]).unwrap_err();
    assert!(matches!(err, Failed::Io(e) if e.kind() == ErrorKind::NotFound));
    assert_eq!(runner.unscripted().len(), 1);
    assert_eq!(runner.unscripted()[0].program, "tmux");
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_call_a_rule_answers_is_not_unscripted() {
    let (runner, _) = fake();
    let runner = runner.on("git", any, Reply::ok(""));
    run(&runner, "git", &[]).unwrap();
    assert!(runner.unscripted().is_empty());
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_rule_that_does_not_match_the_arguments_leaves_the_call_unscripted() {
    let (runner, _) = fake();
    let runner = runner.on(
        "git",
        |c| c.args.first().is_some_and(|a| a == "status"),
        Reply::ok(""),
    );
    assert!(run(&runner, "git", &["fetch"]).is_err());
    assert_eq!(runner.unscripted().len(), 1);
}

#[test]
fn the_first_matching_rule_answers() {
    let (runner, _) = fake();
    let runner = runner
        .on("git", any, Reply::ok("first"))
        .on("git", any, Reply::ok("second"));
    assert_eq!(run(&runner, "git", &[]).unwrap().stdout, b"first");
    assert_eq!(run(&runner, "git", &[]).unwrap().stdout, b"first");
}

#[test]
fn a_once_rule_is_used_up_and_then_the_next_matching_rule_answers() {
    let (runner, _) = fake();
    let runner = runner
        .once("git", any, Reply::ok("once"))
        .on("git", any, Reply::ok("always"));
    assert_eq!(run(&runner, "git", &[]).unwrap().stdout, b"once");
    assert_eq!(run(&runner, "git", &[]).unwrap().stdout, b"always");
    assert_eq!(run(&runner, "git", &[]).unwrap().stdout, b"always");
}

#[test]
fn a_used_up_once_rule_with_no_rule_after_it_leaves_the_call_unscripted() {
    let (runner, _) = fake();
    let runner = runner.once("git", any, Reply::ok("once"));
    run(&runner, "git", &[]).unwrap();
    assert!(
        matches!(run(&runner, "git", &[]), Err(Failed::Io(e)) if e.kind() == ErrorKind::NotFound)
    );
    assert_eq!(runner.unscripted().len(), 1);
}

#[test]
fn the_program_is_matched_by_its_file_name() {
    let (runner, _) = fake();
    let runner = runner.on("tmux", any, Reply::ok("tmux answered"));
    assert_eq!(
        run(&runner, "/x/bin/tmux", &[]).unwrap().stdout,
        b"tmux answered"
    );
    assert!(runner.unscripted().is_empty());
}

#[test]
fn a_program_that_only_ends_with_the_name_does_not_match() {
    let (runner, _) = fake();
    let runner = runner.on("tmux", any, Reply::ok(""));
    assert!(run(&runner, "/x/bin/not-tmux", &[]).is_err());
    assert_eq!(runner.unscripted().len(), 1);
}

#[test]
fn a_reply_that_takes_the_deadline_is_a_tool_timeout_equal_to_a_real_one() {
    let (runner, clock) = fake();
    let runner = runner.on("git", any, Reply::ok("late").takes(DEADLINE));
    let err = match run(&runner, "git", &["fetch", "origin"]) {
        Err(Failed::Timeout(e)) => e,
        other => panic!("expected a timeout, got {other:?}"),
    };
    let argv = vec!["git".to_owned(), "fetch".to_owned(), "origin".to_owned()];
    assert_eq!(err, deadline::timeout("tool", argv.clone(), DEADLINE));
    assert_eq!(err.code(), ErrorCode::ToolTimeout);
    assert_eq!(err.exit().code(), 5);
    assert_eq!(err.details()["tool"], "tool");
    assert_eq!(err.details()["argv"], serde_json::json!(argv));
    assert_eq!(err.details()["deadline_seconds"], 10);
    assert_eq!(clock.elapsed(), DEADLINE);
}

#[test]
fn a_reply_that_takes_longer_than_the_deadline_moves_the_clock_by_the_deadline_only() {
    let (runner, clock) = fake();
    let runner = runner.on("git", any, Reply::ok("").takes(DEADLINE * 3));
    assert!(matches!(run(&runner, "git", &[]), Err(Failed::Timeout(_))));
    assert_eq!(clock.elapsed(), DEADLINE);
}

#[test]
fn a_reply_that_takes_less_than_the_deadline_returns_its_output_and_moves_the_clock_by_what_it_took()
 {
    let (runner, clock) = fake();
    let runner = runner.on(
        "git",
        any,
        Reply::exit(0, "done", "").takes(Duration::from_secs(4)),
    );
    let out = run(&runner, "git", &[]).unwrap();
    assert_eq!(out.stdout, b"done");
    assert_eq!(clock.elapsed(), Duration::from_secs(4));
}

#[test]
fn a_reply_that_takes_nothing_leaves_the_clock_alone() {
    let (runner, clock) = fake();
    let runner = runner.on("git", any, Reply::ok(""));
    run(&runner, "git", &[]).unwrap();
    assert_eq!(clock.elapsed(), Duration::ZERO);
}

#[test]
fn a_signal_reply_ends_on_that_signal_with_no_exit_code_and_its_output() {
    use std::os::unix::process::ExitStatusExt;
    let (runner, clock) = fake();
    let runner = runner.on("sh", any, Reply::signal(9, "started\n", "cut\n"));
    let out = run(&runner, "sh", &[]).unwrap();
    assert_eq!((out.status.code(), out.status.signal()), (None, Some(9)));
    assert_eq!(
        (out.stdout, out.stderr),
        (b"started\n".to_vec(), b"cut\n".to_vec())
    );
    assert_eq!(clock.elapsed(), Duration::ZERO);
}

#[test]
fn an_io_reply_is_an_io_failure_of_that_kind() {
    let (runner, _) = fake();
    let runner = runner.on("git", any, Reply::io(ErrorKind::PermissionDenied));
    assert!(
        matches!(run(&runner, "git", &[]), Err(Failed::Io(e)) if e.kind() == ErrorKind::PermissionDenied)
    );
    assert!(runner.unscripted().is_empty());
}

#[test]
fn a_clone_shares_the_script_and_the_record() {
    let (runner, _) = fake();
    let runner = runner.once("git", any, Reply::ok("once"));
    let clone = runner.clone();
    assert_eq!(run(&clone, "git", &[]).unwrap().stdout, b"once");
    assert!(
        run(&runner, "git", &[]).is_err(),
        "the clone used the once rule up"
    );
    assert_eq!(runner.calls().len(), 2);
    assert_eq!(clone.calls(), runner.calls());
    assert_eq!(clone.unscripted().len(), 1);
}

struct Recording {
    seen: Mutex<Vec<Call>>,
}

impl Handler for Recording {
    fn handle(&self, call: &Call) -> Reply {
        self.seen.lock().unwrap().push(call.clone());
        Reply::ok("from the route")
    }
}

fn recording() -> Arc<Recording> {
    Arc::new(Recording {
        seen: Mutex::new(Vec::new()),
    })
}

#[test]
fn a_route_answers_a_call_of_its_program_that_no_rule_matches() {
    let (runner, _) = fake();
    let handler = recording();
    let runner = runner.route("tmux", handler.clone());
    assert_eq!(
        run(&runner, "tmux", &["ls"]).unwrap().stdout,
        b"from the route"
    );
    assert!(runner.unscripted().is_empty());
    assert_eq!(handler.seen.lock().unwrap().len(), 1);
}

#[test]
fn a_route_does_not_answer_another_program() {
    let (runner, _) = fake();
    let handler = recording();
    let runner = runner.route("tmux", handler.clone());
    assert!(run(&runner, "git", &[]).is_err());
    assert_eq!(runner.unscripted().len(), 1);
    assert!(handler.seen.lock().unwrap().is_empty());
}

#[test]
fn a_rule_wins_over_the_route() {
    let (runner, _) = fake();
    let handler = recording();
    let runner =
        runner
            .route("tmux", handler.clone())
            .on("tmux", |c| c.args == ["-V"], Reply::ok("rule"));
    assert_eq!(run(&runner, "tmux", &["-V"]).unwrap().stdout, b"rule");
    assert_eq!(
        run(&runner, "tmux", &["ls"]).unwrap().stdout,
        b"from the route"
    );
    assert_eq!(
        handler.seen.lock().unwrap().len(),
        1,
        "the route saw only the call no rule matched"
    );
}

#[test]
fn the_handler_sees_the_calls_program_args_envs_and_cwd() {
    let (runner, _) = fake();
    let handler = recording();
    let runner = runner.route("tmux", handler.clone());
    let mut cmd = Command::new("/x/bin/tmux");
    cmd.args(["-L", "sock", "ls"])
        .env("TMUX_TMPDIR", "/t")
        .env_remove("TMUX")
        .current_dir("/w");
    runner.output(&mut cmd, "tmux", DEADLINE).unwrap();
    let seen = handler.seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![Call {
            tool: "tmux".into(),
            program: "/x/bin/tmux".into(),
            args: vec!["-L".into(), "sock".into(), "ls".into()],
            envs: vec![
                ("TMUX".into(), None),
                ("TMUX_TMPDIR".into(), Some("/t".into()))
            ],
            cwd: Some(PathBuf::from("/w")),
            deadline: DEADLINE,
            stdin: None,
            stderr_to: None,
        }]
    );
}

#[test]
fn a_route_reply_that_takes_the_deadline_is_a_timeout() {
    let (runner, clock) = fake();
    struct Slow;
    impl Handler for Slow {
        fn handle(&self, _: &Call) -> Reply {
            Reply::ok("").takes(Duration::from_secs(99))
        }
    }
    let runner = runner.route("tmux", Arc::new(Slow));
    assert!(matches!(run(&runner, "tmux", &[]), Err(Failed::Timeout(_))));
    assert_eq!(clock.elapsed(), DEADLINE);
}

/// A handler that answers `text` on stdout.
struct Says(&'static str);

impl Handler for Says {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::ok(self.0)
    }
}

#[test]
fn a_second_route_for_the_same_program_replaces_the_first_and_rules_still_win() {
    let (runner, _clock) = fake();
    let runner = runner
        .route("gh", Arc::new(Says("one")))
        .route("tmux", Arc::new(Says("tmux")))
        .route("gh", Arc::new(Says("two")));
    let said =
        |program: &str| String::from_utf8(run(&runner, program, &[]).unwrap().stdout).unwrap();
    assert_eq!(said("gh"), "two", "the later route wins");
    assert_eq!(said("tmux"), "tmux", "a route for another program is kept");
    let runner = runner.on("gh", any, Reply::ok("rule"));
    assert_eq!(
        String::from_utf8(run(&runner, "gh", &[]).unwrap().stdout).unwrap(),
        "rule",
        "a rule still wins over a route"
    );
}
