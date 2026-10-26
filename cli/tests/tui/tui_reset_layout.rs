//! `cpd tui action reset` sizes the cockpit 20 % / 50 % / 30 % columns and 30 % / 70 % rows (docs/commands.md §13.2), in the
//! test process, on the fake tmux server of an `Fx`: what the test pins is the `resize-pane` commands the action sent.

use crate::common;

use common::cockpit::action;
use common::tree::fx;

#[test]
fn tui_action_reset_resizes_the_cockpit_20_50_30_columns_and_30_70_rows() {
    let fx = fx();
    let tmux = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).expect("tmux's stdout is UTF-8").trim().to_owned()
    };
    // `cpd:main` holds four views: %0 tree, %1 chat, %2 widgets, %3 plan.
    let first = tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-P", "-F", "#{pane_id}", "cpd tui view tree"]);
    tmux(&["set", "-p", "-t", &first, "@cpd_view", "tree"]);
    for view in ["chat", "widgets", "plan"] {
        let pane = tmux(&["split-window", "-d", "-t", &first, "-P", "-F", "#{pane_id}", &format!("cpd tui view {view}")]);
        tmux(&["set", "-p", "-t", &pane, "@cpd_view", view]);
    }
    // (tmux lists a pane split from %0 next to it, so the list order is not the creation order)
    let mut panes: Vec<String> = tmux(&["list-panes", "-t", "=cpd:main", "-F", "#{pane_id} #{@cpd_view}"]).lines().map(str::to_owned).collect();
    panes.sort();
    assert_eq!(panes, ["%0 tree", "%1 chat", "%2 widgets", "%3 plan"]);

    let before = fx.calls("tmux").len();
    action(&fx, &["reset", "--client", "/dev/pts/nobody"]);

    let resizes: Vec<Vec<String>> = fx.calls("tmux")[before..]
        .iter()
        .filter_map(|c| c.args.iter().position(|a| a == "resize-pane").map(|i| c.args[i..].to_vec()))
        .collect();
    let want: Vec<Vec<String>> = [
        ["resize-pane", "-t", "%0", "-x", "20%"],
        ["resize-pane", "-t", "%2", "-x", "30%"],
        ["resize-pane", "-t", "%3", "-y", "30%"],
    ]
    .iter()
    .map(|c| c.iter().map(|s| s.to_string()).collect())
    .collect();
    assert_eq!(resizes, want);
    assert!(resizes.iter().all(|c| c[2] != "%1"), "the chat slot is never resized");
}
