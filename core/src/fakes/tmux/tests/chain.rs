use super::T;

#[test]
fn a_chained_argv_runs_left_to_right() {
    let t = T::new();
    t.ok(&[
        "new-session",
        "-d",
        "-s",
        "s",
        ";",
        "new-window",
        "-t",
        "=s:",
        "-n",
        "x",
        ";",
        "kill-window",
        "-t",
        "@0",
    ]);
    assert_eq!(
        t.ok(&[
            "list-windows",
            "-t",
            "=s:",
            "-F",
            "#{window_id} #{window_name}"
        ]),
        "@1 x\n"
    );
}

#[test]
fn a_chain_is_one_transaction_with_one_output() {
    let t = T::new();
    let out = t.ok(&[
        "new-session",
        "-d",
        "-s",
        "s",
        "-P",
        "-F",
        "#{window_id}",
        ";",
        "new-window",
        "-P",
        "-F",
        "#{window_id}",
        ";",
        "set-hook",
        "-g",
        "pane-died",
        "run-shell true",
    ]);
    assert_eq!(out, "@0\n@1\n");
    assert_eq!(
        t.state().unwrap().hooks(),
        [("pane-died".to_string(), "run-shell true".to_string())]
    );
}

#[test]
fn a_failing_command_stops_the_chain_with_its_exit_code() {
    let t = T::new();
    t.server();
    let o = t.t(&[
        "new-window",
        "-n",
        "a",
        ";",
        "kill-window",
        "-t",
        "@9",
        ";",
        "new-window",
        "-n",
        "b",
    ]);
    assert_eq!((o.code, o.stderr.trim_end()), (1, "can't find window: @9"));
    let names = t.ok(&["list-windows", "-F", "#{window_name}"]);
    assert!(!names.lines().any(|n| n == "b"), "{names}");
}

#[test]
fn the_effects_before_a_failing_command_stay() {
    let t = T::new();
    t.server();
    t.t(&["new-window", "-n", "a", ";", "kill-window", "-t", "@9"]);
    assert_eq!(t.ok(&["list-windows", "-F", "#{window_name}"]), "w\na\n");
}

#[test]
fn the_adapters_new_session_chain_sets_24_server_options_before_the_session() {
    let t = T::new();
    let mut argv: Vec<String> = Vec::new();
    for i in 0..24 {
        argv.extend(["set", "-g", &format!("@cpd_o{i}"), &i.to_string(), ";"].map(String::from));
    }
    argv.extend(
        [
            "new-session",
            "-d",
            "-s",
            "s",
            "-n",
            "w",
            ";",
            "set-hook",
            "-g",
            "pane-died",
            "run-shell true",
        ]
        .map(String::from),
    );
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    t.ok(&refs);
    let st = t.state().expect("server");
    assert_eq!(st.options().len(), 24);
    assert_eq!(st.sessions()[0].name, "s");
    assert_eq!(st.hooks().len(), 1);
}

#[test]
fn new_session_then_set_g_as_the_adapter_builds_it() {
    let t = T::new();
    t.ok(&[
        "new-session",
        "-d",
        "-s",
        "s",
        ";",
        "set",
        "-g",
        "@cpd_machine",
        "m1",
        ";",
        "set",
        "-g",
        "remain-on-exit",
        "on",
        ";",
        "send-keys",
        "-t",
        "%0",
        "x",
    ]);
    assert_eq!(t.ok(&["show", "-gv", "@cpd_machine"]), "m1\n");
    assert_eq!(t.ok(&["show", "-gv", "remain-on-exit"]), "on\n");
    assert_eq!(t.state().unwrap().keys().len(), 1);
}
