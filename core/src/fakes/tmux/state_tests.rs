//! The in-memory state of `FakeTmux`: one connection per `(TMUX_TMPDIR, socket)` per value, dropped by
//! `kill-server`, read back through `state(socket)`; and the `Backing::File` path the bin keeps.

use super::kit::M;
use super::state::{SessionRow, db_path};
use super::{Backing, FakeTmux, run_commands};
use std::os::unix::fs::MetadataExt;

fn no_server_on(m: &M, socket: &str) -> String {
    let uid = std::fs::metadata(m.dir.path()).unwrap().uid();
    format!(
        "no server running on {}/tmux-{uid}/{socket}\n",
        m.dir.path().display()
    )
}

#[test]
fn two_fake_tmux_values_do_not_share_state() {
    let (a, b) = (M::new(), FakeTmux::new());
    a.ok(&["new-session", "-d", "-s", "s"]);
    let inv = a.invocation(M::argv("t", &["list-sessions"]));
    let seen = b.run(&inv);
    assert_eq!(
        (seen.code, seen.stdout, seen.stderr),
        (1, String::new(), no_server_on(&a, "t"))
    );
}

#[test]
fn a_session_created_on_one_socket_is_invisible_on_another_socket() {
    let m = M::new();
    m.on("a", &["new-session", "-d", "-s", "only-on-a"]);
    let other = m.on("b", &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!((other.code, other.stderr), (1, no_server_on(&m, "b")));
    assert_eq!(
        m.on("a", &["list-sessions", "-F", "#{session_name}"])
            .stdout,
        "only-on-a\n"
    );
}

#[test]
fn a_session_created_in_one_tmux_tmpdir_is_invisible_in_another() {
    let (m, elsewhere) = (M::new(), tempfile::tempdir().unwrap());
    m.on("t", &["new-session", "-d", "-s", "s"]);
    let mut inv = m.invocation(M::argv("t", &["list-sessions"]));
    inv.tmux_tmpdir = elsewhere.path().to_path_buf();
    let out = m.tmux.run(&inv);
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.starts_with(&format!(
            "no server running on {}/",
            elsewhere.path().display()
        )),
        "{}",
        out.stderr
    );
}

#[test]
fn kill_server_drops_the_state_so_list_sessions_fails_like_a_missing_server() {
    let m = M::new();
    m.server();
    assert_eq!(m.t(&["kill-server"]).code, 0);
    let after = m.t(&["list-sessions"]);
    assert_eq!(
        (after.code, after.stdout, after.stderr),
        (1, String::new(), no_server_on(&m, "t"))
    );
}

#[test]
fn a_server_started_after_kill_server_counts_ids_from_zero_again() {
    let m = M::new();
    m.server();
    m.t(&["kill-server"]);
    assert_eq!(
        m.ok(&[
            "new-session",
            "-d",
            "-s",
            "again",
            "-P",
            "-F",
            "#{window_id} #{pane_id}"
        ]),
        "@0 %0\n"
    );
}

#[test]
fn kill_server_on_one_socket_leaves_the_other_socket_running() {
    let m = M::new();
    m.on("a", &["new-session", "-d", "-s", "x"]);
    m.on("b", &["new-session", "-d", "-s", "y"]);
    m.on("a", &["kill-server"]);
    assert_eq!(
        m.on("b", &["list-sessions", "-F", "#{session_name}"])
            .stdout,
        "y\n"
    );
}

#[test]
fn state_shows_the_session_and_window_a_new_session_created() {
    let m = M::new();
    m.ok(&["new-session", "-d", "-s", "n", "-n", "w"]);
    let state = m.tmux.state("t");
    assert_eq!(
        state.sessions(),
        [SessionRow {
            id: "$0".into(),
            name: "n".into()
        }]
    );
    let windows = state.windows();
    assert_eq!(
        (
            windows.len(),
            windows[0].id.as_str(),
            windows[0].session.as_str(),
            windows[0].name.as_str()
        ),
        (1, "@0", "$0", "w")
    );
}

#[test]
fn state_shows_the_argv_cwd_and_env_a_new_session_gave_its_pane() {
    let m = M::new();
    m.ok(&[
        "new-session",
        "-d",
        "-s",
        "n",
        "-n",
        "w",
        "-c",
        "/work",
        "-e",
        "K=V",
        "--",
        "sh",
        "-c",
        "echo hi",
    ]);
    let state = m.tmux.state("t");
    let pane = &state.panes()[0];
    assert_eq!((pane.id.as_str(), pane.cwd.as_str()), ("%0", "/work"));
    assert_eq!(pane.argv, ["sh", "-c", "echo hi"]);
    assert!(
        state
            .pane_env("%0")
            .contains(&("K".to_string(), "V".to_string()))
    );
}

#[test]
fn state_shows_the_window_and_pane_a_new_window_created() {
    let m = M::new();
    m.server();
    assert_eq!(
        m.ok(&[
            "new-window",
            "-d",
            "-t",
            "=s:",
            "-n",
            "x",
            "-P",
            "-F",
            "#{window_id} #{pane_id}"
        ]),
        "@1 %1\n"
    );
    let state = m.tmux.state("t");
    let names: Vec<_> = state.windows().into_iter().map(|w| w.name).collect();
    assert_eq!(names, ["w", "x"]);
    assert_eq!(
        state
            .panes()
            .iter()
            .map(|p| (p.id.as_str(), p.window.as_str()))
            .collect::<Vec<_>>(),
        [("%0", "@0"), ("%1", "@1")]
    );
}

#[test]
fn state_shows_the_pane_option_set_p_stored() {
    let m = M::new();
    m.server();
    m.ok(&["set", "-p", "-t", "%0", "@cpd_view", "tree"]);
    assert_eq!(
        m.tmux.state("t").panes()[0].options,
        [("@cpd_view".to_string(), "tree".to_string())]
    );
}

#[test]
fn state_shows_the_server_option_set_g_stored() {
    let m = M::new();
    m.server();
    m.ok(&["set", "-g", "@cpd_version", "1.2.3"]);
    assert_eq!(
        m.tmux.state("t").options(),
        [("@cpd_version".to_string(), "1.2.3".to_string())]
    );
}

#[test]
fn state_of_one_socket_does_not_show_the_other_sockets_sessions() {
    let m = M::new();
    m.on("a", &["new-session", "-d", "-s", "x"]);
    m.on("b", &["new-session", "-d", "-s", "y"]);
    assert_eq!(
        m.tmux.state("b").sessions(),
        [SessionRow {
            id: "$0".into(),
            name: "y".into()
        }]
    );
}

#[test]
fn only_the_file_backing_writes_fake_tmux_socket_db_and_the_memory_backing_writes_no_file() {
    let (file, memory) = (M::new(), M::new());
    let inv = file.invocation(M::argv("main", &["new-session", "-d", "-s", "s"]));
    run_commands(&inv, Backing::File(file.path())).expect("the file backing runs the call");
    assert!(
        db_path(&file.path(), "main").is_file(),
        "no fake-tmux-main.db in {}",
        file.path().display()
    );
    memory.on("main", &["new-session", "-d", "-s", "s"]);
    let left: Vec<_> = std::fs::read_dir(memory.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "the memory backing left {left:?}");
}
