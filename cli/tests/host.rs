//! The tmux host (contract §6.1–§6.3, amendments 1, 2 and 9) on a private
//! tmux server: the wrapper's calls, and the windows and panes of §6.2 found
//! by their `@cpd_*` options.

mod common;

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use common::{SLEEP_SECONDS, TestEnv, wait_until};
use cpd_core::error::ErrorCode;
use cpd_core::host::layout::{self, View};
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux};
use cpd_core::ids::{AttemptId, PlanId, SessionId};

struct Server {
    env: TestEnv,
    tmux: Tmux,
}

/// The wrapper gets its own `TMUX_TMPDIR` (`Tmux::with_tmpdir`): the process environment is never touched.
fn server() -> Server {
    let env = TestEnv::new();
    let tmux = Tmux::with_tmpdir(env.tmux_socket(), env.root().join("tmux"));
    Server { env, tmux }
}

fn sleeper(cwd: &Path) -> Spawn {
    Spawn { cwd: Some(cwd.to_path_buf()), argv: vec!["sleep".into(), "600".into()], ..Spawn::default() }
}

fn sh(script: &str) -> Spawn {
    Spawn { argv: vec!["sh".into(), "-c".into(), script.into()], ..Spawn::default() }
}

fn pane<'a>(panes: &'a [Pane], id: &PaneId) -> &'a Pane {
    panes.iter().find(|p| &p.pane == id).unwrap_or_else(|| panic!("no pane {id} in {panes:?}"))
}

#[test]
fn starts_the_server_once_with_the_contract_options() {
    let s = server();
    assert!(!s.tmux.server_running().unwrap());
    assert!(!s.tmux.has_session("cpd").unwrap());
    assert!(s.tmux.list_panes().unwrap().is_empty());

    assert!(s.tmux.ensure_session("cpd", "main", &sleeper(s.env.root())).unwrap());
    assert!(s.tmux.server_running().unwrap());
    assert!(s.tmux.has_session("cpd").unwrap());
    assert!(!s.tmux.has_session("cp").unwrap(), "has-session matches the exact name");
    assert_server_options(&s);

    // Applying them again on the running server appends nothing.
    s.tmux.apply_server_options().unwrap();
    assert_server_options(&s);

    // Idempotent: the session exists, nothing is created or appended again.
    assert!(!s.tmux.ensure_session("cpd", "main", &sleeper(s.env.root())).unwrap());
    assert!(s.tmux.ensure_session("cpd-work", "watch", &sleeper(s.env.root())).unwrap());
    let features = s.tmux.global_option("terminal-features").unwrap();
    assert_eq!(features.matches("xterm-ghostty:extkeys").count(), 1, "{features}");

    let size = s.env.tmux(&["display", "-p", "-t", "=cpd:", "#{window_width}x#{window_height}"]);
    assert_eq!(String::from_utf8_lossy(&size.stdout).trim(), "215x62");
}

/// How a right click reaches the tree (commands.md §13.2): cpd adds no `MouseDown3Pane` binding, so on a server it
/// configured the key is tmux 3.4's own default, which selects the pane and passes the click on (`send-keys -M`) to
/// an app that captures the mouse (`#{mouse_any_flag}`, which the tree sets), and shows tmux's menu otherwise.
#[test]
fn a_right_click_is_tmuxs_default_binding_that_passes_it_to_a_pane_that_captures_the_mouse() {
    let s = server();
    s.tmux.ensure_session("cpd", "main", &sleeper(s.env.root())).unwrap();
    let out = s.env.tmux(&["list-keys", "-T", "root", "MouseDown3Pane"]);
    let binding = String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(" ");
    let default_start = "bind-key -T root MouseDown3Pane if-shell -F -t = \"#{||:#{mouse_any_flag},#{&&:#{pane_in_mode},";
    assert!(binding.starts_with(default_start), "{binding}");
    assert!(binding.contains("{ select-pane -t = ; send-keys -M } { display-menu "), "{binding}");
    // and cpd's own binding is on the same server, so the list is of this server's keys
    let own = s.env.tmux(&["list-keys", "-T", "root", "MouseDown1Pane"]);
    assert!(String::from_utf8_lossy(&own.stdout).contains("@cpd_view"));
}

/// Every option, binding and hook of contract §6.1: the reference's
/// `tui/scripts/tmux.conf` lines 2–17, renamed, with the conditional hook.
fn assert_server_options(s: &Server) {
    for (option, value) in [
        ("mouse", "on"),
        ("focus-events", "on"),
        ("default-terminal", "tmux-256color"),
        ("extended-keys", "on"),
        ("escape-time", "10"),
        ("status", "off"),
        ("pane-border-status", "top"),
        ("pane-border-lines", "heavy"),
        (
            "pane-border-format",
            "#{?#{@cpd_session},#{?#{@cpd_title},#[reverse] #{@cpd_title} #[noreverse],},#{?#{==:#{@cpd_view},chat},,#{?#{==:#{@cpd_view},history},,#{?#{==:#{@cpd_view},statusbar},,\
             #[reverse]#{?#{==:#{@cpd_view},worker}, Worker · #{@cpd_job} (read-only) ,#{?#{==:#{@cpd_view},tree}, Context ,\
             #{?#{==:#{@cpd_view},plan}, Plan ,#{?#{==:#{@cpd_view},widgets}, Deploy ,#{pane_current_command}}}}}#[noreverse]}}}}",
        ),
        ("pane-active-border-style", "fg=blue"),
        ("remain-on-exit", "on"),
    ] {
        assert_eq!(s.tmux.global_option(option).unwrap(), value, "{option}");
    }
    assert!(s.tmux.global_option("pane-border-format").unwrap().contains(", Deploy ,"));
    let features = s.tmux.global_option("terminal-features").unwrap();
    for feature in ["xterm*:extkeys", "xterm-ghostty:extkeys", "xterm-ghostty:hyperlinks"] {
        assert_eq!(features.matches(feature).count(), 1, "{feature} in {features}");
    }
    let hook = s.env.tmux(&["show-hooks", "-g", "pane-died"]);
    assert_eq!(String::from_utf8_lossy(&hook.stdout), "pane-died[0] if-shell -F \"#{@cpd_view}\" respawn-pane\n", "{hook:?}");
    let binding = s.env.tmux(&["list-keys", "-T", "root", "MouseDown1Pane"]);
    assert_eq!(
        String::from_utf8_lossy(&binding.stdout).split_whitespace().collect::<Vec<_>>().join(" "),
        "bind-key -T root MouseDown1Pane if-shell -F \"#{==:#{@cpd_view},statusbar}\" \"send-keys -M\" \"select-pane -t = ; send-keys -M\"",
        "{binding:?}"
    );
}

/// Kills a pane's program, as a crash would. Not while a tmux client is
/// still disconnecting: tmux 3.4 was seen to lose the `SIGCHLD` of a child
/// killed then (the pane is dead, its status never read, so `pane-died` never
/// fires), about one time in two right after `display`, never 20 ms later.
fn kill_program(t: &Tmux, id: &PaneId) {
    let pid = pane(&t.list_panes().unwrap(), id).pid.expect("a live pane has a pid");
    sleep(Duration::from_millis(100));
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
}

#[test]
fn a_killed_view_pane_respawns_and_a_killed_lead_or_engine_pane_stays_dead() {
    let s = server();
    let t = &s.tmux;
    let root = s.env.root();
    let (_, engine) = t.create_session("cpd-work", layout::WATCH_WINDOW, &sleeper(root)).unwrap().unwrap();
    let (_, view) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(root)).unwrap().unwrap();
    t.set_pane_view(&view, View::Tree).unwrap();
    let (_, lead) = t.new_window("cpd", "park-pay", &sleeper(root)).unwrap();
    t.set_pane_session(&lead, &SessionId::new_v4()).unwrap();
    let view_pid = pane(&t.list_panes().unwrap(), &view).pid;

    // The Lead and the engine first, and until they are dead: tmux runs the `pane-died` hooks in the order the panes
    // died, so once the view (killed last) is respawned, the hooks of the other two have run and left them dead.
    for id in [&engine, &lead] {
        kill_program(t, id);
    }
    wait_until("the Lead and the engine to die", || {
        let panes = t.list_panes().unwrap();
        pane(&panes, &lead).dead && pane(&panes, &engine).dead
    });
    kill_program(t, &view);
    wait_until("the view to respawn", || {
        let panes = t.list_panes().unwrap();
        let p = pane(&panes, &view);
        !p.dead && p.pid.is_some() && p.pid != view_pid
    });
    let panes = t.list_panes().unwrap();
    assert!(pane(&panes, &lead).dead && pane(&panes, &engine).dead, "{panes:?}");
    assert_eq!(pane(&panes, &view).cpd_view, Some(View::Tree), "the view keeps its option");
}

#[test]
fn respawn_killing_restarts_a_live_pane_in_place() {
    let s = server();
    let t = &s.tmux;
    let (_, id) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap().unwrap();
    t.set_pane_view(&id, View::Plan).unwrap();
    let before = pane(&t.list_panes().unwrap(), &id).clone();
    let out = s.env.root().join("respawned.txt");
    t.respawn_pane_killing(&id, &sh(&format!("echo \"$CPD_HOST_TEST_VAR\" > '{}'; sleep {SLEEP_SECONDS}", out.display()))).unwrap();
    let after = pane(&t.list_panes().unwrap(), &id).clone();
    assert!(!after.dead);
    assert_ne!(after.pid, before.pid);
    assert_eq!(after.cpd_view, Some(View::Plan));

    // An empty argv reruns the pane's own command, with the environment given again.
    let mut rerun = Spawn::default();
    rerun.env.insert("CPD_HOST_TEST_VAR".into(), "again".into());
    t.respawn_pane_killing(&id, &rerun).unwrap();
    wait_until("the rerun to write", || std::fs::read_to_string(&out).is_ok_and(|s| s == "again\n"));
    assert_ne!(pane(&t.list_panes().unwrap(), &id).pid, after.pid);
}

#[test]
fn create_session_returns_the_first_window_and_pane_once() {
    let s = server();
    let mut spawn = sleeper(s.env.root());
    spawn.env.insert("CPD_HOST_TEST_VAR".into(), "pay".into());
    let (window, pane) = s.tmux.create_session("cpd", "park-x", &spawn).unwrap().expect("created");
    // -e reaches the first pane only, not the session's later windows.
    let session_env = s.env.tmux(&["show-environment", "-t", "=cpd"]);
    assert!(!String::from_utf8_lossy(&session_env.stdout).contains("CPD_HOST_TEST_VAR"), "{session_env:?}");
    let later = s.tmux.new_window("cpd", "later", &sh(&format!("echo \"plan=$CPD_HOST_TEST_VAR\"; sleep {SLEEP_SECONDS}"))).unwrap().1;
    wait_until("the later window's output", || {
        let out = s.env.tmux(&["capture-pane", "-p", "-t", later.as_str()]);
        String::from_utf8_lossy(&out.stdout).contains("plan=")
    });
    let out = s.env.tmux(&["capture-pane", "-p", "-t", later.as_str()]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("plan=\n"), "{out:?}");
    assert_eq!(s.tmux.global_option("remain-on-exit").unwrap(), "on", "a fresh server gets the contract options");
    let panes = s.tmux.list_panes().unwrap();
    let first = self::pane(&panes, &pane);
    assert_eq!((&first.window, first.window_name.as_str()), (&window, "park-x"));
    // Idempotent: the session exists, nothing is created.
    assert_eq!(s.tmux.create_session("cpd", "park-y", &sleeper(s.env.root())).unwrap(), None);
    assert_eq!(s.tmux.list_panes().unwrap().len(), 2);
}

#[test]
fn builds_the_cockpit_and_finds_its_panes_by_option() {
    let s = server();
    let t = &s.tmux;
    let root = s.env.root();
    let lead = SessionId::parse("596d007f-1111-4222-8333-444455556666").unwrap();
    let plan = PlanId::parse("cpd2").unwrap();

    // cpd:main, split into tree, plan, chat slot (placeholder) and widgets.
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(root)).unwrap();
    let panes = t.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    let plan_pane = panes[0].pane.clone();
    let tree = t.split_window(&plan_pane, Split::Right, Some("80%"), &sleeper(root)).unwrap();
    t.swap_pane(&tree, &plan_pane).unwrap();
    let widgets = t.split_window(&plan_pane, Split::Right, Some("25%"), &sleeper(root)).unwrap();
    let placeholder = t.split_window(&plan_pane, Split::Below, None, &sleeper(root)).unwrap();
    t.set_pane_view(&tree, View::Tree).unwrap();
    t.set_pane_view(&plan_pane, View::Plan).unwrap();
    t.set_pane_view(&widgets, View::Widgets).unwrap();
    t.set_pane_view(&placeholder, View::Chat).unwrap();

    // A parked Lead in park-<label>.
    let park = layout::park_window(&lead, Some(&plan));
    let (park_window, lead_pane) = t.new_window("cpd", &park, &sleeper(root)).unwrap();
    t.set_pane_session(&lead_pane, &lead).unwrap();

    let panes = t.list_panes().unwrap();
    assert_eq!(panes.len(), 5);
    assert_eq!(layout::chat_slot(&panes, "cpd").unwrap().pane, placeholder);
    assert_eq!(layout::view_pane(&panes, "cpd", View::Tree).unwrap().pane, tree);
    assert_eq!(layout::view_pane(&panes, "cpd", View::Plan).unwrap().pane, plan_pane);
    assert_eq!(layout::view_pane(&panes, "cpd", View::Widgets).unwrap().pane, widgets);
    let parked = layout::session_pane(&panes, &lead).unwrap();
    assert_eq!((&parked.pane, &parked.window, parked.window_name.as_str()), (&lead_pane, &park_window, "park-cpd2"));
    let lead_pid = parked.pid;
    let placeholder_pid = pane(&panes, &placeholder).pid;
    let slot_size = |pid: Option<u32>| {
        let panes = t.list_panes().unwrap();
        let p = panes.iter().find(|p| p.pid == pid).unwrap().pane.clone();
        let out = s.env.tmux(&["display", "-p", "-t", p.as_str(), "#{pane_width}x#{pane_height}"]);
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let size_before = slot_size(placeholder_pid);

    // session open: swap the Lead into the slot; options and pids move with the panes.
    t.swap_pane(&lead_pane, &placeholder).unwrap();
    let panes = t.list_panes().unwrap();
    let slot = layout::chat_slot(&panes, "cpd").unwrap();
    assert_eq!(slot.pane, lead_pane);
    assert_eq!(slot.cpd_session.as_ref(), Some(&lead));
    assert_eq!(slot.pid, lead_pid);
    assert_eq!(slot_size(lead_pid), size_before, "the slot keeps its size");
    let displaced = pane(&panes, &placeholder);
    assert_eq!((displaced.window.clone(), displaced.cpd_view, displaced.pid), (park_window.clone(), Some(View::Chat), placeholder_pid));

    // The displaced pane is the placeholder: kill it, and its window goes too.
    t.kill_pane(&placeholder).unwrap();
    let panes = t.list_panes().unwrap();
    assert_eq!(panes.len(), 4);
    assert!(panes.iter().all(|p| p.window != park_window));

    // session park: a new park window holding a placeholder, swapped into the slot.
    let (window, new_placeholder) = t
        .new_window("cpd", &park, &Spawn { argv: vec!["sleep".into(), "600".into()], ..Spawn::default() })
        .unwrap();
    t.set_pane_view(&new_placeholder, View::Chat).unwrap();
    t.swap_pane(&new_placeholder, &lead_pane).unwrap();
    let panes = t.list_panes().unwrap();
    assert_eq!(layout::chat_slot(&panes, "cpd").unwrap().pane, new_placeholder);
    let parked = layout::session_pane(&panes, &lead).unwrap();
    assert_eq!((&parked.window, parked.pid), (&window, lead_pid));

    // rename-window, then kill-window by id.
    t.rename_window(&window, "park-other").unwrap();
    let panes = t.list_panes().unwrap();
    assert_eq!(pane(&panes, &lead_pane).window_name, "park-other");
    t.kill_window(&window).unwrap();
    assert!(layout::session_pane(&t.list_panes().unwrap(), &lead).is_none());
}

#[test]
fn a_worker_window_gets_its_cwd_environment_and_argv() {
    let s = server();
    let t = &s.tmux;
    let wt = s.env.root().join("repos");
    let out = s.env.root().join("out.txt");
    let plan = PlanId::parse("cpd2").unwrap();
    let attempt = AttemptId::parse("rs-host-a1").unwrap();
    t.ensure_session("cpd-work", layout::WATCH_WINDOW, &sleeper(s.env.root())).unwrap();

    let mut spawn = sh(&format!(
        "printf '%s|%s|%s|%s|%s' \"$PWD\" \"$CPD_PLAN\" \"$A_B\" \"${{TMUX_PANE:+set}}\" \"$1\" > '{}'; sleep {SLEEP_SECONDS}",
        out.display()
    ));
    spawn.argv.extend(["argv0".into(), "two words; $HOME".into()]);
    spawn.cwd = Some(wt.clone());
    spawn.env.insert("CPD_PLAN".into(), "cpd2".into());
    spawn.env.insert("A_B".into(), "x y".into());
    let name = layout::attempt_window(&plan, &attempt);
    let (window, worker) = t.new_window("cpd-work", &name, &spawn).unwrap();
    let session = SessionId::new_v4();
    t.set_pane_session(&worker, &session).unwrap();

    wait_until("the worker to write", || std::fs::read_to_string(&out).is_ok_and(|s| !s.is_empty()));
    // tmux sets TMUX_PANE in its panes itself; the argument arrives verbatim, no shell.
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        format!("{}|cpd2|x y|set|two words; $HOME", wt.display())
    );
    let panes = t.list_panes().unwrap();
    let p = layout::session_pane(&panes, &session).unwrap();
    assert_eq!((&p.pane, &p.window, p.session_name.as_str(), p.window_name.as_str()), (&worker, &window, "cpd-work", "cpd2--rs-host-a1"));
}

#[test]
fn a_dead_pane_stays_and_is_respawned() {
    let s = server();
    let t = &s.tmux;
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap();
    let (_, dying) = t.new_window("cpd", "x", &sh("exit 3")).unwrap();
    wait_until("the pane to die", || pane(&t.list_panes().unwrap(), &dying).dead);
    let dead = pane(&t.list_panes().unwrap(), &dying).clone();
    assert_eq!(dead.dead_status, Some(3));

    t.respawn_pane(&dying, &sleeper(s.env.root())).unwrap();
    let alive = pane(&t.list_panes().unwrap(), &dying).clone();
    assert!(!alive.dead);
    assert_eq!(alive.dead_status, None);
    assert_ne!(alive.pid, dead.pid);

    // Without -k a live pane is refused, never killed.
    let err = t.respawn_pane(&dying, &sleeper(s.env.root())).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(pane(&t.list_panes().unwrap(), &dying).pid, alive.pid);
}

#[test]
fn a_failing_call_is_tmux_failed_with_argv_exit_and_stderr() {
    let s = server();
    let t = &s.tmux;
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap();
    let missing = PaneId::parse("%999").unwrap();
    let err = t.kill_pane(&missing).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    let d = err.details();
    assert_eq!(d["argv"], serde_json::json!(["tmux", "-L", s.env.tmux_socket(), "-f", "/dev/null", "kill-pane", "-t", "%999"]));
    assert_eq!(d["exit"], 1);
    assert!(d["stderr"].as_str().unwrap().contains("%999"), "{d:?}");
}

#[test]
fn set_pane_title_runs_set_p_on_cpd_title_and_a_missing_pane_fails_with_its_argv() {
    let s = server();
    let t = &s.tmux;
    let (_, pane_id) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap().unwrap();
    t.set_pane_title(&pane_id, "Retry #{x} payouts; ok").unwrap();
    let shown = s.env.tmux(&["show", "-pv", "-t", pane_id.as_str(), "@cpd_title"]);
    assert_eq!(String::from_utf8_lossy(&shown.stdout), "Retry #{x} payouts; ok\n");

    let missing = PaneId::parse("%999").unwrap();
    let err = t.set_pane_title(&missing, "X").unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(
        err.details()["argv"],
        serde_json::json!(["tmux", "-L", s.env.tmux_socket(), "-f", "/dev/null", "set", "-p", "-t", "%999", "@cpd_title", "X"])
    );
}

#[test]
fn a_leads_border_shows_its_title_reversed_and_a_titleless_lead_shows_nothing() {
    let s = server();
    let t = &s.tmux;
    let (_, pane_id) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap().unwrap();
    let border = || {
        let out = s.env.tmux(&["display-message", "-p", "-t", pane_id.as_str(), "#{T:pane-border-format}"]);
        String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned()
    };
    t.set_pane_session(&pane_id, &SessionId::new_v4()).unwrap();
    assert_eq!(border(), "");
    t.set_pane_title(&pane_id, "Retry payouts").unwrap();
    assert_eq!(border(), "#[reverse] Retry payouts #[noreverse]");
}

#[test]
fn the_statusbar_is_a_view_and_a_pane_marked_so_parses() {
    assert_eq!(View::parse("statusbar"), Some(View::Statusbar));
    assert_eq!(View::Statusbar.as_str(), "statusbar");
    assert!(View::ALL.contains(&View::Statusbar));
    assert_eq!(View::parse("nope"), None);
}

/// Finding 4: a pane made by a newer cpd (a view this binary does not know)
/// is skipped, and the rest of the listing is intact.
#[test]
fn a_pane_with_an_unknown_view_is_skipped_not_a_failed_listing() {
    let s = server();
    let t = &s.tmux;
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap();
    let first = t.list_panes().unwrap().remove(0).pane;
    let (_, other) = t.new_window("cpd", "x", &sleeper(s.env.root())).unwrap();
    let set = |pane: &PaneId, view: &str| {
        let out = s.env.tmux(&["set", "-p", "-t", pane.as_str(), "@cpd_view", view]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    set(&first, "statusbar");
    set(&other, "hologram");

    let panes = t.list_panes().unwrap();
    assert_eq!(pane(&panes, &first).cpd_view, Some(View::Statusbar));
    assert!(panes.iter().all(|p| p.pane != other), "{panes:?}");
}

/// Whether process `pid` is running (a zombie is not).
fn running(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(") ").next().is_some_and(|r| !r.starts_with('Z')))
}

/// engine-W4: a tmux call that hangs (a fake `tmux` first on `PATH` that sleeps) is
/// killed at its deadline with its whole process group, and fails `tool_timeout`. The
/// deadline is shortened to 1 s by `CPD_TEST_DEADLINE_TMUX`, which a debug build reads
/// from its environment, and the wrapper finds `tmux` on `PATH`: this test runs itself
/// again in a child process born with both, so no other test sees them.
#[test]
fn a_tmux_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout() {
    const NAME: &str = "a_tmux_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout";
    if std::env::var_os("CPD_TEST_DEADLINE_TMUX").is_none() {
        let env = TestEnv::new();
        let pidfile = env.root().join("sleep.pid");
        env.fake("tmux", &format!("sleep {SLEEP_SECONDS} & echo $! > '{}'; wait", pidfile.display()));
        let mut path = vec![env.bin()];
        path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", NAME, "--nocapture"])
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("CPD_TEST_DEADLINE_TMUX", "1");
        let out = common::output(&mut child).unwrap();
        let printed = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success() && printed.contains("1 passed"), "{printed}");
        // The sleeper would run SLEEP_SECONDS: its death is the group kill, not its own end.
        let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        wait_until("the fake tmux's child to die", || !running(pid));
        return;
    }
    let s = server();
    // Without the deadline the fake would sleep SLEEP_SECONDS and succeed: the error is the deadline's.
    let err = s.tmux.server_running().unwrap_err();

    assert_eq!(err.code(), ErrorCode::ToolTimeout, "{err:?}");
    let d = err.details();
    assert_eq!(d["tool"], "tmux");
    assert_eq!(d["deadline_seconds"], 1);
    assert_eq!(d["argv"], serde_json::json!(s.tmux.argv(&["list-sessions".into(), "-F".into(), "#{session_id}".into()])));
}
