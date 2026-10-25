//! `cpd tui action reset` sizes the cockpit 20 % / 50 % / 30 % columns and 30 % / 70 % rows (docs/commands.md §13.2), against a recording fake `tmux`.

use std::fs;

use crate::common::TestEnv;

#[test]
fn tui_action_reset_resizes_the_cockpit_20_50_30_columns_and_30_70_rows() {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake(
        "tmux",
        "case \"$*\" in *list-panes*) printf '%%0\\tcpd\\tmain\\ttree\\n%%1\\tcpd\\tmain\\tchat\\n%%2\\tcpd\\tmain\\twidgets\\n%%3\\tcpd\\tmain\\tplan\\n%%4\\tcpd\\tmain\\tworker\\n';; esac\nexit 0",
    );
    let run = env.cpd(&["tui", "action", "reset", "--client", "/dev/pts/nobody"]);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");

    let resizes: Vec<Vec<String>> = env
        .calls("tmux")
        .into_iter()
        .filter_map(|c| c.iter().position(|a| a == "resize-pane").map(|i| c[i..].to_vec()))
        .collect();
    let want: Vec<Vec<String>> = [
        ["resize-pane", "-t", "%0", "-x", "20%"],
        ["resize-pane", "-t", "%2", "-x", "30%"],
        ["resize-pane", "-t", "%3", "-y", "30%"],
        ["resize-pane", "-t", "%4", "-x", "30%"],
    ]
    .iter()
    .map(|c| c.iter().map(|s| s.to_string()).collect())
    .collect();
    assert_eq!(resizes, want);
    assert!(resizes.iter().all(|c| c[2] != "%1"), "the chat slot is never resized");
}
