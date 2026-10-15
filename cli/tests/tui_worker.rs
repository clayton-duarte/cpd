//! `cpd2 tui view worker` (commands.md §13.2, §17), end to end: the built binary
//! runs in a pane of a private tmux server (its terminal), over a prepared
//! store and a fixture pi session file; a test reads the screen
//! (`capture-pane`), appends to the file and presses keys (`send-keys`). The
//! rendering of messages, wrapping and scrolling is unit-tested in
//! `cli/src/tui/worker/`.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{CPD2, TestEnv};
use cpd2::error::CpdError;
use cpd2::home::CpdHome;
use cpd2::store::Store;
use serde_json::json;

// ---------- the prepared store ----------

fn env() -> TestEnv {
    let env = TestEnv::new();
    std::fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n").unwrap();
    env
}

fn sql(env: &TestEnv, text: &str) {
    let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// Plan `ship` with Job `api` and its first Attempt `api-a1`, running. Its Worker Session `w1`, `working`, has
/// the session file `file`, or none registered yet.
fn populate(env: &TestEnv, file: Option<&Path>) {
    let file = file.map_or("NULL".to_owned(), |f| format!("'{}'", f.display()));
    sql(
        env,
        &format!(
            "INSERT INTO contexts (id, title, jira, created_at, created_on, lead_session, grounded_seq)
             VALUES ('alpha', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1', NULL, NULL);
             INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at, locked_at)
             VALUES ('ship', 'alpha', 'Plan ship', '/repo', 'main', 'cpd/ship', '/wt', 'dispatch', 'l1',
                     '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
             VALUES ('l1', 'lead', 'alpha', 'ship', NULL, 'lead', '/sessions/l1.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                     '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
             INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               created_txn, created_at)
             VALUES ('ship', 'api', 'Job api', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{}',
                     cpd_txn(), '2026-01-02T00:00:00.000Z');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
             VALUES ('ship', 'api-a1', 'api', 1, 'S3', 'ladder', 'running', 'cpd/ship--api-a1', '/w/api-a1', 'w1',
                     '2026-01-03T00:01:00.000Z');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
             VALUES ('w1', 'worker', 'alpha', 'ship', 'api-a1', 'api-a1', {file}, '/w/api-a1', 'S3', NULL, 'working', NULL,
                     '2026-01-03T00:01:00.000Z', '2026-01-03T00:01:00.000Z');",
            "0".repeat(64),
        ),
    );
}

fn entry(role: &str, part: serde_json::Value) -> String {
    json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z", "message": {"role": role, "content": [part]}}).to_string() + "\n"
}

fn session_file(env: &TestEnv) -> PathBuf {
    let path = env.root().join("worker.jsonl");
    let text = [
        json!({"type": "session", "id": "w1"}).to_string() + "\n",
        entry("user", json!({"type": "text", "text": "Implement the API."})),
        entry("assistant", json!({"type": "text", "text": "On it."})),
        entry("assistant", json!({"type": "toolCall", "name": "bash", "arguments": {"command": "cargo test"}})),
        entry("toolResult", json!({"type": "text", "text": "test result: ok"})),
    ]
    .concat();
    std::fs::write(&path, text).unwrap();
    path
}

// ---------- the pane ----------

const SESSION: &str = "worker";

struct Pane<'a> {
    env: &'a TestEnv,
}

impl<'a> Pane<'a> {
    /// Runs `cpd2 tui view worker api-a1 --plan ship` in a `width` × `height` pane of the env's private tmux server.
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
            &format!("{CPD2} tui view worker api-a1 --plan ship"),
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
        let start = Instant::now();
        loop {
            let screen = self.screen();
            if done(&screen) {
                return screen;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "waiting for {what}; the screen is:\n{}", screen.join("\n"));
            std::thread::sleep(Duration::from_millis(50));
        }
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

const STATUS: &str = " api-a1 running · working";

// ---------- tests ----------

#[test]
fn the_session_file_of_the_worker_is_rendered_read_only() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file));
    let pane = Pane::start(&env, 40, 12);
    let screen = pane.wait("the conversation", |s| s.iter().any(|r| r.contains("test result")));
    assert_eq!(
        screen,
        [
            "16:33:44 you",
            "      Implement the API.",
            "16:33:44 agent",
            "      On it.",
            "16:33:44 → bash cargo test",
            "        ← test result: ok",
            "",
            "",
            "",
            "",
            "",
            STATUS,
        ]
    );
}

#[test]
fn an_appended_entry_is_followed_at_the_end() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file));
    let pane = Pane::start(&env, 40, 8);
    pane.wait("the first screen", |s| s.iter().any(|r| r.contains("test result")));
    let mut f = std::fs::OpenOptions::new().append(true).open(&file).unwrap();
    for n in 1..=3 {
        f.write_all(entry("assistant", json!({"type": "text", "text": format!("Appended {n}.")})).as_bytes()).unwrap();
    }
    let screen = pane.wait("the appended entries", |s| s.iter().any(|r| r.contains("Appended 3.")));
    assert_eq!(screen[screen.len() - 3..], ["16:33:44 agent", "      Appended 3.", STATUS].map(str::to_owned), "{screen:?}");
    assert_eq!(screen.len(), 8, "the end is in view: {screen:?}");
}

#[test]
fn scrolling_leaves_the_end_and_q_ends_the_view() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file));
    let pane = Pane::start(&env, 40, 4);
    pane.wait("the end", |s| s.iter().any(|r| r.contains("test result")));
    pane.keys(&["g"]); // not a key
    pane.keys(&["k", "k", "k", "k", "k", "k"]);
    let screen = pane.wait("the top", |s| s.first().is_some_and(|r| r.contains("you")));
    assert_eq!(screen, ["16:33:44 you", "      Implement the API.", "16:33:44 agent", STATUS]);
    pane.keys(&["G"]);
    pane.wait("the end again", |s| s.iter().any(|r| r.contains("test result")));
    pane.keys(&["q"]);
    let start = Instant::now();
    while !pane.dead() {
        assert!(start.elapsed() < Duration::from_secs(10), "q did not end the view");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_session_without_its_file_yet_says_so_and_shows_it_once_it_exists() {
    let env = env();
    let file = env.root().join("later.jsonl");
    populate(&env, Some(&file));
    let pane = Pane::start(&env, 60, 5);
    let screen = pane.wait("the placeholder", |s| s.iter().any(|r| r.contains("No session file at")));
    assert!(screen[0].starts_with(" No session file at ") && screen[0].contains("later.jsonl"), "{screen:?}");
    std::fs::write(&file, entry("user", json!({"type": "text", "text": "Hello."}))).unwrap();
    pane.wait("the file", |s| s.iter().any(|r| r.contains("Hello.")));
}

#[test]
fn a_session_with_no_file_registered_says_so() {
    let env = env();
    populate(&env, None);
    let pane = Pane::start(&env, 70, 3);
    let screen = pane.wait("the placeholder", |s| s.iter().any(|r| r.contains("No session file yet")));
    assert_eq!(screen[0], " No session file yet: the Worker has not started its Session.");
}

#[test]
fn an_unknown_attempt_or_plan_is_refused_before_drawing() {
    let env = env();
    populate(&env, None);
    let run = env.cpd2(&["tui", "view", "worker", "nope-a1", "--plan", "ship"]);
    assert_eq!(X, "{run:?}");
    let run = env.cpd2(&["tui", "view", "worker", "api-a1", "--plan", "nope"]);
    assert_eq!(run.error()["code"], "plan_not_found", "{run:?}");
    let run = env.cpd2(&["tui", "view", "worker", "api-a1"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("plan_required")), "{run:?}");
    let run = env.cpd2(&["tui", "view", "worker"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("usage")), "{run:?}");
}

#[test]
fn the_view_needs_a_terminal() {
    let env = env();
    populate(&env, None);
    let run = env.cpd2(&["tui", "view", "worker", "api-a1", "--plan", "ship"]);
    assert_eq!(run.exit, 3, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "not_a_terminal");
    assert_eq!(error["details"]["command"], "tui view worker");
}
