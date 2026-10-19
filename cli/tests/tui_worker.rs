//! `cpd tui view worker` (commands.md §13.2, §17), end to end: the built binary
//! runs in a pane of a private tmux server (its terminal), over a prepared
//! store and a fixture pi session file; a test reads the screen
//! (`capture-pane`), appends to the file and presses keys (`send-keys`). The
//! rendering of messages, wrapping and scrolling is unit-tested in
//! `tui/src/worker/`.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};

use common::{CPD, SLEEP_SECONDS, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::tmux::PANE_DIED_HOOK;
use cpd_core::store::Store;
use serde_json::json;

// ---------- the prepared store ----------

fn env() -> TestEnv {
    let env = TestEnv::new();
    let config = format!(
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    std::fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env
}

/// Plan `ship` with Job `api` and its first Attempt `api-a1`, running. Its Worker Session `w1`, `working`, has
/// the session file `file`, or none registered yet.
fn populate(env: &TestEnv, file: Option<&Path>) {
    let file = file.map_or("NULL".to_owned(), |f| format!("'{}'", f.display()));
    env.sql(
        &format!(
            "INSERT INTO contexts (id, title, jira, created_at, created_on)
             VALUES ('alpha', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
             VALUES ('ship', 'alpha', 'Plan ship', 'dispatch', 'l1', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
             VALUES ('ship', 'repo', '/repo', 'main', 'dispatch', 'cpd/ship', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z',
                     '2026-01-02T00:00:00.000Z');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
             VALUES ('l1', 'lead', 'alpha', 'ship', NULL, 'lead', '/sessions/l1.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                     '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               created_txn, created_at)
             VALUES ('ship', 'repo', 'api', 'Job api', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{}',
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
    /// Runs `cpd tui view worker api-a1 --plan ship` in a `width` × `height` pane of the env's private tmux server.
    fn start(env: &'a TestEnv, width: u16, height: u16) -> Pane<'a> {
        Pane::start_command(env, width, height, &format!("{CPD} tui view worker api-a1 --plan ship"))
    }

    /// Runs `command` as the cockpit runs a view: the pane carries `@cpd_view` before the command starts, and the
    /// server has the cockpit's `pane-died` hook, so a view that exits is started again (contract §6.1).
    fn start_command(env: &'a TestEnv, width: u16, height: u16, command: &str) -> Pane<'a> {
        let out = env.tmux(&[
            "-u",
            "set-option",
            "-g",
            "remain-on-exit",
            "on",
            ";",
            "set-hook",
            "-g",
            "pane-died",
            PANE_DIED_HOOK,
            ";",
            "new-session",
            "-d",
            "-s",
            SESSION,
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
            &format!("sleep {SLEEP_SECONDS}"),
            ";",
            "set-option",
            "-p",
            "-t",
            SESSION,
            "@cpd_view",
            "worker",
            ";",
            "respawn-pane",
            "-k",
            "-t",
            SESSION,
            command,
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

    /// Whether the pane is gone: it was the session's only pane, so the session goes with it.
    fn closed(&self) -> bool {
        !self.env.tmux(&["has-session", "-t", SESSION]).status.success()
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
            "        󰘌 test result: ok",
            "",
            "",
            "",
            "",
            "",
            STATUS,
        ]
    );
}

/// `you` and the User's text are cyan, `agent` green, `→ tool` yellow (the reference's `Ui` palette).
#[test]
fn the_roles_are_in_the_reference_colours() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file));
    let pane = Pane::start(&env, 40, 12);
    pane.wait("the conversation", |s| s.iter().any(|r| r.contains("test result")));
    let out = env.tmux(&["capture-pane", "-p", "-e", "-t", SESSION]);
    let raw = String::from_utf8(out.stdout).unwrap();
    let row_with = |text: &str| raw.lines().find(|l| l.contains(text)).unwrap_or_else(|| panic!("no row with {text}:\n{raw}")).to_owned();
    // tmux reports the 16 ANSI colours as `38;5;N`: cyan 6, green 2, yellow 3
    assert!(row_with("you").contains("\x1b[38;5;6myou"), "you: {raw:?}");
    assert!(row_with("Implement the API.").contains("\x1b[38;5;6m      Implement"), "the User's text: {raw:?}");
    assert!(row_with("agent").contains("\x1b[38;5;2magent"), "agent: {raw:?}");
    assert!(row_with("→ bash").contains("\x1b[38;5;3m→ bash"), "tool: {raw:?}");
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
fn scrolling_leaves_the_end_and_q_closes_the_pane() {
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
    // The pane carries `@cpd_view` and the server respawns a dead one: `q` must close the pane, not end the program.
    pane.keys(&["q"]);
    common::wait_until("q to close the pane", || pane.closed());
}

/// A view that fails at start (here: no such Attempt) is started again by the hook; it waits before it exits and
/// says why, so it is not started in a tight loop.
#[test]
fn a_view_that_fails_to_start_backs_off_instead_of_looping() {
    let env = env();
    populate(&env, None);
    let log = env.root().join("starts");
    let pane = Pane::start_command(&env, 80, 6, &format!("echo start >> {}; {CPD} tui view worker nosuch-a9 --plan ship", log.display()));
    pane.wait("the failure on screen", |s| s.iter().any(|r| r.contains("nosuch-a9")));
    // time-claim: over 6 s, a view that fails at start is started at most 4 times (it backs off, no tight loop)
    std::thread::sleep(std::time::Duration::from_secs(6));
    let starts = std::fs::read_to_string(&log).unwrap().lines().count();
    assert!((1..=4).contains(&starts), "{starts} starts in 6 s: a tight loop");
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

const EVENT: &str = "INSERT INTO events (txn, at, kind, context, plan, attempt, session, text, data)
     VALUES (cpd_txn(), '2026-01-03T00:02:00.000Z', 'session-state', 'alpha', 'ship', 'api-a1', 'w1', 'fixture', '{}');";

/// The status at the foot follows the Events (commands.md §17): a Session that changed state shows its new one.
#[test]
fn the_status_follows_the_events_of_the_store() {
    let env = env();
    populate(&env, None);
    let pane = Pane::start(&env, 70, 3);
    pane.wait("the status", |s| s.iter().any(|r| r == STATUS));
    env.sql(&format!("UPDATE sessions SET state = 'idle' WHERE id = 'w1'; {EVENT}"));
    pane.wait("the new state", |s| s.iter().any(|r| r == " api-a1 running · idle"));
}

/// A follow that fails is shown, the snapshot stays (tui-G4): the highest Event id fell below the cursor, which is
/// what a replaced database looks like.
#[test]
fn a_follow_that_fails_is_shown_and_the_status_stays() {
    let env = env();
    populate(&env, None);
    env.sql(EVENT);
    let pane = Pane::start(&env, 100, 3);
    pane.wait("the status", |s| s.iter().any(|r| r == STATUS));
    // not through `Store`, which keeps the Events append-only
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("DROP TRIGGER events_no_delete; DELETE FROM events;");
    let screen = pane.wait("the error", |s| s.iter().any(|r| r.contains("Event cursor")));
    assert!(screen.iter().any(|r| r.contains("No session file yet")), "the view stays: {screen:?}");
}

#[test]
fn an_unknown_attempt_or_plan_is_refused_before_drawing() {
    let env = env();
    populate(&env, None);
    let run = env.cpd(&["tui", "view", "worker", "nope-a1", "--plan", "ship"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (4, json!("attempt_not_found")), "{run:?}");
    let run = env.cpd(&["tui", "view", "worker", "api-a1", "--plan", "nope"]);
    assert_eq!(run.error()["code"], "plan_not_found", "{run:?}");
    let run = env.cpd(&["tui", "view", "worker", "api-a1"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("plan_required")), "{run:?}");
    let run = env.cpd(&["tui", "view", "worker"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("usage")), "{run:?}");
}

#[test]
fn the_view_needs_a_terminal() {
    let env = env();
    populate(&env, None);
    let run = env.cpd(&["tui", "view", "worker", "api-a1", "--plan", "ship"]);
    assert_eq!(run.exit, 3, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "not_a_terminal");
    assert_eq!(error["details"]["command"], "tui view worker");
}
