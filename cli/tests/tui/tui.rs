//! The cockpit, `cpd tui` (commands.md §13.1; contract §6.1, §6.2 with
//! commands.md §19 amendments 1, 2 and 9) and the routing of `cpd tui view`
//! (§13.2), on a private tmux server with a fake `cpd` on `PATH` whose
//! not-yet-implemented views just sleep.

use crate::common;

use std::fs::{self, File, OpenOptions};
use std::time::Duration;

use common::{CPD, SLEEP_SECONDS, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::tmux::{LEGEND_GLYPH, LOGS_GLYPH, RESET_GLYPH, SERVER_OPTIONS, STATUS_FORMAT};
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
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

/// `tmux <args>`'s stdout, or `None` when tmux refuses (an option it does not know).
fn tmux_try(env: &TestEnv, args: &[&str]) -> Option<String> {
    let out = env.tmux(args);
    out.status.success().then(|| String::from_utf8(out.stdout).unwrap())
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
/// above the chat slot in the centre, the widgets right, each marked, and no
/// StatusBar pane: the panes reach the window's last row.
fn assert_layout(panes: &[Pane]) {
    assert_eq!(panes.len(), 4, "{panes:?}");
    let (tree, plan, widgets) = (view(panes, "tree"), view(panes, "plan"), view(panes, "widgets"));
    let slot = panes.iter().find(|p| p.view == "chat" || !p.session.is_empty()).expect("a chat slot");
    assert_eq!(tree.left, 0, "{panes:?}");
    assert_eq!(plan.left, slot.left, "{panes:?}");
    assert!(plan.left > tree.left && widgets.left > plan.left, "{panes:?}");
    assert!(slot.top > plan.top, "{panes:?}");
    // 25 % of the 215 columns each, give or take tmux's rounding and borders.
    for side in [tree, widgets] {
        assert!((50..=56).contains(&side.width), "{panes:?}");
    }
    assert!(panes.iter().all(|p| !p.dead && p.view != "statusbar"), "{panes:?}");
    // The status line takes the session's one row: tree and widgets run from the header to the last row.
    let bottom = |p: &Pane| p.top + p.height;
    assert_eq!(bottom(tree), bottom(widgets), "{panes:?}");
    assert_eq!(bottom(slot), bottom(tree), "{panes:?}");
}

/// Asserts the cockpit session `cpd` has the StatusBar as its status line,
/// and `cpd-work` none (contract §6.1).
fn assert_status_line(env: &TestEnv) {
    let shown = |session: &str, format: &str| tmux_out(env, &["display", "-p", "-t", &format!("={session}:"), format]);
    assert_eq!(shown("cpd", "#{status}").trim(), "on");
    assert_eq!(tmux_out(env, &["show", "-v", "-t", "=cpd:", "status-style"]).trim(), "default");
    let format = tmux_out(env, &["show", "-v", "-t", "=cpd:", "status-format[0]"]);
    assert_eq!(format.trim_end_matches('\n'), *STATUS_FORMAT);
    for range in [
        "#[range=user|reset]",
        "#[range=user|legend]",
        "#[range=user|logs]",
        "#[align=right]",
        "#{?#{e|>=:#{client_width},#{e|+:#{e|+:#{e|+:#{@cpd_workers_width},14},#{?#{@cpd_preview},#{e|+:#{n:@cpd_preview},12},0}},#{?@cpd_version,#{e|+:#{n:@cpd_version},3},0}}},#{@cpd_workers},#{@cpd_workers_short}}",
        "#{?#{@cpd_preview},#[fg=colour8]\u{2500}#[fg=colour3]\u{e0b6}",
        "#{R:\u{2500},",
    ] {
        assert!(format.contains(range), "{range} in {format}");
    }
    // The buttons are one `─` apart, outside their ranges: no range starts where the one before it ends.
    assert!(!format.contains("#[norange]#[range"), "{format}");
    // Set on the cockpit session only: the server default and cpd-work have no status row.
    assert_eq!(tmux_out(env, &["show", "-gv", "status"]).trim(), "off");
    assert_eq!(shown("cpd-work", "#{status}").trim(), "off");
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
    let views: Vec<Vec<String>> = ["tree", "plan", "widgets", "chat"].iter().map(|v| vec!["tui".into(), "view".into(), (*v).into()]).collect();
    // The views start side by side: wait for the four calls, whose order is that of the first to be recorded.
    wait_until("the four views to be started", || views_started(&env).len() >= 4);
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
    assert!(border.contains("#{==:#{@cpd_view},tree},Context,"), "{border}");
    assert!(border.contains("#[reverse]") && !border.contains("#[bg="), "{border}");
    // tmux expands the format of the tree pane to its title pill (hs-d530 R1, R3).
    let expanded = tmux_out(&env, &["display", "-p", "-t", &view(&panes, "tree").id, "-F", &border]);
    // (the colours, grey or blue, are the pane's focus: `pane_active`)
    assert!(expanded.contains("#[reverse]Context#[noreverse]"), "{expanded}");
    assert!(!border.contains("statusbar"), "{border}");
    // The StatusBar is the cockpit session's status line; a click on a pane is tmux's default again.
    assert_status_line(&env);
    // Words only: tmux 3.8 pads `bind-key` with a blank `-r` column (two spaces).
    let binding = |key: &str| tmux_out(&env, &["list-keys", "-T", "root", key]).split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(binding("MouseDown1Pane"), "bind-key -T root MouseDown1Pane select-pane -t = \\; send-keys -M");
    assert_eq!(
        binding("MouseDown1Status"),
        "bind-key -T root MouseDown1Status run-shell -b \"cpd tui action #{mouse_status_range} --client #{client_name} 2>/dev/null\""
    );

    // A second run builds nothing and starts nothing.
    let again = env.cpd(&["tui"]).ok();
    assert_eq!(again["built"], false, "{again}");
    assert_eq!(again["watch"], json!({"started": false, "already": true}));
    assert_eq!(main_panes(&env).iter().map(|p| &p.id).collect::<Vec<_>>(), panes.iter().map(|p| &p.id).collect::<Vec<_>>());
    assert_eq!(views_started(&env).len(), 4);
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
    assert_status_line(&env);
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
    for option in SERVER_OPTIONS.iter() {
        let option: Vec<&str> = option.iter().map(String::as_str).collect();
        match option.as_slice() {
            ["set", "-g", name, value] => assert_eq!(tmux_out(env, &["show", "-gv", name]).trim_end_matches('\n'), *value, "{name}"),
            // `-q`: tmux older than 3.5 or so does not know the option, and there is nothing to read back
            ["set", "-gq", name, value] => match tmux_try(env, &["show", "-gv", name]) {
                Some(shown) => assert_eq!(shown.trim_end_matches('\n'), *value, "{name}"),
                None => eprintln!("{name} not asserted: this tmux does not know the option"),
            },
            ["set", "-as", name, value] => {
                let shown = tmux_out(env, &["show", "-sv", name]);
                for feature in value.split(',').filter(|f| !f.is_empty()) {
                    assert_eq!(shown.lines().filter(|l| *l == feature).count(), 1, "{feature} in {shown}");
                }
            }
            ["set", "-s", name, value] => assert_eq!(tmux_out(env, &["show", "-sv", name]).trim_end_matches('\n'), *value, "{name}"),
            // The hooks' exact text is pinned in `tests/host.rs`; here, that the server has each, and that it starts the engine again.
            ["set-hook", "-g", hook, _] => {
                let shown = tmux_out(env, &["show-hooks", "-g", hook]);
                assert!(shown.starts_with(&format!("{hook}[0] if-shell -F ")), "{shown}");
                assert!(shown.contains("cpd watch --ensure --wait"), "{shown}");
            }
            ["bind", "-T", table, key, .., command] => {
                // tmux shows a `;` inside a binding escaped
                let shown = tmux_out(env, &["list-keys", "-T", table, key]).replace("\\;", ";");
                assert!(shown.contains(command), "{command} in {shown}");
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

/// After `cpd tui`: the global `@cpd_version` is the `version` of `cpd --version` (§6.1, hs-d299).
#[test]
fn tui_sets_cpd_version_to_the_binarys_version() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let want = env.cpd(&["--version"]).ok()["version"].as_str().unwrap().to_owned();
    assert_eq!(tmux_out(&env, &["show", "-gv", "@cpd_version"]).trim_end_matches('\n'), want);
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
fn a_killed_view_respawns_at_once_and_the_engine_is_started_again() {
    let env = env();
    let out = env.cpd(&["tui"]).ok();
    assert_window_title(&env);
    let engine = out["watch"]["pane"].as_str().unwrap();
    assert_server_options(&env);
    let before = main_panes(&env);
    let tree = view(&before, "tree").clone();
    let (_, engine_pid) = pane_state(&env, engine);
    // The engine first, so that its hook runs before the tree's.
    let engine_tty = kill(&env, engine, engine_pid);
    wait_until("the engine pane to die", || pane_state(&env, engine).0);
    drop(engine_tty);
    let tree_tty = kill(&env, &tree.id, tree.pid);
    wait_until("the tree to respawn", || {
        let p = view(&main_panes(&env), "tree").clone();
        !p.dead && p.pid.is_some() && p.pid != tree.pid
    });
    drop(tree_tty);
    // tmux runs the `pane-died` hooks in the order the panes died: the tree's has run, so the engine's has. The engine is supervised
    // (contract §6.6, hs-d328): its hook runs `watch --ensure --wait`, which starts it again in its own pane.
    wait_until("the engine to be started again", || {
        let (dead, pid) = pane_state(&env, engine);
        !dead && pid.is_some() && pid != engine_pid
    });
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
    let (tree, widgets) = (view(&before, "tree").clone(), view(&before, "widgets").clone());
    for pane in [&tree, &widgets] {
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
    assert_ne!(view(&after, "widgets").pid, widgets.pid);
    // The pane has its new process before the fake `cpd` has recorded the call: wait for the call.
    let trees = || views_started(&env).iter().filter(|c| c[2] == "tree").count();
    wait_until("the tree's view to be started again", || trees() >= 2);
    assert_eq!(trees(), 2);
    assert_server_options(&env);
}

#[test]
fn tui_removes_an_old_statusbar_pane_then_resets_the_layout() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let full = main_panes(&env);
    // A main built by an older cpd: the StatusBar pane at the bottom, the session without its status line, and the
    // sizes changed by the User.
    for option in ["status", "status-style", "status-format[0]"] {
        tmux_out(&env, &["set", "-u", "-t", "=cpd:", option]);
    }
    let bar = tmux_out(&env, &["split-window", "-d", "-v", "-f", "-l", "1", "-t", &full[0].id, "-P", "-F", "#{pane_id}", "sleep", "3600"]);
    let bar = bar.trim();
    tmux_out(&env, &["set", "-p", "-t", bar, "@cpd_view", "statusbar"]);
    tmux_out(&env, &["resize-pane", "-t", &view(&full, "tree").id, "-x", "80"]);
    let before = main_panes(&env);
    assert_eq!(before.len(), 5, "{before:?}");
    assert_eq!(view(&before, "tree").width, 80, "{before:?}");

    let run = env.cpd(&["tui"]);
    let out = run.ok();
    assert_eq!(out["built"], false, "{out}");
    assert!(!run.stderr.contains("statusbar"), "no warning about the old pane: {run:?}");
    let after = main_panes(&env);
    // The bar is gone, the others have their sizes back, and the status line is on.
    assert_layout(&after);
    assert_status_line(&env);
    // They are the same panes, still alive.
    for old in before.iter().filter(|p| p.id != bar) {
        let new = after.iter().find(|p| p.id == old.id).unwrap_or_else(|| panic!("{} is gone: {after:?}", old.id));
        assert_eq!((new.view.as_str(), new.pid, new.dead), (old.view.as_str(), old.pid, false));
    }

    // Rerunning removes nothing more.
    env.cpd(&["tui"]).ok();
    assert_eq!(main_panes(&env), after);
}

/// `cpd tui action` with `args`, which must print nothing and exit 0 (commands.md §1.2).
fn action(env: &TestEnv, args: &[&str]) {
    let run = env.cpd(&[&["tui", "action"][..], args].concat());
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{args:?}: {run:?}");
}

#[test]
fn tui_action_reset_resizes_as_reset_layout_does() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let before = main_panes(&env);
    tmux_out(&env, &["resize-pane", "-t", &view(&before, "tree").id, "-x", "80"]);
    tmux_out(&env, &["resize-pane", "-t", &view(&before, "plan").id, "-y", "30"]);
    assert_eq!(view(&main_panes(&env), "tree").width, 80);

    action(&env, &["reset", "--client", "/dev/pts/nobody"]);
    let after = main_panes(&env);
    assert_layout(&after);
    let sizes = |panes: &[Pane]| panes.iter().map(|p| (p.id.clone(), p.width, p.height)).collect::<Vec<_>>();
    assert_eq!(sizes(&after), sizes(&before), "the sizes reset-layout gave the new main");
}

/// A fake `tmux` on `PATH` that records its calls and runs the real one.
fn recording_tmux(env: &TestEnv) {
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake("tmux", &format!("exec '{}' \"$@\"", real.trim()));
}

fn messages(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-message")).collect()
}

#[test]
fn a_failing_action_prints_nothing_exits_0_and_tells_the_client() {
    let env = env();
    env.cpd(&["tui"]).ok();
    recording_tmux(&env);

    // an unknown action, or none (an empty `#{mouse_status_range}`)
    action(&env, &["nope", "--client", "/dev/pts/77"]);
    action(&env, &["--client", "/dev/pts/77"]);
    // a client that is not attached: no size to open the popup at
    action(&env, &["legend", "--client", "/dev/pts/77"]);
    action(&env, &["logs", "--client", "/dev/pts/77"]);
    let messages = messages(&env);
    assert_eq!(messages.len(), 4, "{messages:?}");
    for (message, says) in messages.iter().zip(["\"nope\"", "\"\"", "no tmux client", "no tmux client"]) {
        let at = message.iter().position(|a| a == "display-message").unwrap();
        assert_eq!(message[at + 1..at + 4], ["-l", "-c", "/dev/pts/77"], "{message:?}");
        assert!(message[at + 4].starts_with("cpd tui action: ") && message[at + 4].contains(says), "{says}: {message:?}");
    }
    assert!(env.calls("tmux").iter().all(|c| !c.iter().any(|a| a == "display-popup")));

    // with no tmux server at all, even the message fails: still nothing printed, exit 0
    env.kill_tmux_server();
    action(&env, &["reset", "--client", "/dev/pts/77"]);
}

/// A second private tmux server whose one pane is a real client of the cockpit
/// (`tmux -L <socket> attach`), so the cockpit gets mouse events and popups;
/// killed on drop.
struct Outer<'a> {
    env: &'a TestEnv,
    socket: String,
}

impl<'a> Outer<'a> {
    fn attach(env: &'a TestEnv, width: u16, height: u16) -> Outer<'a> {
        // A socket of its own for each client: `kill-server` (on drop) returns before the server is gone, so a
        // test that attaches again (one client per width) on the same socket could reach the dying server ("server
        // exited unexpectedly").
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let outer = Outer { env, socket: format!("{}-outer{n}", env.tmux_socket()) };
        let attach = format!("env -u TMUX tmux -L {} attach -t cpd", env.tmux_socket());
        let (w, h) = (width.to_string(), height.to_string());
        // without its own status line, so the client is the whole window
        outer.run(&["set", "-g", "status", "off", ";", "new-session", "-d", "-s", "o", "-x", &w, "-y", &h, &attach]);
        outer
    }

    fn run(&self, args: &[&str]) -> String {
        let out = self.env.command("tmux").args(["-L", &self.socket, "-f", "/dev/null"]).args(args).output().unwrap();
        assert!(out.status.success(), "outer tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    /// A left click at 0-based cell `x`, `y` of the client, as a terminal sends it (SGR mouse reporting). Half a
    /// second after the one before, else tmux takes it for the second click of a double click, not a `MouseDown1`.
    fn click(&self, x: u16, y: u16) {
        // time-claim: a click after the double-click interval of tmux's is a `MouseDown1`, not a second click; no
        // per-poll signal from tmux's click timer.
        std::thread::sleep(Duration::from_millis(500));
        for end in ['M', 'm'] {
            let bytes = format!("\x1b[<0;{};{}{end}", x + 1, y + 1);
            let hex: Vec<String> = bytes.bytes().map(|b| format!("0x{b:02x}")).collect();
            let mut args = vec!["send-keys", "-t", "o", "-H"];
            args.extend(hex.iter().map(String::as_str));
            self.run(&args);
        }
    }

    fn screen(&self) -> String {
        self.run(&["capture-pane", "-p", "-t", "o"])
    }
}

impl Drop for Outer<'_> {
    fn drop(&mut self) {
        let _ = self.env.command("tmux").args(["-L", &self.socket, "kill-server"]).stderr(std::process::Stdio::null()).status();
    }
}

#[test]
fn a_click_on_a_statusbar_button_runs_its_action_on_the_clicking_client() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let outer = Outer::attach(&env, 120, 30);
    let mut client = String::new();
    wait_until("the client", || {
        client = tmux_out(&env, &["list-clients", "-F", "#{client_name}"]).trim().to_owned();
        !client.is_empty()
    });
    // The status line is the client's last row: the three buttons at the left, one space apart.
    wait_until("the buttons", || outer.screen().lines().nth(29).is_some_and(|l| l.starts_with(&buttons())));
    // It takes one row: the window has the other 29.
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", "=cpd:main", "#{window_height}"]).trim(), "29");
    let chat = view(&main_panes(&env), "chat").clone();
    let actions = || env.calls("cpd").into_iter().filter(|c| c.get(1).map(String::as_str) == Some("action")).collect::<Vec<_>>();

    // ? opens the legend on this client
    outer.click(5, 29);
    wait_until("the legend popup", || outer.screen().contains(" legend "));
    assert_eq!(actions(), [["tui", "action", "legend", "--client", &client]]);
    // The popup's title can show before its command has started.
    wait_until("the legend view", || views_started(&env).contains(&vec!["tui".into(), "view".into(), "legend".into()]));
    // The click went to no pane: the chat keeps the focus and is not put into a mode.
    let active = tmux_out(&env, &["display", "-p", "-t", "=cpd:main", "#{pane_id} #{pane_in_mode}"]);
    assert_eq!(active.trim(), format!("{} 0", chat.id));
    // the legend view here is a fake that sleeps: close its popup
    tmux_out(&env, &["display-popup", "-C", "-c", &client]);
    wait_until("the popup to close", || !outer.screen().contains(" legend "));

    // the logs button opens the logs
    outer.click(9, 29);
    wait_until("the logs popup", || outer.screen().contains(" logs "));
    tmux_out(&env, &["display-popup", "-C", "-c", &client]);

    // the reset button resets the layout
    let tree = view(&main_panes(&env), "tree").clone();
    tmux_out(&env, &["resize-pane", "-t", &tree.id, "-x", "70"]);
    outer.click(1, 29);
    wait_until("the reset", || (28..=32).contains(&view(&main_panes(&env), "tree").width));
    assert_eq!(actions().len(), 3);
}

/// The capture of row `row` of the client with styles, as `capture-pane -e` writes it.
fn styled_row(outer: &Outer, row: usize) -> String {
    outer.run(&["capture-pane", "-p", "-e", "-t", "o"]).lines().nth(row).unwrap_or("").to_owned()
}

/// `row` without its escape sequences: the cells as the client shows them.
fn visible(row: &str) -> String {
    let mut out = String::new();
    let mut chars = row.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for e in chars.by_ref() {
                if e.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Where the pill of `title` is in the styled `row`, its text between the caps with no padding: the
/// 0-based column of its left cap, the SGR parameters in force at that cap (`90` is colour 8, `34` blue) and those of the
/// right cap (the first colour set after the text).
fn pill(row: &str, title: &str) -> (usize, String, String) {
    let (head, after) = row.split_once(&format!("m{title}\x1b[")).unwrap_or_else(|| panic!("{title} in {row:?}"));
    let (before, _) = head.rsplit_once('\u{e0b6}').unwrap_or_else(|| panic!("a left cap before {title} in {row:?}"));
    // The foreground in force: the last foreground SGR (tmux sends none when it already holds, so a bg reset may come last).
    let in_force = |text: &str| {
        text.rsplit("\x1b[")
            .filter_map(|s| s.split_once('m').map(|(p, _)| p))
            .find(|p| p.len() == 2 && (p.starts_with('3') || p.starts_with('9')) && *p != "39")
            .map_or(String::new(), str::to_owned)
    };
    let (right, _) = after.split_once('\u{e0b4}').unwrap_or_else(|| panic!("a right cap after {title} in {row:?}"));
    (visible(before).chars().count(), in_force(before), in_force(right))
}

#[test]
fn a_pane_title_is_an_unpadded_pill_grey_and_blue_when_focused_three_columns_from_the_edge() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let panes = main_panes(&env);
    let (tree, plan, widgets, chat) =
        (view(&panes, "tree").clone(), view(&panes, "plan").clone(), view(&panes, "widgets").clone(), view(&panes, "chat").clone());
    // A Lead in the chat slot, with a header (its Plan's title) and the keyboard focus.
    let lead = tmux_out(&env, &["new-window", "-d", "-t", "=cpd:", "-n", "park-pay", "-P", "-F", "#{pane_id}", "sleep", "3600"]);
    let lead = lead.trim();
    tmux_out(&env, &["set", "-p", "-t", lead, "@cpd_session", "lead-a"]);
    tmux_out(&env, &["set", "-p", "-t", lead, "@cpd_title", "Retry Payouts"]);
    tmux_out(&env, &["swap-pane", "-d", "-s", lead, "-t", &chat.id]);
    tmux_out(&env, &["select-pane", "-t", lead]);
    let outer = Outer::attach(&env, 120, 30);
    // No padding inside the caps: the title between them, nothing around it.
    wait_until("the Lead's header", || outer.screen().contains("\u{e0b6}Retry Payouts\u{e0b4}"));
    // The tree is wide enough for its whole pill on a client this size.
    tmux_out(&env, &["resize-pane", "-t", &tree.id, "-x", "30"]);
    for title in ["Context", "Plan", "Deploy"] {
        wait_until(title, || outer.screen().contains(&format!("\u{e0b6}{title}\u{e0b4}")));
    }
    // The pane's header row (the client's size is the window's now) and its left edge.
    let at = |pane: &str| -> (usize, usize) {
        let out = tmux_out(&env, &["display", "-p", "-t", pane, "#{pane_top} #{pane_left}"]);
        let (top, left) = out.trim().split_once(' ').unwrap();
        (usize::try_from(top.parse::<i32>().unwrap() - 1).unwrap(), left.parse().unwrap())
    };
    let titled = [("Context", tree.id.as_str()), ("Plan", plan.id.as_str()), ("Deploy", widgets.id.as_str()), ("Retry Payouts", lead)];
    // Every pill starts three columns from its pane's edge (hs-d478): three border cells, then the left cap. Its caps
    // and block are colour 7 (SGR 37) and, on the focused pane, blue (SGR 34), whatever the border's colour is.
    let check = |focused: &str| {
        for (title, id) in titled {
            let (row, left) = at(id);
            let colour = if id == focused { "34" } else { "37" };
            let (column, left_cap, right_cap) = pill(&styled_row(&outer, row), title);
            assert_eq!((column, left_cap.as_str(), right_cap.as_str()), (left + 3, colour, colour), "{title} (focus on {focused})");
        }
    };
    check(lead);
    // Focus the plan view: its pill turns blue, the Lead's turns grey.
    tmux_out(&env, &["select-pane", "-t", &plan.id]);
    wait_until("the focus to move", || pill(&styled_row(&outer, at(&plan.id).0), "Plan").1 == "34");
    check(&plan.id);
}

/// The SGR parameters in force at the 0-based column `column` of the styled `row`.
fn sgr_at(row: &str, column: usize) -> String {
    let (mut sgr, mut col, mut chars) = (String::new(), 0, row.chars());
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            let seq: String = chars.by_ref().take_while(|e| !e.is_ascii_alphabetic()).collect();
            sgr = seq.trim_start_matches('[').to_owned();
        } else {
            if col == column {
                return sgr;
            }
            col += 1;
        }
    }
    panic!("no column {column} in {row:?}");
}

/// hs-d489: the border between two unfocused panes is grey (colour 8: SGR 90 or `38;5;8`), the one beside the focused
/// pane is blue.
#[test]
fn a_pane_border_is_grey_and_the_focused_panes_border_is_blue() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let panes = main_panes(&env);
    let (tree, widgets) = (view(&panes, "tree").clone(), view(&panes, "widgets").clone());
    tmux_out(&env, &["select-pane", "-t", &tree.id]);
    let outer = Outer::attach(&env, 120, 30);
    wait_until("the client", || outer.screen().contains('\u{e0b6}'));
    tmux_out(&env, &["resize-pane", "-t", &tree.id, "-x", "30"]);
    wait_until("the tree's header", || outer.screen().contains("\u{e0b6}Context\u{e0b4}"));
    let geometry = |pane: &str| -> (usize, usize, usize) {
        let out = tmux_out(&env, &["display", "-p", "-t", pane, "#{pane_top} #{pane_left} #{pane_width}"]);
        let n: Vec<usize> = out.split_whitespace().map(|v| v.parse().unwrap()).collect();
        (n[0], n[1], n[2])
    };
    // A body row, below the header: the focused tree's right border, then the widgets' left one (between two unfocused panes).
    let (top, left, width) = geometry(&tree.id);
    let (_, widgets_left, _) = geometry(&widgets.id);
    let row = styled_row(&outer, top + 2);
    let focused_border = sgr_at(&row, left + width);
    assert_eq!(focused_border, "34", "the focused pane's border is blue: {row:?}");
    let border = sgr_at(&row, widgets_left - 1);
    assert_eq!(border, "37", "an unfocused pane's border is colour 7 (hs-d530 R4): {row:?}");
}

/// Kills `gone` (views of `main`), runs `cpd tui`, and asserts the layout is whole again with the panes that were
/// not killed unchanged, and that rerunning adds nothing.
fn assert_repaired_after_killing(gone: &[&str]) {
    let env = env();
    env.cpd(&["tui"]).ok();
    let full = main_panes(&env);
    for name in gone {
        tmux_out(&env, &["kill-pane", "-t", &view(&full, name).id]);
    }
    // An old main, its sizes changed by the User.
    if !gone.contains(&"widgets") {
        tmux_out(&env, &["resize-pane", "-t", &view(&full, "widgets").id, "-x", "80"]);
    }
    let before = main_panes(&env);
    assert_eq!(before.len(), 4 - gone.len(), "{before:?}");

    let out = env.cpd(&["tui"]).ok();
    assert_eq!(out["built"], false, "{out}");
    let after = main_panes(&env);
    assert_layout(&after);
    for old in &before {
        let new = after.iter().find(|p| p.id == old.id).unwrap_or_else(|| panic!("{} is gone: {after:?}", old.id));
        assert_eq!((new.view.as_str(), new.pid, new.dead), (old.view.as_str(), old.pid, false));
    }

    // Rerunning adds nothing and starts nothing.
    let started = views_started(&env).len();
    env.cpd(&["tui"]).ok();
    assert_eq!(main_panes(&env), after);
    assert_eq!(views_started(&env).len(), started);
}

#[test]
fn tui_adds_a_missing_tree_to_an_existing_main() {
    assert_repaired_after_killing(&["tree"]);
}

#[test]
fn tui_adds_a_missing_plan_view_to_an_existing_main() {
    assert_repaired_after_killing(&["plan"]);
}

#[test]
fn tui_adds_a_missing_widgets_view_to_an_existing_main() {
    assert_repaired_after_killing(&["widgets"]);
}

#[test]
fn tui_adds_a_missing_chat_placeholder_to_an_existing_main() {
    assert_repaired_after_killing(&["chat"]);
}

#[test]
fn tui_adds_the_plan_view_and_the_chat_slot_when_both_are_missing() {
    assert_repaired_after_killing(&["plan", "chat"]);
}

#[test]
fn tui_finishes_a_build_that_stopped_after_the_first_pane() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let full = main_panes(&env);
    let tree = view(&full, "tree").clone();
    for name in ["plan", "widgets", "chat"] {
        tmux_out(&env, &["kill-pane", "-t", &view(&full, name).id]);
    }
    assert_eq!(main_panes(&env).len(), 1);

    let out = env.cpd(&["tui"]).ok();
    assert_eq!(out["built"], false, "{out}");
    let after = main_panes(&env);
    assert_layout(&after);
    assert_eq!(view(&after, "tree").pid, tree.pid, "the pane that was there is not touched");
    env.cpd(&["tui"]).ok();
    assert_eq!(main_panes(&env), after);
}

#[test]
fn tui_keeps_a_lead_in_the_chat_slot_when_the_plan_view_is_missing() {
    let env = env();
    env.cpd(&["tui"]).ok();
    let placeholder = view(&main_panes(&env), "chat").clone();
    let lead = tmux_out(&env, &["new-window", "-d", "-t", "=cpd:", "-n", "park-pay", "-P", "-F", "#{pane_id}", "sleep", "3600"]);
    let lead = lead.trim();
    tmux_out(&env, &["set", "-p", "-t", lead, "@cpd_session", "lead-a"]);
    tmux_out(&env, &["swap-pane", "-d", "-s", lead, "-t", &placeholder.id]);
    let full = main_panes(&env);
    tmux_out(&env, &["kill-pane", "-t", &view(&full, "plan").id]);

    env.cpd(&["tui"]).ok();
    let after = main_panes(&env);
    assert_layout(&after);
    assert!(after.iter().any(|p| p.id == lead), "the Lead keeps the slot: {after:?}");
    assert!(after.iter().all(|p| p.view != "chat"), "no second slot: {after:?}");
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
    assert_eq!(views.len(), 4, "tree, plan, widgets and the Worker view: {before:?}");
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
    // The cockpit starts the engine, and the engine restarts a server older than the installed tmux as soon as nothing is in
    // the way (`restart::busy`). This test is of `cpd tui` alone, and on a loaded machine it outlasts the engine's pause of a
    // few seconds: the restart would kill the server the test asks about. A running Attempt is in the way; its driver is
    // this process, which lives as long as the test.
    env.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {id}, 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {id}, 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
                   VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', {pid}, 't');",
                fp = "a".repeat(64),
                id = tx.id(),
                pid = std::process::id()
            ))?;
            Ok::<(), CpdError>(())
        })
        .unwrap();

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

/// The status line (last row) of a client of `width` columns once it shows `needle`, as text and with its styles
/// (`capture-pane -e`).
fn status_line(env: &TestEnv, width: u16, needle: &str) -> (String, String) {
    let outer = Outer::attach(env, width, 20);
    let mut line = String::new();
    // The status line fills the row (hs-d479): wait for the whole of it, since a client can show a row mid-draw
    // (frames arrive in pieces, hs-n206) with the needle already there and the fill not yet.
    wait_until(&format!("{needle} on a whole status line"), || {
        line = outer.screen().lines().nth(19).unwrap_or("").trim_end().to_owned();
        line.contains(needle) && line.chars().count() == usize::from(width)
    });
    let styled = outer.run(&["capture-pane", "-p", "-e", "-t", "o"]).lines().nth(19).unwrap_or("").to_owned();
    (line, styled)
}

/// A cockpit whose options hold what `workers.rs` sets for two Inbox items, the most urgent red, and
/// `6/6 · 4 waiting · 9 queued`.
/// No engine runs: the test holds `locks/watch.lock` as one does, so `tui`'s `watch --ensure` starts none
/// and nothing overwrites the options set by hand (the engine's step has its own tests in `watch.rs`).
/// The lock is held while the returned guard lives.
fn env_with_workers() -> (TestEnv, Flock<File>) {
    let env = env();
    let path = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap().watch_lock();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    let engine = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    tmux_out(&env, &["new-session", "-d", "-s", "cpd", "-n", "park-pay", "-x", "215", "-y", "62", "sleep", "3600"]);
    let tui = env.cpd(&["tui"]).ok();
    assert_eq!(tui["watch"], json!({"started": false, "already": true}), "{tui}");
    // These tests pin the line without the version pill (its own tests set it): `tui` set it, so unset it.
    tmux_out(&env, &["set", "-gu", "@cpd_version"]);
    for (option, value) in [
        ("@cpd_workers", "workers 6/6 · 4 waiting · 9 queued"),
        ("@cpd_workers_short", "6/6·4·9"),
        ("@cpd_workers_width", "36"),
        ("@cpd_workers_short_width", "9"),
    ] {
        tmux_out(&env, &["set", "-g", option, value]);
    }
    (env, engine)
}

/// The left group (hs-d479): the leading `─`, then each button as a pill (its glyph between the two caps, 3 cells),
/// joined by a single `─`. 12 cells.
fn buttons() -> String {
    format!("\u{2500}\u{e0b6}{RESET_GLYPH}\u{e0b4}\u{2500}\u{e0b6}{LEGEND_GLYPH}\u{e0b4}\u{2500}\u{e0b6}{LOGS_GLYPH}\u{e0b4}")
}

/// The right group in the long form: the workers pill (`workers 6/6 · 4 waiting · 9 queued`), then the closing `─`:
/// 36 cells (the engine's width) plus 1.
const LONG_ITEMS: &str = "\u{e0b6}workers 6/6 · 4 waiting · 9 queued\u{e0b4}\u{2500}";
/// The same in the short form: 9 + 1 cells.
const SHORT_ITEMS: &str = "\u{e0b6}6/6·4·9\u{e0b4}\u{2500}";
const LONG_WIDTH: usize = 36 + 1;
const SHORT_WIDTH: usize = 9 + 1;
const LEFT_WIDTH: usize = 12;

/// The whole row of a client of `width` columns: the left group, `preview` (the pill with its join, or nothing), the
/// fill `─` and the right group, exactly `width` cells.
fn row(width: usize, preview: Option<&str>, items: &str, items_width: usize) -> String {
    let (pill, pill_width) = match preview {
        Some(value) => (format!("\u{2500}\u{e0b6}preview: {value}\u{e0b4}"), value.chars().count() + 12),
        None => (String::new(), 0),
    };
    let fill = width - LEFT_WIDTH - pill_width - items_width;
    format!("{}{pill}{}{items}", buttons(), "\u{2500}".repeat(fill))
}

// The right group is 37 cells wide in the long form (a text of 34 cells, 2 caps, the closing `─`):
// with the left group's 12 and a fill of at least 1 it needs a client of 50 columns; one less gets the short form.
#[test]
fn the_status_line_is_one_full_line_of_joined_pills_in_the_long_workers_form_from_50_columns() {
    let (env, _engine) = env_with_workers();
    for width in [50usize, 80, 120, 200] {
        let (line, _) = status_line(&env, width as u16, "queued");
        assert_eq!(line, row(width, None, LONG_ITEMS, LONG_WIDTH), "at {width}: {line:?}");
        assert_eq!(line.chars().count(), width, "{line:?}");
    }
}

#[test]
fn the_status_line_is_one_full_line_of_joined_pills_in_the_short_workers_form_below_50_columns() {
    let (env, _engine) = env_with_workers();
    for width in [49usize, 40] {
        let (line, _) = status_line(&env, width as u16, "6/6");
        assert!(!line.contains("workers"), "{line:?}");
        assert_eq!(line, row(width, None, SHORT_ITEMS, SHORT_WIDTH), "at {width}: {line:?}");
    }
}

#[test]
fn the_status_line_starts_and_ends_with_one_grey_line_cell_and_joins_its_pills_with_one() {
    let (env, _engine) = env_with_workers();
    for width in [80u16, 120, 200] {
        let (line, styled) = status_line(&env, width, "queued");
        assert!(line.starts_with(&format!("\u{2500}\u{e0b6}{RESET_GLYPH}\u{e0b4}\u{2500}\u{e0b6}{LEGEND_GLYPH}")), "{line:?}");
        assert!(line.ends_with("queued\u{e0b4}\u{2500}"), "{line:?}");
        // The right pills are joined by one `─` (never a space), and the left group by the buttons' two.
        assert!(!line.contains("inbox"), "{line:?}");
        assert!(line.starts_with(&buttons()), "{line:?}");
        assert!(!line.contains("\u{e0b4} \u{e0b6}"), "no space between two pills: {line:?}");
        // The leading `─`, the joins and the fill are colour 8 (bright black, SGR 90), the pills' caps too.
        assert!(styled.contains("\x1b[90m\u{2500}\x1b[97m\u{e0b6}"), "the leading line cell, grey, then a button's white cap: {styled:?}");
        assert!(styled.trim_end().ends_with("\x1b[0m\x1b[90m\u{e0b4}\u{2500}"), "the pill's cap, then the closing line cell, grey: {styled:?}");
    }
}

#[test]
fn the_status_line_fill_is_grey_and_spans_the_free_width_of_any_client() {
    let (env, _engine) = env_with_workers();
    for width in [40u16, 100] {
        let (line, styled) = status_line(&env, width, if width >= 50 { "queued" } else { "6/6" });
        assert_eq!(line.chars().count(), usize::from(width), "{line:?}");
        let used = LEFT_WIDTH + if width >= 50 { LONG_WIDTH } else { SHORT_WIDTH };
        let fill = "\u{2500}".repeat(usize::from(width) - used);
        // The last button's cap, the fill (the join is its first cell), then the workers' pill.
        assert!(line.contains(&format!("\u{e0b4}{fill}\u{e0b6}")), "{line:?}");
        assert!(styled.contains(&format!("\x1b[90m{fill}\u{e0b6}")), "{styled:?}");
    }
}

/// The SGR state (foreground 0-15, background 0-15, reverse) in force over the first cell of `text` in `styled`, which
/// holds `ESC[…m` sequences; the byte order tmux emits them in is not pinned.
fn sgr_state_at(styled: &str, text: &str) -> (Option<u8>, Option<u8>, bool) {
    let at = styled.find(text).unwrap_or_else(|| panic!("no {text:?} in {styled:?}"));
    let (mut fg, mut bg, mut reverse) = (None, None, false);
    let mut rest = &styled[..at];
    while let Some(start) = rest.find("\x1b[") {
        let body = &rest[start + 2..];
        let end = body.find('m').expect("an unclosed SGR");
        let params: Vec<u32> = body[..end].split(';').map(|p| p.parse().unwrap_or(0)).collect();
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => (fg, bg, reverse) = (None, None, false),
                7 => reverse = true,
                27 => reverse = false,
                n @ 30..=37 => fg = Some((n - 30) as u8),
                n @ 90..=97 => fg = Some((n - 90 + 8) as u8),
                39 => fg = None,
                n @ 40..=47 => bg = Some((n - 40) as u8),
                n @ 100..=107 => bg = Some((n - 100 + 8) as u8),
                49 => bg = None,
                n @ (38 | 48) => {
                    let colour = Some(params[i + 2] as u8);
                    if n == 38 { fg = colour } else { bg = colour }
                    i += 2;
                }
                _ => {}
            }
            i += 1;
        }
        rest = &body[end + 1..];
    }
    (fg, bg, reverse)
}

const PREVIEW: &str = "cpd.tui-visuals.box-order 6c43886";

// The preview pill is `preview: <value>` between caps, no padding, joined to the buttons by a `─`: 45 cells with
// 33 of value. With the left group's 12, the long form's 37 and a fill of 1 it needs 95 columns.
#[test]
fn the_status_line_shows_the_unpadded_preview_pill_after_the_buttons_while_the_option_is_set() {
    let (env, _engine) = env_with_workers();
    tmux_out(&env, &["set", "-g", "@cpd_preview", PREVIEW]);
    let pill = format!("\u{e0b6}preview: {PREVIEW}\u{e0b4}");
    for (width, items, items_width) in [(94usize, SHORT_ITEMS, SHORT_WIDTH), (95, LONG_ITEMS, LONG_WIDTH), (120, LONG_ITEMS, LONG_WIDTH), (200, LONG_ITEMS, LONG_WIDTH)] {
        let (line, styled) = status_line(&env, width as u16, "preview:");
        assert!(line.starts_with(&format!("{}\u{2500}{pill}\u{2500}", buttons())), "a `─` join after the buttons, then the pill, then the line: {line:?}");
        assert!(line.ends_with(items), "at {width}: {line:?}");
        assert_eq!(line, row(width, Some(PREVIEW), items, items_width), "the row stays full at {width}: {line:?}");
        let (fg, bg, reverse) = sgr_state_at(&styled, &format!("preview: {PREVIEW}"));
        assert!(reverse && fg == Some(3) && bg.is_none(), "reverse, foreground 3, no background, unpadded: {styled:?}");
    }
}

#[test]
fn the_status_line_shows_no_preview_once_the_option_is_unset() {
    let (env, _engine) = env_with_workers();
    tmux_out(&env, &["set", "-g", "@cpd_preview", PREVIEW]);
    status_line(&env, 100, "preview:");
    tmux_out(&env, &["set", "-gu", "@cpd_preview"]);
    // The unset shows once a fresh client has drawn the whole line.
    let (line, _) = status_line(&env, 100, "queued");
    assert!(!line.contains("preview"), "{line:?}");
    assert_eq!(line, row(100, None, LONG_ITEMS, LONG_WIDTH), "{line:?}");
}

#[test]
fn tui_respawn_warns_on_stderr_while_a_preview_is_on_and_not_otherwise() {
    let env = env();
    let quiet = env.cpd(&["tui", "--respawn"]);
    assert_eq!(quiet.exit, 0, "{quiet:?}");
    assert!(!quiet.stderr.contains("preview"), "{quiet:?}");

    tmux_out(&env, &["set", "-g", "@cpd_preview", PREVIEW]);
    let warned = env.cpd(&["tui", "--respawn"]);
    assert_eq!(warned.exit, 0, "{warned:?}");
    assert!(
        warned.stderr.contains(&format!(
            "a preview is on ({PREVIEW}); its views keep the preview build until 'preview-tui.sh off'"
        )),
        "{warned:?}"
    );
    // Without --respawn nothing is said; the option's JSON does not change.
    assert!(!env.cpd(&["tui"]).stderr.contains("preview"));

    tmux_out(&env, &["set", "-gu", "@cpd_preview"]);
    assert!(!env.cpd(&["tui", "--respawn"]).stderr.contains("preview"));
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
    child.args(["--ignored", "--exact", "tui::the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command"]);
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
