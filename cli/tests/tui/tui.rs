//! The cockpit, `cpd tui` (commands.md §13.1; contract §6.1, §6.2 with
//! commands.md §19 amendments 1, 2 and 9) and the routing of `cpd tui view`
//! (§13.2).
//!
//! The cockpit runs in the test process (`common::cockpit`, on the fake ports and the fake tmux server of an `Fx`): its
//! tests pin the tmux commands it sends (the layout's `new-session`, `split-window`, `resize-pane` and the rest, in
//! order: `steps`) and the options it sets, never tmux's geometry or rendering. The only tests that still run a process
//! are the two of the harness (`TestEnv`): its denied tools and its environment.

use crate::common;

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::TestEnv;
use common::cockpit::{action, ran, tui};
use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::host::tmux::{
    LEGEND_GLYPH, LOGS_GLYPH, RESET_GLYPH, SERVER_OPTIONS, STATUS_CLICK, STATUS_FORMAT,
};
use cpd_core::store::Store;
use serde_json::{Value, json};

/// The line of `list-keys -T <table>` that binds `key`, in the fake server's rendering (a command's words joined by a space).
fn binding(fx: &Fx, table: &str, key: &str) -> String {
    let prefix = format!("bind-key -T {table} {key} ");
    out(fx, &["list-keys", "-T", table])
        .lines()
        .find(|l| l.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no {key} in {table}"))
        .to_owned()
}

// ---------------------------------------------------------------------------------------------------------------------
// The cockpit in the test process (`common::cockpit`), on the fake tmux server of an `Fx`. tmux's geometry and rendering
// are tmux's, not ours: what a test pins is what the cockpit asked of tmux (the argv of its layout commands, in order)
// and what it set (the option strings), read from the calls of the route and from the server's state.
// ---------------------------------------------------------------------------------------------------------------------

/// An `Fx` on the fake ports with the cockpit's config (machine `m1`, the env's socket).
fn fx() -> Fx {
    common::tree::fx()
}

/// `tmux <args>`'s stdout on the fake server; panics when it fails.
fn out(fx: &Fx, args: &[&str]) -> String {
    let out = fx.tmux(args);
    assert!(
        out.status.success(),
        "tmux {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// `out(..)` without its last newline.
fn shown(fx: &Fx, args: &[&str]) -> String {
    out(fx, args).trim_end_matches('\n').to_owned()
}

/// One pane of `cpd:main`.
#[derive(Debug, Clone, PartialEq)]
struct Pane {
    id: String,
    view: String,
    session: String,
    dead: bool,
    pid: u32,
}

fn main_panes(fx: &Fx) -> Vec<Pane> {
    let format = "#{pane_id}\t#{@cpd_view}\t#{@cpd_session}\t#{pane_dead}\t#{pane_pid}";
    out(fx, &["list-panes", "-t", "=cpd:main", "-F", format])
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            Pane {
                id: f[0].into(),
                view: f[1].into(),
                session: f[2].into(),
                dead: f[3] == "1",
                pid: f[4].parse().expect("a live pane has a pid"),
            }
        })
        .collect()
}

fn view<'a>(panes: &'a [Pane], name: &str) -> &'a Pane {
    panes
        .iter()
        .find(|p| p.view == name)
        .unwrap_or_else(|| panic!("no {name} pane: {panes:?}"))
}

fn ids(panes: &[Pane]) -> Vec<&str> {
    panes.iter().map(|p| p.id.as_str()).collect()
}

fn window_names(fx: &Fx, session: &str) -> Vec<String> {
    out(
        fx,
        &[
            "list-windows",
            "-t",
            &format!("={session}"),
            "-F",
            "#{window_name}",
        ],
    )
    .lines()
    .map(str::to_owned)
    .collect()
}

/// What `pane` runs: its argv, as the server holds it.
fn argv_of(fx: &Fx, pane: &str) -> Vec<String> {
    fx.tmux_state()
        .expect("a server")
        .panes()
        .into_iter()
        .find(|p| p.id == pane)
        .unwrap_or_else(|| panic!("no pane {pane}"))
        .argv
}

/// The mark of "from now on" for [`steps`].
fn mark(fx: &Fx) -> usize {
    fx.calls("tmux").len()
}

/// The commands of the cockpit that shape the layout, sent to tmux since `mark`, in order, one string each: the panes
/// and windows made (`new-session`, `new-window`, `split-window`, what each runs after `--` or at the end), killed,
/// respawned and swapped, their `@cpd_view` and `@cpd_session` marks (`set -p`), the sizes (`resize-pane`) and the focus
/// (`select-pane`). Without what does not shape it: the client's `-u -L <socket> -f /dev/null`, the start directory (`-c`),
/// the environment (`-e`) and the pane id each `split-window` prints back (`-P -F`).
fn steps(fx: &Fx, mark: usize) -> Vec<String> {
    let calls = fx.calls("tmux");
    calls[mark..]
        .iter()
        .flat_map(|call| commands(&call.args))
        .collect()
}

fn commands(args: &[String]) -> Vec<String> {
    let mut rest = args;
    while let Some(first) = rest.first() {
        match first.as_str() {
            "-u" => rest = &rest[1..],
            "-L" | "-f" => rest = &rest[2..],
            _ => break,
        }
    }
    rest.split(|a| a == ";").filter_map(step).collect()
}

fn step(command: &[String]) -> Option<String> {
    let name = command.first()?.as_str();
    let shapes = matches!(
        name,
        "new-session"
            | "new-window"
            | "split-window"
            | "resize-pane"
            | "select-pane"
            | "kill-pane"
            | "respawn-pane"
            | "swap-pane"
    ) || (name == "set" && command.get(1).map(String::as_str) == Some("-p"));
    if !shapes {
        return None;
    }
    let mut words = Vec::new();
    let mut rest = command.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "-c" | "-e" | "-F" => drop(rest.next()),
            "-P" | "--" => {}
            _ => words.push(word.as_str()),
        }
    }
    Some(words.join(" "))
}

/// The steps that make `cpd:main` of a new cockpit: the four views, each marked, then reset-layout and the focus on the chat slot.
fn build_steps() -> Vec<&'static str> {
    vec![
        "new-session -d -s cpd -n main -x 215 -y 62 cpd tui view tree",
        "set -p -t %0 @cpd_view tree",
        "split-window -d -t %0 -h cpd tui view plan",
        "set -p -t %1 @cpd_view plan",
        "split-window -d -t %1 -h cpd tui view widgets",
        "set -p -t %2 @cpd_view widgets",
        "split-window -d -t %1 -v cpd tui view chat",
        "set -p -t %3 @cpd_view chat",
        "resize-pane -t %0 -x 20%",
        "resize-pane -t %2 -x 30%",
        "resize-pane -t %1 -y 30%",
        "select-pane -t %3",
    ]
}

/// What the engine's `watch --ensure` leaves in tmux: the session `cpd-work` with the window `watch`. Returns its pane.
fn start_engine_pane(fx: &Fx) -> String {
    shown(
        fx,
        &[
            "new-session",
            "-d",
            "-s",
            "cpd-work",
            "-n",
            "watch",
            "-P",
            "-F",
            "#{pane_id}",
            "cpd",
            "watch",
        ],
    )
}

/// Asserts the server has every option, binding and hook `host` sets at a server start (contract §6.1), read from the fake
/// server's state: the hooks' text is the very `PANE_DIED_HOOK` and `WINDOW_UNLINKED_HOOK`, which start the engine again.
fn assert_options_on_fake(fx: &Fx) {
    let state = fx.tmux_state().expect("a server");
    for option in SERVER_OPTIONS.iter() {
        let option: Vec<&str> = option.iter().map(String::as_str).collect();
        match option.as_slice() {
            ["set", "-g" | "-gq" | "-s", name, value] => {
                let flag = if option[1] == "-s" { "-sv" } else { "-gv" };
                assert_eq!(shown(fx, &["show", flag, name]), *value, "{name}");
            }
            ["set", "-as", name, value] => {
                let have = out(fx, &["show", "-sv", name]);
                for feature in value.split(',').filter(|f| !f.is_empty()) {
                    // Once: applying the options again does not append them again.
                    assert_eq!(have.matches(feature).count(), 1, "{feature} in {have}");
                }
            }
            ["set-hook", "-g", hook, command] => {
                assert!(
                    state
                        .hooks()
                        .contains(&((*hook).to_owned(), (*command).to_owned())),
                    "{hook}: {:?}",
                    state.hooks()
                );
                assert!(command.contains("cpd watch --ensure --wait"), "{command}");
            }
            ["bind", "-T", table, key, command @ ..] => {
                assert_eq!(
                    binding(fx, table, key),
                    format!("bind-key -T {table} {key} {}", command.join(" "))
                );
            }
            other => panic!("an option this test cannot check: {other:?}"),
        }
    }
}

/// After `cpd tui`: `@cpd_machine` is the test config's machine and the title tmux expands from `set-titles-string` is
/// `φ · <machine>` (contract §6.1, hs-d300).
fn assert_window_title(fx: &Fx) {
    assert_eq!(shown(fx, &["show", "-gv", "set-titles"]), "on");
    assert_eq!(shown(fx, &["show", "-gv", "@cpd_machine"]), "m1");
    let format = shown(fx, &["show", "-gv", "set-titles-string"]);
    assert_eq!(shown(fx, &["display", "-p", &format]), "φ · m1");
}

/// The left group of the status line (hs-d479): the leading `━`, then each button as a pill (its glyph and one space between the two caps, 4 cells),
/// each in its range, joined by a single `━` outside the ranges.
fn buttons() -> String {
    format!(
        "#[fg=colour8]\u{2501}#[default]#[range=user|reset]#[fg=colour15]\u{e0b6}#[reverse]{RESET_GLYPH} #[noreverse]\u{e0b4}#[default]#[norange]\
         #[fg=colour8]\u{2501}#[default]#[range=user|legend]#[fg=colour15]\u{e0b6}#[reverse]{LEGEND_GLYPH} #[noreverse]\u{e0b4}#[default]#[norange]\
         #[fg=colour8]\u{2501}#[default]#[range=user|logs]#[fg=colour15]\u{e0b6}#[reverse]{LOGS_GLYPH} #[noreverse]\u{e0b4}#[default]#[norange]\
         #[fg=colour8]\u{2501}#[default]#[range=user|costs]#[fg=colour15]\u{e0b6}#[reverse]\u{EEC1} #[noreverse]\u{e0b4}#[default]#[norange]"
    )
}

/// Asserts the cockpit session `cpd` has the StatusBar as its status line, and `cpd-work` none (contract §6.1).
fn assert_status_line(fx: &Fx) {
    assert_eq!(shown(fx, &["show", "-v", "-t", "=cpd:", "status"]), "on");
    assert_eq!(
        shown(fx, &["show", "-v", "-t", "=cpd:", "status-style"]),
        "default"
    );
    let format = shown(fx, &["show", "-v", "-t", "=cpd:", "status-format[0]"]);
    assert_eq!(format, *STATUS_FORMAT);
    assert!(format.starts_with(&buttons()), "{format}");
    for part in [
        "#[range=user|incident]",
        "#[range=user|host]",
        "#{R:\u{2501},",
        "#{@cpd_workers},#{@cpd_workers_short}",
    ] {
        assert!(format.contains(part), "{part} in {format}");
    }
    // The buttons are one `━` apart, outside their ranges: no range starts where the one before it ends.
    assert!(!format.contains("#[norange]#[range"), "{format}");
    // Set on the cockpit session only: the server default and cpd-work have no status row.
    assert_eq!(shown(fx, &["show", "-gv", "status"]), "off");
    if fx
        .tmux(&["has-session", "-t", "=cpd-work"])
        .status
        .success()
    {
        let set = out(fx, &["show", "-t", "=cpd-work:"]);
        assert!(!set.lines().any(|l| l.starts_with("status")), "{set}");
    }
}

#[test]
fn tui_builds_the_cockpit_once_and_starts_the_engine() {
    let fx = fx();
    let from = mark(&fx);
    let out_ = tui(&fx, &[]);
    let socket = fx.env.tmux_socket();
    assert_eq!(out_["built"], true, "{out_}");
    assert_eq!(out_["socket"], socket);
    assert_eq!(out_["session"], "cpd");
    assert_eq!(
        out_["attach"],
        json!(["tmux", "-L", socket, "attach", "-t", "cpd"])
    );
    assert_eq!(out_["watch"]["started"], true, "{out_}");
    // The engine is started by the recorded `cpd watch --ensure`; its pane is `watch --ensure`'s.
    assert_eq!(ran(), [["cpd", "watch", "--ensure"]]);

    // The layout of §13.1, as the commands that make it: the tree left, the plan view above the chat slot in the centre, the
    // widgets right, each marked, sized 20 % / 30 % and 30 %, the focus on the chat slot; no StatusBar pane.
    assert_eq!(steps(&fx, from), build_steps());
    start_engine_pane(&fx);
    let panes = main_panes(&fx);
    assert_eq!(panes.len(), 4, "{panes:?}");
    assert!(
        panes.iter().all(|p| !p.dead && p.view != "statusbar"),
        "{panes:?}"
    );
    let chat = view(&panes, "chat");
    assert_eq!(window_names(&fx, "cpd"), ["main"]);
    assert_eq!(window_names(&fx, "cpd-work"), ["watch"]);
    // The four views are started: each pane runs `cpd tui view <view>`.
    for v in ["tree", "plan", "widgets", "chat"] {
        assert_eq!(argv_of(&fx, &view(&panes, v).id), ["cpd", "tui", "view", v]);
    }
    // The keyboard goes to the chat slot.
    assert_eq!(
        shown(&fx, &["display", "-p", "-t", "=cpd:main", "#{pane_id}"]),
        chat.id
    );
    // (The real placeholder view draws its text in the slot: `tui/src/chat.rs` `draws_the_placeholder_text_dim`.)

    // The server has §6.1's options, hyperlinks included, and the TUI's border titles.
    let features = out(&fx, &["show", "-s", "terminal-features"]);
    for feature in [
        "xterm*:extkeys",
        "xterm-ghostty:extkeys",
        "xterm-ghostty:hyperlinks",
    ] {
        assert!(features.contains(feature), "{feature} not in {features}");
    }
    assert_eq!(shown(&fx, &["show", "-gv", "remain-on-exit"]), "on");
    let border = shown(&fx, &["show", "-gv", "pane-border-format"]);
    assert!(
        border.contains("#[reverse]") && !border.contains("#[bg="),
        "{border}"
    );
    // (what the format expands to is pinned by `the_built_cockpits_tree_pane_border_expands_to_the_cpd_mark_and_plan_and_widgets_to_nothing`.)
    assert!(!border.contains("statusbar"), "{border}");
    // The StatusBar is the cockpit session's status line; a click on a pane is tmux's default again.
    assert_status_line(&fx);
    assert_options_on_fake(&fx);
    assert_eq!(
        binding(&fx, "root", "MouseDown1Pane"),
        "bind-key -T root MouseDown1Pane select-pane -t = ; send-keys -M"
    );
    assert_eq!(
        binding(&fx, "root", "MouseDown1Status"),
        format!("bind-key -T root MouseDown1Status run-shell -b {STATUS_CLICK}")
    );

    // A second run builds nothing and starts nothing.
    let again_from = mark(&fx);
    let again = tui(&fx, &[]);
    assert_eq!(again["built"], false, "{again}");
    assert_eq!(again["watch"], json!({"started": false, "already": true}));
    assert_eq!(ids(&main_panes(&fx)), ids(&panes));
    assert_eq!(steps(&fx, again_from), Vec::<String>::new());
    assert_eq!(window_names(&fx, "cpd-work"), ["watch"]);
}

#[test]
fn tui_builds_main_in_a_cockpit_that_has_only_a_park_window() {
    let fx = fx();
    // As plan new leaves it: the cockpit session with the first Lead's park window.
    out(
        &fx,
        &[
            "new-session",
            "-d",
            "-s",
            "cpd",
            "-n",
            "park-pay",
            "-x",
            "215",
            "-y",
            "62",
            "sleep",
            "3600",
        ],
    );
    let from = mark(&fx);
    let out_ = tui(&fx, &[]);
    assert_eq!(out_["built"], true, "{out_}");
    // `main` is a new window of the session, with the same four panes and sizes (the first pane is %1: %0 is the park window's).
    assert_eq!(
        steps(&fx, from),
        [
            "new-window -d -t =cpd: -n main cpd tui view tree",
            "set -p -t %1 @cpd_view tree",
            "split-window -d -t %1 -h cpd tui view plan",
            "set -p -t %2 @cpd_view plan",
            "split-window -d -t %2 -h cpd tui view widgets",
            "set -p -t %3 @cpd_view widgets",
            "split-window -d -t %2 -v cpd tui view chat",
            "set -p -t %4 @cpd_view chat",
            "resize-pane -t %1 -x 20%",
            "resize-pane -t %3 -x 30%",
            "resize-pane -t %2 -y 30%",
            "select-pane -t %4",
        ]
    );
    assert_eq!(main_panes(&fx).len(), 4);
    assert_status_line(&fx);
    assert_eq!(window_names(&fx, "cpd"), ["park-pay", "main"]);
    assert_eq!(tui(&fx, &[])["built"], false);
}

/// After `cpd tui`: the global `@cpd_version` is the `version` of `cpd --version` (§6.1, hs-d299).
#[test]
fn tui_sets_cpd_version_to_the_binarys_version() {
    let fx = fx();
    tui(&fx, &[]);
    let want = cpd::cli::version();
    assert_eq!(fx.cpd(&["--version"]).ok()["version"], want);
    assert_eq!(shown(&fx, &["show", "-gv", "@cpd_version"]), want);
}

#[test]
fn a_killed_view_respawns_at_once_and_the_engine_is_started_again() {
    let fx = fx();
    // The hook's `run-shell` is `/bin/sh -c`, which no test runs: the route records it.
    fx.fake("sh", Reply::ok(""));
    tui(&fx, &[]);
    let engine = start_engine_pane(&fx);
    assert_window_title(&fx);
    assert_options_on_fake(&fx);
    let before = main_panes(&fx);
    let tree = view(&before, "tree").clone();
    assert_eq!(fx.panes().len(), 5, "the four views and the engine");
    let from = mark(&fx);
    // The engine first, so that its hook runs before the tree's.
    fx.pane_exit(&engine, 137);
    // The engine is supervised (contract §6.6, hs-d328): the hook runs `watch --ensure --wait`, which starts it again in its own
    // pane. That the engine is started is `watch --ensure --wait`'s; the cockpit's part is the hook and its `run-shell`.
    assert!(fx.pane_dead(&engine));
    assert_eq!(
        fx.calls("sh")
            .iter()
            .map(|c| c.args.clone())
            .collect::<Vec<_>>(),
        [["-c", "cpd watch --ensure --wait >/dev/null 2>&1"]]
    );
    fx.pane_exit(&tree.id, 137);
    // The hook respawns the tree in place (`respawn-pane`): same pane, a new process, the same command.
    let after = main_panes(&fx);
    assert_eq!(ids(&after), ids(&before));
    let new_tree = view(&after, "tree");
    assert!(
        !new_tree.dead && new_tree.pid != tree.pid,
        "{new_tree:?} was {tree:?}"
    );
    assert_eq!(argv_of(&fx, &tree.id), ["cpd", "tui", "view", "tree"]);
    assert!(after.iter().all(|p| !p.dead), "{after:?}");
    // The layout is whole and untouched: nothing was made, killed or resized.
    assert_eq!(steps(&fx, from), Vec::<String>::new());
    // No Lead's or engine's hook ran for the tree: the engine was ensured once.
    assert_eq!(fx.calls("sh").len(), 1);
}

#[test]
fn tui_respawns_a_dead_pane_of_the_layout_in_place_and_sets_the_hook_again() {
    let fx = fx();
    tui(&fx, &[]);
    // A server from before the hook: a dead view stays dead until tui runs.
    out(&fx, &["set-hook", "-gu", "pane-died"]);
    let before = main_panes(&fx);
    let (tree, widgets) = (
        view(&before, "tree").clone(),
        view(&before, "widgets").clone(),
    );
    for pane in [&tree, &widgets] {
        fx.pane_exit(&pane.id, 137);
        assert!(fx.pane_dead(&pane.id), "the dead {} pane", pane.view);
    }

    let from = mark(&fx);
    let out_ = tui(&fx, &[]);
    assert_eq!(out_["built"], false, "{out_}");
    assert!(out_.get("respawned").is_none(), "{out_}");
    // Each dead pane of the layout is respawned in place, with the command of its view, and nothing else is made or sized.
    assert_eq!(
        steps(&fx, from),
        [
            format!("respawn-pane -t {} cpd tui view tree", tree.id),
            format!("respawn-pane -t {} cpd tui view widgets", widgets.id)
        ]
    );
    let after = main_panes(&fx);
    assert_eq!(ids(&after), ids(&before));
    assert!(after.iter().all(|p| !p.dead), "{after:?}");
    assert_ne!(view(&after, "tree").pid, tree.pid);
    assert_ne!(view(&after, "widgets").pid, widgets.pid);
    assert_eq!(argv_of(&fx, &tree.id), ["cpd", "tui", "view", "tree"]);
    // The hook is set again.
    assert_options_on_fake(&fx);
}

#[test]
fn tui_removes_an_old_statusbar_pane_then_resets_the_layout() {
    let fx = fx();
    tui(&fx, &[]);
    let full = main_panes(&fx);
    // A main built by an older cpd: the StatusBar pane at the bottom, the session without its status line, and the
    // sizes changed by the User.
    for option in ["status", "status-style", "status-format[0]"] {
        out(&fx, &["set", "-u", "-t", "=cpd:", option]);
    }
    let bar = shown(
        &fx,
        &[
            "split-window",
            "-d",
            "-v",
            "-f",
            "-l",
            "1",
            "-t",
            &full[0].id,
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "3600",
        ],
    );
    out(&fx, &["set", "-p", "-t", &bar, "@cpd_view", "statusbar"]);
    out(
        &fx,
        &["resize-pane", "-t", &view(&full, "tree").id, "-x", "80"],
    );
    let before = main_panes(&fx);
    assert_eq!(before.len(), 5, "{before:?}");

    let from = mark(&fx);
    let (result, stderr) = fx.with_stderr(|| tui(&fx, &[]));
    assert_eq!(result["built"], false, "{result}");
    assert!(
        !stderr.contains("statusbar"),
        "no warning about the old pane: {stderr}"
    );
    // The bar is killed, the others get their sizes back (the reset steps), and the status line is on.
    assert_eq!(
        steps(&fx, from),
        [
            format!("kill-pane -t {bar}"),
            "resize-pane -t %0 -x 20%".to_owned(),
            "resize-pane -t %2 -x 30%".to_owned(),
            "resize-pane -t %1 -y 30%".to_owned(),
        ]
    );
    let after = main_panes(&fx);
    assert_eq!(after.len(), 4, "{after:?}");
    assert_status_line(&fx);
    // They are the same panes, still alive.
    for old in before.iter().filter(|p| p.id != bar) {
        let new = after
            .iter()
            .find(|p| p.id == old.id)
            .unwrap_or_else(|| panic!("{} is gone: {after:?}", old.id));
        assert_eq!(
            (new.view.as_str(), new.pid, new.dead),
            (old.view.as_str(), old.pid, false)
        );
    }

    // Rerunning removes nothing more.
    let again = mark(&fx);
    tui(&fx, &[]);
    assert_eq!(main_panes(&fx), after);
    assert_eq!(steps(&fx, again), Vec::<String>::new());
}

#[test]
fn tui_removes_an_old_worker_pane_then_resets_the_layout() {
    let fx = fx();
    tui(&fx, &[]);
    let full = main_panes(&fx);
    // A main built by an older cpd: a Worker pane, and the sizes changed by the User.
    let bar = shown(
        &fx,
        &[
            "split-window",
            "-d",
            "-v",
            "-f",
            "-l",
            "1",
            "-t",
            &full[0].id,
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "3600",
        ],
    );
    out(&fx, &["set", "-p", "-t", &bar, "@cpd_view", "worker"]);
    out(
        &fx,
        &["resize-pane", "-t", &view(&full, "tree").id, "-x", "80"],
    );
    let before = main_panes(&fx);
    assert_eq!(before.len(), 5, "{before:?}");

    let from = mark(&fx);
    let (result, stderr) = fx.with_stderr(|| tui(&fx, &[]));
    assert_eq!(result["built"], false, "{result}");
    assert!(
        !stderr.contains("worker"),
        "no warning about the old pane: {stderr}"
    );
    // The bar is killed, the others get their sizes back (the reset steps), and the status line is on.
    assert_eq!(
        steps(&fx, from),
        [
            format!("kill-pane -t {bar}"),
            "resize-pane -t %0 -x 20%".to_owned(),
            "resize-pane -t %2 -x 30%".to_owned(),
            "resize-pane -t %1 -y 30%".to_owned(),
        ]
    );
    let after = main_panes(&fx);
    assert_eq!(after.len(), 4, "{after:?}");
    // They are the same panes, still alive.
    for old in before.iter().filter(|p| p.id != bar) {
        let new = after
            .iter()
            .find(|p| p.id == old.id)
            .unwrap_or_else(|| panic!("{} is gone: {after:?}", old.id));
        assert_eq!(
            (new.view.as_str(), new.pid, new.dead),
            (old.view.as_str(), old.pid, false)
        );
    }

    // Rerunning removes nothing more.
    let again = mark(&fx);
    tui(&fx, &[]);
    assert_eq!(main_panes(&fx), after);
    assert_eq!(steps(&fx, again), Vec::<String>::new());
}

/// Kills `gone` (views of `main`) after a build, runs `cpd tui`, and asserts it added them back where they go and sized the
/// layout again with `want` (the commands, in order, of that one run), the panes that were not killed unchanged, and that
/// rerunning adds nothing.
fn assert_repaired_after_killing(gone: &[&str], want: &[&str]) {
    let fx = fx();
    tui(&fx, &[]);
    let full = main_panes(&fx);
    for name in gone {
        out(&fx, &["kill-pane", "-t", &view(&full, name).id]);
    }
    // An old main, its sizes changed by the User.
    if !gone.contains(&"widgets") {
        out(
            &fx,
            &["resize-pane", "-t", &view(&full, "widgets").id, "-x", "80"],
        );
    }
    let before = main_panes(&fx);
    assert_eq!(before.len(), 4 - gone.len(), "{before:?}");

    let from = mark(&fx);
    let out_ = tui(&fx, &[]);
    assert_eq!(out_["built"], false, "{out_}");
    assert_eq!(steps(&fx, from), want);
    let after = main_panes(&fx);
    assert_eq!(after.len(), 4, "{after:?}");
    for v in ["tree", "plan", "widgets", "chat"] {
        assert_eq!(
            argv_of(&fx, &view(&after, v).id),
            ["cpd", "tui", "view", v],
            "{v}"
        );
    }
    assert!(
        after.iter().all(|p| !p.dead && p.view != "statusbar"),
        "{after:?}"
    );
    for old in &before {
        let new = after
            .iter()
            .find(|p| p.id == old.id)
            .unwrap_or_else(|| panic!("{} is gone: {after:?}", old.id));
        assert_eq!(
            (new.view.as_str(), new.pid, new.dead),
            (old.view.as_str(), old.pid, false)
        );
    }

    // Rerunning adds nothing and starts nothing.
    let again = mark(&fx);
    tui(&fx, &[]);
    assert_eq!(main_panes(&fx), after);
    assert_eq!(steps(&fx, again), Vec::<String>::new());
}

#[test]
fn tui_adds_a_missing_tree_to_an_existing_main() {
    assert_repaired_after_killing(
        &["tree"],
        &[
            "split-window -d -t %1 -h -b -f -l 20% cpd tui view tree",
            "set -p -t %4 @cpd_view tree",
            "resize-pane -t %4 -x 20%",
            "resize-pane -t %2 -x 30%",
            "resize-pane -t %1 -y 30%",
        ],
    );
}

#[test]
fn tui_adds_a_missing_plan_view_to_an_existing_main() {
    assert_repaired_after_killing(
        &["plan"],
        &[
            "split-window -d -t %3 -v -b -l 30% cpd tui view plan",
            "set -p -t %4 @cpd_view plan",
            "resize-pane -t %0 -x 20%",
            "resize-pane -t %2 -x 30%",
            "resize-pane -t %4 -y 30%",
        ],
    );
}

#[test]
fn tui_adds_a_missing_widgets_view_to_an_existing_main() {
    assert_repaired_after_killing(
        &["widgets"],
        &[
            "split-window -d -t %0 -h -f -l 30% cpd tui view widgets",
            "set -p -t %4 @cpd_view widgets",
            "resize-pane -t %0 -x 20%",
            "resize-pane -t %4 -x 30%",
            "resize-pane -t %1 -y 30%",
        ],
    );
}

#[test]
fn tui_adds_a_missing_chat_placeholder_to_an_existing_main() {
    assert_repaired_after_killing(
        &["chat"],
        &[
            "split-window -d -t %1 -v cpd tui view chat",
            "set -p -t %4 @cpd_view chat",
            "resize-pane -t %0 -x 20%",
            "resize-pane -t %2 -x 30%",
            "resize-pane -t %1 -y 30%",
        ],
    );
}

#[test]
fn tui_adds_the_plan_view_and_the_chat_slot_when_both_are_missing() {
    assert_repaired_after_killing(
        &["plan", "chat"],
        &[
            "split-window -d -t %0 -h cpd tui view plan",
            "set -p -t %4 @cpd_view plan",
            "split-window -d -t %4 -v cpd tui view chat",
            "set -p -t %5 @cpd_view chat",
            "resize-pane -t %0 -x 20%",
            "resize-pane -t %2 -x 30%",
            "resize-pane -t %4 -y 30%",
        ],
    );
}

#[test]
fn tui_finishes_a_build_that_stopped_after_the_first_pane() {
    let fx = fx();
    tui(&fx, &[]);
    let full = main_panes(&fx);
    let tree = view(&full, "tree").clone();
    for name in ["plan", "widgets", "chat"] {
        out(&fx, &["kill-pane", "-t", &view(&full, name).id]);
    }
    assert_eq!(main_panes(&fx).len(), 1);

    let from = mark(&fx);
    let out_ = tui(&fx, &[]);
    assert_eq!(out_["built"], false, "{out_}");
    // The widgets go to the window's right edge, the plan view beside the tree and the chat slot below it, then reset-layout.
    assert_eq!(
        steps(&fx, from),
        [
            "split-window -d -t %0 -h -f -l 30% cpd tui view widgets",
            "set -p -t %4 @cpd_view widgets",
            "split-window -d -t %0 -h cpd tui view plan",
            "set -p -t %5 @cpd_view plan",
            "split-window -d -t %5 -v cpd tui view chat",
            "set -p -t %6 @cpd_view chat",
            "resize-pane -t %0 -x 20%",
            "resize-pane -t %4 -x 30%",
            "resize-pane -t %5 -y 30%",
        ]
    );
    let after = main_panes(&fx);
    assert_eq!(after.len(), 4, "{after:?}");
    assert_eq!(
        view(&after, "tree"),
        &tree,
        "the pane that was there is not touched"
    );
    let again = mark(&fx);
    tui(&fx, &[]);
    assert_eq!(main_panes(&fx), after);
    assert_eq!(steps(&fx, again), Vec::<String>::new());
}

/// A Lead's pane (session `lead-a`, in its own window) that takes the chat slot `placeholder`'s place: `swap-pane` parks the
/// placeholder in the Lead's window. Returns the Lead's pane.
fn lead_takes_the_slot(fx: &Fx, placeholder: &str) -> String {
    let lead = shown(
        fx,
        &[
            "new-window",
            "-d",
            "-t",
            "=cpd:",
            "-n",
            "park-pay",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "3600",
        ],
    );
    out(fx, &["set", "-p", "-t", &lead, "@cpd_session", "lead-a"]);
    out(fx, &["swap-pane", "-d", "-s", &lead, "-t", placeholder]);
    lead
}

#[test]
fn tui_keeps_a_lead_in_the_chat_slot_when_the_plan_view_is_missing() {
    let fx = fx();
    tui(&fx, &[]);
    let placeholder = view(&main_panes(&fx), "chat").clone();
    let lead = lead_takes_the_slot(&fx, &placeholder.id);
    let full = main_panes(&fx);
    out(&fx, &["kill-pane", "-t", &view(&full, "plan").id]);

    let from = mark(&fx);
    tui(&fx, &[]);
    // The plan view is added above the Lead's pane (the slot), and no second chat slot is made.
    assert_eq!(
        steps(&fx, from),
        [
            format!("split-window -d -t {lead} -v -b -l 30% cpd tui view plan"),
            "set -p -t %5 @cpd_view plan".to_owned(),
            "resize-pane -t %0 -x 20%".to_owned(),
            "resize-pane -t %2 -x 30%".to_owned(),
            "resize-pane -t %5 -y 30%".to_owned(),
        ]
    );
    let after = main_panes(&fx);
    assert_eq!(after.len(), 4, "{after:?}");
    assert!(
        after.iter().any(|p| p.id == lead),
        "the Lead keeps the slot: {after:?}"
    );
    assert!(
        after.iter().all(|p| p.view != "chat"),
        "no second slot: {after:?}"
    );
}

#[test]
fn tui_action_reset_resizes_as_reset_layout_does() {
    let fx = fx();
    tui(&fx, &[]);
    let before = main_panes(&fx);
    out(
        &fx,
        &["resize-pane", "-t", &view(&before, "tree").id, "-x", "80"],
    );
    out(
        &fx,
        &["resize-pane", "-t", &view(&before, "plan").id, "-y", "30"],
    );

    let from = mark(&fx);
    action(&fx, &["reset", "--client", "/dev/pts/nobody"]);
    // The sizes reset-layout gave the new main: the same `resize-pane` commands as the build's, on the same panes.
    assert_eq!(
        steps(&fx, from),
        [
            "resize-pane -t %0 -x 20%",
            "resize-pane -t %2 -x 30%",
            "resize-pane -t %1 -y 30%"
        ]
    );
    assert_eq!(
        main_panes(&fx),
        before,
        "no pane was made, killed or restarted"
    );
}

#[test]
fn a_failing_action_prints_nothing_exits_0_and_tells_the_client() {
    let fx = fx();
    tui(&fx, &[]);
    let from = mark(&fx);

    // an unknown action, or none (an empty `#{mouse_status_range}`)
    action(&fx, &["nope", "--client", "/dev/pts/77"]);
    action(&fx, &["--client", "/dev/pts/77"]);
    // a client that is not attached: no size to open the popup at
    action(&fx, &["legend", "--client", "/dev/pts/77"]);
    action(&fx, &["logs", "--client", "/dev/pts/77"]);
    let calls = fx.calls("tmux");
    let messages: Vec<&Vec<String>> = calls[from..]
        .iter()
        .map(|c| &c.args)
        .filter(|c| c.iter().any(|a| a == "display-message"))
        .collect();
    assert_eq!(messages.len(), 4, "{messages:?}");
    for (message, says) in
        messages
            .iter()
            .zip(["\"nope\"", "\"\"", "no tmux client", "no tmux client"])
    {
        let at = message.iter().position(|a| a == "display-message").unwrap();
        assert_eq!(
            message[at + 1..at + 4],
            ["-l", "-c", "/dev/pts/77"],
            "{message:?}"
        );
        assert!(
            message[at + 4].starts_with("cpd tui action: ") && message[at + 4].contains(says),
            "{says}: {message:?}"
        );
    }
    assert!(fx.tmux_state().unwrap().popups().is_empty());
    assert!(
        calls[from..]
            .iter()
            .all(|c| !c.args.iter().any(|a| a == "display-popup"))
    );

    // with no tmux server at all, even the message fails: still nothing printed, exit 0
    fx.kill_tmux_server();
    action(&fx, &["reset", "--client", "/dev/pts/77"]);
}

/// What tmux runs for a click on the StatusBar's `range` by `client`: the binding's command with its two formats expanded
/// (tmux's part), as the arguments of `cpd tui` (the program's name and `tui` dropped, the redirect of stderr too).
fn click(range: &str, client: &str) -> Vec<String> {
    let command = STATUS_CLICK
        .replace("#{mouse_status_range}", range)
        .replace("#{client_name}", client);
    let words: Vec<&str> = command
        .strip_suffix(" 2>/dev/null")
        .expect("the click drops stderr")
        .split(' ')
        .collect();
    assert_eq!(&words[..2], ["cpd", "tui"], "{command}");
    words[2..].iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn a_click_on_a_statusbar_button_runs_its_action_on_the_clicking_client() {
    let fx = fx();
    tui(&fx, &[]);
    fx.tmux_route.fake().add_client("/dev/pts/0", 120, 30);
    let client = shown(&fx, &["list-clients", "-F", "#{client_name}"]);
    // The status line is the client's last row: the three buttons at the left, one `━` apart, each its own range.
    let format = shown(&fx, &["show", "-v", "-t", "=cpd:", "status-format[0]"]);
    assert!(format.starts_with(&buttons()), "{format}");
    // It takes one row: the session's status is on and tmux gives the other rows to the window.
    assert_eq!(shown(&fx, &["show", "-v", "-t", "=cpd:", "status"]), "on");
    let chat = view(&main_panes(&fx), "chat").clone();
    // A click on a button is the binding's `run-shell`, not a click on a pane: it selects no pane and sends no key.
    assert!(binding(&fx, "root", "MouseDown1Status").contains(" run-shell -b "));
    let ran_before = ran().len();
    let from = mark(&fx);

    // ? opens the legend on this client
    let args = click("legend", &client);
    assert_eq!(args, ["action", "legend", "--client", &client]);
    let args: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    action(&fx, &args);
    let popups = fx.tmux_state().unwrap().popups();
    assert_eq!(popups.len(), 1, "{popups:?}");
    assert_eq!(popups[0][..3], ["display-popup", "-c", &client]);
    assert!(
        popups[0].iter().any(|a| a == " legend ")
            && popups[0].last().unwrap().contains("cpd tui view legend"),
        "{:?}",
        popups[0]
    );
    // The click went to no pane: the chat keeps the focus and is not put into a mode.
    assert_eq!(
        shown(
            &fx,
            &[
                "display",
                "-p",
                "-t",
                "=cpd:main",
                "#{pane_id} #{pane_in_mode}"
            ]
        ),
        format!("{} 0", chat.id)
    );
    assert!(fx.tmux_state().unwrap().keys().is_empty());

    // the logs button opens the logs
    let args = click("logs", &client);
    action(
        &fx,
        &args[1..].iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let popups = fx.tmux_state().unwrap().popups();
    assert_eq!(popups.len(), 2, "{popups:?}");
    assert!(popups[1].iter().any(|a| a == " logs "), "{:?}", popups[1]);

    // the reset button resets the layout
    let tree = view(&main_panes(&fx), "tree").clone();
    out(&fx, &["resize-pane", "-t", &tree.id, "-x", "70"]);
    let before_reset = mark(&fx);
    let args = click("reset", &client);
    action(
        &fx,
        &args[1..].iter().map(String::as_str).collect::<Vec<_>>(),
    );
    assert_eq!(
        steps(&fx, before_reset),
        [
            "resize-pane -t %0 -x 20%",
            "resize-pane -t %2 -x 30%",
            "resize-pane -t %1 -y 30%"
        ]
    );
    // No pane was made, killed or selected by any of the three.
    assert!(
        steps(&fx, from)
            .iter()
            .all(|s| s.starts_with("resize-pane")),
        "{:?}",
        steps(&fx, from)
    );
    assert_eq!(ran().len(), ran_before, "the actions run no other command");
}

#[test]
fn a_pane_title_is_an_unpadded_pill_grey_and_blue_when_focused_three_columns_from_the_edge() {
    let fx = fx();
    tui(&fx, &[]);
    let panes = main_panes(&fx);
    let (tree, plan, widgets, chat) = (
        view(&panes, "tree").clone(),
        view(&panes, "plan").clone(),
        view(&panes, "widgets").clone(),
        view(&panes, "chat").clone(),
    );
    // A Lead in the chat slot, with a header (its Plan's title) and the keyboard focus.
    let lead = shown(
        &fx,
        &[
            "new-window",
            "-d",
            "-t",
            "=cpd:",
            "-n",
            "park-pay",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "3600",
        ],
    );
    out(&fx, &["set", "-p", "-t", &lead, "@cpd_session", "lead-a"]);
    out(
        &fx,
        &["set", "-p", "-t", &lead, "@cpd_title", "Retry Payouts"],
    );
    out(&fx, &["swap-pane", "-d", "-s", &lead, "-t", &chat.id]);
    out(&fx, &["select-pane", "-t", &lead]);
    assert!(
        main_panes(&fx)
            .iter()
            .any(|p| p.id == lead && p.session == "lead-a"),
        "the Lead is in the slot"
    );

    let border = shown(&fx, &["show", "-gv", "pane-border-format"]);
    // No padding inside the caps: the title between them, nothing around it; one cap on each side of every pill.
    assert!(
        border.contains("\u{e0b6}#[reverse]#{@cpd_title}#[noreverse]\u{e0b4}"),
        "{border}"
    );
    assert_eq!(border.matches('\u{e0b6}').count(), 2, "{border}");
    assert_eq!(border.matches('\u{e0b4}').count(), 2, "{border}");
    assert!(
        !border.contains("\u{e0b6}#[reverse] ") && !border.contains(" #[noreverse]"),
        "{border}"
    );
    // Every pill starts three columns from its pane's edge (hs-d478): tmux draws two border cells, then one `━` with no colour
    // of its own (it takes the border line's style, pro-d61), then the pill's colour, then the left cap.
    assert!(!border.contains("#[fg=colour7]\u{2501}"), "{border}");
    // The Lead's and another pane's pill is colour 7 (grey) and, on the focused pane, blue (colour 4): the format
    // itself says it (`pane_active`), whatever the border's colour is.
    assert!(
        border.contains(
            "\u{2501}#{?pane_active,#[fg=colour4],#[fg=colour7]}\u{e0b6}#[reverse]#{@cpd_title}"
        ),
        "{border}"
    );
    // The Lead's pane carries the header the pill shows.
    assert_eq!(
        shown(&fx, &["show", "-pv", "-t", &lead, "@cpd_title"]),
        "Retry Payouts"
    );

    // tmux expands the format of each pane to its pill: the title, and the colour of the focus.
    let pill = |pane: &str| shown(&fx, &["display", "-p", "-t", pane, "-F", &border]);
    let titled = [("Retry Payouts", lead.as_str())];
    for focused in [lead.as_str(), plan.id.as_str()] {
        out(&fx, &["select-pane", "-t", focused]);
        for (title, id) in titled {
            let colour = if id == focused { "colour4" } else { "colour7" };
            let want =
                format!("\u{2501}#[fg={colour}]\u{e0b6}#[reverse]{title}#[noreverse]\u{e0b4}");
            assert_eq!(pill(id), want, "{title} (focus on {focused})");
        }
    }
    let _ = (&tree, &widgets);
}

/// The live server option is wired: the tree pane expands to the cpd mark, the plan and widgets panes to nothing.
#[test]
fn the_built_cockpits_tree_pane_border_expands_to_the_cpd_mark_and_plan_and_widgets_to_nothing() {
    let fx = fx();
    tui(&fx, &[]);
    let panes = main_panes(&fx);
    let (tree, plan, widgets, chat) = (
        view(&panes, "tree").clone(),
        view(&panes, "plan").clone(),
        view(&panes, "widgets").clone(),
        view(&panes, "chat").clone(),
    );
    let lead = shown(
        &fx,
        &[
            "new-window",
            "-d",
            "-t",
            "=cpd:",
            "-n",
            "park-pay",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "3600",
        ],
    );
    out(&fx, &["set", "-p", "-t", &lead, "@cpd_session", "lead-a"]);
    out(
        &fx,
        &["set", "-p", "-t", &lead, "@cpd_title", "Retry Payouts"],
    );
    out(&fx, &["swap-pane", "-d", "-s", &lead, "-t", &chat.id]);
    let border = shown(&fx, &["show", "-gv", "pane-border-format"]);
    let expand = |pane: &str| shown(&fx, &["display", "-p", "-t", pane, "-F", &border]);
    for focused in [plan.id.as_str(), lead.as_str()] {
        out(&fx, &["select-pane", "-t", focused]);
        let mark = expand(&tree.id);
        let at = |needle: &str| {
            mark.find(needle)
                .unwrap_or_else(|| panic!("{needle} in {mark:?} (focus on {focused})"))
        };
        let (c, p, d) = (
            at("#[fg=colour13]\u{f0bf2}"),
            at("#[fg=colour15]\u{f0c19}"),
            at("#[fg=colour10]\u{f0bf5}"),
        );
        assert!(c < p && p < d, "{mark:?}");
        assert!(
            !mark.contains("Context") && !mark.contains('\u{e0b6}') && !mark.contains("reverse"),
            "{mark:?}"
        );
        assert_eq!(expand(&plan.id), "", "plan (focus on {focused})");
        assert_eq!(expand(&widgets.id), "", "widgets (focus on {focused})");
    }
}

/// hs-d489: the border between two unfocused panes is grey (colour 7), the one beside the focused pane is blue.
#[test]
fn a_pane_border_is_grey_and_the_focused_panes_border_is_blue() {
    let fx = fx();
    tui(&fx, &[]);
    let panes = main_panes(&fx);
    let tree = view(&panes, "tree").clone();
    out(&fx, &["select-pane", "-t", &tree.id]);
    assert_eq!(
        shown(&fx, &["display", "-p", "-t", "=cpd:main", "#{pane_id}"]),
        tree.id
    );
    // The border of an unfocused pane is colour 7 (hs-d530 R4), the focused pane's blue: tmux draws each from these options.
    assert_eq!(
        shown(&fx, &["show", "-gv", "pane-border-style"]),
        "fg=colour7"
    );
    assert_eq!(
        shown(&fx, &["show", "-gv", "pane-active-border-style"]),
        "fg=blue"
    );
    // A heavy line, with the title on its top row.
    assert_eq!(shown(&fx, &["show", "-gv", "pane-border-lines"]), "heavy");
    assert_eq!(shown(&fx, &["show", "-gv", "pane-border-status"]), "top");
}

#[test]
fn tui_respawn_restarts_every_live_view_and_no_lead_or_engine() {
    let fx = fx();
    let out_ = tui(&fx, &["--respawn"]);
    assert_eq!(
        (&out_["built"], &out_["respawned"]),
        (&json!(true), &json!([])),
        "a new cockpit restarts nothing: {out_}"
    );
    let engine = start_engine_pane(&fx);
    let engine_pid = pid_of(&fx, &engine);

    // A Lead takes the chat slot and its placeholder is parked.
    let placeholder = view(&main_panes(&fx), "chat").clone();
    let lead = lead_takes_the_slot(&fx, &placeholder.id);
    let before = main_panes(&fx);
    let lead_pid = before.iter().find(|p| p.id == lead).unwrap().pid;

    let from = mark(&fx);
    let out_ = tui(&fx, &["--respawn"]);
    assert_eq!(out_["built"], false, "{out_}");
    let mut respawned: Vec<String> = serde_json::from_value(out_["respawned"].clone()).unwrap();
    respawned.sort();
    let mut views: Vec<String> = before
        .iter()
        .filter(|p| !p.view.is_empty())
        .map(|p| p.id.clone())
        .collect();
    views.sort();
    assert_eq!(views.len(), 3, "tree, plan and widgets: {before:?}");
    assert_eq!(respawned, views, "{before:?}");
    // Each live view is restarted in place, killing its process (`respawn-pane -k`), and nothing is made, killed or sized.
    let mut restarts = steps(&fx, from);
    restarts.sort();
    let mut want: Vec<String> = views
        .iter()
        .map(|id| format!("respawn-pane -k -t {id}"))
        .collect();
    want.sort();
    assert_eq!(restarts, want);

    let after = main_panes(&fx);
    assert_eq!(ids(&after), ids(&before));
    for (old, new) in before.iter().zip(&after) {
        assert!(!new.dead, "{after:?}");
        assert_eq!(new.view, old.view);
        if old.view.is_empty() {
            assert_eq!(new.pid, lead_pid, "the Lead is never touched");
        } else {
            assert_ne!(new.pid, old.pid, "{} runs again", old.view);
        }
    }
    // Each view reruns its own command: the tree its view.
    assert_eq!(
        argv_of(&fx, &view(&after, "tree").id),
        ["cpd", "tui", "view", "tree"]
    );
    // The parked placeholder (not in main) and the engine are not touched.
    assert_eq!(pid_of(&fx, &placeholder.id), placeholder.pid);
    assert!(!fx.pane_dead(&placeholder.id));
    assert_eq!(
        (fx.pane_dead(&engine), pid_of(&fx, &engine)),
        (false, engine_pid)
    );
}

/// `#{pane_pid}` of `pane`.
fn pid_of(fx: &Fx, pane: &str) -> u32 {
    shown(fx, &["display", "-p", "-t", pane, "#{pane_pid}"])
        .parse()
        .expect("a pid")
}

#[test]
fn tui_brings_the_last_chat_lead_into_a_new_slot() {
    let fx = fx();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    // lead-b was in the chat last among Leads with a Plan; ground-c, a Lead with
    // no Plan (what migration 0010 leaves of a grounded Lead), came later but is
    // never restored there.
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('lead-a', 'lead', 'ctx', 'pay', 'A', '/', 'O3', 'stopped', 't', 't'),
           ('lead-b', 'lead', 'ctx', 'pay', 'B', '/', 'O3', 'stopped', 't', 't'),
           ('ground-c', 'lead', 'ctx', NULL, 'C', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead-a', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'repo', '/', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
         INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-a', 'x', '{\"place\": \"chat\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-b', 'x', '{\"place\": \"chat\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-b', 'x', '{\"place\": \"parked\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', NULL, 'ground-c', 'x', '{\"place\": \"chat\"}');",
    );
    out(
        &fx,
        &[
            "new-session",
            "-d",
            "-s",
            "cpd",
            "-n",
            "park-pay",
            "-x",
            "215",
            "-y",
            "62",
            "sleep",
            "3600",
        ],
    );
    let lead_pane = shown(&fx, &["display", "-p", "-t", "=cpd:park-pay", "#{pane_id}"]);
    out(
        &fx,
        &["set", "-p", "-t", &lead_pane, "@cpd_session", "lead-b"],
    );

    let out_ = tui(&fx, &[]);
    assert_eq!(out_["built"], true, "{out_}");
    // The cockpit runs `session open lead-b` for the Lead of the Event, and no other, then starts the engine.
    assert_eq!(
        ran().iter().map(|argv| argv.join(" ")).collect::<Vec<_>>(),
        ["cpd session open lead-b", "cpd watch --ensure"]
    );
    let panes = main_panes(&fx);
    assert_eq!(panes.len(), 4, "{panes:?}");
    // The Lead's own pane took the slot; the placeholder went, with its park window.
    let slot = panes
        .iter()
        .find(|p| p.session == "lead-b")
        .unwrap_or_else(|| panic!("{panes:?}"));
    assert_eq!(slot.id, lead_pane);
    assert!(panes.iter().all(|p| p.view != "chat"), "{panes:?}");
    assert_eq!(window_names(&fx, "cpd"), ["main"]);
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
/// The older server is the route's answer: `display -p #{version}` says `1.0` once the test arms it (`-V`, the installed
/// tmux, is the route's `3.6a`).
#[test]
fn tui_marks_a_restart_of_an_older_tmux_server_pending_and_does_not_kill_it() {
    let fx = fx();
    let older = Arc::new(AtomicBool::new(false));
    let says = older.clone();
    let _ = (*fx.fakes.runner).clone().on(
        "tmux",
        move |call| {
            says.load(Ordering::SeqCst)
                && call
                    .args
                    .ends_with(&["display".into(), "-p".into(), "#{version}".into()])
        },
        Reply::ok("1.0\n"),
    );
    let pending = fx.env.cpd_home().join("run/tmux-restart.pending");

    let (built, stderr) = fx.with_stderr(|| tui(&fx, &[]));
    assert_eq!(built["built"], true);
    assert!(
        !pending.exists(),
        "a server as new as the installed tmux needs no restart"
    );
    assert_eq!(stderr, "");
    let pid = shown(&fx, &["display", "-p", "#{pid}"]);
    older.store(true, Ordering::SeqCst);
    let (run, stderr) = fx.with_stderr(|| tui(&fx, &[]));
    assert_eq!(run["built"], false);
    // Said once on stderr, with what happens next: the engine restarts it.
    assert_eq!(
        stderr,
        "cpd tui: the tmux server runs an older tmux than the installed one; the restart happens once no Attempt runs.\n"
    );
    let marked: Value = serde_json::from_str(&fs::read_to_string(&pending).unwrap()).unwrap();
    assert_eq!(
        (
            marked["server"].as_str(),
            marked["installed"].as_str(),
            marked["failed"].is_null()
        ),
        (Some("1.0"), Some("3.6a"), true)
    );
    let at = marked["at"].clone();
    // Again: still pending, with its first time; and nothing was killed.
    tui(&fx, &[]);
    let again: Value = serde_json::from_str(&fs::read_to_string(&pending).unwrap()).unwrap();
    assert_eq!(again["at"], at);
    assert!(
        !fx.calls("tmux")
            .iter()
            .any(|c| c.args.iter().any(|a| a == "kill-server"))
    );
    assert_eq!(shown(&fx, &["display", "-p", "#{pid}"]), pid);
}

/// The preview build the views keep until `preview-tui.sh off` (the server's `@cpd_preview`).
const PREVIEW: &str = "cpd.tui-visuals.box-order 6c43886";

#[test]
fn tui_respawn_warns_on_stderr_while_a_preview_is_on_and_not_otherwise() {
    let fx = fx();
    let (quiet, stderr) = fx.with_stderr(|| tui(&fx, &["--respawn"]));
    assert_eq!(quiet["built"], true, "{quiet}");
    assert!(!stderr.contains("preview"), "{stderr}");

    out(&fx, &["set", "-g", "@cpd_preview", PREVIEW]);
    let (warned, stderr) = fx.with_stderr(|| tui(&fx, &["--respawn"]));
    assert_eq!(warned["built"], false, "{warned}");
    assert!(
        stderr.contains(&format!("a preview is on ({PREVIEW}); its views keep the preview build until 'preview-tui.sh off'")),
        "{stderr}"
    );
    // Without --respawn nothing is said; the option's JSON does not change.
    let (_, stderr) = fx.with_stderr(|| tui(&fx, &[]));
    assert!(!stderr.contains("preview"), "{stderr}");

    out(&fx, &["set", "-gu", "@cpd_preview"]);
    let (_, stderr) = fx.with_stderr(|| tui(&fx, &["--respawn"]));
    assert!(!stderr.contains("preview"), "{stderr}");
}

#[test]
fn a_server_started_by_plan_new_has_every_server_option() {
    let fx = Fx::with_ports();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay"])
        .ok();
    assert_eq!(
        window_names(&fx, "cpd"),
        ["park-pay"],
        "plan new started the server, not tui"
    );
    assert_options_on_fake(&fx);
}

#[test]
fn tui_attach_without_a_terminal_is_refused_before_anything_is_built() {
    let fx = common::tree::fx();
    let run = fx.cpd_without_a_terminal(&["tui", "--attach"]);
    assert_eq!(run.exit, 3, "{run:?}");
    assert_eq!(run.error()["code"], "not_a_terminal");
    assert!(
        !fx.tmux(&["list-sessions"]).status.success(),
        "a tmux server was started"
    );
    assert!(fx.calls("cpd").is_empty());
}

#[test]
fn tui_view_routes_by_name() {
    let fx = common::tree::fx();
    let run = fx.cpd_without_a_terminal(&["tui", "view", "chat"]);
    assert_eq!(
        (run.exit, run.error()["code"].clone()),
        (3, json!("not_a_terminal")),
        "{run:?}"
    );
    for words in [
        &["tui", "view", "nope"][..],
        &["tui", "view"],
        &["tui", "--bogus"],
    ] {
        let run = fx.cpd(words);
        assert_eq!(
            (run.exit, run.error()["code"].clone()),
            (2, json!("usage")),
            "{words:?}: {run:?}"
        );
    }
}

/// The left group (hs-d479): the leading `━`, then each button as a pill (its glyph and one space between the two caps, 4 cells),

#[test]
fn an_unfaked_gh_is_denied_with_exit_97() {
    let env = TestEnv::new();
    for tool in ["gh", "pi", "curl", "wget", "ssh"] {
        let out = common::output(env.command(tool).arg("x")).unwrap();
        assert_eq!(out.status.code(), Some(97), "{tool}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            format!("no fake installed for {tool}\n")
        );
    }
    env.fake_gh("echo faked");
    let out = common::output(&mut env.command("gh")).unwrap();
    assert_eq!(
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).trim()
        ),
        (Some(0), "faked")
    );
}

const AMBIENT: [&str; 10] = [
    "GH_TOKEN",
    "GH_HOST",
    "GITHUB_TOKEN",
    "SSH_AUTH_SOCK",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// Runs [`the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command`] in a child
/// test process that has them set (the harness reads the process's own environment).
#[test]
fn a_command_has_no_gh_ssh_or_proxy_variable() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child.args([
        "--ignored",
        "--exact",
        "tui::the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command",
    ]);
    for name in AMBIENT {
        child.env(name, "leak");
    }
    let out = common::output(&mut child).unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "run by a_command_has_no_gh_ssh_or_proxy_variable, with the variables set"]
fn the_ambient_gh_ssh_and_proxy_variables_do_not_reach_a_command() {
    for name in AMBIENT {
        assert_eq!(
            std::env::var(name).as_deref(),
            Ok("leak"),
            "the parent test sets {name}"
        );
    }
    let env = TestEnv::new();
    let text = String::from_utf8(common::output(&mut env.command("env")).unwrap().stdout).unwrap();
    let leaked: Vec<&str> = text
        .lines()
        .filter(|l| AMBIENT.iter().any(|n| l.starts_with(&format!("{n}="))))
        .collect();
    assert!(leaked.is_empty(), "{leaked:?}");
}
