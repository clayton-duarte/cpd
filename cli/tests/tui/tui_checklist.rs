//! The `checklist` kind of the dialog kit (commands.md §13.2; hs-d549, hs-d551) on a private tmux server: rows
//! `[ ] <id> <short> — <detail>`, Space toggles, `a` toggles all, `[Approve] [Cancel]`, the outcome file.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{CPD, TestEnv, wait_until};
use serde_json::{Value, json};

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// `cpd tui view dialog checklist` over three rows, in a pane of 60 by 12.
fn start(env: &TestEnv) -> (String, PathBuf) {
    let items = env.root().join("items.json");
    let rows = json!([
        {"id": "r-1", "short": "use sqlite", "detail": "duplicate r-0"},
        {"id": "r-2", "short": "old plan", "detail": "superseded r-9"},
        {"id": "r-3", "short": "wire format", "detail": "documented docs/a.md"},
    ]);
    fs::write(&items, rows.to_string()).unwrap();
    let out = env.root().join("checklist-out.json");
    let (items_arg, out_arg) = (format!("--items={}", items.display()), format!("--out={}", out.display()));
    let args = ["new-session", "-d", "-s", "view", "-x", "60", "-y", "12", "-P", "-F", "#{pane_id}", CPD, "tui", "view", "dialog", "checklist", "--title=Title", &items_arg, &out_arg];
    (tmux_out(env, &args).trim().to_owned(), out)
}

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

fn wait_screen(env: &TestEnv, pane: &str, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), done)
}

fn keys(env: &TestEnv, pane: &str, keys: &[&str]) {
    let mut args = vec!["send-keys", "-t", pane];
    args.extend_from_slice(keys);
    tmux_out(env, &args);
}

fn outcome(env: &TestEnv, out: &PathBuf) -> Value {
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());
    serde_json::from_str(&fs::read_to_string(out).expect("the view wrote its outcome")).unwrap()
}

#[test]
fn checklist_draws_one_row_per_item_with_a_box() {
    let env = TestEnv::new();
    let (pane, _) = start(&env);
    let rows = wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Approve]")));
    for want in ["[ ] r-1 use sqlite — duplicate r-0", "[ ] r-2 old plan — superseded r-9", "[ ] r-3 wire format — documented docs/a.md"] {
        assert!(rows.iter().any(|l| l.contains(want)), "{want:?} in {rows:?}");
    }
    assert!(rows.iter().any(|l| l.contains("[Approve]") && l.contains("[Cancel]")), "{rows:?}");
}

#[test]
fn space_toggles_and_approve_writes_ticked_and_unticked() {
    let env = TestEnv::new();
    let (pane, out) = start(&env);
    wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Approve]")));
    // tick the first row, move down two rows and tick the third
    keys(&env, &pane, &["Space", "Down", "Down", "Space"]);
    let rows = wait_screen(&env, &pane, "the ticks", |r| r.iter().any(|l| l.contains("[x] r-3")));
    assert!(rows.iter().any(|l| l.contains("[x] r-1")), "{rows:?}");
    assert!(rows.iter().any(|l| l.contains("[ ] r-2")), "{rows:?}");
    keys(&env, &pane, &["Enter"]);
    assert_eq!(outcome(&env, &out), json!({"approved": true, "ticked": ["r-1", "r-3"], "unticked": ["r-2"]}));
}

#[test]
fn cancel_writes_not_approved() {
    let env = TestEnv::new();
    let (pane, out) = start(&env);
    wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Cancel]")));
    keys(&env, &pane, &["a", "Escape"]);
    assert_eq!(outcome(&env, &out)["approved"], json!(false));
}
