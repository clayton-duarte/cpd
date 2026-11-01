use super::T;
use std::path::PathBuf;

/// A file the hooks write to; `cmd` appends `line` to it with `run-shell`.
fn log(t: &T) -> PathBuf {
    t.path().join("log")
}

fn append(t: &T, line: &str) -> String {
    format!("run-shell 'echo {line} >> {}'", log(t).display())
}

fn read(t: &T) -> String {
    std::fs::read_to_string(log(t)).unwrap_or_default()
}

/// `s` with windows `@0` (%0), `@1` (%1), `@2` (%2), `@3` (%3).
fn four(t: &T) {
    t.server();
    for n in ["b", "c", "d"] {
        t.ok(&["new-window", "-t", "=s:", "-n", n]);
    }
}

#[test]
fn set_hook_g_stores_the_hook() {
    let t = T::new();
    t.server();
    t.ok(&["set-hook", "-g", "pane-died", "run-shell true"]);
    assert_eq!(
        t.state().unwrap().hooks(),
        [("pane-died".to_string(), "run-shell true".to_string())]
    );
}

#[test]
fn show_hooks_g_lists_the_hook() {
    let t = T::new();
    t.server();
    t.ok(&["set-hook", "-g", "pane-died", "run-shell true"]);
    let out = t.ok(&["show-hooks", "-g"]);
    assert!(
        out.contains("pane-died") && out.contains("run-shell true"),
        "{out}"
    );
}

#[test]
fn set_hook_gu_removes_the_hook_so_it_no_longer_fires() {
    let t = T::new();
    four(&t);
    t.ok(&["set-hook", "-g", "pane-died", &append(&t, "died")]);
    t.ok(&["set-hook", "-gu", "pane-died"]);
    assert!(t.state().unwrap().hooks().is_empty());
    t.ok(&["__ctl", "pane-exit", "%1", "--status", "0"]);
    assert_eq!(read(&t), "");
}

#[test]
fn ctl_pane_exit_marks_the_pane_dead_with_its_status() {
    let t = T::new();
    four(&t);
    t.ok(&["__ctl", "pane-exit", "%3", "--status", "1"]);
    let p = t.state().unwrap().panes().remove(3);
    assert_eq!((p.dead, p.dead_status), (true, Some(1)));
    assert_eq!(
        t.ok(&[
            "display-message",
            "-p",
            "-t",
            "%3",
            "#{pane_dead} #{pane_dead_status}"
        ]),
        "1 1\n"
    );
}

#[test]
fn ctl_pane_exit_fires_pane_died_with_the_pane_as_target_and_expanded_formats() {
    let t = T::new();
    four(&t);
    let cmd = format!(
        "run-shell 'echo #{{pane_id}} #{{pane_dead_status}} >> {}'",
        log(&t).display()
    );
    t.ok(&["set-hook", "-g", "pane-died", &cmd]);
    t.ok(&["__ctl", "pane-exit", "%3", "--status", "1"]);
    assert_eq!(read(&t), "%3 1\n");
}

#[test]
fn respawn_pane_without_a_target_in_a_hook_is_the_hooks_pane_not_the_active_one() {
    let t = T::new();
    four(&t);
    t.ok(&["set-hook", "-g", "pane-died", "respawn-pane"]);
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "1"]);
    assert_eq!(
        t.ok(&["display-message", "-p", "-t", "%0", "#{pane_dead}"]),
        "0\n"
    );
}

#[test]
fn pane_died_fires_inside_the_same_call() {
    let t = T::new();
    t.server();
    t.ok(&["set-hook", "-g", "pane-died", "set -g @seen #{pane_id}"]);
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(t.ok(&["show", "-gv", "@seen"]), "%0\n");
}

#[test]
fn kill_window_fires_window_unlinked_with_the_window_name() {
    let t = T::new();
    four(&t);
    let cmd = format!(
        "run-shell 'echo #{{hook_window_name}} >> {}'",
        log(&t).display()
    );
    t.ok(&["set-hook", "-g", "window-unlinked", &cmd]);
    t.ok(&["kill-window", "-t", "=s:c"]);
    assert_eq!(read(&t), "c\n");
}

#[test]
fn kill_pane_of_the_last_pane_fires_window_unlinked() {
    let t = T::new();
    four(&t);
    let cmd = format!(
        "run-shell 'echo #{{hook_window_name}} >> {}'",
        log(&t).display()
    );
    t.ok(&["set-hook", "-g", "window-unlinked", &cmd]);
    t.ok(&["kill-pane", "-t", "%1"]);
    assert_eq!(read(&t), "b\n");
}

#[test]
fn run_shell_runs_in_the_foreground_and_the_call_returns_when_it_ends() {
    let t = T::new();
    t.server();
    t.ok(&["set-hook", "-g", "pane-died", &append(&t, "done")]);
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(read(&t), "done\n");
}

#[test]
fn run_shell_uses_the_callers_path() {
    let mut t = T::new();
    t.server();
    let bin = t.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let tool = bin.join("cpd-marker");
    std::fs::write(
        &tool,
        format!("#!/bin/sh\necho marked >> {}\n", log(&t).display()),
    )
    .unwrap();
    std::process::Command::new("chmod")
        .arg("+x")
        .arg(&tool)
        .status()
        .unwrap();
    t.env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    t.ok(&["set-hook", "-g", "pane-died", "run-shell cpd-marker"]);
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(read(&t), "marked\n");
}

#[test]
fn run_shell_b_also_runs_the_command() {
    let t = T::new();
    t.server();
    t.ok(&[
        "set-hook",
        "-g",
        "pane-died",
        &format!("run-shell -b 'echo bg >> {}'", log(&t).display()),
    ]);
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(read(&t), "bg\n");
}

#[test]
fn if_f_runs_the_then_branch_when_the_format_is_true_and_else_otherwise() {
    let t = T::new();
    four(&t);
    let cmd = format!(
        "if -F '#{{pane_dead_status}}' '{}' '{}'",
        append(&t, "failed"),
        append(&t, "clean")
    );
    t.ok(&["set-hook", "-g", "pane-died", &cmd]);
    t.ok(&["__ctl", "pane-exit", "%1", "--status", "1"]);
    t.ok(&["__ctl", "pane-exit", "%2", "--status", "0"]);
    assert_eq!(read(&t), "failed\nclean\n");
}

#[test]
fn the_interpreter_runs_respawn_select_pane_and_send_keys_m_in_a_hook() {
    let t = T::new();
    four(&t);
    t.ok(&[
        "set-hook",
        "-g",
        "pane-died",
        "respawn-pane -k -t #{pane_id} -- sh -c again ; select-pane -t #{pane_id} ; send-keys -M",
    ]);
    t.ok(&["__ctl", "pane-exit", "%2", "--status", "5"]);
    let st = t.state().unwrap();
    let p = &st.panes()[2];
    assert_eq!(
        (p.argv.clone(), p.dead),
        (vec!["sh".to_string(), "-c".into(), "again".into()], false)
    );
    assert_eq!(
        t.ok(&["display-message", "-p", "-t", "@2", "#{pane_id}"]),
        "%2\n"
    );
}

#[test]
fn a_semicolon_in_a_hook_runs_both_commands() {
    let t = T::new();
    t.server();
    t.ok(&[
        "set-hook",
        "-g",
        "pane-died",
        &format!("{} ; {}", append(&t, "one"), append(&t, "two")),
    ]);
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    assert_eq!(read(&t), "one\ntwo\n");
}

#[test]
fn a_hook_that_recurses_is_bounded_and_the_queued_deeper_one_is_drained_before_the_call_returns() {
    let t = T::new();
    four(&t);
    // pane-died kills @1; its window-unlinked kills @2; that one's kills @3: three levels deep.
    let cmd = format!(
        "{} ; kill-window -t #{{window_id}}",
        append(&t, "unlinked #{hook_window_name}")
    );
    t.ok(&[
        "set-hook",
        "-g",
        "window-unlinked",
        &cmd.replace("#{window_id}", "@3"),
    ]);
    t.ok(&["set-hook", "-g", "pane-died", "kill-window -t @1"]);
    t.ok(&["__ctl", "pane-exit", "%0", "--status", "0"]);
    let log = read(&t);
    assert!(log.starts_with("unlinked b\n"), "{log}");
    assert!(log.contains("unlinked d\n"), "{log}");
    let st = t.state().unwrap();
    assert!(st.windows().iter().all(|w| w.id != "@1" && w.id != "@3"));
}
