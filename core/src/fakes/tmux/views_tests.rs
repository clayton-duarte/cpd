//! The commands the views and the `tui action`s of the cockpit send, one test each (the list is in the module doc,
//! "What the views and actions need"): a client, its size and a popup on it, the clipboard buffer, the `@cpd_session` of
//! a pane, the chat slot's `select-pane` and the Worker pane's `split-window` and `respawn-pane`; and a popup that fails.

use super::TmuxRoute;
use super::kit::M;
use crate::clock::ManualClock;
use crate::host::runner::{FakeRunner, Reply};
use crate::host::tmux::Tmux;
use std::sync::Arc;

#[test]
fn list_clients_and_display_dash_p_dash_c_answer_each_clients_name_and_size() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/1", 100, 30);
    m.tmux.add_client("/dev/pts/2", 200, 60);
    assert_eq!(
        m.ok(&[
            "list-clients",
            "-F",
            "#{client_name} #{client_width} #{client_height}"
        ]),
        "/dev/pts/1 100 30\n/dev/pts/2 200 60\n"
    );
    assert_eq!(
        m.ok(&[
            "display",
            "-p",
            "-c",
            "/dev/pts/2",
            "#{client_width} #{client_height}"
        ]),
        "200 60\n"
    );
}

#[test]
fn display_popup_is_recorded_with_its_whole_argv_and_exits_0() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/1", 100, 30);
    let argv = [
        "display-popup",
        "-c",
        "/dev/pts/1",
        "-E",
        "-w",
        "50",
        "-h",
        "7",
        "cpd tui view dialog text --out='/tmp/o'",
    ];
    assert_eq!(m.t(&argv).code, 0);
    assert_eq!(
        m.tmux.state("t").popups(),
        [argv.map(str::to_owned).to_vec()]
    );
}

#[test]
fn a_display_popup_a_runner_rule_fails_exits_1_and_records_nothing() {
    // The failure is injected where every external failure is: a rule of the `FakeRunner`, ahead of the route.
    let dir = tempfile::tempdir().unwrap();
    let route = Arc::new(TmuxRoute::new());
    let runner = Arc::new(
        FakeRunner::new(ManualClock::new())
            .on(
                "tmux",
                |call| call.args.iter().any(|a| a == "display-popup"),
                Reply::exit(1, "", "no popup\n"),
            )
            .route("tmux", route.clone()),
    );
    let tmux = Tmux::with_tmpdir("t", dir.path()).with_runner(runner.clone());
    tmux.run(&["new-session", "-d", "-s", "s", "-n", "w"].map(str::to_owned))
        .unwrap();
    assert!(
        tmux.run(&["display-popup", "-E", "cpd tui view dialog text"].map(str::to_owned))
            .is_err()
    );
    assert!(route.fake().state("t").popups().is_empty());
    // the call is still the runner's to read, and any other command goes on to the fake
    assert_eq!(runner.calls().len(), 2);
    assert!(tmux.run(&["list-sessions"].map(str::to_owned)).is_ok());
}

#[test]
fn set_buffer_dash_w_stores_the_text_after_the_double_dash() {
    let m = M::new();
    m.server();
    assert_eq!(
        m.t(&["set-buffer", "-w", "--", "https://github.com/o/r/pull/7"])
            .code,
        0
    );
    assert_eq!(m.ok(&["show-buffer"]), "https://github.com/o/r/pull/7");
}

#[test]
fn list_panes_prints_the_cpd_session_of_a_pane_that_has_one() {
    let m = M::new();
    m.server();
    let pane = m
        .ok(&[
            "split-window",
            "-d",
            "-t",
            "%0",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep 1",
        ])
        .trim()
        .to_owned();
    m.ok(&["set", "-p", "-t", &pane, "@cpd_session", "l-ship"]);
    assert_eq!(
        m.ok(&["list-panes", "-a", "-F", "#{pane_id} #{@cpd_session}"]),
        format!("%0 \n{pane} l-ship\n")
    );
}

#[test]
fn select_pane_makes_the_pane_the_windows_active_one() {
    let m = M::new();
    m.server();
    let pane = m
        .ok(&[
            "split-window",
            "-d",
            "-t",
            "%0",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep 1",
        ])
        .trim()
        .to_owned();
    assert_eq!(
        m.ok(&["display", "-p", "-t", &pane, "#{pane_active}"]),
        "0\n"
    );
    assert_eq!(m.t(&["select-pane", "-t", &pane]).code, 0);
    assert_eq!(
        m.ok(&["display", "-p", "-t", &pane, "#{pane_active}"]),
        "1\n"
    );
}

#[test]
fn respawn_pane_dash_k_restarts_the_pane_with_a_new_command() {
    let m = M::new();
    m.server();
    let pane = m
        .ok(&[
            "split-window",
            "-d",
            "-t",
            "%0",
            "-P",
            "-F",
            "#{pane_id}",
            "cpd tui view worker",
        ])
        .trim()
        .to_owned();
    assert_eq!(
        m.t(&["respawn-pane", "-k", "-t", &pane, "cpd tui view chat"])
            .code,
        0
    );
    assert_eq!(
        m.ok(&["display", "-p", "-t", &pane, "#{pane_start_command}"]),
        "cpd tui view chat\n"
    );
}

#[test]
fn join_pane_is_not_modelled_because_the_product_never_sends_it() {
    // The chat slot moves with `swap-pane` (`Tmux::swap_pane`); `rg join-pane` over core, tui, engine and cli finds none.
    let m = M::new();
    m.server();
    let out = m.t(&["join-pane", "-s", "%0", "-t", "%0"]);
    assert_eq!(
        (out.code, out.stderr.as_str()),
        (1, "unknown command: join-pane\n")
    );
}
