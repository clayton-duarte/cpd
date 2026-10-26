use super::T;

fn show(t: &T, args: &[&str]) -> String {
    t.ok(args)
}

#[test]
fn set_g_then_show_gv_round_trips() {
    let t = T::new();
    t.server();
    t.ok(&["set", "-g", "status-left", "hi"]);
    assert_eq!(show(&t, &["show", "-gv", "status-left"]), "hi\n");
}

#[test]
fn set_s_stores_a_server_option() {
    let t = T::new();
    t.server();
    t.ok(&["set", "-s", "escape-time", "0"]);
    assert_eq!(show(&t, &["show", "-sv", "escape-time"]), "0\n");
}

#[test]
fn set_as_appends() {
    let t = T::new();
    t.server();
    t.ok(&["set", "-g", "terminal-features", "a"]);
    t.ok(&["set", "-as", "terminal-features", ",b"]);
    assert_eq!(show(&t, &["show", "-gv", "terminal-features"]), "a,b\n");
}

#[test]
fn set_gq_is_quiet_about_an_unknown_option() {
    let t = T::new();
    t.server();
    let o = t.t(&["set", "-gq", "no-such-option", "1"]);
    assert_eq!((o.code, o.stderr.as_str()), (0, ""));
}

#[test]
fn show_gqv_of_an_unset_option_prints_nothing_and_exits_0() {
    let t = T::new();
    t.server();
    let o = t.t(&["show", "-gqv", "@cpd_unset"]);
    assert_eq!((o.code, o.stdout.as_str()), (0, ""));
}

#[test]
fn user_options_live_on_the_server() {
    let t = T::new();
    t.server();
    for (k, v) in [("@cpd_machine", "m1"), ("@cpd_version", "0.1"), ("@cpd_workers_a", "x")] {
        t.ok(&["set", "-g", k, v]);
        assert_eq!(show(&t, &["show", "-gv", k]), format!("{v}\n"));
    }
}

#[test]
fn user_options_live_on_a_window() {
    let t = T::new();
    t.server();
    for k in ["@cpd_session", "@cpd_title", "@cpd_view", "@cpd_history", "@cpd_preview"] {
        t.ok(&["set", "-t", "@0", k, "v"]);
        assert_eq!(show(&t, &["show", "-t", "@0", "-v", k]), "v\n", "{k}");
    }
}

#[test]
fn user_options_live_on_a_pane() {
    let t = T::new();
    t.server();
    t.ok(&["set", "-p", "-t", "%0", "@cpd_history", "j1"]);
    assert_eq!(show(&t, &["show", "-p", "-t", "%0", "-v", "@cpd_history"]), "j1\n");
    assert_eq!(t.ok(&["display-message", "-p", "-t", "%0", "#{@cpd_history}"]), "j1\n");
    assert!(t.state().expect("server").panes()[0].options.contains(&("@cpd_history".into(), "j1".into())));
}

#[test]
fn remain_on_exit_is_an_option() {
    let t = T::new();
    t.server();
    t.ok(&["set", "-t", "@0", "remain-on-exit", "on"]);
    assert_eq!(show(&t, &["show", "-t", "@0", "-v", "remain-on-exit"]), "on\n");
}

#[test]
fn set_environment_and_show_environment_round_trip_and_unset() {
    let t = T::new();
    t.server();
    t.ok(&["set-environment", "-g", "A", "1"]);
    t.ok(&["set-environment", "-t", "=s", "B", "2"]);
    assert_eq!(t.ok(&["show-environment", "-g", "A"]), "A=1\n");
    assert_eq!(t.ok(&["show-environment", "-t", "=s", "B"]), "B=2\n");
    t.ok(&["set-environment", "-g", "-r", "A"]);
    assert_eq!(t.ok(&["show-environment", "-g", "A"]), "-A\n");
}
