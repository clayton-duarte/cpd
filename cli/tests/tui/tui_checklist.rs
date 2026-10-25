//! The `checklist` kind of the dialog kit (commands.md §13.2; hs-d549, hs-d551, hs-d568, hs-d569 CD2, CD3) on a private
//! tmux server: what stays here is the refusal of an items file, whose JSON reply goes to the shell's stdout. The
//! drawing and the keys are in-process (`tui/src/dialog/checklist_tests.rs`, on a `TestBackend`).

use crate::common;

use std::fs;

use common::{CPD, TestEnv, wait_until};
use serde_json::{Value, json};

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

const SQLITE: &str = "use sqlite as the store";

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

/// The screen once `done`, with each row of the frame reduced to its content (the wall and the padding cut off both
/// sides, the right trimmed): the rows are as the content is drawn at column 0. The border rows stay as they are.
fn wait_screen(env: &TestEnv, pane: &str, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), done).iter().map(|l| inside(l)).collect()
}

fn inside(line: &str) -> String {
    let cells: Vec<char> = line.chars().collect();
    if cells.first() == Some(&'│') && cells.last() == Some(&'│') && cells.len() >= 4 {
        cells[2..cells.len() - 2].iter().collect::<String>().trim_end().to_owned()
    } else {
        line.to_owned()
    }
}

/// A pill: its label between the caps U+E0B6 and U+E0B4, no padding.
fn pill(label: &str) -> String {
    format!("\u{E0B6}{label}\u{E0B4}")
}

fn ready(r: &[String]) -> bool {
    r.iter().any(|l| l.contains(&pill("Approve")))
}

/// The row of the line that contains `want`.
fn row_of(rows: &[String], want: &str) -> usize {
    rows.iter().position(|l| l.contains(want)).unwrap_or_else(|| panic!("{want:?} in {rows:?}"))
}

#[test]
fn an_items_file_with_a_short_field_is_refused() {
    let env = TestEnv::new();
    let items = env.root().join("items.json");
    let stdout = env.root().join("stdout.json");
    let run = |rows: Value, redirect: bool| {
        fs::write(&items, rows.to_string()).unwrap();
        let to = if redirect { format!(" > {}", stdout.display()) } else { String::new() };
        let shell = format!("{CPD} tui view dialog checklist --title=Title --items={} --out={}/o.json{to}", items.display(), env.root().display());
        tmux_out(&env, &["new-session", "-d", "-s", "view", "-x", "60", "-y", "12", "-P", "-F", "#{pane_id}", "sh", "-c", &shell]).trim().to_owned()
    };
    run(json!([{"id": "r-1", "short": "use sqlite", "text": SQLITE, "detail": "duplicate r-0"}]), true);
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());
    let reply: Value = serde_json::from_str(&fs::read_to_string(&stdout).unwrap()).unwrap();
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert_eq!(reply["error"]["code"], json!("args_invalid"), "{reply}");
    assert!(reply["error"]["message"].as_str().unwrap().contains("short"), "the refusal names the field: {reply}");
    // the same entry without `short` is the shape: drawn, its text under the header
    let pane = run(json!([{"id": "r-1", "text": SQLITE, "detail": "duplicate r-0"}]), false);
    let rows = wait_screen(&env, &pane, "the dialog", ready);
    let h = row_of(&rows, "[ ] r-1 — duplicate r-0");
    assert_eq!(rows[h + 1].trim(), SQLITE, "{rows:?}");
}
