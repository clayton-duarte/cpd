use super::T;

fn d(t: &T, f: &str) -> String {
    t.ok(&["display-message", "-p", "-t", "%0", f])
        .trim_end_matches('\n')
        .to_string()
}

fn one() -> T {
    let t = T::new();
    t.ok(&[
        "new-session",
        "-d",
        "-s",
        "s",
        "-n",
        "w",
        "-x",
        "80",
        "-y",
        "24",
    ]);
    t
}

#[test]
fn a_variable_and_a_user_option_expand() {
    let t = one();
    t.ok(&["set", "-p", "-t", "%0", "@cpd_history", "j"]);
    assert_eq!(d(&t, "#{window_name}/#{@cpd_history}"), "w/j");
}

#[test]
fn display_and_display_message_agree_and_list_f_evaluates() {
    let t = one();
    assert_eq!(t.ok(&["display", "-p", "-t", "%0", "#{pane_id}"]), "%0\n");
    assert_eq!(t.ok(&["list-panes", "-a", "-F", "<#{pane_id}>"]), "<%0>\n");
}

#[test]
fn conditional_picks_a_branch() {
    let t = one();
    assert_eq!(d(&t, "#{?pane_dead,dead,alive}"), "alive");
    assert_eq!(d(&t, "#{?1,a,b}#{?0,a,b}"), "ab");
}

#[test]
fn equality_and_inequality() {
    let t = one();
    assert_eq!(d(&t, "#{==:a,a}#{==:a,b}#{!=:a,b}#{!=:a,a}"), "1010");
}

#[test]
fn and_or_not() {
    let t = one();
    assert_eq!(
        d(&t, "#{&&:1,0}#{&&:1,1}#{||:0,0}#{||:0,1}#{!:0}#{!:1}"),
        "010110"
    );
}

#[test]
fn n_is_the_length_of_a_string() {
    let t = one();
    assert_eq!(d(&t, "#{n:abcd}"), "4");
}

#[test]
fn conditionals_nest() {
    let t = one();
    assert_eq!(d(&t, "#{?1,#{?0,a,b},c}"), "b");
    assert_eq!(d(&t, "#{?#{==:x,x},yes,no}"), "yes");
}

#[test]
fn an_unknown_variable_prints_empty() {
    let t = one();
    assert_eq!(d(&t, "[#{no_such_variable}]"), "[]");
}

#[test]
fn id_and_name_variables() {
    let t = one();
    assert_eq!(
        d(
            &t,
            "#{pane_id} #{window_id} #{session_name} #{session_id} #{window_name} #{pane_index}"
        ),
        "%0 @0 s $0 w 0"
    );
}

#[test]
fn pane_state_variables() {
    let t = one();
    assert_eq!(
        d(&t, "#{pane_dead}|#{pane_dead_status}|#{pane_width}"),
        "0||80"
    );
    assert!(!d(&t, "#{pane_pid}").is_empty());
}

#[test]
fn client_and_version_variables_are_defined() {
    let t = one();
    let v = d(&t, "#{version}");
    assert!(v.chars().next().is_some_and(|c| c.is_ascii_digit()), "{v}");
    assert!(d(&t, "#{client_width}x#{client_height}").contains('x'));
    assert_ne!(
        d(&t, "#{client_name}#{client_activity}"),
        "#{client_name}#{client_activity}"
    );
}

#[test]
fn pid_prints_fake_tmux_server_pid_from_the_invocation_env() {
    let t = one();
    assert_eq!(d(&t, "#{pid}"), "4242");
}

#[test]
fn hook_window_name_is_empty_outside_a_hook_and_e_and_loops_are_unevaluated() {
    let t = one();
    assert_eq!(d(&t, "[#{hook_window_name}]"), "[]");
    assert_eq!(d(&t, "#{e|+:1,2}"), "#{e|+:1,2}");
    assert_eq!(d(&t, "#{P/i/:x}"), "#{P/i/:x}");
}
