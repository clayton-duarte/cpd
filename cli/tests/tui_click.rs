//! `cpd tui click <reset|legend|logs> --client <name>` (commands.md §13.2): what the status line's buttons run.
//! `tmux` on `PATH` is a fake that records its argv and answers `display` and `display-popup` itself; the reset
//! test runs against a private tmux server, through the same fake, which passes the rest on to the real tmux.

mod common;

use std::fs;

use common::TestEnv;

const CLIENT: &str = "/dev/pts/7";

fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!("version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()),
    )
    .unwrap();
    env
}

/// A fake `tmux` whose `case` arms are `arms`, over the real tmux for everything they do not match.
fn fake_tmux(env: &TestEnv, arms: &str) {
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake("tmux", &format!("case \"$*\" in\n{arms}\n *) exec {} \"$@\";;\nesac", real.trim()));
}

/// The tmux calls after `-L <socket> -f /dev/null`, whose first word is `command`.
fn calls(env: &TestEnv, command: &str) -> Vec<Vec<String>> {
    env.calls("tmux")
        .into_iter()
        .map(|c| c[4..].to_vec())
        .filter(|c| c.first().is_some_and(|w| w == command))
        .collect()
}

/// Runs `cpd tui click <args> --client c` and checks the contract of the command: exit 0, nothing printed.
fn click(env: &TestEnv, button: &[&str]) {
    let mut args = vec!["tui", "click"];
    args.extend(button);
    args.extend(["--client", CLIENT]);
    let run = env.cpd(&args);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

#[test]
fn legend_opens_its_popup_on_the_client_sized_by_that_client() {
    let env = env();
    fake_tmux(&env, " *display-popup*) ;;\n *display*) echo '215 62';;");
    click(&env, &["legend"]);
    assert_eq!(calls(&env, "display"), [strings(&["display", "-p", "-c", CLIENT, "#{client_width} #{client_height}"])]);
    let (w, h) = cpd_tui::widgets::legend::size();
    assert_eq!(
        calls(&env, "display-popup"),
        [strings(&["display-popup", "-c", CLIENT, "-E", "-T", " legend ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd tui view legend"])]
    );
    assert!(calls(&env, "display-message").is_empty());
}

#[test]
fn logs_opens_its_popup_on_the_client_at_most_80_percent_of_its_size() {
    let env = env();
    fake_tmux(&env, " *display-popup*) ;;\n *display*) echo '80 40';;");
    click(&env, &["logs"]);
    let popup = calls(&env, "display-popup");
    assert_eq!(popup.len(), 1, "{popup:?}");
    // display-popup -c <client> -E -T ' logs ' -w <w> -h <h> cpd tui view logs
    assert_eq!(popup[0][..4], ["display-popup", "-c", CLIENT, "-E"]);
    assert_eq!((popup[0][4].as_str(), popup[0][5].as_str()), ("-T", " logs "));
    assert_eq!((popup[0][7].as_str(), popup[0][9].as_str()), ("64", "32"));
    assert_eq!(popup[0].last().unwrap(), "cpd tui view logs");
    assert!(calls(&env, "display-message").is_empty());
}

#[test]
fn reset_resizes_the_panes_of_main() {
    let env = env();
    let tmux = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let tree = tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "215", "-y", "62", "-P", "-F", "#{pane_id}", "sleep", "120"]);
    let chat = tmux(&["split-window", "-h", "-d", "-t", &tree, "-P", "-F", "#{pane_id}", "sleep", "120"]);
    tmux(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
    tmux(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    fake_tmux(&env, "");
    click(&env, &["reset"]);
    // The chat slot is never resized; the tree gets its 20 %.
    assert_eq!(calls(&env, "resize-pane"), [strings(&["resize-pane", "-t", &tree, "-x", "20%"])]);
    assert!(calls(&env, "display-message").is_empty());
}

#[test]
fn a_failing_action_shows_display_message_on_the_client_and_still_exits_0_silently() {
    for (button, failing) in [("reset", "list-panes"), ("legend", "display -p"), ("logs", "display-popup")] {
        let env = env();
        fake_tmux(
            &env,
            &format!(
                " *display-message*) ;;\n *{failing}*) echo 'boom' >&2; exit 1;;\n *display*) echo '215 62';;\n *list-sessions*) echo '$0';;",
            ),
        );
        click(&env, &[button]);
        let shown = calls(&env, "display-message");
        assert_eq!(shown.len(), 1, "{button}: {shown:?}");
        assert_eq!(shown[0][..3], ["display-message", "-c", CLIENT], "{button}");
        assert!(shown[0][3].contains("failed"), "{button}: {shown:?}");
    }
}

#[test]
fn a_range_that_is_not_a_button_does_nothing_and_exits_0() {
    let env = env();
    fake_tmux(&env, "");
    for button in [&["none"][..], &["window"], &[]] {
        click(&env, button);
    }
    assert!(env.calls("tmux").is_empty());
}
