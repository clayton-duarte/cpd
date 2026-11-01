use super::T;

fn ids(out: String) -> Vec<String> {
    out.lines().map(String::from).collect()
}

/// Session `s` with windows `@0` (%0), `@1` (%1), `@2` (%2).
fn three(t: &T) {
    t.server();
    t.ok(&["new-window", "-t", "=s:", "-n", "b"]);
    t.ok(&["new-window", "-t", "=s:", "-n", "c"]);
}

#[test]
fn kill_window_removes_the_window_and_its_panes() {
    let t = T::new();
    three(&t);
    t.ok(&["kill-window", "-t", "@1"]);
    let st = t.state().expect("server");
    assert_eq!(
        st.windows()
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>(),
        ["@0", "@2"]
    );
    assert_eq!(
        st.panes().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["%0", "%2"]
    );
}

#[test]
fn kill_window_by_name_target() {
    let t = T::new();
    three(&t);
    t.ok(&["kill-window", "-t", "=s:b"]);
    assert_eq!(
        ids(t.ok(&["list-windows", "-t", "=s:", "-F", "#{window_name}"])),
        ["w", "c"]
    );
}

#[test]
fn kill_pane_removes_one_pane_and_keeps_the_window() {
    let t = T::new();
    t.server();
    t.ok(&["split-window", "-d", "-t", "%0"]);
    t.ok(&["kill-pane", "-t", "%1"]);
    assert_eq!(t.ok(&["list-panes", "-F", "#{pane_id}"]), "%0\n");
    t.ok(&["kill-window", "-t", "@0"]);
    assert!(t.state().is_none_or(|s| s.panes().is_empty()));
}

#[test]
fn killing_the_last_pane_of_a_window_removes_the_window() {
    let t = T::new();
    three(&t);
    t.ok(&["kill-pane", "-t", "%1"]);
    assert_eq!(
        ids(t.ok(&["list-windows", "-t", "=s:", "-F", "#{window_id}"])),
        ["@0", "@2"]
    );
    t.ok(&["kill-pane", "-t", "%2"]);
    t.ok(&["kill-window", "-t", "@0"]);
    assert!(t.t(&["has-session", "-t", "=s"]).code != 0);
}

#[test]
fn killing_the_last_window_removes_the_session() {
    let t = T::new();
    t.server();
    t.ok(&["new-session", "-d", "-s", "other"]);
    t.ok(&["kill-window", "-t", "@0"]);
    assert_eq!(t.t(&["has-session", "-t", "=s"]).code, 1);
    assert_eq!(t.t(&["has-session", "-t", "=other"]).code, 0);
}

#[test]
fn kill_session_removes_its_windows_and_panes() {
    let t = T::new();
    three(&t);
    t.ok(&["new-session", "-d", "-s", "other"]);
    t.ok(&["kill-session", "-t", "=s"]);
    let st = t.state().expect("server");
    assert_eq!(
        st.sessions()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["other"]
    );
    assert_eq!(st.panes().len(), 1);
}

#[test]
fn ids_are_not_reused_after_a_kill() {
    let t = T::new();
    t.server();
    t.ok(&["kill-window", "-t", "@0"]);
    t.ok(&["new-session", "-d", "-s", "again"]);
    assert_eq!(
        t.ok(&[
            "new-window",
            "-t",
            "=again:",
            "-P",
            "-F",
            "#{window_id} #{pane_id}"
        ]),
        "@2 %2\n"
    );
}

#[test]
fn list_commands_after_a_kill_show_only_what_is_left() {
    let t = T::new();
    three(&t);
    t.ok(&["kill-window", "-t", "@0"]);
    assert_eq!(
        ids(t.ok(&["list-panes", "-s", "-t", "=s", "-F", "#{pane_id}"])),
        ["%1", "%2"]
    );
    assert_eq!(
        t.ok(&["list-windows", "-t", "=s:", "-F", "#{window_name}"]),
        "b\nc\n"
    );
}

#[test]
fn kill_server_removes_the_server() {
    let t = T::new();
    three(&t);
    t.ok(&["kill-window", "-t", "@2"]);
    t.ok(&["kill-server"]);
    assert!(t.state().is_none());
    assert_eq!(t.t(&["list-sessions"]).code, 1);
}

#[test]
fn killing_an_unknown_window_exits_1_with_tmuxs_text() {
    let t = T::new();
    t.server();
    t.fail(&["kill-window", "-t", "@9"], 1, "can't find window: @9");
}

#[test]
fn killing_an_unknown_session_exits_1_with_tmuxs_text() {
    let t = T::new();
    t.server();
    t.fail(
        &["kill-session", "-t", "=nope"],
        1,
        "can't find session: nope",
    );
}

#[test]
fn respawn_pane_k_replaces_the_argv_and_clears_dead() {
    let t = T::new();
    t.server();
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "3"]);
    assert!(t.state().unwrap().panes()[0].dead);
    t.ok(&[
        "respawn-pane",
        "-k",
        "-t",
        "%0",
        "--",
        "sh",
        "-c",
        "echo again",
    ]);
    let p = &t.state().unwrap().panes()[0];
    assert_eq!(p.argv, ["sh", "-c", "echo again"]);
    assert!(!p.dead);
}
