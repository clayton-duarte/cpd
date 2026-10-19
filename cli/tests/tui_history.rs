//! `cpd tui view history` (commands.md §13.2, §17), end to end: the built binary
//! runs in a pane of a private tmux server (its terminal), over a prepared
//! store and a fixture pi session file; a test reads the screen
//! (`capture-pane`) and presses keys (`send-keys`). Rendering and scrolling are
//! unit-tested in `tui/src/history/` and `tui/src/worker/`.

mod common;

use std::path::{Path, PathBuf};

use common::{CPD, TestEnv};
use serde_json::json;

// ---------- the prepared store ----------

fn env() -> TestEnv {
    let env = TestEnv::new();
    std::fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    env
}

/// Context `alpha` with the Lead Session `l1`, named `old lead`, with `file` (or none registered), archived or not.
fn populate(env: &TestEnv, file: Option<&Path>, archived: bool) {
    let file = file.map_or("NULL".to_owned(), |f| format!("'{}'", f.display()));
    let archived = if archived { "'2026-01-04T00:00:00.000Z'" } else { "NULL" };
    env.sql(
        &format!(
            "INSERT INTO contexts (id, title, jira, created_at, created_on)
             VALUES ('alpha', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at,
                                   created_at, archived_at)
             VALUES ('l1', 'lead', 'alpha', NULL, NULL, 'old lead', {file}, '/repo', 'O3', NULL, 'stopped', NULL,
                     '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z', {archived});",
        ),
    );
}

fn entry(role: &str, part: serde_json::Value) -> String {
    json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z", "message": {"role": role, "content": [part]}}).to_string() + "\n"
}

fn session_file(env: &TestEnv) -> PathBuf {
    let path = env.root().join("lead.jsonl");
    let text = [
        json!({"type": "session", "id": "l1"}).to_string() + "\n",
        entry("user", json!({"type": "text", "text": "Plan the API."})),
        entry("assistant", json!({"type": "text", "text": "Here is the plan."})),
        entry("assistant", json!({"type": "toolCall", "name": "bash", "arguments": {"command": "cpd plan show"}})),
        entry("toolResult", json!({"type": "text", "text": "ok"})),
    ]
    .concat();
    std::fs::write(&path, text).unwrap();
    path
}

// ---------- the pane ----------

const SESSION: &str = "history";
const TOP: &str = "old lead (archived, read-only)";
const PLACEHOLDER: &str = "No Lead here: pick a Plan in the session tree";

struct Pane<'a> {
    env: &'a TestEnv,
}

impl<'a> Pane<'a> {
    /// Runs `cpd tui view history l1` in a `width` × `height` pane of the env's private tmux server.
    fn start(env: &'a TestEnv, width: u16, height: u16) -> Pane<'a> {
        let out = env.tmux(&[
            "-u",
            "set-option",
            "-g",
            "remain-on-exit",
            "on",
            ";",
            "new-session",
            "-d",
            "-s",
            SESSION,
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
            &format!("{CPD} tui view history l1"),
        ]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Pane { env }
    }

    /// The screen, each row without trailing blanks, without the empty rows at the end.
    fn screen(&self) -> Vec<String> {
        let out = self.env.tmux(&["capture-pane", "-p", "-t", SESSION]);
        let text = String::from_utf8(out.stdout).unwrap();
        let mut rows: Vec<String> = text.lines().map(|l| l.trim_end().to_owned()).collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows
    }

    fn wait(&self, what: &str, done: impl Fn(&[String]) -> bool) -> Vec<String> {
        common::wait_screen(what, || self.screen(), done)
    }

    fn keys(&self, keys: &[&str]) {
        let mut args = vec!["send-keys", "-t", SESSION];
        args.extend(keys);
        assert!(self.env.tmux(&args).status.success());
    }

    fn dead(&self) -> bool {
        let out = self.env.tmux(&["list-panes", "-t", SESSION, "-F", "#{pane_dead}"]);
        String::from_utf8_lossy(&out.stdout).trim() == "1"
    }
}

// ---------- tests ----------

#[test]
fn the_session_file_of_an_archived_lead_is_rendered_read_only() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file), true);
    let pane = Pane::start(&env, 40, 12);
    let screen = pane.wait("the conversation", |s| s.iter().any(|r| r.contains("󰘌 ok")));
    assert_eq!(
        screen,
        [
            TOP,
            "16:33:44 you",
            "      Plan the API.",
            "16:33:44 agent",
            "      Here is the plan.",
            "16:33:44 → bash cpd plan show",
            "        󰘌 ok",
        ]
    );
}

#[test]
fn a_session_that_is_not_archived_says_read_only_only() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file), false);
    let pane = Pane::start(&env, 40, 12);
    let screen = pane.wait("the conversation", |s| s.iter().any(|r| r.contains("󰘌 ok")));
    assert_eq!(screen[0], "old lead (read-only)");
}

#[test]
fn the_keys_scroll_and_q_leaves_for_the_placeholder_text() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file), true);
    let pane = Pane::start(&env, 40, 4);
    pane.wait("the end", |s| s.iter().any(|r| r.contains("󰘌 ok")));
    pane.keys(&["g"]); // not a key
    pane.keys(&["k", "k", "k", "k", "k", "k"]);
    let screen = pane.wait("the top", |s| s.get(1).is_some_and(|r| r.contains("you")));
    assert_eq!(screen, [TOP, "16:33:44 you", "      Plan the API.", "16:33:44 agent"]);
    pane.keys(&["G"]);
    pane.wait("the end again", |s| s.iter().any(|r| r.contains("󰘌 ok")));
    pane.keys(&["q"]);
    // the placeholder wraps in a 40-column pane
    let screen = pane.wait("the placeholder", |s| s.join(" ").replace("  ", " ").contains(PLACEHOLDER));
    assert!(!pane.dead(), "the pane stays, showing the placeholder: {screen:?}");
    assert!(!screen.iter().any(|r| r.contains("read-only")), "the history is gone: {screen:?}");
}

#[test]
fn an_unknown_session_or_a_missing_file_is_refused_before_drawing() {
    let env = env();
    populate(&env, None, true);
    let run = env.cpd(&["tui", "view", "history", "nope"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (4, json!("session_not_found")), "{run:?}");
    let run = env.cpd(&["tui", "view", "history", "l1"]);
    assert_eq!(run.error()["code"], "file_unreadable", "a Session with no file: {run:?}");
    env.sql(&format!("UPDATE sessions SET file = '{}' WHERE id = 'l1'", env.root().join("gone.jsonl").display()));
    let run = env.cpd(&["tui", "view", "history", "l1"]);
    assert_eq!(run.error()["code"], "file_unreadable", "a file that is not there: {run:?}");
    let run = env.cpd(&["tui", "view", "history"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("usage")), "{run:?}");
}

#[test]
fn the_view_needs_a_terminal() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file), true);
    let run = env.cpd(&["tui", "view", "history", "l1"]);
    assert_eq!(run.exit, 3, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "not_a_terminal");
    assert_eq!(error["details"]["command"], "tui view history");
}
