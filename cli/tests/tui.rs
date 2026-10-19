//! The cockpit, `cpd tui` (commands.md §13.1; contract §6.1, §6.2 with
//! commands.md §19 amendments 1, 2 and 9) and the routing of `cpd tui view`
//! (§13.2), on a private tmux server with a fake `cpd` on `PATH` whose
//! not-yet-implemented views just sleep.

mod common;

use std::fs;

use common::{CPD, SLEEP_SECONDS, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::tmux::SERVER_OPTIONS;
use cpd_core::store::Store;
use serde_json::{Value, json};

const PLACEHOLDER: &str = "No Lead here: pick a Plan in the session tree";

/// An env with a config on its private tmux server, and a fake `cpd` that
/// runs the real one except for the views whose Jobs have not landed, which
/// sleep so that their panes stay alive.
fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    // The engine the cockpit starts polls GitHub from its first tick: it finds nothing.
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    env.fake(
        "cpd",
        &format!("if [ \"$1\" = tui ] && [ \"$2\" = view ] && [ \"$3\" != chat ]; then exec sleep {SLEEP_SECONDS}; fi\nexec '{CPD}' \"$@\""),
    );
    env
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// One pane of `cpd:main`.
#[derive(Debug, Clone, PartialEq)]
struct Pane {
    id: String,
    view: String,
    session: String,
    dead: bool,
    pid: Option<u32>,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

fn main_panes(env: &TestEnv) -> Vec<Pane> {
    let format = "#{pane_id}\t#{@cpd_view}\t#{@cpd_session}\t#{pane_dead}\t#{pane_pid}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}";
    tmux_out(env, &["list-panes", "-t", "=cpd:main", "-F", format])
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            Pane {
                id: f[0].into(),
                view: f[1].into(),
                session: f[2].into(),
                dead: f[3] == "1",
                pid: pane_pid(f[4]),
                left: f[5].parse().unwrap(),
                top: f[6].parse().unwrap(),
                width: f[7].parse().unwrap(),
                height: f[8].parse().unwrap(),
            }
        })
        .collect()
}

fn view<'a>(panes: &'a [Pane], name: &str) -> &'a Pane {
    panes.iter().find(|p| p.view == name).unwrap_or_else(|| panic!("no {name} pane: {panes:?}"))
}

fn windows(env: &TestEnv, session: &str) -> Vec<String> {
    tmux_out(env, &["list-windows", "-t", &format!("={session}"), "-F", "#{window_name}"]).lines().map(str::to_owned).collect()
}

fn views_started(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd").into_iter().filter(|c| c.first().map(String::as_str) == Some("tui")).collect()
}

/// Asserts `main` holds the layout of §13.1: the tree left, the plan view
/// above the chat slot in the centre, the widgets right, the one-row StatusBar
/// across the bottom, each marked.
fn assert_layout(panes: &[Pane]) {
    assert_eq!(panes.len(), 5, "{panes:?}");
    let (tree, plan, widgets) = (view(panes, "tree"), view(panes, "plan"), view(panes, "widgets"));
    let bar = view(panes, "statusbar");
    assert_eq!((bar.left, bar.width, bar.height), (0, 215, 1), "{panes:?}");
    assert!(panes.iter().filter(|p| p.id != bar.id).all(|p| p.top < bar.top), "{panes:?}");
    let slot = panes.iter().find(|p| p.view == "chat" || !p.session.is_empty()).expect("a chat slot");
    assert_eq!(tree.left, 0, "{panes:?}");
    assert_eq!(plan.left, slot.left, "{panes:?}");
    assert!(plan.left > tree.left && widgets.left > plan.left, "{panes:?}");
    assert!(slot.top > plan.top, "{panes:?}");
    // 20 % of the 215 columns each, give or take tmux's rounding and borders.
    for side in [tree, widgets] {
        assert!((40..=46).contains(&side.width), "{panes:?}");
    }
    assert!(panes.iter().all(|p| !p.dead), "{panes:?}");
}

#[test]
fn tui_builds_the_cockpit_once_and_starts_the_engine() {
    let env = env();
    let out = env.cpd(&["tui"]).ok();
    let socket = env.tmux_socket();
    assert_eq!(out["built"], true, "{out}");
    assert_eq!(out["socket"], socket);
    assert_eq!(out["session"], "cpd");
    assert_eq!(out["attach"], json!(["tmux", "-L", socket, "attach", "-t", "cpd"]));
    assert_eq!(out["watch"]["started"], true, "{out}");
    let engine = out["watch"]["pane"].as_str().unwrap();

    let panes = main_panes(&env);
    assert_layout(&panes);
    let chat = view(&panes, "chat");
    assert_eq!(windows(&env, "cpd"), ["main"]);
    assert_eq!(windows(&env, "cpd-work"), ["watch"]);
    let views: Vec<Vec<String>> = ["tree", "plan", "widgets", "chat", "statusbar"].iter().map(|v| vec!["tui".into(), "view".into(), (*v).into()]).collect();
    // The views start side by side: wait for the five calls, whose order is that of the first to be recorded.
    wait_until("the five views to be started", || views_started(&env).len() >= 5);
    let (mut expected, mut started) = (views, views_started(&env));
    expected.sort();
    started.sort();
    assert_eq!(started, expected);
    // The keyboard goes to the chat slot.
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", "=cpd:main", "#{pane_id}"]).trim(), chat.id);

    // The real placeholder view draws its text in the slot.
    wait_until("the placeholder text", || {
        String::from_utf8_lossy(&env.tmux(&["capture-pane", "-p", "-t", &chat.id]).stdout).contains(PLACEHOLDER)
    });

    // The server shows §6.1's options, hyperlinks included, and the TUI's border titles.
    let features = tmux_out(&env, &["show", "-s", "terminal-features"]);
    for feature in ["xterm*:extkeys", "xterm-ghostty:extkeys", "xterm-ghostty:hyperlinks"] {
        assert!(features.contains(feature), "{feature} not in {features}");
    }
    assert_eq!(tmux_out(&env, &["show", "-gv", "remain-on-exit"]).trim(), "on");
    let border = tmux_out(&env, &["show", "-gv", "pane-border-format"]);
    assert!(border.contains("#{==:#{@cpd_view},tree}, Context "), "{border}");
    assert!(border.contains("#[reverse]") && border.contains("#[noreverse]"), "{border}");
    // tmux expands the format of the tree pane to its reversed title.
    let expanded = tmux_out(&env, &["display", "-p", "-t", &view(&panes, "tree").id, "-F", &border]);
    assert!(expanded.contains("#[reverse] Context #[noreverse]"), "{expanded}");
    // The StatusBar has no title, and a click on it never selects it.
    assert!(border.contains("#{?#{==:#{@cpd_view},statusbar},,"), "{border}");
    let binding = tmux_out(&env, &["list-keys", "-T", "root", "MouseDown1Pane"]);
    assert!(binding.contains("#{==:#{@cpd_view},statusbar}"), "{binding}");
    assert!(binding.contains("send-keys -M"), "{binding}");

    // A second run builds nothing and starts nothing.
    let again = env.cpd(&["tui"]).ok();
    assert_eq!(again["built"], false, "{again}");
    assert_eq!(again["watch"], json!({"started": false, "already": true}));
    assert_eq!(main_panes(&env).iter().map(|p| &p.id).collect::<Vec<_>>(), panes.iter().map(|p| &p.id).collect::<Vec<_>>());
    assert_eq!(views_started(&env).len(), 5);
    assert_eq!(windows(&env, "cpd-work"), ["watch"]);
    assert!(tmux_out(&env, &["list-panes", "-t", engine, "-F", "#{pane_dead}"]).trim() == "0");
}

#[test]
fn tui_builds_main_in_a_cockpit_that_has_only_a_park_window() {
    let env = env();
    // As plan new leaves it: the cockpit session with the first Lead's park window.
    tmux_out(&env, &["new-session", "-d", "-s", "cpd", "-n", "park-pay", "-x", "215", "-y", "62", "sleep", "3600"]);
    let out = env.cpd(&["tui"]).ok();
    assert_eq!(out["built"], true, "{out}");
    assert_layout(&main_panes(&env));
    assert_eq!(windows(&env, "cpd"), ["park-pay", "main"]);
    assert_eq!(env.cpd(&["tui"]).ok()["built"], false);
}

/// Kills the program of `pane` (its pid `pid`), as a crash would, with the
/// pane's tty held open by the returned file: keep it until tmux has seen the
/// pane die, and kill no other pane of the server before then.
///
/// Why the tty: tmux before 3.6, built with libutempter (Ubuntu 24.04's 3.4,
/// CI's), can lose the `SIGCHLD` of a pane whose tty closes first. On that
/// EOF it calls `utempter_remove_record`, which sets `SIGCHLD` to `SIG_DFL`
/// while it forks and waits for its helper. The kernel closes a dying
/// process's files before it signals the parent, so under load the signal
/// lands in that window and is discarded. The pane is then dead (`pane_dead`),
/// but its status is never read and `pane-died` never fires, until the
/// server's next `SIGCHLD`. That is tmux's bug, fixed upstream in tmux 3.6
/// (commit fa5f3cef3d, "Send SIGCHLD after utempter_remove_record as well",
/// tmux GitHub issue 4559); contract §6.1 notes it. While the tty is held
/// there is no EOF, so tmux reads the status on `SIGCHLD` with no utempter
/// call running, and these tests pin cpd's contract (which pane the hook
/// respawns), not that race. tmux makes that `utempter_remove_record` call
/// on every pane death, so another pane killed before tmux has seen this one
/// die could have its `SIGCHLD` lost the same way.
fn kill(env: &TestEnv, pane: &str, pid: Option<u32>) -> fs::File {
    let pid = pid.expect("a live pane has a pid");
    use std::os::unix::fs::OpenOptionsExt;
    let tty = tmux_out(env, &["display", "-p", "-t", pane, "#{pane_tty}"]);
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(nix::fcntl::OFlag::O_NOCTTY.bits())
        .open(tty.trim())
        .unwrap_or_else(|e| panic!("open {}: {e}", tty.trim()));
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
    held
}

/// `(dead, pid)` of a pane.
fn pane_state(env: &TestEnv, pane: &str) -> (bool, Option<u32>) {
    let out = tmux_out(env, &["display", "-p", "-t", pane, "#{pane_dead}\t#{pane_pid}"]);
    let (dead, pid) = out.trim_end_matches('\n').split_once('\t').unwrap();
    (dead == "1", pane_pid(pid))
}

/// `#{pane_pid}`: tmux >= 3.8 prints it empty once the pane's tty has closed.
fn pane_pid(pid: &str) -> Option<u32> {
    (!pid.is_empty()).then(|| pid.parse().unwrap_or_else(|e| panic!("pane_pid {pid:?}: {e}")))
}

/// Asserts the server has every option, binding and hook `host` sets at a
/// server start (contract §6.1; their values are pinned in `tests/host.rs`).
fn assert_server_options(env: &TestEnv) {
    for option in SERVER_OPTIONS {
        match option {
            ["set", "-g", name, value] => assert_eq!(tmux_out(env, &["show", "-gv", name]).trim_end_matches('\n'), *value, "{name}"),
            ["set", "-as", name, value] => {
                let shown = tmux_out(env, &["show", "-sv", name]);
                for feature in value.split(',').filter(|f| !f.is_empty()) {
                    assert_eq!(shown.lines().filter(|l| *l == feature).count(), 1, "{feature} in {shown}");
                }
            }
            ["set-hook", "-g", hook, _] => {
                let shown = tmux_out(env, &["show-hooks", "-g", hook]);
                assert_eq!(shown, format!("{hook}[0] if-shell -F \"#{{@cpd_view}}\" respawn-pane\n"));
            }
            ["bind", "-T", table, key, "if", "-F", condition, ..] => {
                let shown = tmux_out(env, &["list-keys", "-T", table, key]);
                assert!(shown.contains(condition), "{shown}");
            }
            other => panic!("an option this test cannot check: {other:?}"),
        }
    }
}

/// After `cpd tui`: `@cpd_machine` is the test config's machine and the title
/// tmux expands from `set-titles-string` is `cpd · <machine>` (contract §6.1, hs-d300).
fn assert_window_title(env: &TestEnv) {
    assert_eq!(tmux_out(env, &["show", "-gv", "set-titles"]).trim_end_matches('\n'), "on");
    assert_eq!(tmux_out(env, &["show", "-gv", "@cpd_machine"]).trim_end_matches('\n'), "m1");
    let format = tmux_out(env, &["show", "-gv", "set-titles-string"]);
    assert_eq!(tmux_out(env, &["display", "-p", format.trim_end_matches('\n')]).trim_end_matches('\n'), "cpd · m1");
}

#[test]
fn a_server_started_by_plan_new_has_every_server_option() {
    let env = env();
    env.fake("cpd", &format!("if [ \"$1 $2\" = 'session run' ]; then exec sleep {SLEEP_SECONDS}; fi\nexec '{CPD}' \"$@\""));
    fs::write(
        env.cpd_home().join("config.toml"),
        format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()),
    )
    .unwrap();
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay"]).ok();
    assert_eq!(windows(&env, "cpd"), ["park-pay"], "plan new started the server, not tui");
    assert_server_options(&env);
}

#[test]
fn a_killed_view_respawns_at_once_and_the_engine_stays_dead() {
    let env = env();
    let out = env.cpd(&["tui"]).ok();
    assert_window_title(&env);
    let engine = out["watch"]["pane"].as_str().unwrap();
    assert_server_options(&env);
    let before = main_panes(&env);
    let tree = view(&before, "tree").clone();
    let (_, engine_pid) = pane_state(&env, engine);
    // The engine first: once dead, it makes no tmux calls of its own.
    let engine_tty = kill(&env, engine, engine_pid);
    wait_until("the engine pane to die", || pane_state(&env, engine).0);
    drop(engine_tty);
    let tree_tty = kill(&env, &tree.id, tree.pid);
    wait_until("the tree to respawn", || {
        let p = view(&main_panes(&env), "tree").clone();
        !p.dead && p.pid.is_some() && p.pid != tree.pid
    });
    drop(tree_tty);
    // tmux runs the `pane-died` hooks in the order the panes died: the tree's has run, so the engine's has.
    assert!(pane_state(&env, engine).0, "the engine pane stays dead for E2");
    let after = main_panes(&env);
    assert_layout(&after);
    assert_eq!(view(&after, "tree").id, tree.id);
    // The pane has its new process before the fake `cpd` has recorded the call: wait for the call.
    let trees = || views_started(&env).iter().filter(|c| c[2] == "tree").count();
    wait_until("the tree's view to be started again", || trees() >= 2);
    assert_eq!(trees(), 2);
}

#[test]
fn tui_respawns_a_dead_pane_of_the_layout_in_place_and_sets_the_hook_again() {
    let env = env();
    env.cpd(&["tui"]).ok();
    // A server from before the hook: a dead view stays dead until tui runs.
    tmux_out(&env, &["set-hook", "-gu", "pane-died"]);
    let before = main_panes(&env);
    let (tree, bar) = (view(&before, "tree").clone(), view(&before, "statusbar").clone());
    for pane in [&tree, &bar] {
        let tty = kill(&env, &pane.id, pane.pid);
        wait_until(&format!("the dead {} pane", pane.view), || pane_state(&env, &pane.id).0);
        drop(tty);
    }

    let out = env.cpd(&["tui"]).ok();
    assert_eq!(out["built"], false, "{out}");
    assert!(out.get("respawned").is_none(), "{out}");
    let after = main_panes(&env);
    assert_layout(&after);
    assert_eq!(after.iter().map(|p| &p.id).collect::<Vec<_>>(), before.iter().map(|p| &p.id).collect::<Vec<_>>());
    assert_ne!(view(&after, "tree").pid, tree.pid);
    assert_ne!(view(&after, "statusbar").pid, bar.pid);
    // The pane has its new process before the fake `cpd` has recorded the call: wait for the call.
    let trees = || views_started(&env).iter().filter(|c| c[2] == "tree").count();
    wait_until("the tree's view to be started again", || trees() >= 2);
    assert_eq!(trees(), 2);
    assert_server_options(&env);
}

#[test]
fn tui_adds_a_missing_statusbar_to_an_existing_main_then_resets_the_layout() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let full = main_panes(&env);
    tmux_out(&env, &["kill-pane", "-t", &view(&full, "statusbar").id]);
    // An old main, its sizes changed by the User.
    tmux_out(&env, &["resize-pane", "-t", &view(&full, "tree").id, "-x", "80"]);
    let before = main_panes(&env);
    assert_eq!(before.len(), 4, "{before:?}");
    assert_eq!(view(&before, "tree").width, 80, "{before:?}");

    let out = env.cpd(&["tui"]).ok();
    assert_eq!(out["built"], false, "{out}");
    let after = main_panes(&env);
    // The bar spans the window and the other panes have their sizes back.
    assert_layout(&after);
    // They are the same panes, still alive.
    for old in &before {
        let new = after.iter().find(|p| p.id == old.id).unwrap_or_else(|| panic!("{} is gone: {after:?}", old.id));
        assert_eq!((new.view.as_str(), new.pid, new.dead), (old.view.as_str(), old.pid, false));
    }

    // Rerunning adds none.
    env.cpd(&["tui"]).ok();
    assert_eq!(main_panes(&env).len(), 5);
}

#[test]
fn tui_respawn_restarts_every_live_view_and_no_lead_or_engine() {
    let env = env();
    let out = env.cpd(&["tui", "--respawn"]).ok();
    assert_eq!((&out["built"], &out["respawned"]), (&json!(true), &json!([])), "a new cockpit restarts nothing: {out}");
    let engine = out["watch"]["pane"].as_str().unwrap();
    let (_, engine_pid) = pane_state(&env, engine);

    // A Lead takes the chat slot and its placeholder is parked; a Worker view opens beside it.
    let placeholder = view(&main_panes(&env), "chat").clone();
    let lead = tmux_out(&env, &["new-window", "-d", "-t", "=cpd:", "-n", "park-pay", "-P", "-F", "#{pane_id}", "sleep", "3600"]);
    let lead = lead.trim();
    tmux_out(&env, &["set", "-p", "-t", lead, "@cpd_session", "lead-a"]);
    tmux_out(&env, &["swap-pane", "-d", "-s", lead, "-t", &placeholder.id]);
    let worker = tmux_out(&env, &["split-window", "-h", "-d", "-t", lead, "-l", "50%", "-P", "-F", "#{pane_id}", "sleep", "3601"]);
    let worker = worker.trim();
    tmux_out(&env, &["set", "-p", "-t", worker, "@cpd_view", "worker"]);
    let before = main_panes(&env);
    let lead_pid = before.iter().find(|p| p.id == lead).unwrap().pid;

    let out = env.cpd(&["tui", "--respawn"]).ok();
    assert_eq!(out["built"], false, "{out}");
    let mut respawned: Vec<String> = serde_json::from_value(out["respawned"].clone()).unwrap();
    respawned.sort();
    let mut views: Vec<String> = before.iter().filter(|p| !p.view.is_empty()).map(|p| p.id.clone()).collect();
    views.sort();
    assert_eq!(views.len(), 5, "tree, plan, widgets, statusbar and the Worker view: {before:?}");
    assert_eq!(respawned, views, "{before:?}");

    let after = main_panes(&env);
    assert_eq!(after.iter().map(|p| &p.id).collect::<Vec<_>>(), before.iter().map(|p| &p.id).collect::<Vec<_>>());
    for (old, new) in before.iter().zip(&after) {
        assert!(!new.dead, "{after:?}");
        assert_eq!(new.view, old.view);
        if old.view.is_empty() {
            assert_eq!(new.pid, lead_pid, "the Lead is never touched");
        } else {
            assert_ne!(new.pid, old.pid, "{} runs again", old.view);
        }
    }
    // Each view reruns its own command: the Worker view keeps its argv.
    let worker_command = tmux_out(&env, &["display", "-p", "-t", worker, "#{pane_start_command}"]);
    assert!(worker_command.contains("3601"), "{worker_command}");
    // The pane has its new process before the fake `cpd` has recorded the call: wait for the call.
    let trees = || views_started(&env).iter().filter(|c| c[2] == "tree").count();
    wait_until("the tree's view to be started again", || trees() >= 2);
    assert_eq!(trees(), 2);
    // The parked placeholder (not in main) and the engine are not touched.
    assert_eq!(pane_state(&env, &placeholder.id).1, placeholder.pid);
    assert_eq!(pane_state(&env, engine), (false, engine_pid));
}

#[test]
fn tui_brings_the_last_chat_lead_into_a_new_slot() {
    let env = env();
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    // lead-b was in the chat last among Leads with a Plan; ground-c, a Lead with
    // no Plan (what migration 0010 leaves of a grounded Lead), came later but is
    // never restored there.
    let sql = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('lead-a', 'lead', 'ctx', 'pay', 'A', '/', 'O3', 'stopped', 't', 't'),
           ('lead-b', 'lead', 'ctx', 'pay', 'B', '/', 'O3', 'stopped', 't', 't'),
           ('ground-c', 'lead', 'ctx', NULL, 'C', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead-a', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'repo', '/', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
         INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-a', 'x', '{\"place\": \"chat\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-b', 'x', '{\"place\": \"chat\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-b', 'x', '{\"place\": \"parked\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', NULL, 'ground-c', 'x', '{\"place\": \"chat\"}');";
    Store::open(&home, 5000)
        .unwrap()
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<_, CpdError>(())
        })
        .unwrap();
    tmux_out(&env, &["new-session", "-d", "-s", "cpd", "-n", "park-pay", "-x", "215", "-y", "62", "sleep", "3600"]);
    let lead_pane = tmux_out(&env, &["display", "-p", "-t", "=cpd:park-pay", "#{pane_id}"]).trim().to_owned();
    tmux_out(&env, &["set", "-p", "-t", &lead_pane, "@cpd_session", "lead-b"]);

    let out = env.cpd(&["tui"]).ok();
    assert_eq!(out["built"], true, "{out}");
    let panes = main_panes(&env);
    assert_layout(&panes);
    // The Lead's own pane took the slot; the placeholder went, with its park window.
    let slot = panes.iter().find(|p| p.session == "lead-b").unwrap_or_else(|| panic!("{panes:?}"));
    assert_eq!(slot.id, lead_pane);
    assert!(panes.iter().all(|p| p.view != "chat"), "{panes:?}");
    assert_eq!(windows(&env, "cpd"), ["main"]);
    let moved: Value = Store::open(&home, 5000)
        .unwrap()
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT json_object('session', session, 'data', json(data)) FROM events WHERE kind = 'session-moved' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )?)
        })
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap();
    assert_eq!(moved["session"], "lead-b");
    assert_eq!(moved["data"]["place"], "chat");
    assert_eq!(moved["data"]["pane"], lead_pane);
}

/// `cpd tui` on a server older than the installed tmux marks a restart pending, once, and never restarts
/// it itself (it may run inside the server): the engine does, once nothing is in the way (contract §6.1).
#[test]
fn tui_marks_a_restart_of_an_older_tmux_server_pending_and_does_not_kill_it() {
    let env = env();
    let path = std::env::var_os("PATH").unwrap();
    let real = std::env::split_paths(&path).map(|d| d.join("tmux")).find(|p| p.is_file()).expect("tmux on PATH");
    let older = env.root().join("older-server");
    // While the file exists, the server says it runs tmux 1.0.
    env.fake(
        "tmux",
        &format!("[ -e '{}' ] && case \" $* \" in *\" display -p #{{version}} \"*) echo 1.0; exit 0;; esac\nexec '{}' \"$@\"", older.display(), real.display()),
    );
    let pending = env.cpd_home().join("run/tmux-restart.pending");

    env.cpd(&["tui"]).ok();
    assert!(!pending.exists(), "a server as new as the installed tmux needs no restart");
    let pid = tmux_out(&env, &["display", "-p", "#{pid}"]);
    fs::write(&older, "").unwrap();
    let run = env.cpd(&["tui"]);
    assert_eq!(run.ok()["built"], false);
    let marked: Value = serde_json::from_str(&fs::read_to_string(&pending).unwrap()).unwrap();
    assert_eq!((marked["server"].as_str(), marked["failed"].is_null()), (Some("1.0"), true));
    let at = marked["at"].clone();
    // Again: still pending, with its first time; and nothing was killed.
    env.cpd(&["tui"]).ok();
    let again: Value = serde_json::from_str(&fs::read_to_string(&pending).unwrap()).unwrap();
    assert_eq!(again["at"], at);
    assert!(!env.calls("tmux").iter().any(|c| c.iter().any(|a| a == "kill-server")));
    assert_eq!(tmux_out(&env, &["display", "-p", "#{pid}"]), pid);
}

#[test]
fn tui_attach_without_a_terminal_is_refused_before_anything_is_built() {
    let env = env();
    let run = env.cpd(&["tui", "--attach"]);
    assert_eq!(run.exit, 3, "{run:?}");
    assert_eq!(run.error()["code"], "not_a_terminal");
    assert!(!env.tmux(&["list-sessions"]).status.success(), "a tmux server was started");
    assert!(env.calls("cpd").is_empty());
}

#[test]
fn tui_view_routes_by_name() {
    let env = env();
    let run = env.cpd(&["tui", "view", "chat"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (3, json!("not_a_terminal")), "{run:?}");
    for words in [&["tui", "view", "nope"][..], &["tui", "view"], &["tui", "--bogus"]] {
        let run = env.cpd(words);
        assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("usage")), "{words:?}: {run:?}");
    }
}

#[test]
fn an_unfaked_gh_is_denied_with_exit_97() {
    let env = TestEnv::new();
    for tool in ["gh", "pi", "curl", "wget", "ssh"] {
        let out = common::output(env.command(tool).arg("x")).unwrap();
        assert_eq!(out.status.code(), Some(97), "{tool}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), format!("no fake installed for {tool}\n"));
    }
    env.fake_gh("echo faked");
    let out = common::output(&mut env.command("gh")).unwrap();
    assert_eq!((out.status.code(), String::from_utf8_lossy(&out.stdout).trim()), (Some(0), "faked"));
}

const AMBIENT: [&str; 10] = ["GH_TOKEN", "GH_HOST", "GITHUB_TOKEN", "SSH_AUTH_SOCK", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy"];

/// Runs [`the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command`] in a child
/// test process that has them set (the harness reads the process's own environment).
#[test]
fn a_command_has_no_gh_ssh_or_proxy_variable() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child.args(["--ignored", "--exact", "the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command"]);
    for name in AMBIENT {
        child.env(name, "leak");
    }
    let out = common::output(&mut child).unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && stdout.contains("1 passed"), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
#[ignore = "run by a_command_has_no_gh_ssh_or_proxy_variable, with the variables set"]
fn the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command() {
    for name in AMBIENT {
        assert_eq!(std::env::var(name).as_deref(), Ok("leak"), "the parent test sets {name}");
    }
    let env = TestEnv::new();
    let text = String::from_utf8(common::output(&mut env.command("env")).unwrap().stdout).unwrap();
    let leaked: Vec<&str> = text.lines().filter(|l| AMBIENT.iter().any(|n| l.starts_with(&format!("{n}=")))).collect();
    assert!(leaked.is_empty(), "{leaked:?}");
}
