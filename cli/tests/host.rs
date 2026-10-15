//! The tmux host (contract §6.1–§6.3, amendments 1, 2 and 9) on a private
//! tmux server: the wrapper's calls, and the windows and panes of §6.2 found
//! by their `@cpd_*` options.

mod common;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::{Duration, Instant};

use common::TestEnv;
use cpd2::error::ErrorCode;
use cpd2::host::layout::{self, View};
use cpd2::host::{Pane, PaneId, Spawn, Split, Tmux};
use cpd2::ids::{AttemptId, PlanId, SessionId};

/// The wrapper runs `tmux` with this process's environment, so each test
/// points `TMUX_TMPDIR` at its own `TestEnv` while holding this lock. Rust's
/// `Command` reads the environment under std's own lock, so this is the only
/// cross-test interference to prevent.
static TMUX_ENV: Mutex<()> = Mutex::new(());

struct Server {
    env: TestEnv,
    tmux: Tmux,
    _lock: MutexGuard<'static, ()>,
}

fn server() -> Server {
    let lock = TMUX_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let env = TestEnv::new();
    // SAFETY: serialised by TMUX_ENV; no other thread of this test binary
    // spawns tmux or reads TMUX_TMPDIR without holding it.
    unsafe { std::env::set_var("TMUX_TMPDIR", env.root().join("tmux")) };
    let tmux = Tmux::new(env.tmux_socket());
    Server { env, tmux, _lock: lock }
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

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        sleep(Duration::from_millis(50));
    }
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
    for (option, value) in [
        ("mouse", "on"),
        ("default-terminal", "tmux-256color"),
        ("extended-keys", "on"),
        ("escape-time", "10"),
        ("status", "off"),
        ("pane-border-status", "top"),
        ("pane-border-lines", "heavy"),
        ("remain-on-exit", "on"),
    ] {
        assert_eq!(s.tmux.global_option(option).unwrap(), value, "{option}");
    }
    let features = s.tmux.global_option("terminal-features").unwrap();
    assert_eq!(features.matches("xterm-ghostty:extkeys").count(), 1, "{features}");

    // Idempotent: the session exists, nothing is created or appended again.
    assert!(!s.tmux.ensure_session("cpd", "main", &sleeper(s.env.root())).unwrap());
    assert!(s.tmux.ensure_session("cpd-work", "watch", &sleeper(s.env.root())).unwrap());
    let features = s.tmux.global_option("terminal-features").unwrap();
    assert_eq!(features.matches("xterm-ghostty:extkeys").count(), 1, "{features}");

    let size = s.env.tmux(&["display", "-p", "-t", "=cpd:", "#{window_width}x#{window_height}"]);
    assert_eq!(String::from_utf8_lossy(&size.stdout).trim(), "215x62");
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
    let slot_size = |pid: u32| {
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
        "printf '%s|%s|%s|%s|%s' \"$PWD\" \"$CPD_PLAN\" \"$A_B\" \"${{TMUX_PANE:+set}}\" \"$1\" > '{}'; sleep 600",
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

    wait_for("the worker to write", || std::fs::read_to_string(&out).is_ok_and(|s| !s.is_empty()));
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
    wait_for("the pane to die", || pane(&t.list_panes().unwrap(), &dying).dead);
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
