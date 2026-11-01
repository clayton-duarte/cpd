//! `exec::run_with` over a `FakeRunner` (docs/implementing.md, Tests): one `Runner` call per hook, `sh -c <hook>`
//! with the context as stdin, the hooks of a block on a thread each, the limit on virtual time. No process, no sleep.
//!
//! Behaviour change pinned here: a hook's output reaches the log and the tail when the hook ends, not line by line.
//! No consumer needs the lines as they come: `run_rendered` only reads the tail after `execute` returns, and the
//! `tail -f` of its comment is a convenience; the log holds the whole output once the run is over (last test).

use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
use std::time::Duration;

use super::exec::{Ended, Executed, Rendered, run_on, run_with};
use crate::clock::ManualClock;
use crate::host::runner::{Call, FakeRunner, Handler, Reply};

const LIMIT: Duration = Duration::from_secs(30);

fn hook(name: Option<&str>, command: &str) -> Rendered {
    Rendered {
        name: name.map(str::to_owned),
        command: command.to_owned(),
        context: format!("{{\"for\":\"{command}\"}}"),
    }
}

fn runner() -> FakeRunner {
    FakeRunner::new(ManualClock::new())
}

/// A hook answers by its command (`-c <hook>`).
fn answers(runner: FakeRunner, command: &'static str, reply: Reply) -> FakeRunner {
    runner.on(
        "sh",
        move |c| c.args.get(1).map(String::as_str) == Some(command),
        reply,
    )
}

struct Run {
    done: Executed,
    log: PathBuf,
    _dir: tempfile::TempDir,
}

fn run(runner: &FakeRunner, blocks: &[Vec<Rendered>], timeout: Duration, tail: usize) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("hook.log");
    let done = run_with(
        runner,
        blocks,
        Path::new("/work/tree"),
        timeout,
        &[("KEPT", "yes")],
        File::create(&log).unwrap(),
        tail,
    );
    Run {
        done,
        log,
        _dir: dir,
    }
}

fn one(runner: &FakeRunner, command: &str) -> Run {
    run(runner, &[vec![hook(None, command)]], LIMIT, 60)
}

#[test]
fn a_hook_is_one_call_of_sh_with_c_and_the_command() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    one(&runner, "cargo test");
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].program.as_str(), calls[0].args.clone()),
        ("sh", vec!["-c".to_owned(), "cargo test".to_owned()])
    );
}

#[test]
fn a_hook_call_is_of_the_hook_tool_so_the_system_runner_runs_it_at_idle_priority() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    one(&runner, "cargo test");
    assert_eq!(runner.calls()[0].tool, crate::host::runner::HOOK_TOOL);
}

#[test]
fn a_hook_runs_in_the_given_working_directory() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    one(&runner, "true");
    assert_eq!(
        runner.calls()[0].cwd.as_deref(),
        Some(Path::new("/work/tree"))
    );
}

#[test]
fn the_context_is_the_stdin_of_the_call() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    one(&runner, "true");
    assert_eq!(runner.calls()[0].stdin, Some(br#"{"for":"true"}"#.to_vec()));
}

#[test]
fn the_callers_environment_is_on_the_call() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    one(&runner, "true");
    assert!(
        runner.calls()[0]
            .envs
            .contains(&("KEPT".to_owned(), Some("yes".to_owned())))
    );
}

#[test]
fn the_cockpit_pi_and_cpd_variables_are_removed_from_the_call() {
    // The process environment of the test is not touched: the call names what is removed, as `*` of each prefix is not
    // enumerable, only `TMUX` and `TMUX_PANE` are named; `PI_*` and `CPD_*` are the runner's `is_withheld` rule.
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    one(&runner, "true");
    let envs = runner.calls()[0].envs.clone();
    for gone in ["TMUX", "TMUX_PANE"] {
        assert!(
            envs.contains(&(gone.to_owned(), None)),
            "{gone} is not removed: {envs:?}"
        );
    }
}

#[test]
fn a_hook_that_exits_zero_passes_with_no_failure() {
    let runner = runner().on("sh", |_| true, Reply::ok("fine\n"));
    assert_eq!(one(&runner, "true").done.failure, None);
}

#[test]
fn the_exit_code_of_a_failing_hook_reaches_the_result() {
    let runner = answers(runner(), "exit 7", Reply::exit(7, "", ""));
    assert_eq!(one(&runner, "exit 7").done.failure, Some(Ended::Exit(7)));
}

#[test]
fn stdout_then_stderr_is_the_merged_output_in_the_tail() {
    let runner = runner().on("sh", |_| true, Reply::exit(0, "out\n", "err\n"));
    assert_eq!(
        one(&runner, "echo out; echo err >&2").done.tail,
        vec!["out", "err"]
    );
}

#[test]
fn the_output_of_a_failing_hook_is_kept() {
    let runner = runner().on("sh", |_| true, Reply::exit(7, "broken\n", ""));
    assert_eq!(
        one(&runner, "echo broken; exit 7").done.tail,
        vec!["broken"]
    );
}

#[test]
fn only_the_last_lines_are_kept_in_the_tail_and_the_log_has_all() {
    let all: String = (1..=100).map(|n| format!("{n}\n")).collect();
    let runner = runner().on("sh", |_| true, Reply::ok(&all));
    let r = run(&runner, &[vec![hook(None, "seq 1 100")]], LIMIT, 60);
    assert_eq!(
        (
            r.done.tail.len(),
            r.done.tail.first().cloned(),
            r.done.tail.last().cloned()
        ),
        (60, Some("41".into()), Some("100".into()))
    );
    assert_eq!(fs::read_to_string(&r.log).unwrap().lines().count(), 100);
}

#[test]
fn the_log_holds_the_whole_output_once_the_run_is_over() {
    let runner = runner().on("sh", |_| true, Reply::exit(0, "out\n", "err\n"));
    assert_eq!(
        fs::read_to_string(one(&runner, "x").log).unwrap(),
        "out\nerr\n"
    );
}

#[test]
fn a_hook_that_takes_longer_than_its_limit_timed_out() {
    let runner = runner().on(
        "sh",
        |_| true,
        Reply::ok("late\n").takes(Duration::from_secs(60)),
    );
    assert_eq!(
        run(
            &runner,
            &[vec![hook(None, "sleep 60")]],
            Duration::from_secs(5),
            60
        )
        .done
        .failure,
        Some(Ended::TimedOut)
    );
}

#[test]
fn the_limit_is_the_deadline_of_the_call() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    run(
        &runner,
        &[vec![hook(None, "true")]],
        Duration::from_secs(5),
        60,
    );
    assert_eq!(runner.calls()[0].deadline, Duration::from_secs(5));
}

#[test]
fn a_hook_that_takes_less_than_its_limit_does_not_time_out() {
    let runner = runner().on("sh", |_| true, Reply::ok("").takes(Duration::from_secs(4)));
    assert_eq!(
        run(
            &runner,
            &[vec![hook(None, "build")]],
            Duration::from_secs(5),
            60
        )
        .done
        .failure,
        None
    );
}

#[test]
fn a_sh_that_cannot_start_is_not_started() {
    let runner = runner().on("sh", |_| true, Reply::io(std::io::ErrorKind::NotFound));
    assert!(matches!(
        one(&runner, "true").done.failure,
        Some(Ended::NotStarted(_))
    ));
}

/// Records the thread of each call and the order of entry.
#[derive(Default)]
struct Entries(Mutex<Vec<(usize, ThreadId)>>);

impl Handler for Entries {
    fn handle(&self, _: &Call) -> Reply {
        let mut seen = self.0.lock().unwrap();
        let n = seen.len();
        seen.push((n, thread::current().id()));
        Reply::ok("")
    }
}

#[test]
fn two_hooks_of_a_block_are_called_from_different_threads() {
    let entries = Arc::new(Entries::default());
    let runner = runner().route("sh", entries.clone());
    run(
        &runner,
        &[vec![hook(Some("lint"), "lint"), hook(Some("test"), "test")]],
        LIMIT,
        60,
    );
    let seen = entries.0.lock().unwrap();
    let threads: HashSet<ThreadId> = seen.iter().map(|(_, t)| *t).collect();
    assert_eq!((seen.len(), threads.len()), (2, 2));
}

#[test]
fn two_hooks_of_a_block_are_called_off_the_callers_thread() {
    let entries = Arc::new(Entries::default());
    let runner = runner().route("sh", entries.clone());
    run(
        &runner,
        &[vec![hook(Some("lint"), "lint"), hook(Some("test"), "test")]],
        LIMIT,
        60,
    );
    let me = thread::current().id();
    assert!(entries.0.lock().unwrap().iter().all(|(_, t)| *t != me));
}

#[test]
fn both_hooks_of_a_block_are_called_even_when_the_first_fails() {
    let runner =
        answers(runner(), "first", Reply::exit(5, "", "")).on("sh", |_| true, Reply::ok(""));
    run(
        &runner,
        &[vec![hook(Some("a"), "first"), hook(Some("b"), "second")]],
        LIMIT,
        60,
    );
    assert_eq!(runner.calls().len(), 2);
}

#[test]
fn the_failure_of_a_block_is_the_first_in_file_order() {
    let runner = answers(
        answers(runner(), "alpha", Reply::exit(3, "", "")),
        "zeta",
        Reply::exit(5, "", ""),
    )
    .on("sh", |_| true, Reply::ok(""));
    let r = run(
        &runner,
        &[vec![
            hook(Some("zeta"), "zeta"),
            hook(Some("alpha"), "alpha"),
        ]],
        LIMIT,
        60,
    );
    assert_eq!(r.done.failure, Some(Ended::Exit(5)));
}

#[test]
fn the_failed_command_is_the_first_failing_one_of_a_table_in_file_order() {
    let runner = answers(
        answers(runner(), "build", Reply::exit(7, "", "")),
        "test",
        Reply::exit(9, "", ""),
    )
    .on("sh", |_| true, Reply::ok(""));
    let block = vec![
        hook(Some("lint"), "lint"),
        hook(Some("build"), "build"),
        hook(Some("test"), "test"),
    ];
    let r = run(&runner, &[block], LIMIT, 60);
    assert_eq!(
        (r.done.failure, r.done.failed_command),
        (Some(Ended::Exit(7)), Some("build".to_owned()))
    );
}

#[test]
fn the_failed_command_of_a_pipeline_is_the_failing_blocks() {
    let runner =
        answers(runner(), "second", Reply::exit(4, "", "")).on("sh", |_| true, Reply::ok(""));
    let r = run(
        &runner,
        &[vec![hook(None, "first")], vec![hook(None, "second")]],
        LIMIT,
        60,
    );
    assert_eq!(r.done.failed_command.as_deref(), Some("second"));
}

#[test]
fn a_passing_run_has_no_failed_command() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    assert_eq!(one(&runner, "true").done.failed_command, None);
}

#[test]
fn the_output_of_the_hooks_of_a_block_is_prefixed_with_their_names() {
    let runner = answers(runner(), "lint", Reply::ok("lint-ok\n")).on(
        "sh",
        |_| true,
        Reply::ok("test-ok\n"),
    );
    let mut tail = run(
        &runner,
        &[vec![hook(Some("lint"), "lint"), hook(Some("test"), "test")]],
        LIMIT,
        60,
    )
    .done
    .tail;
    tail.sort();
    assert_eq!(tail, vec!["lint │ lint-ok", "test │ test-ok"]);
}

#[test]
fn the_output_of_a_lone_hook_has_no_prefix() {
    let runner = runner().on("sh", |_| true, Reply::ok("only\n"));
    assert_eq!(
        run(&runner, &[vec![hook(Some("name"), "x")]], LIMIT, 60)
            .done
            .tail,
        vec!["only"]
    );
}

#[test]
fn a_failing_block_stops_the_pipeline() {
    let runner =
        answers(runner(), "fail", Reply::exit(4, "", "")).on("sh", |_| true, Reply::ok(""));
    let blocks = [
        vec![hook(None, "one")],
        vec![hook(Some("a"), "two"), hook(Some("b"), "fail")],
        vec![hook(None, "three")],
    ];
    let r = run(&runner, &blocks, LIMIT, 60);
    let commands: Vec<String> = runner.calls().iter().map(|c| c.args[1].clone()).collect();
    assert_eq!(r.done.failure, Some(Ended::Exit(4)));
    assert!(!commands.contains(&"three".to_owned()), "{commands:?}");
}

#[test]
fn the_blocks_of_a_pipeline_run_in_order() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    run(
        &runner,
        &[
            vec![hook(None, "one")],
            vec![hook(None, "two")],
            vec![hook(None, "three")],
        ],
        LIMIT,
        60,
    );
    let commands: Vec<String> = runner.calls().iter().map(|c| c.args[1].clone()).collect();
    assert_eq!(commands, vec!["one", "two", "three"]);
}

#[test]
fn an_empty_block_is_skipped() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    run(&runner, &[vec![], vec![hook(None, "one")]], LIMIT, 60);
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_timed_out_hook_stops_the_pipeline() {
    let runner = runner().on("sh", |_| true, Reply::ok("").takes(Duration::from_secs(60)));
    let r = run(
        &runner,
        &[vec![hook(None, "one")], vec![hook(None, "two")]],
        Duration::from_secs(5),
        60,
    );
    assert_eq!(
        (r.done.failure, runner.calls().len()),
        (Some(Ended::TimedOut), 1)
    );
}

#[test]
fn nothing_unscripted_is_called() {
    let runner = runner().on("sh", |_| true, Reply::ok(""));
    run(
        &runner,
        &[vec![hook(Some("a"), "a"), hook(Some("b"), "b")]],
        LIMIT,
        60,
    );
    assert!(runner.unscripted().is_empty());
}

#[test]
fn what_one_block_took_on_the_clock_is_not_left_to_the_next() {
    // One timeout for the whole run, measured on the clock the runner's replies move: the first block takes 4 s of
    // the 5, so the second is called with 1 s.
    let clock = ManualClock::new();
    let runner = answers(
        FakeRunner::new(clock.clone()),
        "one",
        Reply::ok("").takes(Duration::from_secs(4)),
    )
    .on("sh", |_| true, Reply::ok(""));
    let dir = tempfile::tempdir().unwrap();
    let log = File::create(dir.path().join("hook.log")).unwrap();
    let blocks = [vec![hook(None, "one")], vec![hook(None, "two")]];
    let done = run_on(
        &runner,
        &clock,
        &blocks,
        Path::new("/work/tree"),
        Duration::from_secs(5),
        &[],
        log,
        60,
    );
    let deadlines: Vec<Duration> = runner.calls().iter().map(|c| c.deadline).collect();
    assert_eq!(
        (done.failure, deadlines),
        (None, vec![Duration::from_secs(5), Duration::from_secs(1)])
    );
}
