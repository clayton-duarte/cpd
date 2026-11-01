use super::T;

fn two() -> T {
    let t = T::new();
    t.ok(&["new-session", "-d", "-s", "alpha"]);
    t.ok(&["new-session", "-d", "-s", "alpine"]);
    t
}

fn name(t: &T, target: &str) -> String {
    t.ok(&["display-message", "-p", "-t", target, "#{session_name}"])
        .trim_end()
        .to_string()
}

#[test]
fn equals_name_is_an_exact_session_target() {
    assert_eq!(name(&two(), "=alpha"), "alpha");
}

#[test]
fn equals_name_colon_is_a_session_target_for_its_current_window() {
    let t = two();
    assert_eq!(
        t.ok(&["display-message", "-p", "-t", "=alpine:", "#{window_id}"]),
        "@1\n"
    );
}

#[test]
fn a_pane_id_target_resolves_the_pane() {
    let t = two();
    assert_eq!(
        t.ok(&["display-message", "-p", "-t", "%1", "#{pane_id}"]),
        "%1\n"
    );
}

#[test]
fn a_window_id_target_resolves_the_window() {
    let t = two();
    assert_eq!(
        t.ok(&["display-message", "-p", "-t", "@1", "#{session_name}"]),
        "alpine\n"
    );
}

#[test]
fn a_bare_equals_is_the_current_session() {
    let t = T::new();
    t.ok(&["new-session", "-d", "-s", "only"]);
    assert_eq!(name(&t, "="), "only");
}

#[test]
fn equals_name_is_exact_so_a_prefix_is_not_found() {
    t_fail(&["has-session", "-t", "=alp"], "can't find session: alp");
}

#[test]
fn a_bare_name_is_a_prefix_match() {
    assert_eq!(name(&two(), "alph"), "alpha");
}

#[test]
fn a_missing_session_target_is_not_found() {
    t_fail(&["list-windows", "-t", "=x"], "can't find session: x");
}

#[test]
fn a_missing_window_target_is_not_found() {
    t_fail(&["rename-window", "-t", "@9", "n"], "can't find window: @9");
}

#[test]
fn a_missing_pane_target_is_not_found() {
    t_fail(&["split-window", "-t", "%9"], "can't find pane: %9");
}

#[test]
fn an_ambiguous_prefix_is_not_found() {
    t_fail(&["has-session", "-t", "al"], "can't find session: al");
}

#[test]
fn a_missing_window_name_in_a_session_is_not_found() {
    t_fail(
        &["select-pane", "-t", "=alpha:nope"],
        "can't find window: nope",
    );
}

fn t_fail(args: &[&str], stderr: &str) {
    let t = two();
    t.fail(args, 1, stderr);
}
