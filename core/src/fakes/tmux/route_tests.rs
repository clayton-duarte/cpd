//! The `tmux` route of the `FakeRunner`: how a `Call` becomes an `Invocation` and an `Outcome` a `Reply`, the
//! program match, `-u -V`, a scripted reply before the fake, and what a test reads back (screens, keys, popups).

use super::kit::M;
use super::{KeyLog, TmuxRoute};
use crate::clock::ManualClock;
use crate::host::runner::{Call, FakeRunner, Handler, Reply, Runner};
use crate::host::tmux::Tmux;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The `Call` of `Tmux::command`: `-u -L <socket> -f /dev/null <args>`, `TMUX_TMPDIR` set, the `TMUX*`/`CPD_*` removed.
fn call(dir: Option<&Path>, cwd: Option<&str>, args: &[&str]) -> Call {
    let mut envs: Vec<(String, Option<String>)> =
        ["TMUX", "TMUX_PANE", "CPD_SESSION", "CPD_ATTEMPT"]
            .iter()
            .map(|k| (k.to_string(), None))
            .collect();
    if let Some(dir) = dir {
        envs.push(("TMUX_TMPDIR".into(), Some(dir.display().to_string())));
    }
    Call {
        tool: "tmux".into(),
        program: "tmux".into(),
        args: M::argv("t", args),
        envs,
        cwd: cwd.map(PathBuf::from),
        deadline: Duration::from_secs(10),
        stdin: None,
        stderr_to: None,
    }
}

fn exit(code: i32, stdout: &str, stderr: &str) -> Reply {
    Reply::Exit {
        code,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
        takes: Duration::ZERO,
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// A route (with a started server on socket `t` in `dir`) and its `Tmux` on a runner that has only the route.
struct Rig {
    dir: tempfile::TempDir,
    route: Arc<TmuxRoute>,
    runner: FakeRunner,
    tmux: Tmux,
}

fn rig_of(route: TmuxRoute) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let route = Arc::new(route);
    let runner = FakeRunner::new(ManualClock::new()).route("tmux", route.clone());
    let tmux = Tmux::with_tmpdir("t", dir.path()).with_runner(Arc::new(runner.clone()));
    Rig {
        dir,
        route,
        runner,
        tmux,
    }
}

fn rig() -> Rig {
    rig_of(TmuxRoute::new())
}

impl Rig {
    fn run(&self, args: &[&str]) -> String {
        self.tmux
            .run(&strings(args))
            .unwrap_or_else(|e| panic!("{args:?}: {e:?}"))
    }
    fn started() -> Rig {
        let rig = rig();
        rig.run(&["new-session", "-d", "-s", "s", "-n", "w"]);
        rig
    }
}

fn stdout(reply: &Reply) -> String {
    match reply {
        Reply::Exit { stdout, .. } => String::from_utf8(stdout.clone()).unwrap(),
        other => panic!("not an exit: {other:?}"),
    }
}

fn code(reply: &Reply) -> i32 {
    match reply {
        Reply::Exit { code, .. } => *code,
        other => panic!("not an exit: {other:?}"),
    }
}

fn stderr(reply: &Reply) -> String {
    match reply {
        Reply::Exit { stderr, .. } => String::from_utf8(stderr.clone()).unwrap(),
        other => panic!("not an exit: {other:?}"),
    }
}

#[test]
fn the_route_answers_a_call_with_the_argv_of_the_adapter_global_flags_included() {
    let dir = tempfile::tempdir().unwrap();
    let route = TmuxRoute::new();
    let reply = route.handle(&call(
        Some(dir.path()),
        None,
        &[
            "new-session",
            "-d",
            "-s",
            "s",
            "-P",
            "-F",
            "#{window_id} #{pane_id}",
        ],
    ));
    assert_eq!(reply, exit(0, "@0 %0\n", ""));
}

#[test]
fn the_tmux_tmpdir_entry_of_the_envs_names_the_server() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let route = TmuxRoute::new();
    assert_eq!(
        code(&route.handle(&call(
            Some(a.path()),
            None,
            &["new-session", "-d", "-s", "s"]
        ))),
        0
    );
    assert_eq!(
        code(&route.handle(&call(Some(a.path()), None, &["has-session", "-t", "=s"]))),
        0
    );
    let other = route.handle(&call(Some(b.path()), None, &["has-session", "-t", "=s"]));
    assert_eq!(code(&other), 1);
    assert!(
        stderr(&other).starts_with(&format!(
            "no server running on {}/tmux-",
            b.path().display()
        )),
        "{}",
        stderr(&other)
    );
}

#[test]
fn the_cwd_of_the_call_is_the_directory_of_a_new_pane() {
    let dir = tempfile::tempdir().unwrap();
    let route = TmuxRoute::new();
    route.handle(&call(
        Some(dir.path()),
        Some("/work/dir"),
        &["new-session", "-d", "-s", "s"],
    ));
    let shown = route.handle(&call(
        Some(dir.path()),
        None,
        &["display-message", "-p", "-t", "%0", "#{pane_current_path}"],
    ));
    assert_eq!(stdout(&shown), "/work/dir\n");
}

#[test]
fn the_envs_of_the_call_are_the_env_of_the_invocation_and_a_removed_one_is_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let route = TmuxRoute::new();
    route.handle(&call(
        Some(dir.path()),
        None,
        &["new-session", "-d", "-s", "s"],
    ));
    let mut with_pid = call(Some(dir.path()), None, &["display", "-p", "#{pid}"]);
    with_pid
        .envs
        .push(("FAKE_TMUX_SERVER_PID".into(), Some("4242".into())));
    assert_eq!(stdout(&route.handle(&with_pid)), "4242\n");
}

#[test]
fn a_missing_tmux_tmpdir_is_exit_1_with_the_error_creating_text_and_no_stdout() {
    let reply = TmuxRoute::new().handle(&call(None, None, &["new-session", "-d", "-s", "s"]));
    assert_eq!(
        reply,
        exit(
            1,
            "",
            "error creating /tmp/tmux-<uid> (TMUX_TMPDIR is not set)\n"
        )
    );
}

#[test]
fn the_tmux_tmpdir_a_call_does_not_name_is_the_one_the_process_inherited_as_a_child_would() {
    let none = call(None, None, &["list-sessions"]);
    assert_eq!(
        super::route::tmux_tmpdir(&none, Some("/inherited".into())).as_deref(),
        Some("/inherited")
    );
    assert_eq!(super::route::tmux_tmpdir(&none, None), None);
    // The call's own entry wins; a removed variable (`None`) is unset whatever the process has.
    let own = call(Some(Path::new("/own")), None, &["list-sessions"]);
    assert_eq!(
        super::route::tmux_tmpdir(&own, Some("/inherited".into())).as_deref(),
        Some("/own")
    );
    let mut removed = call(None, None, &["list-sessions"]);
    removed.envs.push(("TMUX_TMPDIR".into(), None));
    assert_eq!(
        super::route::tmux_tmpdir(&removed, Some("/inherited".into())),
        None
    );
}

#[test]
fn an_empty_tmux_tmpdir_is_exit_1_with_the_error_creating_text_and_no_stdout() {
    let mut empty = call(None, None, &["list-sessions"]);
    empty.envs.push(("TMUX_TMPDIR".into(), Some(String::new())));
    assert_eq!(
        TmuxRoute::new().handle(&empty),
        exit(
            1,
            "",
            "error creating /tmp/tmux-<uid> (TMUX_TMPDIR is not set)\n"
        )
    );
}

#[test]
fn u_dash_v_is_answered_by_the_route_with_the_fakes_version() {
    let mut version = call(None, None, &[]);
    version.args = strings(&["-u", "-V"]);
    assert_eq!(
        TmuxRoute::new().handle(&version),
        exit(0, "tmux 3.6a\n", "")
    );
}

#[test]
fn the_runner_answers_tmux_u_dash_v_through_the_route() {
    let runner = FakeRunner::with_tmux();
    let mut cmd = Command::new("tmux");
    cmd.args(["-u", "-V"]);
    let out = runner
        .output(&mut cmd, "tmux", Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        (out.status.code(), String::from_utf8(out.stdout).unwrap()),
        (Some(0), "tmux 3.6a\n".to_string())
    );
}

#[test]
fn the_route_matches_the_program_by_its_file_name() {
    let runner = FakeRunner::with_tmux();
    let mut cmd = Command::new("/x/bin/tmux");
    cmd.args(["-u", "-V"]);
    let out = runner
        .output(&mut cmd, "tmux", Duration::from_secs(10))
        .unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "tmux 3.6a\n");
    assert!(runner.unscripted().is_empty());
}

#[test]
fn a_tmux_with_a_program_runs_on_the_route() {
    let dir = tempfile::tempdir().unwrap();
    let tmux = Tmux::with_tmpdir("t", dir.path())
        .with_program("/opt/tmux-3.4/bin/tmux")
        .with_runner(Arc::new(FakeRunner::with_tmux()));
    assert!(!tmux.server_running().unwrap());
    assert!(
        tmux.run(&strings(&["new-session", "-d", "-s", "s"]))
            .is_ok()
    );
    assert!(tmux.server_running().unwrap());
}

#[test]
fn a_scripted_reply_is_tried_before_the_route() {
    let route = TmuxRoute::new().script(
        |argv| argv.iter().any(|a| a == "list-sessions"),
        exit(1, "", "scripted\n"),
    );
    let dir = tempfile::tempdir().unwrap();
    route.handle(&call(
        Some(dir.path()),
        None,
        &["new-session", "-d", "-s", "s"],
    ));
    let listed = route.handle(&call(Some(dir.path()), None, &["list-sessions"]));
    assert_eq!(listed, exit(1, "", "scripted\n"));
}

#[test]
fn a_call_no_scripted_reply_matches_goes_to_the_route() {
    let route = TmuxRoute::new().script(
        |argv| argv.iter().any(|a| a == "list-sessions"),
        exit(1, "", "scripted\n"),
    );
    let dir = tempfile::tempdir().unwrap();
    let made = route.handle(&call(
        Some(dir.path()),
        None,
        &[
            "new-session",
            "-d",
            "-s",
            "s",
            "-P",
            "-F",
            "#{session_name}",
        ],
    ));
    assert_eq!(made, exit(0, "s\n", ""));
}

#[test]
fn the_first_scripted_reply_that_matches_answers() {
    let route = TmuxRoute::new()
        .script(
            |argv| argv.iter().any(|a| a == "kill-server"),
            exit(0, "first", ""),
        )
        .script(|_| true, exit(0, "second", ""));
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        stdout(&route.handle(&call(Some(dir.path()), None, &["kill-server"]))),
        "first"
    );
    assert_eq!(
        stdout(&route.handle(&call(Some(dir.path()), None, &["list-sessions"]))),
        "second"
    );
}

#[test]
fn a_scripted_reply_stands_for_a_wrapper_that_answers_one_subcommand_and_the_call_record_replaces_env_calls()
 {
    let rig = rig_of(TmuxRoute::new().script(
        |argv| argv.iter().any(|a| a == "display-message"),
        exit(0, "wrapped\n", ""),
    ));
    rig.run(&["new-session", "-d", "-s", "s"]);
    assert_eq!(rig.run(&["display-message", "-p", "x"]), "wrapped\n");
    let calls = rig.runner.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].program, "tmux");
    assert_eq!(
        calls[1].args,
        strings(&[
            "-u",
            "-L",
            "t",
            "-f",
            "/dev/null",
            "display-message",
            "-p",
            "x"
        ])
    );
}

#[test]
fn a_call_carries_the_removed_variables_as_none() {
    let runner = FakeRunner::with_tmux();
    let dir = tempfile::tempdir().unwrap();
    let tmux = Tmux::with_tmpdir("t", dir.path()).with_runner(Arc::new(runner.clone()));
    tmux.run(&strings(&["new-session", "-d", "-s", "s"]))
        .unwrap();
    let envs = runner.calls()[0].envs.clone();
    for key in ["TMUX", "TMUX_PANE", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert!(envs.contains(&(key.to_owned(), None)), "{key}: {envs:?}");
    }
}

#[test]
fn set_screen_feeds_capture_pane_p() {
    let rig = Rig::started();
    rig.route.fake().set_screen("%0", "hello screen");
    assert_eq!(
        rig.run(&["capture-pane", "-p", "-t", "%0"]),
        "hello screen\n"
    );
}

#[test]
fn send_keys_is_recorded_as_a_key_log() {
    let rig = Rig::started();
    rig.run(&["send-keys", "-t", "%0", "-l", "hello"]);
    rig.run(&["send-keys", "-t", "%0", "Enter"]);
    let keys = rig.route.fake().state("t").keys();
    assert_eq!(
        keys,
        [
            KeyLog {
                pane: "%0".into(),
                keys: strings(&["hello"]),
                literal: true
            },
            KeyLog {
                pane: "%0".into(),
                keys: strings(&["Enter"]),
                literal: false
            },
        ]
    );
}

#[test]
fn display_popup_records_its_argv() {
    let rig = Rig::started();
    rig.run(&[
        "display-popup",
        "-E",
        "-B",
        "-w",
        "60",
        "-h",
        "8",
        "cpd tui view legend",
    ]);
    assert_eq!(
        rig.route.fake().state("t").popups(),
        [strings(&[
            "display-popup",
            "-E",
            "-B",
            "-w",
            "60",
            "-h",
            "8",
            "cpd tui view legend"
        ])]
    );
}

/// The command a dialog's popup runs, as `view_command` builds it: `--out='<path>'` is one word of it.
fn dialog_command(out: &Path) -> String {
    format!(
        "cpd tui view dialog confirm --title='Close' --out='{}' --text='Sure?'",
        out.display()
    )
}

#[test]
fn on_popup_writes_the_outcome_file_named_by_out_before_the_call_returns() {
    let rig = Rig::started();
    let out = rig.dir.path().join("outcome.json");
    rig.route
        .fake()
        .on_popup(|_| Some(r#"{"confirmed":true,"value":null}"#.to_string()));
    rig.run(&[
        "display-popup",
        "-E",
        "-B",
        "-w",
        "60",
        "-h",
        "8",
        &dialog_command(&out),
    ]);
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        r#"{"confirmed":true,"value":null}"#
    );
}

#[test]
fn on_popup_is_given_the_argv_of_the_popup() {
    let rig = Rig::started();
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::default();
    let sink = seen.clone();
    rig.route.fake().on_popup(move |argv| {
        sink.lock().unwrap().push(argv.to_vec());
        None
    });
    let command = dialog_command(&rig.dir.path().join("o.json"));
    rig.run(&["display-popup", "-E", "-w", "60", &command]);
    assert_eq!(
        *seen.lock().unwrap(),
        [strings(&["display-popup", "-E", "-w", "60", &command])]
    );
}

#[test]
fn on_popup_answering_none_writes_no_file() {
    let rig = Rig::started();
    let out = rig.dir.path().join("outcome.json");
    rig.route.fake().on_popup(|_| None);
    rig.run(&["display-popup", "-E", &dialog_command(&out)]);
    assert!(!out.exists(), "the popup wrote a file for no answer");
}

#[test]
fn without_on_popup_a_dialog_gets_no_outcome_file_the_real_no_answer_path() {
    let rig = Rig::started();
    let out = rig.dir.path().join("outcome.json");
    rig.run(&["display-popup", "-E", &dialog_command(&out)]);
    assert!(!out.exists(), "a popup nobody answered left a file");
    assert_eq!(rig.route.fake().state("t").popups().len(), 1);
}

// ---- fault injections (the table of the `route` module docs) ---------------------------------------------------------

fn names(args: &[&str]) -> impl Fn(&Call) -> bool + Send + Sync + 'static {
    let args = strings(args);
    move |call| args.iter().all(|a| call.args.contains(a))
}

fn kills(rig: &Rig) -> usize {
    rig.runner
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a == "kill-window"))
        .count()
}

#[test]
fn a_once_rule_makes_the_first_kill_window_fail_and_the_next_goes_to_the_fake() {
    let rig = Rig::started();
    rig.run(&["new-window", "-d", "-t", "s", "-n", "x"]);
    let _ = rig
        .runner
        .clone()
        .once("tmux", names(&["kill-window"]), exit(1, "", "boom\n"));
    let window = crate::host::tmux::WindowId::parse("@1").unwrap();

    let first = rig.tmux.kill_window(&window).unwrap_err();
    assert!(first.message().contains("boom"), "{first:?}");
    assert_eq!(
        rig.route.fake().state("t").windows().len(),
        2,
        "the failed call changed nothing"
    );

    rig.tmux.kill_window(&window).unwrap();
    assert_eq!(
        rig.route.fake().state("t").windows().len(),
        1,
        "the second call reached the fake"
    );
    assert_eq!(kills(&rig), 2);
}

#[test]
fn a_once_rule_makes_the_first_list_panes_fail_and_the_client_lists_again_while_the_server_runs() {
    let rig = Rig::started();
    let _ = rig.runner.clone().once(
        "tmux",
        names(&["list-panes"]),
        exit(1, "", "server exited unexpectedly\n"),
    );

    let panes = rig.tmux.list_panes().unwrap();

    assert_eq!(panes.len(), 1, "{panes:?}");
    let lists = rig
        .runner
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a == "list-panes"))
        .count();
    assert_eq!(lists, 2, "listed again once");
}

#[test]
fn a_scripted_listing_with_tabs_printed_as_underscores_is_tmux_failed_unexpected_output() {
    let rig = Rig::started();
    let _ = rig.runner.clone().on(
        "tmux",
        names(&["list-panes"]),
        Reply::ok("%1_cpd_main_0_1234_x\n"),
    );

    let error = rig.tmux.list_panes().unwrap_err();

    assert_eq!(error.code(), crate::error::ErrorCode::TmuxFailed);
    assert!(error.message().contains("unexpected output"), "{error:?}");
}

fn spawn() -> crate::host::tmux::Spawn {
    crate::host::tmux::Spawn {
        cwd: None,
        env: Default::default(),
        argv: strings(&["cpd", "watch"]),
    }
}

#[test]
fn new_panes_end_with_a_status_leaves_the_new_session_pane_dead_with_that_status() {
    let rig = rig_of(TmuxRoute::new().new_panes_end(super::PaneEnd::Status(7)));
    let (_, pane) = rig
        .tmux
        .create_session("work", "watch", &spawn())
        .unwrap()
        .unwrap();

    let found = rig
        .tmux
        .list_panes()
        .unwrap()
        .into_iter()
        .find(|p| p.pane == pane)
        .unwrap();

    assert_eq!((found.dead, found.dead_status), (true, Some(7)));
}

#[test]
fn new_panes_end_by_a_signal_leaves_the_pane_dead_with_no_status() {
    let rig = rig_of(TmuxRoute::new().new_panes_end(super::PaneEnd::Signal));
    rig.tmux.create_session("work", "watch", &spawn()).unwrap();
    let (_, pane) = rig.tmux.new_window("work", "other", &spawn()).unwrap();

    let found = rig
        .tmux
        .list_panes()
        .unwrap()
        .into_iter()
        .find(|p| p.pane == pane)
        .unwrap();

    assert_eq!(
        (found.dead, found.dead_status),
        (true, None),
        "dead, and tmux prints no status for it"
    );
}

#[test]
fn new_panes_end_also_ends_a_respawned_pane_and_only_the_new_pane_of_a_call() {
    let rig = rig_of(TmuxRoute::new().new_panes_end(super::PaneEnd::Status(3)));
    let (_, first) = rig
        .tmux
        .create_session("work", "watch", &spawn())
        .unwrap()
        .unwrap();
    let (_, second) = rig.tmux.new_window("work", "other", &spawn()).unwrap();
    rig.tmux.respawn_pane(&first, &spawn()).unwrap();

    let dead: Vec<_> = rig
        .tmux
        .list_panes()
        .unwrap()
        .into_iter()
        .map(|p| (p.pane, p.dead_status))
        .collect();

    assert_eq!(dead, [(first, Some(3)), (second, Some(3))]);
}

#[test]
fn without_new_panes_end_a_new_pane_is_alive() {
    let rig = rig();
    let (_, pane) = rig
        .tmux
        .create_session("work", "watch", &spawn())
        .unwrap()
        .unwrap();
    assert!(
        rig.tmux
            .list_panes()
            .unwrap()
            .iter()
            .all(|p| p.pane == pane && !p.dead)
    );
}

#[test]
fn ctl_pane_exit_signal_is_a_dead_pane_with_no_status() {
    let rig = Rig::started();
    rig.run(&["__ctl", "pane-exit", "%0", "--signal"]);
    let pane = &rig.route.fake().state("t").panes()[0];
    assert_eq!((pane.dead, pane.dead_status), (true, None));
}
