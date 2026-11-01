//! The protocol gaps the product reads (research tests-mock-tmux-route.md, section 2: G1-G4 and the variables of G8).

use super::kit::M;
use std::os::unix::fs::MetadataExt;

fn display(m: &M, target: &str, format: &str) -> String {
    m.ok(&["display-message", "-p", "-t", target, format])
}

// ---- G1: `-V` ----

#[test]
fn dash_u_dash_capital_v_prints_the_version_line() {
    let m = M::new();
    let out = m.tmux.run(&m.invocation(vec!["-u".into(), "-V".into()]));
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "tmux 3.6a\n", "")
    );
}

#[test]
fn dash_capital_v_needs_no_server_and_creates_none() {
    let m = M::new();
    m.tmux.run(&m.invocation(vec!["-u".into(), "-V".into()]));
    assert_eq!(m.t(&["list-sessions"]).code, 1);
}

// ---- G2: `display-message -l` ----

#[test]
fn display_message_dash_l_is_accepted() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/7", 200, 60);
    let out = m.t(&["display-message", "-l", "-c", "/dev/pts/7", "hello"]);
    assert_eq!((out.code, out.stderr.as_str()), (0, ""));
}

// ---- G3: the size of a client ----

#[test]
fn a_client_added_with_a_size_answers_client_width_and_client_height() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/9", 200, 60);
    assert_eq!(
        m.ok(&[
            "display",
            "-p",
            "-c",
            "/dev/pts/9",
            "#{client_width} #{client_height}"
        ]),
        "200 60\n"
    );
}

#[test]
fn two_clients_answer_each_their_own_size() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/1", 100, 30);
    m.tmux.add_client("/dev/pts/2", 200, 60);
    assert_eq!(
        m.ok(&[
            "display",
            "-p",
            "-c",
            "/dev/pts/2",
            "#{client_width}x#{client_height}"
        ]),
        "200x60\n"
    );
    assert_eq!(
        m.ok(&[
            "display",
            "-p",
            "-c",
            "/dev/pts/1",
            "#{client_width}x#{client_height}"
        ]),
        "100x30\n"
    );
}

#[test]
fn a_client_that_was_not_added_gives_an_empty_line_and_exit_0() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/9", 200, 60);
    let out = m.t(&[
        "display",
        "-p",
        "-c",
        "/dev/pts/404",
        "#{client_width} #{client_height}",
    ]);
    assert_eq!((out.code, out.stdout.as_str()), (0, "\n"));
}

#[test]
fn the_client_name_variable_is_the_name_the_client_was_added_with() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/3", 80, 24);
    assert_eq!(
        m.ok(&["display", "-p", "-c", "/dev/pts/3", "#{client_name}"]),
        "/dev/pts/3\n"
    );
}

// ---- G4: list-clients and client_activity ----

#[test]
fn list_clients_lists_the_added_clients_with_their_sizes_in_the_order_they_were_added() {
    let m = M::new();
    m.server();
    m.tmux.add_client("/dev/pts/1", 100, 30);
    m.tmux.add_client("/dev/pts/2", 200, 60);
    let out = m.ok(&[
        "list-clients",
        "-F",
        "#{client_name} #{client_width} #{client_height}",
    ]);
    assert_eq!(out, "/dev/pts/1 100 30\n/dev/pts/2 200 60\n");
}

fn activities(out: &str) -> Vec<(u64, String)> {
    out.lines()
        .map(|l| {
            let (n, name) = l
                .split_once(' ')
                .unwrap_or_else(|| panic!("no activity in {l:?}"));
            (
                n.parse()
                    .unwrap_or_else(|_| panic!("activity is not a number in {l:?}")),
                name.to_owned(),
            )
        })
        .collect()
}

#[test]
fn client_activity_grows_with_the_order_clients_were_added() {
    let m = M::new();
    m.server();
    for (i, name) in ["/dev/pts/1", "/dev/pts/2", "/dev/pts/3"]
        .into_iter()
        .enumerate()
    {
        m.tmux.add_client(name, 80 + i as u32, 24);
    }
    let seen = activities(&m.ok(&["list-clients", "-F", "#{client_activity} #{client_name}"]));
    assert_eq!(
        seen.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(),
        ["/dev/pts/1", "/dev/pts/2", "/dev/pts/3"]
    );
    assert!(
        seen.windows(2).all(|w| w[0].0 < w[1].0),
        "the newest client is the last: {seen:?}"
    );
}

#[test]
fn client_activity_grows_with_the_order_clients_attached() {
    let m = M::new();
    m.server();
    m.ok(&["attach", "-t", "=s"]);
    m.ok(&["attach", "-t", "=s"]);
    let seen = activities(&m.ok(&["list-clients", "-F", "#{client_activity} #{client_name}"]));
    assert_eq!(seen.len(), 2);
    assert!(
        seen[0].0 < seen[1].0,
        "the second attach is the newest: {seen:?}"
    );
}

#[test]
fn list_clients_prints_nothing_for_a_server_without_clients() {
    let m = M::new();
    m.server();
    assert_eq!(m.ok(&["list-clients", "-F", "#{client_name}"]), "");
}

// ---- G8: the format variables the tests read and the fake lacked ----

#[test]
fn mouse_any_flag_is_0_for_a_pane_nobody_set_it_on() {
    let m = M::new();
    m.server();
    assert_eq!(display(&m, "%0", "#{mouse_any_flag}"), "0\n");
}

#[test]
fn alternate_on_is_0_for_a_pane_that_runs_nothing() {
    let m = M::new();
    m.server();
    assert_eq!(display(&m, "%0", "#{alternate_on}"), "0\n");
}

#[test]
fn pane_in_mode_is_0_for_a_pane_in_no_mode() {
    let m = M::new();
    m.server();
    assert_eq!(display(&m, "%0", "#{pane_in_mode}"), "0\n");
}

#[test]
fn pane_active_is_1_for_the_current_pane_of_the_window_and_0_for_the_others() {
    let m = M::new();
    m.server();
    m.ok(&["split-window", "-d", "-t", "%0", "-h"]);
    assert_eq!(
        m.ok(&[
            "list-panes",
            "-t",
            "=s:w",
            "-F",
            "#{pane_id}:#{pane_active}"
        ]),
        "%0:1\n%1:0\n"
    );
}

#[test]
fn pane_active_follows_select_pane() {
    let m = M::new();
    m.server();
    m.ok(&["split-window", "-d", "-t", "%0", "-h"]);
    m.ok(&["select-pane", "-t", "%1"]);
    assert_eq!(
        m.ok(&[
            "list-panes",
            "-t",
            "=s:w",
            "-F",
            "#{pane_id}:#{pane_active}"
        ]),
        "%0:0\n%1:1\n"
    );
}

#[test]
fn pane_current_path_is_the_directory_the_pane_started_in() {
    let m = M::new();
    m.ok(&["new-session", "-d", "-s", "s", "-c", "/work/here"]);
    assert_eq!(display(&m, "%0", "#{pane_current_path}"), "/work/here\n");
}

#[test]
fn pane_start_command_is_the_command_the_pane_was_started_with() {
    let m = M::new();
    m.ok(&["new-session", "-d", "-s", "s", "--", "top"]);
    assert_eq!(display(&m, "%0", "#{pane_start_command}"), "top\n");
}

#[test]
fn pane_current_command_is_the_file_name_of_the_program_of_the_pane() {
    let m = M::new();
    m.ok(&["new-session", "-d", "-s", "s", "--", "/usr/bin/top"]);
    assert_eq!(display(&m, "%0", "#{pane_current_command}"), "top\n");
}

#[test]
fn window_width_and_window_height_are_the_size_the_session_was_created_with() {
    let m = M::new();
    m.ok(&["new-session", "-d", "-s", "s", "-x", "215", "-y", "62"]);
    assert_eq!(
        display(&m, "=s:", "#{window_width}x#{window_height}"),
        "215x62\n"
    );
}

#[test]
fn socket_path_is_the_path_a_missing_server_is_reported_at() {
    let m = M::new();
    m.server();
    let uid = std::fs::metadata(m.dir.path()).unwrap().uid();
    assert_eq!(
        display(&m, "%0", "#{socket_path}"),
        format!("{}/tmux-{uid}/t\n", m.dir.path().display())
    );
}
