use super::T;

#[test]
fn a_command_with_no_server_says_no_server_running() {
    let t = T::new();
    let want = format!(
        "no server running on {}/tmux-{}/t",
        t.path().display(),
        t.uid()
    );
    t.fail(&["list-sessions"], 1, &want);
}

#[test]
fn has_session_exits_1_when_the_session_is_absent_and_0_when_present() {
    let t = T::new();
    t.ok(&["new-session", "-d", "-s", "n"]);
    assert_eq!(t.t(&["has-session", "-t", "=n"]).code, 0);
    assert_eq!(t.t(&["has-session", "-t", "=absent"]).code, 1);
}

#[test]
fn new_session_starts_the_server_with_ids_from_zero() {
    let t = T::new();
    assert!(t.state().is_none());
    let out = t.ok(&[
        "new-session",
        "-d",
        "-s",
        "n",
        "-P",
        "-F",
        "#{session_id} #{window_id} #{pane_id}",
    ]);
    assert_eq!(out.trim_end(), "$0 @0 %0");
    assert!(t.path().join("fake-tmux-t.db").exists());
    assert!(t.state().is_some());
}

#[test]
fn kill_server_removes_it_and_the_next_call_says_no_server() {
    let t = T::new();
    t.server();
    t.ok(&["kill-server"]);
    assert!(!t.path().join("fake-tmux-t.db").exists());
    let o = t.t(&["list-sessions"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("no server running on "), "{o:?}");
}

#[test]
fn two_sockets_are_independent() {
    let t = T::new();
    assert_eq!(t.on("a", &["new-session", "-d", "-s", "only-a"]).code, 0);
    assert_eq!(t.on("b", &["list-sessions"]).code, 1);
    assert_eq!(t.on("b", &["new-session", "-d", "-s", "only-b"]).code, 0);
    assert_eq!(t.on("a", &["has-session", "-t", "=only-b"]).code, 1);
    assert_eq!(t.on("b", &["has-session", "-t", "=only-b"]).code, 0);
}

#[test]
fn a_missing_tmux_tmpdir_is_refused_and_named() {
    let t = T::new();
    let gone = t.path().join("gone");
    let o = super::run(&super::Invocation {
        argv: ["-u", "-L", "t", "list-sessions"]
            .map(String::from)
            .to_vec(),
        tmux_tmpdir: gone.clone(),
        cwd: t.path(),
        env: vec![],
    });
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains(&gone.display().to_string()), "{o:?}");
}

#[test]
fn the_global_flags_u_and_f_dev_null_are_accepted() {
    let t = T::new();
    let o = t.t(&["new-session", "-d", "-s", "n"]);
    assert_eq!((o.code, o.stderr.as_str()), (0, ""));
}

#[test]
fn an_unknown_subcommand_exits_1_with_unknown_command() {
    let t = T::new();
    t.server();
    t.fail(&["frobnicate"], 1, "unknown command: frobnicate");
}
