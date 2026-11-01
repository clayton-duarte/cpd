//! The 39 `adapter_*` conformance tests of `cli/tests/tooling/fake_tmux.rs`, in-process: every public method of
//! `Tmux` run against the `tmux` route of a `FakeRunner`, no process, no script, no real tmux.

use super::TmuxRoute;
use crate::clock::ManualClock;
use crate::host::layout::{LEGACY_VIEWS, View};
use crate::host::runner::{Call, FakeRunner, Handler, Reply};
use crate::host::tmux::{PaneId, Spawn, Split, Tmux, WindowId, supported_version};
use crate::ids::SessionId;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// The adapter on socket `t`, `TMUX_TMPDIR` a temp dir, every call run by a `FakeRunner` with the `tmux` route;
/// what `run-shell` starts is recorded by `shell`.
struct Fx {
    dir: tempfile::TempDir,
    tmux: Tmux,
    route: Arc<TmuxRoute>,
    shell: FakeRunner,
}

impl Fx {
    fn new() -> Fx {
        let shell = FakeRunner::new(ManualClock::new());
        let route = Arc::new(TmuxRoute::new().with_runner(Arc::new(shell.clone())));
        let runner = FakeRunner::new(ManualClock::new()).route("tmux", route.clone());
        let dir = tempfile::tempdir().expect("temp dir");
        let tmux = Tmux::with_tmpdir("t", dir.path()).with_runner(Arc::new(runner));
        Fx {
            dir,
            tmux,
            route,
            shell,
        }
    }

    /// A fake with a session `s`, window `main` (`@0`, `%0`) made by `create_session`.
    fn started() -> Fx {
        let fx = Fx::new();
        fx.tmux
            .create_session("s", "main", &spawn())
            .expect("create_session");
        fx
    }

    fn run(&self, args: &[&str]) -> String {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        self.tmux
            .run(&args)
            .unwrap_or_else(|e| panic!("{args:?}: {e:?}"))
    }

    fn state(&self) -> crate::fakes::tmux::State {
        self.route.fake().state("t")
    }
}

fn spawn() -> Spawn {
    Spawn {
        cwd: Some(PathBuf::from("/work")),
        argv: vec!["sh".into(), "-c".into(), "true".into()],
        ..Spawn::default()
    }
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
    assert_eq!(
        (
            p.pane.as_str(),
            p.window.as_str(),
            p.session_name.as_str(),
            p.window_name.as_str()
        ),
        ("%0", "@0", "s", "main")
    );
    assert_eq!(
        (p.dead, p.dead_status, p.cpd_session.is_none(), p.cpd_view),
        (false, None, true, None)
    );
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
    fx.run(&["set", "-p", "-t", "%0", "@cpd_view", LEGACY_VIEWS[0]]);
    assert_eq!(fx.tmux.list_panes().unwrap(), Vec::new());
}

#[test]
fn adapter_set_pane_session_and_view_show_in_list_panes() {
    let fx = Fx::started();
    let session = SessionId::parse("lead-a").unwrap();
    fx.tmux.set_pane_session(&pane("%0"), &session).unwrap();
    fx.tmux.set_pane_view(&pane("%0"), View::Chat).unwrap();
    let p = &fx.tmux.list_panes().unwrap()[0];
    assert_eq!(
        (p.cpd_session.clone(), p.cpd_view),
        (Some(session), Some(View::Chat))
    );
}

#[test]
fn adapter_set_pane_title_stores_the_title_in_title_case_and_history_the_session() {
    let fx = Fx::started();
    fx.tmux
        .set_pane_title(&pane("%0"), "hello big world")
        .unwrap();
    fx.tmux
        .set_pane_history(&pane("%0"), &SessionId::parse("lead-a").unwrap())
        .unwrap();
    let state = fx.state();
    let options = &state.panes()[0].options;
    assert!(
        options.contains(&("@cpd_title".to_string(), "Hello Big World".to_string())),
        "{options:?}"
    );
    assert!(
        options.contains(&("@cpd_history".to_string(), "lead-a".to_string())),
        "{options:?}"
    );
}

#[test]
fn adapter_new_window_returns_the_ids_of_the_window_and_its_pane() {
    let fx = Fx::started();
    assert_eq!(
        fx.tmux.new_window("s", "chat", &spawn()).unwrap(),
        (window("@1"), pane("%1"))
    );
}

#[test]
fn adapter_new_window_stores_the_spawn_cwd_env_and_argv() {
    let fx = Fx::started();
    let mut spawn = spawn();
    spawn.env.insert("K".into(), "V".into());
    fx.tmux.new_window("s", "chat", &spawn).unwrap();
    let state = fx.state();
    let p = state
        .panes()
        .into_iter()
        .find(|p| p.id == "%1")
        .expect("pane %1");
    assert_eq!(
        (p.argv, p.cwd),
        (
            vec!["sh".to_string(), "-c".into(), "true".into()],
            "/work".to_string()
        )
    );
    assert!(
        state
            .pane_env("%1")
            .contains(&("K".to_string(), "V".to_string()))
    );
}

#[test]
fn adapter_split_window_returns_the_new_pane_for_both_directions_and_a_size() {
    let fx = Fx::started();
    assert_eq!(
        fx.tmux
            .split_window(&pane("%0"), Split::Right, Some("30%"), &spawn())
            .unwrap(),
        pane("%1")
    );
    assert_eq!(
        fx.tmux
            .split_window(&pane("%0"), Split::Below, None, &spawn())
            .unwrap(),
        pane("%2")
    );
}

#[test]
fn adapter_swap_pane_exchanges_the_windows_of_two_panes() {
    let fx = Fx::started();
    fx.tmux.new_window("s", "chat", &spawn()).unwrap();
    fx.tmux.swap_pane(&pane("%0"), &pane("%1")).unwrap();
    let panes = fx.tmux.list_panes().unwrap();
    let of = |id: &str| {
        panes
            .iter()
            .find(|p| p.pane.as_str() == id)
            .expect("pane")
            .window
            .as_str()
            .to_owned()
    };
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
    let err = fx
        .tmux
        .respawn_pane(&pane("%0"), &Spawn::default())
        .unwrap_err();
    assert_eq!(err.code().as_str(), "tmux_failed");
    fx.run(&["__ctl", "pane-exit", "%0", "--status", "1"]);
    fx.tmux
        .respawn_pane(&pane("%0"), &Spawn::default())
        .unwrap();
    assert!(!fx.tmux.list_panes().unwrap()[0].dead);
}

#[test]
fn adapter_respawn_pane_killing_restarts_a_live_pane_with_the_new_argv() {
    let fx = Fx::started();
    fx.tmux
        .respawn_pane_killing(
            &pane("%0"),
            &Spawn {
                argv: vec!["sleep".into(), "9".into()],
                ..Spawn::default()
            },
        )
        .unwrap();
    let state = fx.state();
    assert_eq!(state.panes()[0].argv, ["sleep", "9"]);
}

#[test]
fn adapter_kill_pane_removes_the_pane() {
    let fx = Fx::started();
    fx.tmux
        .split_window(&pane("%0"), Split::Right, None, &spawn())
        .unwrap();
    fx.tmux.kill_pane(&pane("%1")).unwrap();
    let ids: Vec<String> = fx
        .tmux
        .list_panes()
        .unwrap()
        .iter()
        .map(|p| p.pane.to_string())
        .collect();
    assert_eq!(ids, ["%0"]);
}

#[test]
fn adapter_kill_window_removes_the_window_and_its_panes() {
    let fx = Fx::started();
    fx.tmux.new_window("s", "chat", &spawn()).unwrap();
    fx.tmux.kill_window(&window("@1")).unwrap();
    let ids: Vec<String> = fx
        .tmux
        .list_panes()
        .unwrap()
        .iter()
        .map(|p| p.window.to_string())
        .collect();
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
    // The fake prints this process's pid when the call names none (`FAKE_TMUX_SERVER_PID`): this test process.
    let fx = Fx::new();
    fx.tmux.create_session("s", "main", &spawn()).unwrap();
    let exe = fx.tmux.server_exe().unwrap();
    assert_eq!(
        std::fs::canonicalize(exe).unwrap(),
        std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
    );
}

#[test]
fn adapter_run_display_message_expands_formats_for_a_pane() {
    let fx = Fx::started();
    assert_eq!(
        fx.run(&[
            "display-message",
            "-p",
            "-t",
            "%0",
            "#{session_name}:#{window_name}"
        ]),
        "s:main\n"
    );
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
    fx.run(&[
        "bind",
        "-T",
        "root",
        "MouseDown1Status",
        "run-shell",
        "-b",
        "true",
    ]);
    assert!(
        fx.run(&["list-keys", "-T", "root"])
            .contains("MouseDown1Status")
    );
}

#[test]
fn adapter_run_display_popup_returns_at_once() {
    let fx = Fx::started();
    fx.run(&["display-popup", "-E", "-t", "%0", "true"]);
    let state = fx.state();
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
    // In-process the command is `/bin/sh -c <cmd>` handed to the runner of the route: it has been handed over
    // when the call returns.
    let fx = Fx::started();
    let marker = fx.dir.path().join("ran");
    let command = format!("touch {}", marker.display());
    fx.run(&["run-shell", &command]);
    let shells: Vec<(String, Vec<String>)> = fx
        .shell
        .calls()
        .into_iter()
        .map(|c: Call| (c.program, c.args))
        .collect();
    assert_eq!(
        shells,
        [("/bin/sh".to_string(), vec!["-c".to_string(), command])],
        "run-shell returned before its command ran"
    );
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
    let err = fx
        .tmux
        .run(&[
            "new-session".to_string(),
            "-d".into(),
            "-s".into(),
            "s".into(),
        ])
        .unwrap_err();
    assert_eq!(err.code().as_str(), "tmux_failed");
}

/// Answers the `sh` a hook starts by calling the adapter again, as a hook that runs `tmux set` does.
struct CallsTmuxAgain(Arc<OnceLock<Tmux>>);

impl Handler for CallsTmuxAgain {
    fn handle(&self, _call: &Call) -> Reply {
        let tmux = self
            .0
            .get()
            .expect("the adapter is set before the first call");
        let args = ["set", "-g", "@again", "yes"].map(String::from);
        match tmux.run(&args) {
            Ok(_) => Reply::ok(""),
            Err(e) => Reply::exit(1, "", &format!("{e:?}")),
        }
    }
}

#[test]
fn a_run_shell_b_hook_that_calls_the_fake_again_does_not_deadlock() {
    let slot: Arc<OnceLock<Tmux>> = Arc::default();
    let shell = FakeRunner::new(ManualClock::new())
        .route("/bin/sh", Arc::new(CallsTmuxAgain(slot.clone())));
    let route = Arc::new(TmuxRoute::new().with_runner(Arc::new(shell)));
    let runner = FakeRunner::new(ManualClock::new()).route("tmux", route);
    let dir = tempfile::tempdir().unwrap();
    let tmux = Tmux::with_tmpdir("t", dir.path()).with_runner(Arc::new(runner));
    slot.set(tmux.clone()).expect("set once");
    tmux.create_session("s", "main", &spawn()).unwrap();
    let run = |args: &[&str]| {
        tmux.run(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
            .unwrap()
    };
    run(&[
        "set-hook",
        "-g",
        "pane-died",
        "run-shell -b 'tmux set -g @again yes'",
    ]);
    run(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(run(&["show", "-gv", "@again"]), "yes\n");
}
