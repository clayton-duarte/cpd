//! The fake tmux binary, its harness hooks and its conformance with the adapter (docs/implementing.md, Tests, "The
//! fake tmux"). Three groups: the built `fake-tmux` run as a process (`bin_*`), `TestEnv`'s hooks (`harness_*`) and
//! every public method of `cpd_core::host::tmux::Tmux` run against the fake (`adapter_*`). No test uses a real tmux,
//! sleeps or leaves a process: every child is waited for, a hung one is killed after a bound by a channel timeout.

use crate::common;

use common::TestEnv;
use cpd::fakes::tmux::State;
use cpd_core::host::layout::{LEGACY_STATUSBAR, View};
use cpd_core::host::tmux::{PaneId, Spawn, Split, Tmux, WindowId, supported_version};
use cpd_core::ids::SessionId;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const FAKE: &str = env!("CARGO_BIN_EXE_fake-tmux");
/// What a hung call is bounded by: far above a call's time, so only a deadlock reaches it.
const BOUND: Duration = Duration::from_secs(60);

// ---- the bin ----

/// `fake-tmux -u -L <socket> -f /dev/null <args>` with `TMUX_TMPDIR` `dir`.
fn tmux_cmd(dir: &Path, socket: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new(FAKE);
    cmd.args(["-u", "-L", socket, "-f", "/dev/null"]).args(args).env("TMUX_TMPDIR", dir).env("FAKE_TMUX_SERVER_PID", "4242");
    cmd.stdin(Stdio::null());
    cmd
}

/// Runs the call; a call that outlasts [`BOUND`] is killed and fails the test.
fn bounded(mut cmd: Command) -> Output {
    let child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("run fake-tmux");
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || tx.send(child.wait_with_output()).expect("send"));
    let out = match rx.recv_timeout(BOUND) {
        Ok(out) => out.expect("wait for fake-tmux"),
        Err(_) => {
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
            waiter.join().expect("waiter");
            panic!("fake-tmux did not return within {BOUND:?}: a deadlock");
        }
    };
    waiter.join().expect("waiter");
    out
}

fn call(dir: &Path, socket: &str, args: &[&str]) -> Output {
    bounded(tmux_cmd(dir, socket, args))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("utf-8 output")
}

/// A call that must exit 0; its stdout.
fn ok(dir: &Path, socket: &str, args: &[&str]) -> String {
    let out = call(dir, socket, args);
    assert_eq!(out.status.code(), Some(0), "{args:?}: stderr {:?}", text(&out.stderr));
    text(&out.stdout)
}

/// A temp `TMUX_TMPDIR` with a server on socket `t` (session `s`, window `w`: `@0`, pane `%0`).
fn server() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    ok(dir.path(), "t", &["new-session", "-d", "-s", "s", "-n", "w"]);
    dir
}

#[test]
fn bin_accepts_the_global_flags_and_starts_a_server() {
    let dir = tempfile::tempdir().unwrap();
    let out = call(dir.path(), "x", &["new-session", "-d", "-s", "s"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(0), String::new()));
    assert!(State::open(dir.path(), "x").is_some(), "no server state after new-session");
}

#[test]
fn bin_has_session_exits_0_for_a_session_that_exists() {
    let dir = server();
    assert_eq!(call(dir.path(), "t", &["has-session", "-t", "=s"]).status.code(), Some(0));
}

#[test]
fn bin_has_session_exits_1_for_a_session_that_does_not_exist() {
    let dir = server();
    assert_eq!(call(dir.path(), "t", &["has-session", "-t", "=nope"]).status.code(), Some(1));
}

#[test]
fn bin_without_a_server_says_no_server_running_on_the_socket_path_and_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let out = call(dir.path(), "x", &["has-session", "-t", "=s"]);
    let uid = std::fs::metadata(dir.path()).unwrap().uid();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stderr), format!("no server running on {}/tmux-{uid}/x\n", dir.path().display()));
}

#[test]
fn bin_kill_server_ends_the_server_so_the_next_call_says_no_server_running() {
    let dir = server();
    ok(dir.path(), "t", &["kill-server"]);
    let out = call(dir.path(), "t", &["list-sessions"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).starts_with("no server running on "), "{:?}", text(&out.stderr));
}

#[test]
fn bin_refuses_a_missing_tmux_tmpdir() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone");
    let out = call(&gone, "x", &["new-session", "-d", "-s", "s"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains(&gone.display().to_string()), "{:?}", text(&out.stderr));
    assert!(!gone.exists(), "the fake created the missing directory");
}

#[test]
fn bin_runs_ctl_when_it_is_the_first_argument() {
    let dir = server();
    ok(dir.path(), "t", &["__ctl", "pane-exit", "%0", "--status", "7"]);
    assert_eq!(ok(dir.path(), "t", &["display-message", "-p", "-t", "%0", "#{pane_dead} #{pane_dead_status}"]), "1 7\n");
}

#[test]
fn bin_refuses_ctl_anywhere_but_first() {
    let dir = server();
    let out = call(dir.path(), "t", &["list-sessions", ";", "__ctl", "pane-exit", "%0", "--status", "7"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stderr), "__ctl must be the first argument\n");
}

#[test]
fn bin_answers_an_unknown_subcommand_with_unknown_command_and_exit_1() {
    let dir = server();
    let out = call(dir.path(), "t", &["frobnicate"]);
    assert_eq!((out.status.code(), text(&out.stderr)), (Some(1), "unknown command: frobnicate\n".to_string()));
}

#[test]
fn bin_prints_the_ids_of_a_chained_new_session() {
    let dir = tempfile::tempdir().unwrap();
    let out = ok(
        dir.path(),
        "t",
        &["set", "-g", "mouse", "on", ";", "new-session", "-d", "-s", "s", "-P", "-F", "#{window_id} #{pane_id}"],
    );
    assert_eq!(out, "@0 %0\n");
}

#[test]
fn bin_display_pid_prints_fake_tmux_server_pid() {
    let dir = server();
    assert_eq!(ok(dir.path(), "t", &["display", "-p", "#{pid}"]), "4242\n");
}

#[test]
fn bin_eight_concurrent_new_windows_give_eight_distinct_pane_ids() {
    let dir = server();
    let path = dir.path().to_path_buf();
    let threads: Vec<_> = (0..8)
        .map(|n| {
            let path = path.clone();
            std::thread::spawn(move || {
                ok(&path, "t", &["new-window", "-d", "-t", "=s:", "-n", &format!("w{n}"), "-P", "-F", "#{pane_id}"])
            })
        })
        .collect();
    let mut ids: Vec<String> = threads.into_iter().map(|t| t.join().expect("thread").trim().to_owned()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 8, "{ids:?}");
    assert!(ids.iter().all(|id| id.starts_with('%')), "{ids:?}");
}

#[test]
fn bin_a_hook_that_appends_to_a_file_fires_in_order_across_two_calls() {
    let dir = server();
    ok(dir.path(), "t", &["split-window", "-t", "%0"]);
    let log = dir.path().join("order.log");
    let hook = format!("run-shell 'echo #{{pane_id}} >> {}'", log.display());
    ok(dir.path(), "t", &["set-hook", "-g", "pane-died", &hook]);
    ok(dir.path(), "t", &["__ctl", "pane-exit", "%1", "--status", "0"]);
    ok(dir.path(), "t", &["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "%1\n%0\n");
}

#[test]
fn bin_a_run_shell_b_hook_that_calls_the_fake_again_does_not_deadlock() {
    let dir = server();
    let hook = format!(
        "run-shell -b 'TMUX_TMPDIR={} {FAKE} -u -L t -f /dev/null set -g @again yes'",
        dir.path().display()
    );
    ok(dir.path(), "t", &["set-hook", "-g", "pane-died", &hook]);
    ok(dir.path(), "t", &["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(ok(dir.path(), "t", &["show", "-gv", "@again"]), "yes\n");
}

// ---- the harness ----

#[test]
fn harness_fake_tmux_makes_tmux_on_the_envs_path_the_fake() {
    let env = TestEnv::new();
    env.fake_tmux();
    let link = env.bin().join("tmux");
    assert_eq!(std::fs::read_link(&link).expect("bin/tmux is a symlink"), PathBuf::from(FAKE));
    let found = env.command("sh").args(["-c", "command -v tmux"]).output().unwrap();
    assert_eq!(text(&found.stdout).trim(), link.display().to_string());
}

#[test]
fn harness_tmux_state_is_none_before_and_some_after_new_session() {
    let env = TestEnv::new();
    env.fake_tmux();
    assert!(env.tmux_state().is_none(), "a server state before any call");
    let out = env.tmux(&["new-session", "-d", "-s", "s"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(env.tmux_state().is_some(), "no server state after new-session");
}

#[test]
fn harness_command_sets_fake_tmux_server_pid_to_the_test_process() {
    let env = TestEnv::new();
    let out = env.command("sh").args(["-c", "printf %s \"${FAKE_TMUX_SERVER_PID-unset}\""]).output().unwrap();
    assert_eq!(text(&out.stdout), std::process::id().to_string());
}

#[test]
fn harness_teardown_removes_the_fake_state_files_of_the_socket_from_a_tmux_dir_outside_the_env() {
    let env = TestEnv::new();
    let outside = tempfile::tempdir().unwrap();
    let socket = env.tmux_socket().to_owned();
    let (mine, derived, other) = (
        outside.path().join(format!("fake-tmux-{socket}.db")),
        outside.path().join(format!("fake-tmux-{socket}-outer-x.db")),
        outside.path().join("fake-tmux-someone-else.db"),
    );
    for file in [&mine, &derived, &other] {
        std::fs::write(file, "").unwrap();
    }
    // The teardown script of the env, with its tmux dir moved outside the env (the `/tmp` case of `tmux_tmpdir`).
    // Written beside it and renamed over it, so the reaper's running copy keeps the old file.
    let script = env.root().join("teardown.sh");
    let moved: String = std::fs::read_to_string(&script)
        .unwrap()
        .lines()
        .map(|l| if l.starts_with("tmuxdir=") { format!("tmuxdir='{}'", outside.path().display()) } else { l.to_owned() })
        .collect::<Vec<_>>()
        .join("\n");
    let tmp = env.root().join("teardown.sh.new");
    std::fs::write(&tmp, moved + "\n").unwrap();
    std::fs::rename(&tmp, &script).unwrap();
    env.teardown();
    assert!(!mine.exists(), "the socket's state file survived the teardown");
    assert!(!derived.exists(), "a derived socket's state file survived the teardown");
    assert!(other.exists(), "the teardown removed another socket's state file");
}

// ---- the adapter ----

/// The adapter on socket `t`, `TMUX_TMPDIR` a temp dir, every call run by `program`.
struct Fx {
    dir: tempfile::TempDir,
    tmux: Tmux,
}

impl Fx {
    fn new() -> Fx {
        Fx::with_program(FAKE)
    }

    fn with_program(program: impl Into<PathBuf>) -> Fx {
        let dir = tempfile::tempdir().expect("temp dir");
        let tmux = Tmux::with_tmpdir("t", dir.path()).with_program(program);
        Fx { dir, tmux }
    }

    /// A fake with a session `s`, window `main` (`@0`, `%0`) made by `create_session`.
    fn started() -> Fx {
        let fx = Fx::new();
        fx.tmux.create_session("s", "main", &spawn()).expect("create_session");
        fx
    }

    fn run(&self, args: &[&str]) -> String {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        self.tmux.run(&args).unwrap_or_else(|e| panic!("{args:?}: {e:?}"))
    }
}

fn spawn() -> Spawn {
    Spawn { cwd: Some(PathBuf::from("/work")), argv: vec!["sh".into(), "-c".into(), "true".into()], ..Spawn::default() }
}

fn pane(id: &str) -> PaneId {
    PaneId::parse(id).expect("pane id")
}

fn window(id: &str) -> WindowId {
    WindowId::parse(id).expect("window id")
}

#[test]
fn adapter_create_session_on_a_fresh_server_returns_the_first_window_and_pane() {
    let fx = Fx::new();
    let made = fx.tmux.create_session("s", "main", &spawn()).unwrap();
    assert_eq!(made, Some((window("@0"), pane("%0"))));
}

#[test]
fn adapter_create_session_applies_the_server_options_chain_before_the_session() {
    let fx = Fx::new();
    fx.tmux.create_session("s", "main", &spawn()).unwrap();
    assert_eq!(fx.tmux.global_option("remain-on-exit").unwrap(), "on");
}

#[test]
fn adapter_create_session_of_an_existing_session_returns_none() {
    let fx = Fx::started();
    assert_eq!(fx.tmux.create_session("s", "main", &spawn()).unwrap(), None);
}

#[test]
fn adapter_ensure_session_says_whether_it_created_the_session() {
    let fx = Fx::new();
    assert!(fx.tmux.ensure_session("s", "main", &spawn()).unwrap());
    assert!(!fx.tmux.ensure_session("s", "main", &spawn()).unwrap());
}

#[test]
fn adapter_has_session_is_false_without_a_server_and_true_for_an_existing_session_only() {
    let fx = Fx::new();
    assert!(!fx.tmux.has_session("s").unwrap());
    fx.tmux.create_session("s", "main", &spawn()).unwrap();
    assert!(fx.tmux.has_session("s").unwrap());
    assert!(!fx.tmux.has_session("other").unwrap());
}

#[test]
fn adapter_server_running_is_false_before_the_server_and_true_after() {
    let fx = Fx::new();
    assert!(!fx.tmux.server_running().unwrap());
    fx.tmux.create_session("s", "main", &spawn()).unwrap();
    assert!(fx.tmux.server_running().unwrap());
}

#[test]
fn adapter_kill_server_ends_the_server() {
    let fx = Fx::started();
    fx.tmux.kill_server().unwrap();
    assert!(!fx.tmux.server_running().unwrap());
}

#[test]
fn adapter_list_panes_is_empty_without_a_server() {
    assert_eq!(Fx::new().tmux.list_panes().unwrap(), Vec::new());
}

#[test]
fn adapter_list_panes_parses_a_new_sessions_pane() {
    let fx = Fx::started();
    let panes = fx.tmux.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    let p = &panes[0];
    assert_eq!((p.pane.as_str(), p.window.as_str(), p.session_name.as_str(), p.window_name.as_str()), ("%0", "@0", "s", "main"));
    assert_eq!((p.dead, p.dead_status, p.cpd_session.is_none(), p.cpd_view), (false, None, true, None));
}

#[test]
fn adapter_list_panes_parses_a_dead_panes_status() {
    let fx = Fx::started();
    fx.run(&["__ctl", "pane-exit", "%0", "--status", "3"]);
    let p = &fx.tmux.list_panes().unwrap()[0];
    assert_eq!((p.dead, p.dead_status), (true, Some(3)));
}

#[test]
fn adapter_list_panes_skips_a_pane_with_an_unknown_view() {
    let fx = Fx::started();
    fx.run(&["set", "-p", "-t", "%0", "@cpd_view", "from-the-future"]);
    assert_eq!(fx.tmux.list_panes().unwrap(), Vec::new());
}

#[test]
fn adapter_list_panes_skips_a_legacy_statusbar_pane() {
    let fx = Fx::started();
    fx.run(&["set", "-p", "-t", "%0", "@cpd_view", LEGACY_STATUSBAR]);
    assert_eq!(fx.tmux.list_panes().unwrap(), Vec::new());
}

#[test]
fn adapter_set_pane_session_and_view_show_in_list_panes() {
    let fx = Fx::started();
    let session = SessionId::parse("lead-a").unwrap();
    fx.tmux.set_pane_session(&pane("%0"), &session).unwrap();
    fx.tmux.set_pane_view(&pane("%0"), View::Chat).unwrap();
    let p = &fx.tmux.list_panes().unwrap()[0];
    assert_eq!((p.cpd_session.clone(), p.cpd_view), (Some(session), Some(View::Chat)));
}

#[test]
fn adapter_set_pane_title_stores_the_title_in_title_case_and_history_the_session() {
    let fx = Fx::started();
    fx.tmux.set_pane_title(&pane("%0"), "hello big world").unwrap();
    fx.tmux.set_pane_history(&pane("%0"), &SessionId::parse("lead-a").unwrap()).unwrap();
    let state = State::open(fx.dir.path(), "t").expect("state");
    let options = &state.panes()[0].options;
    assert!(options.contains(&("@cpd_title".to_string(), "Hello Big World".to_string())), "{options:?}");
    assert!(options.contains(&("@cpd_history".to_string(), "lead-a".to_string())), "{options:?}");
}

#[test]
fn adapter_new_window_returns_the_ids_of_the_window_and_its_pane() {
    let fx = Fx::started();
    assert_eq!(fx.tmux.new_window("s", "chat", &spawn()).unwrap(), (window("@1"), pane("%1")));
}

#[test]
fn adapter_new_window_stores_the_spawn_cwd_env_and_argv() {
    let fx = Fx::started();
    let mut spawn = spawn();
    spawn.env.insert("K".into(), "V".into());
    fx.tmux.new_window("s", "chat", &spawn).unwrap();
    let state = State::open(fx.dir.path(), "t").expect("state");
    let p = state.panes().into_iter().find(|p| p.id == "%1").expect("pane %1");
    assert_eq!((p.argv, p.cwd), (vec!["sh".to_string(), "-c".into(), "true".into()], "/work".to_string()));
    assert!(state.pane_env("%1").contains(&("K".to_string(), "V".to_string())));
}

#[test]
fn adapter_split_window_returns_the_new_pane_for_both_directions_and_a_size() {
    let fx = Fx::started();
    assert_eq!(fx.tmux.split_window(&pane("%0"), Split::Right, Some("30%"), &spawn()).unwrap(), pane("%1"));
    assert_eq!(fx.tmux.split_window(&pane("%0"), Split::Below, None, &spawn()).unwrap(), pane("%2"));
}

#[test]
fn adapter_swap_pane_exchanges_the_windows_of_two_panes() {
    let fx = Fx::started();
    fx.tmux.new_window("s", "chat", &spawn()).unwrap();
    fx.tmux.swap_pane(&pane("%0"), &pane("%1")).unwrap();
    let panes = fx.tmux.list_panes().unwrap();
    let of = |id: &str| panes.iter().find(|p| p.pane.as_str() == id).expect("pane").window.as_str().to_owned();
    assert_eq!((of("%0"), of("%1")), ("@1".to_string(), "@0".to_string()));
}

#[test]
fn adapter_rename_window_shows_in_list_panes() {
    let fx = Fx::started();
    fx.tmux.rename_window(&window("@0"), "renamed").unwrap();
    assert_eq!(fx.tmux.list_panes().unwrap()[0].window_name, "renamed");
}

#[test]
fn adapter_respawn_pane_restarts_a_dead_pane_and_refuses_a_live_one() {
    let fx = Fx::started();
    let err = fx.tmux.respawn_pane(&pane("%0"), &Spawn::default()).unwrap_err();
    assert_eq!(err.code().as_str(), "tmux_failed");
    fx.run(&["__ctl", "pane-exit", "%0", "--status", "1"]);
    fx.tmux.respawn_pane(&pane("%0"), &Spawn::default()).unwrap();
    assert!(!fx.tmux.list_panes().unwrap()[0].dead);
}

#[test]
fn adapter_respawn_pane_killing_restarts_a_live_pane_with_the_new_argv() {
    let fx = Fx::started();
    fx.tmux.respawn_pane_killing(&pane("%0"), &Spawn { argv: vec!["sleep".into(), "9".into()], ..Spawn::default() }).unwrap();
    let state = State::open(fx.dir.path(), "t").expect("state");
    assert_eq!(state.panes()[0].argv, ["sleep", "9"]);
}

#[test]
fn adapter_kill_pane_removes_the_pane() {
    let fx = Fx::started();
    fx.tmux.split_window(&pane("%0"), Split::Right, None, &spawn()).unwrap();
    fx.tmux.kill_pane(&pane("%1")).unwrap();
    let ids: Vec<String> = fx.tmux.list_panes().unwrap().iter().map(|p| p.pane.to_string()).collect();
    assert_eq!(ids, ["%0"]);
}

#[test]
fn adapter_kill_window_removes_the_window_and_its_panes() {
    let fx = Fx::started();
    fx.tmux.new_window("s", "chat", &spawn()).unwrap();
    fx.tmux.kill_window(&window("@1")).unwrap();
    let ids: Vec<String> = fx.tmux.list_panes().unwrap().iter().map(|p| p.window.to_string()).collect();
    assert_eq!(ids, ["@0"]);
}

#[test]
fn adapter_pane_width_parses_the_number_the_fake_prints() {
    let fx = Fx::started();
    assert!(fx.tmux.pane_width(&pane("%0")).unwrap() > 0);
}

#[test]
fn adapter_global_option_reads_back_a_set_option_without_its_newline() {
    let fx = Fx::started();
    fx.run(&["set", "-g", "@cpd_machine", "m1"]);
    assert_eq!(fx.tmux.global_option("@cpd_machine").unwrap(), "m1");
}

#[test]
fn adapter_apply_server_options_runs_on_a_running_server_and_again() {
    let fx = Fx::started();
    fx.tmux.apply_server_options().unwrap();
    fx.tmux.apply_server_options().unwrap();
    assert_eq!(fx.tmux.global_option("exit-empty").unwrap(), "off");
}

#[test]
fn adapter_apply_status_line_sets_the_version_and_the_sessions_status_options() {
    let fx = Fx::started();
    fx.tmux.apply_status_line("s", "2.0.1").unwrap();
    assert_eq!(fx.tmux.global_option("@cpd_version").unwrap(), "2.0.1");
}

#[test]
fn adapter_preview_is_none_until_the_option_is_set() {
    let fx = Fx::started();
    assert_eq!(fx.tmux.preview().unwrap(), None);
    fx.run(&["set", "-g", "@cpd_preview", "pr-12"]);
    assert_eq!(fx.tmux.preview().unwrap(), Some("pr-12".to_string()));
}

#[test]
fn adapter_server_version_is_one_the_adapter_supports() {
    let fx = Fx::started();
    let version = fx.tmux.server_version().unwrap();
    assert!(supported_version(&version), "{version:?}");
}

#[test]
fn adapter_server_exe_names_the_executable_of_the_pid_the_fake_prints() {
    // The fake prints $FAKE_TMUX_SERVER_PID: a shim sets it to its parent, this test process.
    let shim_dir = tempfile::tempdir().unwrap();
    let shim = shim_dir.path().join("tmux");
    common::write_executable(&shim, &format!("#!/bin/sh\nFAKE_TMUX_SERVER_PID=$PPID exec '{FAKE}' \"$@\"\n"));
    let fx = Fx::with_program(&shim);
    fx.tmux.create_session("s", "main", &spawn()).unwrap();
    let exe = fx.tmux.server_exe().unwrap();
    assert_eq!(std::fs::canonicalize(exe).unwrap(), std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap());
}

#[test]
fn adapter_run_display_message_expands_formats_for_a_pane() {
    let fx = Fx::started();
    assert_eq!(fx.run(&["display-message", "-p", "-t", "%0", "#{session_name}:#{window_name}"]), "s:main\n");
}

#[test]
fn adapter_run_set_hook_and_show_hooks_list_the_hook() {
    let fx = Fx::started();
    fx.run(&["set-hook", "-g", "pane-died", "respawn-pane"]);
    assert!(fx.run(&["show-hooks", "-g"]).contains("pane-died"));
}

#[test]
fn adapter_run_bind_and_list_keys_list_the_binding() {
    let fx = Fx::started();
    fx.run(&["bind", "-T", "root", "MouseDown1Status", "run-shell", "-b", "true"]);
    assert!(fx.run(&["list-keys", "-T", "root"]).contains("MouseDown1Status"));
}

#[test]
fn adapter_run_display_popup_returns_at_once() {
    let fx = Fx::started();
    fx.run(&["display-popup", "-E", "-t", "%0", "true"]);
    let state = State::open(fx.dir.path(), "t").expect("state");
    assert_eq!(state.popups().len(), 1);
}

#[test]
fn adapter_run_set_buffer_and_show_buffer_round_trip() {
    let fx = Fx::started();
    fx.run(&["set-buffer", "-b", "b1", "payload"]);
    assert_eq!(fx.run(&["show-buffer", "-b", "b1"]), "payload");
}

#[test]
fn adapter_run_run_shell_runs_the_command_before_it_returns() {
    let fx = Fx::started();
    let marker = fx.dir.path().join("ran");
    fx.run(&["run-shell", &format!("touch {}", marker.display())]);
    assert!(marker.exists(), "run-shell returned before its command ran");
}

#[test]
fn adapter_run_list_clients_prints_nothing_for_a_server_without_clients() {
    let fx = Fx::started();
    assert_eq!(fx.run(&["list-clients"]), "");
}

#[test]
fn adapter_run_select_pane_and_list_sessions_answer_with_the_session() {
    let fx = Fx::started();
    fx.run(&["select-pane", "-t", "%0"]);
    assert_eq!(fx.run(&["list-sessions", "-F", "#{session_name}"]), "s\n");
}

#[test]
fn adapter_run_new_session_of_a_taken_name_fails_as_tmux_failed() {
    let fx = Fx::started();
    let err = fx.tmux.run(&["new-session".to_string(), "-d".into(), "-s".into(), "s".into()]).unwrap_err();
    assert_eq!(err.code().as_str(), "tmux_failed");
}
