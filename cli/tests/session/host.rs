//! The tmux host (contract §6.1–§6.3, amendments 1, 2 and 9) on a private
//! tmux server: the wrapper's calls, and the windows and panes of §6.2 found
//! by their `@cpd_*` options.

use crate::common;

use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use common::{SLEEP_SECONDS, TestEnv, wait_until};
use cpd_core::error::ErrorCode;
use cpd_core::host::layout::{self, View};
use cpd_core::host::tmux::SERVER_OPTIONS;
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux, WindowId};
use cpd_core::ids::{AttemptId, PlanId, SessionId};

struct Server {
    env: TestEnv,
    tmux: Tmux,
}

/// The wrapper gets its own `TMUX_TMPDIR` (`Tmux::with_tmpdir`): the process environment is never touched.
fn server() -> Server {
    let env = TestEnv::new();
    let tmux = Tmux::with_tmpdir(env.tmux_socket(), env.tmux_tmpdir());
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

/// Stops the server `-L <socket>` in tmux's default directory (`/tmp/tmux-<uid>`) and removes its socket file, when
/// dropped: a server started where no teardown looks is ended here.
struct DefaultDirServer(String);

impl DefaultDirServer {
    fn socket_file(&self) -> std::path::PathBuf {
        Path::new("/tmp").join(format!("tmux-{}", { use std::os::unix::fs::MetadataExt; std::fs::metadata(std::env::current_exe().unwrap()).unwrap().uid() })).join(&self.0)
    }
}

impl Drop for DefaultDirServer {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux").args(["-L", &self.0, "kill-server"]).env_remove("TMUX_TMPDIR").env_remove("TMUX").output();
        let _ = std::fs::remove_file(self.socket_file());
    }
}

/// hygiene.md 3b: tmux falls back to `/tmp/tmux-<uid>/` for a `TMUX_TMPDIR` that does not exist, so a late start escapes
/// every teardown; cpd refuses instead.
#[test]
fn a_tmux_call_whose_tmux_tmpdir_is_missing_is_refused_and_starts_no_server_in_tmp() {
    let env = TestEnv::new();
    let socket = format!("cpdt-missing-tmpdir-{}", std::process::id());
    let guard = DefaultDirServer(socket.clone());
    let tmux = Tmux::with_tmpdir(socket, env.root().join("no-such-tmux-dir"));

    let err = tmux.ensure_session("cpd", "main", &sleeper(env.root())).unwrap_err();

    assert_eq!(err.code(), ErrorCode::TmuxFailed, "{err:?}");
    assert!(!guard.socket_file().exists(), "a server started in {}", guard.socket_file().display());
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
    let own = s.env.tmux(&["list-keys", "-T", "root", "MouseDown1Status"]);
    assert!(String::from_utf8_lossy(&own.stdout).contains("cpd tui action"));
}

/// Contract §6.1 (hs-d489): the borders of unfocused panes are grey, `pane-border-style fg=colour7` set right before
/// `pane-active-border-style`.
#[test]
fn the_pane_border_style_is_a_server_option_set_right_before_the_active_border_style() {
    let at = SERVER_OPTIONS.iter().position(|o| *o == ["set", "-g", "pane-active-border-style", "fg=blue"]).unwrap();
    assert_eq!(SERVER_OPTIONS[at.checked_sub(1).unwrap()], ["set", "-g", "pane-border-style", "fg=colour7"]);
}

/// Contract §6.1 (hs-d353): tmux 3.8 defaults the popup, popup border and menu styles to a grey background, so
/// cpd sets each to `default`, with `-gq`, right after `pane-active-border-style`.
#[test]
fn the_popup_and_menu_styles_are_server_options_set_to_default_after_the_active_border() {
    let at = SERVER_OPTIONS.iter().position(|o| *o == ["set", "-g", "pane-active-border-style", "fg=blue"]).unwrap();
    let want: [&[&str]; 3] = [
        &["set", "-gq", "popup-style", "default"],
        &["set", "-gq", "popup-border-style", "fg=colour7"],
        &["set", "-gq", "menu-style", "fg=colour15"],
    ];
    assert_eq!(&SERVER_OPTIONS[at + 1..at + 4], &want);
}

#[test]
fn a_server_started_by_cpd_reads_the_popup_and_menu_styles_back_as_default() {
    let s = server();
    s.tmux.ensure_session("cpd", "main", &sleeper(s.env.root())).unwrap();
    for option in ["popup-style", "popup-border-style", "menu-style"] {
        let shown = s.env.tmux(&["show", "-gv", option]);
        if shown.status.success() {
            let want = match option {
                "popup-border-style" => "fg=colour7",
                "menu-style" => "fg=colour15",
                _ => "default",
            };
            assert_eq!(String::from_utf8_lossy(&shown.stdout).trim(), want, "{option}");
        } else {
            eprintln!("{option} not asserted: this tmux does not know the option");
        }
    }
}

/// `pane-border-format`, exactly (contract §6.1, hs-d475, hs-d478): every title is a pill with no padding, grey
/// (colour 8) and blue on the focused pane, set in the format itself; one `━` in the pill's colour before the left
/// cap, so the cap is the fourth cell of the border row, three columns in counted from the pane's edge.
const BORDER_FORMAT_WANT: &str = "#{?#{@cpd_session},#{?#{@cpd_title},\
#{?pane_active,#[fg=colour4],#[fg=colour7]}━\u{e0b6}#[reverse]#{@cpd_title}#[noreverse]\u{e0b4},},\
#{?#{==:#{@cpd_view},chat},,#{?#{==:#{@cpd_view},history},,\
#{?pane_active,#[fg=colour4],#[fg=colour7]}━\u{e0b6}#[reverse]\
#{?#{==:#{@cpd_view},worker},Worker · #{@cpd_job} (Read-Only),#{?#{==:#{@cpd_view},tree},Context,\
#{?#{==:#{@cpd_view},plan},Plan,#{?#{==:#{@cpd_view},widgets},Deploy,#{pane_current_command}}}}}\
#[noreverse]\u{e0b4}}}}";

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
            BORDER_FORMAT_WANT,
        ),
        ("pane-border-style", "fg=colour7"),
        ("pane-active-border-style", "fg=blue"),
        ("remain-on-exit", "on"),
        ("set-titles", "on"),
        ("set-titles-string", "cpd · #{?@cpd_machine,#{@cpd_machine},#{host_short}}"),
    ] {
        assert_eq!(s.tmux.global_option(option).unwrap(), value, "{option}");
    }
    // `extended-keys-format` exists from tmux 3.5 or so; an older tmux ignores the `set -gq`
    let format = s.env.tmux(&["show", "-gv", "extended-keys-format"]);
    if format.status.success() {
        assert_eq!(String::from_utf8_lossy(&format.stdout).trim(), "csi-u", "extended-keys-format");
    } else {
        eprintln!("extended-keys-format not asserted: this tmux does not know the option");
    }
    assert!(s.tmux.global_option("pane-border-format").unwrap().contains(",Deploy,"));
    let features = s.tmux.global_option("terminal-features").unwrap();
    for feature in ["xterm*:extkeys", "xterm-ghostty:extkeys", "xterm-ghostty:hyperlinks"] {
        assert_eq!(features.matches(feature).count(), 1, "{feature} in {features}");
    }
    // A view is respawned; the engine's window (`watch`) starts the engine again, as does its window's loss (contract §6.6, hs-d328).
    let hook = s.env.tmux(&["show-hooks", "-g", "pane-died"]);
    assert_eq!(
        String::from_utf8_lossy(&hook.stdout),
        "pane-died[0] if-shell -F \"#{@cpd_view}\" respawn-pane \"if -F '#{==:#{window_name},watch}' \\\"run-shell -b 'cpd watch --ensure --wait >/dev/null 2>&1'\\\"\"\n",
        "{hook:?}"
    );
    let hook = s.env.tmux(&["show-hooks", "-g", "window-unlinked"]);
    assert_eq!(
        String::from_utf8_lossy(&hook.stdout),
        "window-unlinked[0] if-shell -F \"#{==:#{hook_window_name},watch}\" \"run-shell -b 'cpd watch --ensure --wait >/dev/null 2>&1'\"\n",
        "{hook:?}"
    );
    assert_eq!(s.env.tmux(&["show", "-sv", "exit-empty"]).stdout, b"off\n");
    let binding = |key| {
        let out = s.env.tmux(&["list-keys", "-T", "root", key]);
        String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(" ")
    };
    // tmux's own default: a click selects the pane, then passes the click on
    assert_eq!(binding("MouseDown1Pane"), "bind-key -T root MouseDown1Pane select-pane -t = \\; send-keys -M");
    assert_eq!(
        binding("MouseDown1Status"),
        "bind-key -T root MouseDown1Status run-shell -b \"cpd tui action #{mouse_status_range} --client #{client_name} 2>/dev/null\""
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
    assert_eq!(d["argv"], serde_json::json!(["tmux", "-u", "-L", s.env.tmux_socket(), "-f", "/dev/null", "kill-pane", "-t", "%999"]));
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
    assert_eq!(String::from_utf8_lossy(&shown.stdout), "Retry #{x} Payouts; Ok\n");

    let missing = PaneId::parse("%999").unwrap();
    let err = t.set_pane_title(&missing, "X").unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(
        err.details()["argv"],
        serde_json::json!(["tmux", "-u", "-L", s.env.tmux_socket(), "-f", "/dev/null", "set", "-p", "-t", "%999", "@cpd_title", "X"])
    );
}

#[test]
fn a_leads_border_shows_its_title_reversed_between_caps_and_a_titleless_lead_shows_nothing() {
    let s = server();
    let t = &s.tmux;
    let (_, pane_id) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap().unwrap();
    let border = || {
        let out = s.env.tmux(&["display-message", "-p", "-t", pane_id.as_str(), "#{T:pane-border-format}"]);
        String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned()
    };
    t.set_pane_session(&pane_id, &SessionId::new_v4()).unwrap();
    assert_eq!(border(), "");
    // Title Case, no padding, the pill in blue (this pane is the active one), one border line character before the
    // left cap (hs-d475, hs-d478)
    t.set_pane_title(&pane_id, "Retry payouts").unwrap();
    assert_eq!(border(), "#[fg=colour4]━\u{e0b6}#[reverse]Retry Payouts#[noreverse]\u{e0b4}");
}

/// The status line's format, exactly (hs-d479): one grey `─` line across the row, starting and ending with a `─`; the
/// three button pills each in its own range, joined by a single `─` outside the ranges; the optional unpadded
/// preview pill after a join; the fill; then the workers' pill and the closing `─`
/// (long form from `@cpd_workers_width` + 14: the left group's 12, the closing `─` and a fill of 1).
#[test]
fn the_status_format_is_the_buttons_the_grey_fill_line_and_the_right_pills() {
    let prefix = format!(
            "#[fg=colour8]\u{2500}#[default]\
             #[range=user|reset]#[fg=colour15]\u{e0b6}#[reverse]{}#[noreverse]\u{e0b4}#[default]#[norange]\
             #[fg=colour8]\u{2500}#[default]\
             #[range=user|legend]#[fg=colour15]\u{e0b6}#[reverse]{}#[noreverse]\u{e0b4}#[default]#[norange]\
             #[fg=colour8]\u{2500}#[default]\
             #[range=user|logs]#[fg=colour15]\u{e0b6}#[reverse]{}#[noreverse]\u{e0b4}#[default]#[norange]",
            cpd_core::host::tmux::RESET_GLYPH,
            cpd_core::host::tmux::LEGEND_GLYPH,
            cpd_core::host::tmux::LOGS_GLYPH
        );
    let prefix = prefix + "#{?#{@cpd_preview},#[fg=colour8]\u{2500}#[fg=colour3]\u{e0b6}#[reverse]preview: #{@cpd_preview}#[noreverse]\u{e0b4}#[default],}\
         #[fg=colour8]";
    let suffix = "#[default]#[align=right]\
         #[fg=colour8]\u{e0b6}#[reverse]\
         #{?#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{@cpd_workers_width},14},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}}},#{@cpd_workers},#{@cpd_workers_short}}\
         #[noreverse]\u{e0b4}#[default]\
         #{?#{@cpd_version},#[fg=colour8]\u{2500}#[fg=colour8]\u{e0b6}#[reverse]#{@cpd_version}#[noreverse]\u{e0b4}#[default],}\
         #[fg=colour8]\u{2500}#[default]";
    let format = cpd_core::host::tmux::STATUS_FORMAT.as_str();
    assert!(format.starts_with(&prefix), "{format}");
    assert!(format.ends_with(suffix), "{format}");
}

/// The buttons' glyphs are the hs-d235 Material Design code points (width 1 is not asserted: no `unicode-width` dependency here).
#[test]
fn the_button_glyphs_are_the_hs_d235_code_points() {
    use cpd_core::host::tmux::{LEGEND_GLYPH, LOGS_GLYPH, RESET_GLYPH};
    assert_eq!([RESET_GLYPH, LEGEND_GLYPH, LOGS_GLYPH], ["\u{F0A1D}", "\u{F0A01}", "\u{F09ED}"]);
}

#[test]
fn the_statusbar_is_no_view() {
    assert_eq!(View::parse("statusbar"), None);
    assert!(View::ALL.iter().all(|v| v.as_str() != "statusbar"));
    assert_eq!(View::parse("nope"), None);
}

/// Finding 4: a pane made by a newer cpd (a view this binary does not know)
/// is skipped, and the rest of the listing is intact; so is an older cpd's
/// StatusBar pane, which `cpd tui` removes.
#[test]
fn a_pane_with_an_unknown_view_is_skipped_not_a_failed_listing() {
    let s = server();
    let t = &s.tmux;
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(s.env.root())).unwrap();
    let first = t.list_panes().unwrap().remove(0).pane;
    let (_, other) = t.new_window("cpd", "x", &sleeper(s.env.root())).unwrap();
    let (_, kept) = t.new_window("cpd", "y", &sleeper(s.env.root())).unwrap();
    let set = |pane: &PaneId, view: &str| {
        let out = s.env.tmux(&["set", "-p", "-t", pane.as_str(), "@cpd_view", view]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    set(&first, "statusbar");
    set(&other, "hologram");

    let panes = t.list_panes().unwrap();
    assert!(panes.iter().all(|p| p.pane != first && p.pane != other), "{panes:?}");
    assert_eq!(pane(&panes, &kept).cpd_view, None);
}

/// Runs the test `name` of this binary again in a child process born with `env`'s `bin/` first on `PATH` and `vars`,
/// so the wrapper finds `env`'s fake `tmux` and no other test sees either; panics unless it passed.
fn rerun_with_fake_tmux(env: &TestEnv, name: &str, vars: &[(&str, &str)]) {
    let mut path = vec![env.bin()];
    path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child.args(["--exact", &format!("host::{name}"), "--nocapture"]).env("PATH", std::env::join_paths(path).unwrap()).envs(vars.iter().copied());
    let out = common::output(&mut child).unwrap();
    let printed = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && printed.contains("1 passed"), "{printed}");
}

/// Whether process `pid` is running (a zombie is not).
fn running(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(") ").next().is_some_and(|r| !r.starts_with('Z')))
}

/// engine-W8 (§6.3 liveness): only tmux saying that no server runs means "no panes".
/// Any other failure of `list-panes` and `list-sessions` is `tmux_failed`, so E2 and the
/// driver never read a tmux hiccup as every pane gone.
#[test]
fn list_panes_is_empty_only_when_tmux_says_no_server_runs() {
    const NAME: &str = "list_panes_is_empty_only_when_tmux_says_no_server_runs";
    const CASE: &str = "CPD_TEST_FAKE_TMUX_CASE";
    // In the child: the fake `tmux` first on `PATH`, which ignores the socket and the tmpdir.
    let fake = || Tmux::with_tmpdir("cpd-test-fake", std::env::temp_dir());
    match std::env::var(CASE).as_deref() {
        Ok("failure") => {
            assert_eq!(fake().list_panes().unwrap_err().code(), ErrorCode::TmuxFailed);
            assert_eq!(fake().server_running().unwrap_err().code(), ErrorCode::TmuxFailed);
            return;
        }
        Ok("no-server") => {
            assert!(fake().list_panes().unwrap().is_empty());
            assert!(!fake().server_running().unwrap());
            return;
        }
        _ => {}
    }
    let s = server();
    for (case, body) in [("failure", "echo 'lost server' >&2; exit 1"), ("no-server", "echo 'no server running on /tmp/tmux-1000/x' >&2; exit 1")] {
        let before = s.env.calls("tmux").len();
        s.env.fake("tmux", body);
        rerun_with_fake_tmux(&s.env, NAME, &[(CASE, case)]);
        assert!(s.env.calls("tmux").len() > before, "the child of {case} ran the fake tmux");
    }
    // The real tmux with no socket file: "error connecting to <socket> (No such file or directory)".
    assert!(s.tmux.list_panes().unwrap().is_empty());
    assert!(!s.tmux.server_running().unwrap());
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
        rerun_with_fake_tmux(&env, NAME, &[("CPD_TEST_DEADLINE_TMUX", "1")]);
        // The sleeper would run SLEEP_SECONDS: its death is the group kill, not its own end.
        let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        wait_until("the fake tmux's child to die", || !running(pid));
        return;
    }
    // No TestEnv here: its teardown would run the hanging fake `tmux` first on this process's `PATH`. The fake
    // ignores the socket and the tmpdir, and no real tmux runs.
    let tmux = Tmux::with_tmpdir("cpd-test-deadline", std::env::temp_dir());
    let started = std::time::Instant::now();
    let err = tmux.server_running().unwrap_err();

    // time-claim: the call returns at its 1 s deadline, not when the sleeper ends; no per-poll signal from the deadline.
    assert!(started.elapsed() < Duration::from_secs(4), "took {:?}", started.elapsed());
    assert_eq!(err.code(), ErrorCode::ToolTimeout, "{err:?}");
    let d = err.details();
    assert_eq!(d["tool"], "tmux");
    assert_eq!(d["deadline_seconds"], 1);
    assert_eq!(d["argv"], serde_json::json!(tmux.argv(&["list-sessions".into(), "-F".into(), "#{session_id}".into()])));
}

// ---- Every tmux call is a UTF-8 client (hs-d463; contract §6.1 "Every tmux call is a UTF-8 client") ----
//
// A tmux 3.8 client with no UTF-8 locale prints the tabs of a `-F` format as `_`, so `list-panes` cannot be parsed.
// The mechanism is `tmux -u`, first after the program, whatever the caller's `LANG`/`LC_*`.

use crate::common::{LOCALES, Locale, real_tmux};

/// A program that runs the real tmux as a caller with the locale `locale` and nothing else would: `env -i`, `PATH` and
/// `TMUX_TMPDIR` (which `Tmux::with_tmpdir` sets for the call).
fn tmux_as_caller_with(env: &TestEnv, name: &str, locale: Locale) -> std::path::PathBuf {
    let path = env.root().join(name);
    let script = format!("#!/bin/sh\nexec env -i PATH=\"$PATH\" TMUX_TMPDIR=\"$TMUX_TMPDIR\" {} '{}' \"$@\"\n", locale.env_words(), real_tmux().display());
    common::write_executable(&path, &script);
    path
}

/// The defect itself (hs-d463): the liveness call, `list-panes -a -F` (§6.3), through `Tmux` against a real server whose
/// caller has no UTF-8 locale. Before the fix the tabs print as `_` and `list_panes` fails `tmux_failed: tmux list-panes
/// printed unexpected output`.
#[test]
fn list_panes_parses_for_a_caller_without_a_utf8_locale() {
    let mut failures = Vec::new();
    for locale in LOCALES {
        let env = TestEnv::new();
        let program = tmux_as_caller_with(&env, "tmux-caller", locale);
        let tmux = Tmux::with_tmpdir(env.tmux_socket(), env.tmux_tmpdir()).with_program(program);
        if let Err(e) = tmux.ensure_session("cpd", "main", &sleeper(env.root())) {
            failures.push(format!("{locale:?}: ensure_session: {e:?}"));
            continue;
        }
        match tmux.list_panes() {
            Ok(panes) if panes.len() == 1 => {}
            other => failures.push(format!("{locale:?}: list_panes: {other:?}")),
        }
    }
    assert!(failures.is_empty(), "list-panes does not parse for a caller whose locale is not UTF-8:\n{}", failures.join("\n"));
}

/// `tmux -V` and the other program-less calls are UTF-8 clients too: the argv of every `Tmux` call starts `-u`, in `Tmux::argv`
/// (`tmux -u -L <socket> -f /dev/null <command>`) and in what a fake tmux receives from every method.
#[test]
fn every_tmux_method_runs_tmux_with_u_first() {
    let env = TestEnv::new();
    let fake = env.fake("tmux-recorder", "exit 0");
    let tmux = Tmux::with_tmpdir("cpdt-argv", env.tmux_tmpdir()).with_program(&fake);

    let argv = tmux.argv(&["list-panes".into()]);
    assert_eq!(&argv[..2], ["tmux", "-u"], "{argv:?}");
    assert_eq!(argv[2..], ["-L", "cpdt-argv", "-f", "/dev/null", "list-panes"], "{argv:?}");

    let pane = PaneId::parse("%1").unwrap();
    let other = PaneId::parse("%2").unwrap();
    let window = WindowId::parse("@1").unwrap();
    let spawn = sleeper(env.root());
    // Each method runs one or more tmux calls; what a fake answers does not matter, only what it is called with.
    let _ = tmux.server_running();
    let _ = tmux.has_session("cpd");
    let _ = tmux.ensure_session("cpd", "main", &spawn);
    let _ = tmux.create_session("cpd", "main", &spawn);
    let _ = tmux.apply_server_options();
    let _ = tmux.apply_status_line("cpd", "0.0.5-abc");
    let _ = tmux.preview();
    let _ = tmux.server_version();
    let _ = tmux.global_option("status");
    let _ = tmux.set_pane_session(&pane, &SessionId::new_v4());
    let _ = tmux.set_pane_title(&pane, "t");
    let _ = tmux.set_pane_view(&pane, View::Tree);
    let _ = tmux.new_window("cpd", "w", &spawn);
    let _ = tmux.split_window(&pane, Split::Right, None, &spawn);
    let _ = tmux.swap_pane(&pane, &other);
    let _ = tmux.rename_window(&window, "w");
    let _ = tmux.respawn_pane(&pane, &spawn);
    let _ = tmux.respawn_pane_killing(&pane, &spawn);
    let _ = tmux.kill_pane(&pane);
    let _ = tmux.kill_window(&window);
    let _ = tmux.list_panes();
    let _ = tmux.kill_server();

    let calls = env.calls("tmux-recorder");
    assert!(calls.len() >= 20, "the fake tmux saw {} calls", calls.len());
    let bare: Vec<String> = calls.iter().filter(|c| c.first().map(String::as_str) != Some("-u")).map(|c| c.join(" ")).collect();
    assert!(bare.is_empty(), "tmux calls that are not UTF-8 clients (`-u` first):\n{}", bare.join("\n"));
}

/// `apply_status_line` sets the global user option `@cpd_version` to the version it is given (§6.1).
#[test]
fn apply_status_line_sets_the_global_cpd_version_option() {
    let env = TestEnv::new();
    let fake = env.fake("tmux-recorder", "exit 0");
    let tmux = Tmux::with_tmpdir("cpdt-version", env.tmux_tmpdir()).with_program(&fake);
    tmux.apply_status_line("cpd", "0.0.5-5ba3a1d3").unwrap();
    let calls: Vec<String> = env.calls("tmux-recorder").iter().map(|c| c.join(" ")).collect();
    assert!(calls.iter().any(|c| c.ends_with("set -g @cpd_version 0.0.5-5ba3a1d3")), "{calls:#?}");
}

/// Writes `$CPD_HOME/config.toml` of `env` for the fake-tmux runs below.
fn fake_tmux_recording(env: &TestEnv) {
    env.write_tmux_config("");
    // `-V` answers like a tmux; the server has no session, so every other call fails or prints nothing.
    env.fake("tmux", "case \"$*\" in *\" -V\"*|-V*) echo 'tmux 3.8' ;; *) exit 1 ;; esac");
}

/// The tmux calls of `cpd` itself, from the commands that make them, with a fake tmux first on `PATH`, for each locale: the
/// `-V` probe of `doctor` and `watch --ensure`, `tui`, `tui --respawn`: every one starts `-u` (a bare call is the defect).
#[test]
fn every_tmux_call_of_cpd_is_a_utf8_client_whatever_the_callers_locale() {
    let mut bare = Vec::new();
    for locale in LOCALES {
        let env = TestEnv::new();
        fake_tmux_recording(&env);
        for args in [&["doctor"][..], &["tui"], &["tui", "--respawn"], &["watch", "--ensure"]] {
            let mut cmd = env.command(common::CPD);
            locale.apply(&mut cmd).args(args);
            let _ = common::output(&mut cmd).unwrap();
        }
        let calls = env.calls("tmux");
        assert!(calls.len() >= 4, "{locale:?}: the fake tmux saw {} calls", calls.len());
        for call in calls.iter().filter(|c| c.first().map(String::as_str) != Some("-u")) {
            bare.push(format!("{locale:?}: tmux {}", call.join(" ")));
        }
    }
    bare.sort();
    bare.dedup();
    assert!(bare.is_empty(), "calls cpd makes that are not UTF-8 clients (`-u` first):\n{}", bare.join("\n"));
}

/// The structural guard (hs-d463): the one place that spawns tmux is `core/src/host/tmux.rs`, and what it spawns goes
/// through `client(program)`, which puts `-u` first; no source names the program in a `Command::new("tmux")` of its own.
#[test]
fn no_source_spawns_tmux_with_a_command_of_its_own() {
    fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for dir in ["core/src", "cli/src", "tui/src"] {
        sources(&root.join(dir), &mut files);
    }
    assert!(files.len() > 50, "found only {} sources", files.len());
    let mut hits = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // Production code only: unit tests (`#[cfg(test)]`, the end of every file here) may name the program.
        let code = text.split("#[cfg(test)]").next().unwrap();
        let name = file.strip_prefix(&root).unwrap().display().to_string();
        for (n, line) in code.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            // `Command::new("tmux")`: the program named in place, not given to `client(program)`.
            if line.contains("Command::new(\"tmux\"") {
                hits.push(format!("{name}:{}: {}", n + 1, t));
            }
        }
    }
    assert!(hits.is_empty(), "tmux is spawned outside `host::tmux::client` (it puts `-u` first, hs-d463):\n{}", hits.join("\n"));
}
