//! `run-shell` through the injected `Runner`: a command a call, a hook or a binding runs is `/bin/sh -c <cmd>`
//! handed to it, never a process, and the state lock is not held while it runs.

use super::kit::M;
use super::{FakeTmux, Invocation};
use crate::clock::ManualClock;
use crate::host::runner::{Call, FakeRunner, Handler, Reply};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

const WORK: &str = "/work/dir";

/// The environment a call carries (what `Tmux::command` hands the fake); the nested call must pass it on.
fn env() -> Vec<(String, String)> {
    vec![("CPD_HOME".into(), "/home/cpd".into()), ("PATH".into(), "/usr/bin".into())]
}

fn nested() -> (M, FakeRunner) {
    let runner = FakeRunner::new(ManualClock::new());
    (M::with_runner(Arc::new(runner.clone())), runner)
}

/// A call as the test's tmux directory sees it: cwd and environment of the invocation that caused it.
fn call(m: &M, socket: &str, args: &[&str]) -> super::Outcome {
    let mut inv = m.invocation(M::argv(socket, args));
    inv.cwd = PathBuf::from(WORK);
    inv.env = env();
    m.tmux.run(&inv)
}

/// A shell call: program, arguments, directory and environment.
type Shell = (String, Vec<String>, Option<PathBuf>, Vec<(String, Option<String>)>);

/// What the product's test asserts on: the program, the arguments, the directory and the environment.
fn shells(runner: &FakeRunner) -> Vec<Shell> {
    runner.calls().into_iter().map(|c: Call| (c.program, c.args, c.cwd, c.envs)).collect()
}

fn sh(cmd: &str) -> Shell {
    let envs = env().into_iter().map(|(k, v)| (k, Some(v))).collect();
    ("/bin/sh".to_owned(), vec!["-c".to_owned(), cmd.to_owned()], Some(PathBuf::from(WORK)), envs)
}

#[test]
fn run_shell_calls_the_runner_with_sh_dash_c_the_directory_and_the_environment_of_the_call() {
    let (m, runner) = nested();
    m.server();
    assert_eq!(call(&m, "t", &["run-shell", "cpd watch --ensure"]).code, 0);
    assert_eq!(shells(&runner), [sh("cpd watch --ensure")]);
}

#[test]
fn run_shell_dash_b_calls_the_runner_the_same_way() {
    let (m, runner) = nested();
    m.server();
    assert_eq!(call(&m, "t", &["run-shell", "-b", "cpd watch --ensure --wait >/dev/null 2>&1"]).code, 0);
    assert_eq!(shells(&runner), [sh("cpd watch --ensure --wait >/dev/null 2>&1")]);
}

#[test]
fn run_shell_ignores_the_exit_status_of_the_command() {
    let runner = FakeRunner::new(ManualClock::new()).on("/bin/sh", |_| true, Reply::exit(3, "out", "boom"));
    let m = M::with_runner(Arc::new(runner));
    m.server();
    let out = m.t(&["run-shell", "false"]);
    assert_eq!((out.code, out.stdout.as_str(), out.stderr.as_str()), (0, "", ""));
}

#[test]
fn run_shell_without_a_script_records_the_call_and_the_call_exits_0() {
    let (m, runner) = nested();
    m.server();
    let out = m.t(&["run-shell", "echo hi"]);
    assert_eq!(out.code, 0, "{out:?}");
    assert_eq!(runner.calls().len(), 1);
    assert_eq!(runner.unscripted().len(), 1, "no script answered it");
}

#[test]
fn a_chain_goes_on_after_a_run_shell() {
    let (m, runner) = nested();
    m.server();
    let out = m.t(&["run-shell", "echo one", ";", "display-message", "-p", "after"]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "after\n"));
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_pane_died_hook_calls_the_runner_with_its_run_shell_command() {
    let (m, runner) = nested();
    m.server();
    m.ok(&["set-hook", "-g", "pane-died", "run-shell 'cpd watch --ensure'"]);
    assert!(runner.calls().is_empty(), "setting the hook ran nothing");
    assert_eq!(call(&m, "t", &["__ctl", "pane-exit", "%0", "--status", "1"]).code, 0);
    assert_eq!(shells(&runner), [sh("cpd watch --ensure")]);
}

#[test]
fn a_window_unlinked_hook_calls_the_runner_when_a_window_is_killed() {
    let (m, runner) = nested();
    m.server();
    m.ok(&["new-window", "-d", "-t", "=s:", "-n", "gone"]);
    m.ok(&["set-hook", "-g", "window-unlinked", "run-shell 'echo #{hook_window_name} left'"]);
    assert_eq!(call(&m, "t", &["kill-window", "-t", "=s:gone"]).code, 0);
    assert_eq!(shells(&runner), [sh("echo gone left")]);
}

#[test]
fn a_status_click_binding_calls_the_runner_with_the_range_of_the_click() {
    let (m, runner) = nested();
    m.server();
    m.ok(&["bind", "-T", "root", "MouseDown1Status", "run-shell -b 'cpd tui action #{mouse_status_range}'"]);
    assert_eq!(call(&m, "t", &["__ctl", "click", "status", "merge"]).code, 0);
    assert_eq!(shells(&runner), [sh("cpd tui action merge")]);
}

/// Answers the `sh` the fake starts by calling the fake again, as a hook that runs `tmux set` does.
struct CallsTheFakeAgain {
    tmux: Arc<OnceLock<Arc<FakeTmux>>>,
    dir: PathBuf,
}

impl Handler for CallsTheFakeAgain {
    fn handle(&self, _call: &Call) -> Reply {
        let tmux = self.tmux.get().expect("the fake is set before the first call");
        let argv = M::argv("t", &["set", "-g", "@again", "yes"]);
        let out = tmux.run(&Invocation { argv, tmux_tmpdir: self.dir.clone(), cwd: self.dir.clone(), env: env() });
        Reply::exit(out.code, "", &out.stderr)
    }
}

#[test]
fn a_run_shell_hook_that_calls_the_fake_again_does_not_deadlock_and_its_change_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let slot: Arc<OnceLock<Arc<FakeTmux>>> = Arc::default();
    let handler = CallsTheFakeAgain { tmux: slot.clone(), dir: dir.path().to_path_buf() };
    let runner = FakeRunner::new(ManualClock::new()).route("/bin/sh", Arc::new(handler));
    let m = M { dir, tmux: Arc::new(FakeTmux::new().with_runner(Arc::new(runner))) };
    slot.set(m.tmux.clone()).ok().expect("set once");
    m.server();
    m.ok(&["set-hook", "-g", "pane-died", "run-shell -b 'tmux set -g @again yes'"]);
    m.ok(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(m.ok(&["show", "-gv", "@again"]), "yes\n");
}

#[test]
fn a_run_shell_that_calls_the_fake_again_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let slot: Arc<OnceLock<Arc<FakeTmux>>> = Arc::default();
    let handler = CallsTheFakeAgain { tmux: slot.clone(), dir: dir.path().to_path_buf() };
    let runner = FakeRunner::new(ManualClock::new()).route("/bin/sh", Arc::new(handler));
    let m = M { dir, tmux: Arc::new(FakeTmux::new().with_runner(Arc::new(runner))) };
    slot.set(m.tmux.clone()).ok().expect("set once");
    m.server();
    assert_eq!(m.t(&["run-shell", "tmux set -g @again yes"]).code, 0);
    assert_eq!(m.ok(&["show", "-gv", "@again"]), "yes\n");
}
