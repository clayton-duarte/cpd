//! `cpd tui action costs` (docs/commands.md §13.2; pro-d189, pro-d196) sends the literal key `$` to the tree pane of
//! `cpd:main`, in the test process, on the fake tmux server of an `Fx`: what the test pins is the keys the fake got and,
//! with no tree pane, the message shown on the clicking client.

use crate::common;

use common::cockpit::action;
use common::fx::Fx;
use common::tree::fx;
use cpd::fakes::tmux::KeyLog;

/// `cpd:main` holding a pane per view of `views`, each with its `@cpd_view`, the first made with `new-session`.
fn cockpit(fx: &Fx, views: &[&str]) {
    let tmux = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).expect("tmux's stdout is UTF-8").trim().to_owned()
    };
    let first = tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-P", "-F", "#{pane_id}", &format!("cpd tui view {}", views[0])]);
    tmux(&["set", "-p", "-t", &first, "@cpd_view", views[0]]);
    for view in &views[1..] {
        let pane = tmux(&["split-window", "-d", "-t", &first, "-P", "-F", "#{pane_id}", &format!("cpd tui view {view}")]);
        tmux(&["set", "-p", "-t", &pane, "@cpd_view", view]);
    }
}

#[test]
fn tui_action_costs_sends_the_dollar_key_to_the_tree_pane() {
    let fx = fx();
    cockpit(&fx, &["tree", "chat", "widgets", "plan"]); // %0 is the tree
    action(&fx, &["costs", "--client", "/dev/pts/9"]);
    let keys = fx.tmux_state().unwrap().keys();
    assert_eq!(keys, [KeyLog { pane: "%0".into(), keys: vec!["$".into()], literal: true }], "one literal `$`, to the tree's pane only");
}

#[test]
fn tui_action_costs_without_a_tree_pane_says_so_on_the_client() {
    let fx = fx();
    cockpit(&fx, &["chat", "widgets", "plan"]);
    let before = fx.calls("tmux").len();
    action(&fx, &["costs", "--client", "/dev/pts/9"]); // exits 0 and prints nothing
    assert!(fx.tmux_state().unwrap().keys().is_empty(), "no key went to any pane");
    let messages: Vec<Vec<String>> = fx.calls("tmux")[before..]
        .iter()
        .map(|c| c.args.clone())
        .filter(|a| a.iter().any(|x| x == "display-message"))
        .collect();
    assert_eq!(messages.len(), 1, "one message on the client: {messages:?}");
    let at = messages[0].iter().position(|a| a == "display-message").unwrap();
    assert_eq!(messages[0][at + 1..at + 4], ["-l", "-c", "/dev/pts/9"], "{:?}", messages[0]);
    let text = &messages[0][at + 4];
    assert!(text.contains("cpd tui action"), "the message names the action command: {text}");
    assert!(!text.contains("unknown action"), "`costs` is a known action that failed for want of a tree pane: {text}");
}
