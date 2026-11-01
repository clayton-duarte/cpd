use super::T;

#[test]
fn new_session_stores_name_window_argv_cwd_and_env() {
    let t = T::new();
    t.ok(&[
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
    let st = t.state().expect("server");
    assert_eq!(st.sessions()[0].name, "n");
    assert_eq!(st.windows()[0].name, "w");
    let p = &st.panes()[0];
    assert_eq!(p.argv, ["sh", "-c", "echo hi"]);
    assert_eq!(p.cwd, "/work");
    assert!(
        st.pane_env("%0")
            .contains(&("K".to_string(), "V".to_string()))
    );
}

#[test]
fn new_session_prints_window_and_pane_ids_with_p_f() {
    let t = T::new();
    let out = t.ok(&[
        "new-session",
        "-d",
        "-s",
        "n",
        "-P",
        "-F",
        "#{window_id} #{pane_id}",
    ]);
    assert_eq!(out, "@0 %0\n");
}

#[test]
fn a_duplicate_session_name_exits_1() {
    let t = T::new();
    t.ok(&["new-session", "-d", "-s", "n"]);
    t.fail(&["new-session", "-d", "-s", "n"], 1, "duplicate session: n");
}

#[test]
fn new_window_allocates_the_next_window_and_pane_ids() {
    let t = T::new();
    t.ok(&["new-session", "-d", "-s", "n"]);
    let out = t.ok(&[
        "new-window",
        "-t",
        "=n:",
        "-n",
        "x",
        "-P",
        "-F",
        "#{window_id} #{pane_id}",
    ]);
    assert_eq!(out, "@1 %1\n");
}

#[test]
fn split_window_allocates_the_next_pane_id() {
    let t = T::new();
    t.server();
    assert_eq!(
        t.ok(&["split-window", "-t", "%0", "-P", "-F", "#{pane_id}"]),
        "%1\n"
    );
}

#[test]
fn a_killed_id_is_never_reused() {
    let t = T::new();
    t.server();
    t.ok(&["split-window", "-t", "%0"]);
    t.ok(&["kill-pane", "-t", "%1"]);
    assert_eq!(
        t.ok(&["split-window", "-t", "%0", "-P", "-F", "#{pane_id}"]),
        "%2\n"
    );
}

#[test]
fn windows_keep_creation_order() {
    let t = T::new();
    t.server();
    t.ok(&["new-window", "-t", "=s:", "-n", "b"]);
    t.ok(&["new-window", "-t", "=s:", "-n", "a"]);
    assert_eq!(
        t.ok(&["list-windows", "-t", "=s", "-F", "#{window_name}"]),
        "w\nb\na\n"
    );
}

#[test]
fn rename_window_changes_the_name() {
    let t = T::new();
    t.server();
    t.ok(&["rename-window", "-t", "@0", "new"]);
    assert_eq!(t.state().expect("server").windows()[0].name, "new");
}

#[test]
fn swap_pane_swaps_the_two_panes_positions() {
    let t = T::new();
    t.server();
    t.ok(&["split-window", "-t", "%0"]);
    t.ok(&["swap-pane", "-s", "%1", "-t", "%0"]);
    assert_eq!(
        t.ok(&["list-panes", "-t", "@0", "-F", "#{pane_id}"]),
        "%1\n%0\n"
    );
}

#[test]
fn list_sessions_prints_one_line_per_session_in_order() {
    let t = T::new();
    t.ok(&["new-session", "-d", "-s", "b"]);
    t.ok(&["new-session", "-d", "-s", "a"]);
    assert_eq!(t.ok(&["list-sessions", "-F", "#{session_name}"]), "b\na\n");
}

#[test]
fn list_windows_prints_one_line_per_window() {
    let t = T::new();
    t.server();
    t.ok(&["new-window", "-t", "=s:", "-n", "x"]);
    assert_eq!(
        t.ok(&[
            "list-windows",
            "-t",
            "=s",
            "-F",
            "#{window_id}:#{window_name}"
        ]),
        "@0:w\n@1:x\n"
    );
}

#[test]
fn list_panes_a_and_t_print_one_line_per_pane() {
    let t = T::new();
    t.server();
    t.ok(&["new-window", "-t", "=s:", "-n", "x"]);
    assert_eq!(t.ok(&["list-panes", "-a", "-F", "#{pane_id}"]), "%0\n%1\n");
    assert_eq!(
        t.ok(&["list-panes", "-t", "@1", "-F", "#{pane_id}"]),
        "%1\n"
    );
}
