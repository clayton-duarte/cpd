//! The tmux host (contract §6.1–§6.3, amendments 1, 2 and 9): the wrapper's calls, and the windows and panes of §6.2
//! found by their `@cpd_*` options, as library calls on `Tmux` over the in-process fake tmux (the `tmux` route of the
//! `FakeRunner`, ADR 0013): no tmux, no process, no wall clock. The static formats and options are unit tests beside
//! the code (`core/src/host/tmux/pill_lines_tests.rs`).

use crate::common;

use std::path::Path;
use std::sync::Arc;

use common::{LOCALES, Locale};
use cpd_core::clock::ManualClock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::fakes::tmux::TmuxRoute;
use cpd_core::host::layout::{self, View};
use cpd_core::host::tmux::{PANE_DIED_HOOK, WINDOW_UNLINKED_HOOK};
use cpd_core::host::runner::{Call, FakeRunner, Reply};
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux, WindowId};
use cpd_core::ids::{AttemptId, PlanId, SessionId};

/// The socket of the fake server of every test here.
const SOCKET: &str = "cpdt-host";

/// A fake tmux server on [`SOCKET`] in a directory of its own, and the `Tmux` that talks to it.
struct Server {
    dir: tempfile::TempDir,
    tmux: Tmux,
    runner: FakeRunner,
    route: Arc<TmuxRoute>,
}

/// `FakeRunner::with_tmux()`, with the route kept so a test can read the fake's state.
fn server() -> Server {
    let runner = FakeRunner::new(ManualClock::new());
    // Weak: the route lives in the runner's script, a strong handle back would be a cycle (as `FakeRunner::with_tmux`).
    let route = Arc::new(TmuxRoute::new().with_runner(runner.weak()));
    let runner = runner.route("tmux", route.clone());
    let dir = tempfile::tempdir().unwrap();
    let tmux = Tmux::with_tmpdir(SOCKET, dir.path()).with_runner(Arc::new(runner.clone()));
    Server { dir, tmux, runner, route }
}

impl Server {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// `tmux -L <socket> <args>` through the wrapper: the stdout of a call that must succeed.
    fn raw(&self, args: &[&str]) -> String {
        self.try_raw(args).unwrap_or_else(|e| panic!("tmux {args:?}: {e:?}"))
    }

    fn try_raw(&self, args: &[&str]) -> Result<String, CpdError> {
        self.tmux.run(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
    }

    /// What the fake server holds: the panes' recorded argv, cwd and environment.
    fn state(&self) -> cpd_core::fakes::tmux::State {
        self.route.fake().state(SOCKET)
    }

    fn fake_pane(&self, id: &PaneId) -> cpd_core::fakes::tmux::Pane {
        self.state().panes().into_iter().find(|p| p.id == id.as_str()).unwrap_or_else(|| panic!("no pane {id} in the fake"))
    }
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

/// A `Tmux` over a runner that answers every `tmux` call with `reply`: for what a call is, not what a server does.
fn answering(reply: Reply) -> (FakeRunner, Tmux) {
    let runner = FakeRunner::new(ManualClock::new()).on("tmux", |_| true, reply);
    let tmux = Tmux::with_tmpdir(SOCKET, std::env::temp_dir()).with_runner(Arc::new(runner.clone()));
    (runner, tmux)
}

/// hygiene.md 3b: tmux falls back to `/tmp/tmux-<uid>/` for a `TMUX_TMPDIR` that does not exist, so a late start escapes
/// every teardown; cpd refuses instead, before any call reaches the runner.
#[test]
fn a_tmux_call_whose_tmux_tmpdir_is_missing_is_refused_and_starts_no_server_in_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let runner = FakeRunner::with_tmux();
    let tmux = Tmux::with_tmpdir("cpdt-missing-tmpdir", dir.path().join("no-such-tmux-dir")).with_runner(Arc::new(runner.clone()));

    let err = tmux.ensure_session("cpd", "main", &sleeper(dir.path())).unwrap_err();

    assert_eq!(err.code(), ErrorCode::TmuxFailed, "{err:?}");
    assert!(runner.calls().is_empty(), "a call reached the runner: {:?}", runner.calls());
}

#[test]
fn starts_the_server_once_with_the_contract_options() {
    let s = server();
    assert!(!s.tmux.server_running().unwrap());
    assert!(!s.tmux.has_session("cpd").unwrap());
    assert!(s.tmux.list_panes().unwrap().is_empty());

    assert!(s.tmux.ensure_session("cpd", "main", &sleeper(s.root())).unwrap());
    assert!(s.tmux.server_running().unwrap());
    assert!(s.tmux.has_session("cpd").unwrap());
    assert!(!s.tmux.has_session("cp").unwrap(), "has-session matches the exact name");
    assert_server_options(&s);

    // Applying them again on the running server appends nothing.
    s.tmux.apply_server_options().unwrap();
    assert_server_options(&s);

    // Idempotent: the session exists, nothing is created or appended again.
    assert!(!s.tmux.ensure_session("cpd", "main", &sleeper(s.root())).unwrap());
    assert!(s.tmux.ensure_session("cpd-work", "watch", &sleeper(s.root())).unwrap());
    let features = s.tmux.global_option("terminal-features").unwrap();
    assert_eq!(features.matches("xterm-ghostty:extkeys").count(), 1, "{features}");

    let size = s.raw(&["display", "-p", "-t", "=cpd:", "#{window_width}x#{window_height}"]);
    assert_eq!(size.trim(), "215x62");
}

/// `pane-border-format`, exactly (contract §6.1, hs-d475, hs-d478, hs-d671): every title is a pill with no padding;
/// the three view titles are dango in the cursor cycle order (Context sakura 5, Plan shiro 7, Dispatch midori 2) and
/// keep their colour on the focused pane, while every other title (a Lead's `@cpd_title` and the pane
/// command) is grey (colour 7) and blue on the focused pane, set in the format itself; one `━` with no colour of its own (it takes the
/// pane border line's style, blue on the focused pane and colour 7 otherwise, whatever the pill's colour; pro-d61) before the left cap, so the cap is the fourth
/// cell of the border row, three columns in counted from the pane's edge.
const BORDER_FORMAT_WANT: &str = "#{?#{@cpd_session},#{?#{@cpd_title},\
━#{?pane_active,#[fg=colour4],#[fg=colour7]}\u{e0b6}#[reverse]#{@cpd_title}#[noreverse]\u{e0b4},},\
#{?#{==:#{@cpd_view},chat},,#{?#{==:#{@cpd_view},history},,\
━#{?#{==:#{@cpd_view},tree},#[fg=colour5],#{?#{==:#{@cpd_view},plan},#[fg=colour7],#{?#{==:#{@cpd_view},widgets},#[fg=colour2],#{?pane_active,#[fg=colour4],#[fg=colour7]}}}}\u{e0b6}#[reverse]\
#{?#{==:#{@cpd_view},tree},Context,\
#{?#{==:#{@cpd_view},plan},Plan,#{?#{==:#{@cpd_view},widgets},Dispatch,#{pane_current_command}}}}\
#[noreverse]\u{e0b4}}}}";


#[test]
fn the_worker_is_no_view_any_more() {
    assert!(View::parse("worker").is_none());
    assert!(View::ALL.iter().all(|v| v.as_str() != "worker"));
    for name in ["tree", "plan", "widgets", "chat", "history"] {
        assert_eq!(View::parse(name).map(|v| v.as_str()), Some(name), "{name}");
    }
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
            BORDER_FORMAT_WANT,
        ),
        ("pane-border-style", "fg=colour7"),
        ("pane-active-border-style", "fg=blue"),
        ("remain-on-exit", "on"),
        ("set-titles", "on"),
        ("set-titles-string", "φ · #{?@cpd_machine,#{@cpd_machine},#{host_short}}"),
    ] {
        assert_eq!(s.tmux.global_option(option).unwrap(), value, "{option}");
    }
    assert_eq!(s.raw(&["show", "-gv", "extended-keys-format"]).trim(), "csi-u", "extended-keys-format");
    assert!(s.tmux.global_option("pane-border-format").unwrap().contains(",Dispatch,"));
    let features = s.tmux.global_option("terminal-features").unwrap();
    for feature in ["xterm*:extkeys", "xterm-ghostty:extkeys", "xterm-ghostty:hyperlinks"] {
        assert_eq!(features.matches(feature).count(), 1, "{feature} in {features}");
    }
    // A view is respawned; the engine's window (`watch`) starts the engine again, as does its window's loss (contract §6.6, hs-d328).
    // What the fake keeps is the command cpd set; tmux's own rendering of it (`pane-died[0] if-shell ...`) is tmux's.
    assert_eq!(
        s.state().hooks(),
        [("pane-died".to_owned(), PANE_DIED_HOOK.to_owned()), ("window-unlinked".to_owned(), WINDOW_UNLINKED_HOOK.to_owned())],
    );
    assert!(PANE_DIED_HOOK.contains("if -F \"#{@cpd_view}\" \"respawn-pane\" "), "a view is respawned: {PANE_DIED_HOOK}");
    for hook in [PANE_DIED_HOOK, WINDOW_UNLINKED_HOOK] {
        assert!(hook.contains("run-shell -b 'cpd watch --ensure --wait >/dev/null 2>&1'"), "{hook}");
    }
    assert_eq!(s.raw(&["show", "-sv", "exit-empty"]), "off\n");
    // tmux's own default: a click selects the pane, then passes the click on; the status line's click runs `cpd tui action`.
    // (The fake lists every binding of the table, in its own spelling of the commands.)
    let bindings = s.raw(&["list-keys", "-T", "root"]).split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(bindings.contains("bind-key -T root MouseDown1Pane select-pane -t = ; send-keys -M"), "{bindings}");
    assert!(
        bindings.contains("bind-key -T root MouseDown1Status run-shell -b cpd tui action #{mouse_status_range} --client #{client_name} 2>/dev/null"),
        "{bindings}"
    );
}

/// Kills a pane's program, as a crash would: the fake ends the pane's program with status 137 (`SIGKILL`) and runs the
/// `pane-died` hook at once, as the server does when it reads the status of the child.
fn kill_program(s: &Server, id: &PaneId) {
    assert!(pane(&s.tmux.list_panes().unwrap(), id).pid.is_some(), "a live pane has a pid");
    s.raw(&["__ctl", "pane-exit", id.as_str(), "--status", "137"]);
}

#[test]
fn a_killed_view_pane_respawns_and_a_killed_lead_or_engine_pane_stays_dead() {
    let s = server();
    let t = &s.tmux;
    let root = s.root();
    let (_, engine) = t.create_session("cpd-work", layout::WATCH_WINDOW, &sleeper(root)).unwrap().unwrap();
    let (_, view) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(root)).unwrap().unwrap();
    t.set_pane_view(&view, View::Tree).unwrap();
    let (_, lead) = t.new_window("cpd", "park-pay", &sleeper(root)).unwrap();
    t.set_pane_session(&lead, &SessionId::new_v4()).unwrap();
    let view_pid = pane(&t.list_panes().unwrap(), &view).pid;

    // The Lead and the engine first, and until they are dead: tmux runs the `pane-died` hooks in the order the panes
    // died, so once the view (killed last) is respawned, the hooks of the other two have run and left them dead.
    for id in [&engine, &lead] {
        kill_program(&s, id);
    }
    let panes = t.list_panes().unwrap();
    assert!(pane(&panes, &lead).dead && pane(&panes, &engine).dead, "the Lead and the engine are dead: {panes:?}");
    kill_program(&s, &view);
    let panes = t.list_panes().unwrap();
    let p = pane(&panes, &view);
    assert!(!p.dead && p.pid.is_some() && p.pid != view_pid, "the view respawned: {p:?}");
    assert!(pane(&panes, &lead).dead && pane(&panes, &engine).dead, "{panes:?}");
    assert_eq!(pane(&panes, &view).cpd_view, Some(View::Tree), "the view keeps its option");
    // The hook of the engine's window, run by the server (`run-shell`) when the engine died, is the only call it made.
    let started: Vec<_> = s.runner.calls().into_iter().filter(|c| c.tool == "sh").map(|c| c.args.join(" ")).collect();
    assert_eq!(started, ["-c cpd watch --ensure --wait >/dev/null 2>&1"], "the engine pane's death starts the engine again once");
}

#[test]
fn respawn_killing_restarts_a_live_pane_in_place() {
    let s = server();
    let t = &s.tmux;
    let (_, id) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(s.root())).unwrap().unwrap();
    t.set_pane_view(&id, View::Plan).unwrap();
    let before = pane(&t.list_panes().unwrap(), &id).clone();
    let script = "echo \"$CPD_HOST_TEST_VAR\" > respawned.txt; sleep 120";
    t.respawn_pane_killing(&id, &sh(script)).unwrap();
    let after = pane(&t.list_panes().unwrap(), &id).clone();
    assert!(!after.dead);
    assert_ne!(after.pid, before.pid);
    assert_eq!(after.cpd_view, Some(View::Plan));
    assert_eq!(s.fake_pane(&id).argv, ["sh", "-c", script]);

    // An empty argv reruns the pane's own command, with the environment given again.
    let mut rerun = Spawn::default();
    rerun.env.insert("CPD_HOST_TEST_VAR".into(), "again".into());
    t.respawn_pane_killing(&id, &rerun).unwrap();
    assert_ne!(pane(&t.list_panes().unwrap(), &id).pid, after.pid);
    let rerun_pane = s.fake_pane(&id);
    assert_eq!(rerun_pane.argv, ["sh", "-c", script], "the pane's own command ran again");
    assert!(s.state().pane_env(id.as_str()).contains(&("CPD_HOST_TEST_VAR".to_owned(), "again".to_owned())), "with the environment given again");
}

#[test]
fn create_session_returns_the_first_window_and_pane_once() {
    let s = server();
    let mut spawn = sleeper(s.root());
    spawn.env.insert("CPD_HOST_TEST_VAR".into(), "pay".into());
    let (window, pane) = s.tmux.create_session("cpd", "park-x", &spawn).unwrap().expect("created");
    // -e reaches the first pane only, not the session's later windows.
    let session_env = s.raw(&["show-environment", "-t", "=cpd"]);
    assert!(!session_env.contains("CPD_HOST_TEST_VAR"), "{session_env:?}");
    let later = s.tmux.new_window("cpd", "later", &sh("echo \"plan=$CPD_HOST_TEST_VAR\"; sleep 120")).unwrap().1;
    let env_of = |id: &PaneId| s.state().pane_env(id.as_str()).into_iter().filter(|(k, _)| k == "CPD_HOST_TEST_VAR").collect::<Vec<_>>();
    assert_eq!(env_of(&pane), [("CPD_HOST_TEST_VAR".to_owned(), "pay".to_owned())], "the first pane has it");
    assert_eq!(env_of(&later), [], "the later window's pane has not: it prints `plan=` and nothing");
    assert_eq!(s.tmux.global_option("remain-on-exit").unwrap(), "on", "a fresh server gets the contract options");
    let panes = s.tmux.list_panes().unwrap();
    let first = self::pane(&panes, &pane);
    assert_eq!((&first.window, first.window_name.as_str()), (&window, "park-x"));
    // Idempotent: the session exists, nothing is created.
    assert_eq!(s.tmux.create_session("cpd", "park-y", &sleeper(s.root())).unwrap(), None);
    assert_eq!(s.tmux.list_panes().unwrap().len(), 2);
}

#[test]
fn builds_the_cockpit_and_finds_its_panes_by_option() {
    let s = server();
    let t = &s.tmux;
    let root = s.root();
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
        s.raw(&["display", "-p", "-t", p.as_str(), "#{pane_width}x#{pane_height}"]).trim().to_owned()
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
    let wt = s.root().join("repos");
    let plan = PlanId::parse("cpd2").unwrap();
    let attempt = AttemptId::parse("rs-host-a1").unwrap();
    t.ensure_session("cpd-work", layout::WATCH_WINDOW, &sleeper(s.root())).unwrap();

    let mut spawn = sh("printf '%s|%s|%s|%s|%s' \"$PWD\" \"$CPD_PLAN\" \"$A_B\" \"${TMUX_PANE:+set}\" \"$1\"; sleep 120");
    spawn.argv.extend(["argv0".into(), "two words; $HOME".into()]);
    spawn.cwd = Some(wt.clone());
    spawn.env.insert("CPD_PLAN".into(), "cpd2".into());
    spawn.env.insert("A_B".into(), "x y".into());
    let name = layout::attempt_window(&plan, &attempt);
    let (window, worker) = t.new_window("cpd-work", &name, &spawn).unwrap();
    let session = SessionId::new_v4();
    t.set_pane_session(&worker, &session).unwrap();

    // The pane starts in the worktree with the argument verbatim, no shell of ours between, and the given environment;
    // tmux sets TMUX_PANE in its panes itself, so the client that made the call must not carry its own.
    let started = s.fake_pane(&worker);
    assert_eq!(started.cwd, wt.display().to_string());
    assert_eq!(started.argv, spawn.argv, "the argument arrives verbatim, no shell");
    assert_eq!(started.argv.last().unwrap(), "two words; $HOME");
    let env = s.state().pane_env(worker.as_str());
    assert!(env.contains(&("CPD_PLAN".to_owned(), "cpd2".to_owned())) && env.contains(&("A_B".to_owned(), "x y".to_owned())), "{env:?}");
    let calls = s.runner.calls();
    let made = calls.iter().find(|c| c.args.iter().any(|a| a == "new-window") && c.args.contains(&name)).expect("the new-window call");
    assert!(made.envs.contains(&("TMUX_PANE".to_owned(), None)) && made.envs.contains(&("TMUX".to_owned(), None)), "{:?}", made.envs);
    let panes = t.list_panes().unwrap();
    let p = layout::session_pane(&panes, &session).unwrap();
    assert_eq!((&p.pane, &p.window, p.session_name.as_str(), p.window_name.as_str()), (&worker, &window, "cpd-work", "cpd2--rs-host-a1"));
}

#[test]
fn a_dead_pane_stays_and_is_respawned() {
    let s = server();
    let t = &s.tmux;
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(s.root())).unwrap();
    let (_, dying) = t.new_window("cpd", "x", &sh("exit 3")).unwrap();
    s.raw(&["__ctl", "pane-exit", dying.as_str(), "--status", "3"]);
    let dead = pane(&t.list_panes().unwrap(), &dying).clone();
    assert_eq!(dead.dead_status, Some(3));

    t.respawn_pane(&dying, &sleeper(s.root())).unwrap();
    let alive = pane(&t.list_panes().unwrap(), &dying).clone();
    assert!(!alive.dead);
    assert_eq!(alive.dead_status, None);
    assert_ne!(alive.pid, dead.pid);

    // Without -k a live pane is refused, never killed.
    let err = t.respawn_pane(&dying, &sleeper(s.root())).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(pane(&t.list_panes().unwrap(), &dying).pid, alive.pid);
}

#[test]
fn a_failing_call_is_tmux_failed_with_argv_exit_and_stderr() {
    let s = server();
    let t = &s.tmux;
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(s.root())).unwrap();
    let missing = PaneId::parse("%999").unwrap();
    let err = t.kill_pane(&missing).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    let d = err.details();
    assert_eq!(d["argv"], serde_json::json!(["tmux", "-u", "-L", SOCKET, "-f", "/dev/null", "kill-pane", "-t", "%999"]));
    assert_eq!(d["exit"], 1);
    assert!(d["stderr"].as_str().unwrap().contains("%999"), "{d:?}");
}

#[test]
fn set_pane_title_runs_set_p_on_cpd_title_and_a_missing_pane_fails_with_its_argv() {
    let s = server();
    let t = &s.tmux;
    let (_, pane_id) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(s.root())).unwrap().unwrap();
    t.set_pane_title(&pane_id, "Retry #{x} payouts; ok").unwrap();
    let shown = s.raw(&["show", "-pv", "-t", pane_id.as_str(), "@cpd_title"]);
    assert_eq!(shown, "Retry #{x} Payouts; Ok\n");

    let missing = PaneId::parse("%999").unwrap();
    let err = t.set_pane_title(&missing, "X").unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(
        err.details()["argv"],
        serde_json::json!(["tmux", "-u", "-L", SOCKET, "-f", "/dev/null", "set", "-p", "-t", "%999", "@cpd_title", "X"])
    );
}

#[test]
fn a_leads_border_shows_its_title_reversed_between_caps_and_a_titleless_lead_shows_nothing() {
    let s = server();
    let t = &s.tmux;
    let (_, pane_id) = t.create_session("cpd", layout::MAIN_WINDOW, &sleeper(s.root())).unwrap().unwrap();
    // `#{T:pane-border-format}`: the server's `pane-border-format` option, expanded as a format on this pane.
    let format = t.global_option("pane-border-format").unwrap();
    let border = || s.raw(&["display-message", "-p", "-t", pane_id.as_str(), &format]).trim_end_matches('\n').to_owned();
    t.set_pane_session(&pane_id, &SessionId::new_v4()).unwrap();
    assert_eq!(border(), "");
    // Title Case, no padding, the pill in blue (this pane is the active one), one `━` first with no colour before it
    // (it takes the border line's style, pro-d61) before the left cap (hs-d475, hs-d478)
    t.set_pane_title(&pane_id, "Retry payouts").unwrap();
    assert_eq!(border(), "━#[fg=colour4]\u{e0b6}#[reverse]Retry Payouts#[noreverse]\u{e0b4}");
}

/// Finding 4: a pane made by a newer cpd (a view this binary does not know)
/// is skipped, and the rest of the listing is intact; so is an older cpd's
/// StatusBar pane, which `cpd tui` removes.
#[test]
fn a_pane_with_an_unknown_view_is_skipped_not_a_failed_listing() {
    let s = server();
    let t = &s.tmux;
    t.ensure_session("cpd", layout::MAIN_WINDOW, &sleeper(s.root())).unwrap();
    let first = t.list_panes().unwrap().remove(0).pane;
    let (_, other) = t.new_window("cpd", "x", &sleeper(s.root())).unwrap();
    let (_, kept) = t.new_window("cpd", "y", &sleeper(s.root())).unwrap();
    let set = |pane: &PaneId, view: &str| {
        s.raw(&["set", "-p", "-t", pane.as_str(), "@cpd_view", view]);
    };
    set(&first, "statusbar");
    set(&other, "hologram");

    let panes = t.list_panes().unwrap();
    assert!(panes.iter().all(|p| p.pane != first && p.pane != other), "{panes:?}");
    assert_eq!(pane(&panes, &kept).cpd_view, None);
}


/// engine-W8 (§6.3 liveness): only tmux saying that no server runs means "no panes".
/// Any other failure of `list-panes` and `list-sessions` is `tmux_failed`, so E2 and the
/// driver never read a tmux hiccup as every pane gone.
#[test]
fn list_panes_is_empty_only_when_tmux_says_no_server_runs() {
    let (_, lost) = answering(Reply::exit(1, "", "lost server\n"));
    assert_eq!(lost.list_panes().unwrap_err().code(), ErrorCode::TmuxFailed);
    assert_eq!(lost.server_running().unwrap_err().code(), ErrorCode::TmuxFailed);

    let (_, none) = answering(Reply::exit(1, "", "no server running on /tmp/tmux-1000/x\n"));
    assert!(none.list_panes().unwrap().is_empty());
    assert!(!none.server_running().unwrap());

    // The fake tmux with no socket file: "error connecting to <socket> (No such file or directory)".
    let s = server();
    assert!(s.tmux.list_panes().unwrap().is_empty());
    assert!(!s.tmux.server_running().unwrap());
}

/// engine-W4: a tmux call that hangs is cut at its deadline and fails `tool_timeout` with the tool, the argv and the
/// deadline. The hang is a reply that takes 600 s of virtual time on a `ManualClock`: the call ends at the deadline, not
/// when the reply would. The group kill is the real runner's `killpg`, not ours (research section 5).
#[test]
fn a_tmux_call_past_its_deadline_is_killed_with_its_group_and_is_tool_timeout() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("tmux", |_| true, Reply::ok("").takes(std::time::Duration::from_secs(600)));
    let dir = tempfile::tempdir().unwrap();
    let tmux = Tmux::with_tmpdir("cpd-test-deadline", dir.path()).with_runner(Arc::new(runner.clone()));

    let err = tmux.server_running().unwrap_err();

    // The call returns at its deadline, not when the hung tmux would have ended.
    assert_eq!(clock.elapsed(), std::time::Duration::from_secs(10));
    assert_eq!(err.code(), ErrorCode::ToolTimeout, "{err:?}");
    let d = err.details();
    assert_eq!(d["tool"], "tmux");
    assert_eq!(d["deadline_seconds"], 10);
    assert_eq!(d["argv"], serde_json::json!(tmux.argv(&["list-sessions".into(), "-F".into(), "#{session_id}".into()])));
    assert_eq!(runner.calls().len(), 1, "no retry of a call that timed out");
}

// ---- Every tmux call is a UTF-8 client (hs-d463; contract §6.1 "Every tmux call is a UTF-8 client") ----
//
// A tmux 3.8 client with no UTF-8 locale prints the tabs of a `-F` format as `_`, so `list-panes` cannot be parsed.
// The mechanism is `tmux -u`, first after the program, whatever the caller's `LANG`/`LC_*`.

/// A tmux client as the real one is, over the fake server: given a caller whose locale is not UTF-8 and no `-u` first, it
/// prints the tabs of a `-F` format as `_` (what tmux 3.8 was seen to do); otherwise it answers as the fake server does.
struct Client {
    route: TmuxRoute,
    locale: Locale,
}

/// Whether a caller with this locale is a UTF-8 client by itself: none of the four is.
fn utf8_by_itself(locale: Locale) -> bool {
    match locale {
        Locale::Unset | Locale::EmptyLang | Locale::LangC | Locale::LcAllC => false,
    }
}

impl cpd_core::host::runner::Handler for Client {
    fn handle(&self, call: &Call) -> Reply {
        let reply = self.route.handle(call);
        if call.args.first().map(String::as_str) == Some("-u") || utf8_by_itself(self.locale) {
            return reply;
        }
        match reply {
            Reply::Exit { code, stdout, stderr, takes } => {
                Reply::Exit { code, stdout: String::from_utf8_lossy(&stdout).replace('\t', "_").into_bytes(), stderr, takes }
            }
            other => other,
        }
    }
}

/// The defect itself (hs-d463): the liveness call, `list-panes -a -F` (§6.3), through `Tmux` against a server whose
/// caller has no UTF-8 locale. Before the fix the tabs print as `_` and `list_panes` fails `tmux_failed: tmux list-panes
/// printed unexpected output`.
#[test]
fn list_panes_parses_for_a_caller_without_a_utf8_locale() {
    let mut failures = Vec::new();
    for locale in LOCALES {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::new(ManualClock::new());
        let client = Client { route: TmuxRoute::new().with_runner(runner.weak()), locale };
        let runner = runner.route("tmux", Arc::new(client));
        let tmux = Tmux::with_tmpdir(SOCKET, dir.path()).with_runner(Arc::new(runner));
        if let Err(e) = tmux.ensure_session("cpd", "main", &sleeper(dir.path())) {
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
    let dir = tempfile::tempdir().unwrap();
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on("tmux", |_| true, Reply::ok(""));
    // `kill_server` waits for the server to go on the clock: virtual time, so the answer that never ends costs no wait.
    let tmux = Tmux::with_tmpdir("cpdt-argv", dir.path()).with_runner(Arc::new(runner.clone())).with_clock(Arc::new(clock));

    let argv = tmux.argv(&["list-panes".into()]);
    assert_eq!(&argv[..2], ["tmux", "-u"], "{argv:?}");
    assert_eq!(argv[2..], ["-L", "cpdt-argv", "-f", "/dev/null", "list-panes"], "{argv:?}");

    let pane = PaneId::parse("%1").unwrap();
    let other = PaneId::parse("%2").unwrap();
    let window = WindowId::parse("@1").unwrap();
    let spawn = sleeper(dir.path());
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

    let calls = runner.calls();
    assert!(calls.len() >= 20, "the fake tmux saw {} calls", calls.len());
    let bare: Vec<String> = calls.iter().filter(|c| c.args.first().map(String::as_str) != Some("-u")).map(|c| c.args.join(" ")).collect();
    assert!(bare.is_empty(), "tmux calls that are not UTF-8 clients (`-u` first):\n{}", bare.join("\n"));
}

/// `apply_status_line` sets the global user option `@cpd_version` to the version it is given (§6.1).
#[test]
fn apply_status_line_sets_the_global_cpd_version_option() {
    let (runner, tmux) = answering(Reply::ok(""));
    tmux.apply_status_line("cpd", "0.0.5-5ba3a1d3").unwrap();
    let calls: Vec<String> = runner.calls().iter().map(|c| c.args.join(" ")).collect();
    assert!(calls.iter().any(|c| c.ends_with("set -g @cpd_version 0.0.5-5ba3a1d3")), "{calls:#?}");
}

/// The tmux calls of `cpd` itself, from the commands that make them, on the fake ports, for each locale: the `-V` probe
/// of `doctor` and `watch --ensure`, `tui`, `tui --respawn`: every one starts `-u` (a bare call is the defect).
#[test]
fn every_tmux_call_of_cpd_is_a_utf8_client_whatever_the_callers_locale() {
    let mut bare = Vec::new();
    for locale in LOCALES {
        let fx = common::fx::Fx::with_ports();
        fx.env.write_tmux_config("");
        // `-V` answers like a tmux; the server has no session, so every other call fails.
        let runner = (*fx.fakes.runner).clone();
        let _ = runner.on("tmux", |c| c.args.iter().any(|a| a == "-V"), Reply::ok("tmux 3.8\n")).on("tmux", |_| true, Reply::exit(1, "", ""));
        fx.allow_unscripted();
        for args in [&["doctor"][..], &["tui"], &["tui", "--respawn"], &["watch", "--ensure"]] {
            let _ = fx.cpd_with(fx.env.root(), args, locale.vars());
        }
        let calls = fx.calls("tmux");
        assert!(calls.len() >= 4, "{locale:?}: the fake tmux saw {} calls", calls.len());
        for call in calls.iter().filter(|c| c.args.first().map(String::as_str) != Some("-u")) {
            bare.push(format!("{locale:?}: tmux {}", call.args.join(" ")));
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
        // A file that is only tests (`*_tests.rs`, a `tests/` directory) is test code too.
        if name.ends_with("_tests.rs") || name.contains("/tests/") {
            continue;
        }
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
