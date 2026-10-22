//! `context manager --by-user` (commands.md §3.10, §10.8 step 3, hs-d542).

use crate::common;

use std::fs;

use common::{SLEEP_SECONDS, TestEnv, wait_until};

/// A fake `pi` that creates its session file and sleeps.
fn fake_pi(env: &TestEnv) {
    env.fake_pi(&format!(
        "id=''; prev=''\nfor a in \"$@\"; do [ \"$prev\" = --session-id ] && id=$a; prev=$a; done\n\
         if [ -n \"$id\" ]; then mkdir -p \"$HOME/.pi/agent/sessions/g\"; \
         printf '{{\"type\":\"session\"}}\\n' > \"$HOME/.pi/agent/sessions/g/2026_$id.jsonl\"; fi\n\
         exec sleep {SLEEP_SECONDS}"
    ));
}

/// The cockpit: session `cpd`, window `main` with a tree pane and the placeholder in the chat slot.
fn cockpit(env: &TestEnv) {
    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let tree = ok(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "215", "-y", "62", "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]);
    let chat = ok(&["split-window", "-d", "-t", &tree, "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
}

fn setup(env: &TestEnv) {
    env.write_tmux_config("");
    env.fake_cpd_views();
    fake_pi(env);
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    cockpit(env);
}

/// The argv of the one pi start, once it has happened.
fn pi_argv(env: &TestEnv) -> Vec<String> {
    wait_until("the fake pi", || !env.calls("pi").is_empty());
    env.calls("pi")[0].clone()
}

fn manager(env: &TestEnv, by_user: bool) -> String {
    let mut args = vec!["context", "manager", "--context", "ctx"];
    if by_user {
        args.push("--by-user");
    }
    env.cpd(&args).ok()["manager"]["session"].as_str().unwrap().to_owned()
}

#[test]
fn context_manager_by_user_starts_the_manager_without_a_first_message() {
    let env = TestEnv::new();
    setup(&env);
    let id = manager(&env, true);
    let argv = pi_argv(&env);
    assert!(!argv.iter().any(|a| a == "--" || a.starts_with('@')), "no first message file: {argv:?}");
    assert!(!env.cpd_home().join("run/ctx/manager-message.md").exists(), "no first-message file");
    assert_eq!(env.cpd(&["session", "show", &id]).ok()["session"]["by_user"], true);
}

#[test]
fn context_manager_without_by_user_keeps_the_first_message() {
    let env = TestEnv::new();
    setup(&env);
    let id = manager(&env, false);
    let argv = pi_argv(&env);
    let path = env.cpd_home().join("run/ctx/manager-message.md");
    assert_eq!(argv[argv.len() - 2..], ["--".to_owned(), format!("@{}", path.display())], "{argv:?}");
    assert!(fs::read_to_string(&path).unwrap().starts_with("You are the Manager of Context ctx"));
    assert_eq!(env.cpd(&["session", "show", &id]).ok()["session"]["by_user"], false);
}
