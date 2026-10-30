//! `cpd tui action runner --client <name>` (pro-d199): flips the `worker_runner` line of config.toml, sets the global
//! `@cpd_runner` at once and refreshes the clicking client; it opens no store, touches no Attempt, sends no signal. The
//! tmux is the fake server of an `Fx`; the config is a file of the test's own temporary home.

use crate::common;

use common::cockpit::action;
use common::fx::Fx;
use common::tree::fx;

const CLIENT: &str = "/dev/pts/9";

fn config(fx: &Fx) -> std::path::PathBuf {
    fx.env.cpd_home().join("config.toml")
}

/// The fixture's config plus `extra` top-level lines (before the tables), and a running fake server.
fn with_config(extra: &str) -> Fx {
    let fx = fx();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\n{extra}[events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
    let out = fx.tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "cpd tui view tree"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    fx
}

fn tmux_calls(fx: &Fx, since: usize) -> Vec<String> {
    fx.calls("tmux")[since..].iter().map(|c| c.args.join(" ")).collect()
}

#[test]
fn tui_action_runner_flips_the_config_line_and_sets_the_option_and_refreshes_the_client() {
    let fx = with_config("worker_runner = \"pi\"\n");
    let before = fx.calls("tmux").len();
    action(&fx, &["runner", "--client", CLIENT]);
    let text = std::fs::read_to_string(config(&fx)).unwrap();
    assert!(text.contains("worker_runner = \"durable\""), "{text}");
    let calls = tmux_calls(&fx, before);
    assert!(calls.iter().any(|c| c.contains("set -g @cpd_runner durable")), "{calls:?}");
    assert!(calls.iter().any(|c| c.contains(&format!("refresh-client -S -c {CLIENT}"))), "{calls:?}");
    assert_eq!(fx.tmux(&["show", "-gv", "@cpd_runner"]).stdout, b"durable\n");
}

#[test]
fn tui_action_runner_changes_only_the_worker_runner_line_of_the_config() {
    let fx = with_config("# mine\nconcurrency = 5\nworker_runner = \"durable\"   # old\n");
    let before = std::fs::read_to_string(config(&fx)).unwrap();
    action(&fx, &["runner", "--client", CLIENT]);
    let after = std::fs::read_to_string(config(&fx)).unwrap();
    let (old, new): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    assert_eq!(old.len(), new.len(), "{after}");
    let changed: Vec<_> = old.iter().zip(&new).filter(|(a, b)| a != b).collect();
    assert_eq!(changed.len(), 1, "only one line differs: {changed:?}");
    assert!(changed[0].0.starts_with("worker_runner = \"durable\""), "{changed:?}");
    assert!(changed[0].1.starts_with("worker_runner = \"pi\" # "), "{changed:?}");
}

#[test]
fn tui_action_runner_touches_no_attempt_and_sends_no_signal() {
    let fx = with_config("worker_runner = \"pi\"\n");
    let before = fx.calls("tmux").len();
    action(&fx, &["runner", "--client", CLIENT]);
    assert!(!fx.env.cpd_home().join("store.db").exists(), "no store is opened");
    let calls = tmux_calls(&fx, before);
    assert!(
        calls.iter().all(|c| !c.contains("kill") && !c.contains("send-keys") && !c.contains("respawn")),
        "no pane, key or process is touched: {calls:?}"
    );
    assert!(calls.iter().any(|c| c.contains("set -g @cpd_runner durable")), "the click did its work: {calls:?}");
    assert!(fx.calls("kill").is_empty(), "no signal");
}

#[test]
fn tui_action_runner_with_a_broken_config_says_so_on_the_client_and_leaves_the_file() {
    let fx = with_config("");
    let broken = "version = 1\nworker_runner = [\n";
    std::fs::write(config(&fx), broken).unwrap();
    let before = fx.calls("tmux").len();
    action(&fx, &["runner", "--client", CLIENT]); // exits 0 and prints nothing
    assert_eq!(std::fs::read_to_string(config(&fx)).unwrap(), broken, "the file is untouched");
    let calls = tmux_calls(&fx, before);
    let shown: Vec<_> = calls.iter().filter(|c| c.contains(&format!("display-message -l -c {CLIENT}"))).collect();
    assert_eq!(shown.len(), 1, "{calls:?}");
    assert!(!shown[0].contains("unknown action"), "`runner` is a known action that failed: {shown:?}");
    assert!(shown[0].contains("config"), "the message names the config: {shown:?}");
    assert!(calls.iter().all(|c| !c.contains("@cpd_runner")), "no option is set: {calls:?}");
}
