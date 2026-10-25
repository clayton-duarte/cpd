//! `cpd tui view worker` (commands.md §13.2, §17): the view runs in this process, over a prepared store and a fixture
//! pi session file, on a `TestBackend` with scripted input and a `ManualClock` (`worker::run_on`); a test reads the
//! screen, changes the store or the file between two polls and lets the clock run the 500 ms check. The rendering of
//! messages, wrapping and scrolling is unit-tested in `tui/src/worker/`.
//!
//! Two tests stay in a pane of a private tmux server, their subject being tmux's (`mig-tui-a-3` and `mig-tui-a-4`
//! migrate them): `scrolling_leaves_the_end_and_q_closes_the_pane` (`q` kills the pane `$TMUX_PANE` names) and
//! `an_unknown_attempt_or_plan_is_refused_before_drawing` (the root parser's `usage` refusals).

use crate::common;

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use clap::Parser;
use common::fx::Fx;
use common::screen::{Bytes, Display, FakeTty, Screen, Script, Step, ch, idle, then};
use common::{CPD, SLEEP_SECONDS, TestEnv};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::tmux::PANE_DIED_HOOK;
use cpd_core::output::Output;
use cpd_core::store::Store;
use cpd_tui::view_ports::backed_off_with;
use cpd_tui::worker;
use serde_json::json;

// ---------- the prepared store ----------

fn config(env: &TestEnv) {
    let config = format!(
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    env.write_config(&config);
}

/// A real env, for the tests that still run in a pane.
fn env() -> TestEnv {
    let env = TestEnv::new();
    config(&env);
    env
}

/// An env in this process: the fake ports, the store and the config of [`config`].
fn fx() -> Fx {
    let fx = Fx::with_ports();
    config(&fx.env);
    fx
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

// ---------- the view in this process ----------

/// The Job form's rows and helpers (commands.md §13.2, Job `planview-docs`): the reason header (P4, P5, P6).
const PLACEHOLDER: &str = " No Attempt yet: the Job has not started.";

/// A Job `key` of PR `repo` in `status`, with `reason` as its cancel reason.
fn job(key: &str, status: &str, reason: Option<&str>) -> String {
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let (reason, requested) = match reason {
        Some(r) => (format!("'{r}'"), "'2026-01-04T00:00:00.000Z'"),
        None => ("NULL".to_owned(), "NULL"),
    };
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, merged_sha, created_at)
         VALUES ('ship', 'repo', '{key}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{}',
                 cpd_txn(), {reason}, {requested}, {merged}, '2026-01-02T00:00:00.000Z');",
        "0".repeat(64)
    )
}

/// Another process commits `text` to the store of `$CPD_HOME` `home` while the view runs.
fn sql_in(home: &Path, text: &str) {
    let home = CpdHome::resolve(Some(home.as_os_str().to_owned()), None).unwrap();
    common::sql(&mut Store::open(&home, 5000).unwrap(), text);
}

/// The arguments of `cpd tui view worker <argv>`, parsed as the command's parser does.
fn args(argv: &[&str]) -> worker::Args {
    #[derive(Parser)]
    struct Command {
        #[command(flatten)]
        args: worker::Args,
    }
    Command::try_parse_from(std::iter::once("worker").chain(argv.iter().copied())).unwrap().args
}

/// What `cpd tui view worker <argv>` did in a `width` x `height` terminal that is there or not (`terminal`): its result,
/// and the bytes it wrote to the terminal. `steps` are what the terminal does at each poll; the view returns when the
/// last step ends it (`q`).
fn view(fx: &Fx, argv: &[&str], terminal: bool, display: &Display, steps: Vec<Step>) -> (Result<Output, CpdError>, Vec<u8>) {
    let mut input = Script::new(fx.fakes.ports.clock.clone(), steps);
    let mut out = Bytes::default();
    let result = worker::run_on(&fx.fakes.ports, args(argv), &mut FakeTty::new(terminal), &mut out, display.terminal(), &mut input);
    assert_eq!(input.remaining(), 0, "the view ended before its script: {result:?}");
    (result, out.bytes())
}

/// The screens a script took: each one a row of text, without trailing blanks and without the empty rows at the end.
#[derive(Clone, Default)]
struct Shots(Rc<RefCell<Vec<Vec<String>>>>);

impl Shots {
    /// A step that takes the screen as drawn so far.
    fn take(&self, display: &Display) -> Step {
        let (shots, display) = (self.0.clone(), display.clone());
        then(move || shots.borrow_mut().push(rows(&display.screen())))
    }

    fn get(&self, i: usize) -> Vec<String> {
        self.0.borrow()[i].clone()
    }
}

fn rows(screen: &Screen) -> Vec<String> {
    let mut rows = screen.rows();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

/// The polls after which the view's 500 ms check (§17) has come, one tick of 100 ms each, with one to spare.
fn a_check() -> Vec<Step> {
    idle(6)
}

/// The steps of a view that shows its first screen, lets `change` happen, runs a check, shows its screen again and is
/// closed by `q`.
fn after(shots: &Shots, display: &Display, change: Step) -> Vec<Step> {
    let mut steps = vec![shots.take(display), change];
    steps.extend(a_check());
    steps.extend([shots.take(display), Step::Event(ch('q'))]);
    steps
}

// ---------- tests ----------

#[test]
fn the_job_form_of_an_unknown_job_is_refused_before_drawing() {
    let fx = fx();
    populate(&fx.env, None);
    let (result, written) = view(&fx, &["--job", "nope", "--plan", "ship"], true, &Display::new(60, 12), Vec::new());
    let e = result.unwrap_err();
    assert_eq!((e.exit().code(), e.code()), (4, ErrorCode::JobNotFound), "{e:?}");
    assert!(written.is_empty(), "nothing is written to the terminal: {written:?}");
}

/// The pane opened for a Job without an Attempt follows it to its first Attempt.
#[test]
fn the_job_form_follows_the_job_to_its_first_attempt() {
    let fx = fx();
    let file = session_file(&fx.env);
    populate(&fx.env, None);
    fx.env.sql(&job("later", "queued", None));
    let (display, shots) = (Display::new(60, 12), Shots::default());
    let home = fx.env.cpd_home();
    let change = then(move || {
        sql_in(
            &home,
            &format!(
                "UPDATE jobs SET status = 'running' WHERE key = 'later';
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
                 VALUES ('ship', 'later-a1', 'later', 1, 'S3', 'ladder', 'running', 'cpd/ship--later-a1', '/w/later-a1', 'w2',
                         '2026-01-03T00:01:00.000Z');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
                 VALUES ('w2', 'worker', 'alpha', 'ship', 'later-a1', 'later-a1', '{}', '/w/later-a1', 'S3', NULL, 'working', NULL,
                         '2026-01-03T00:01:00.000Z', '2026-01-03T00:01:00.000Z');
                 INSERT INTO events (txn, at, kind, context, plan, attempt, session, text, data)
                 VALUES (cpd_txn(), '2026-01-03T00:02:00.000Z', 'attempt-started', 'alpha', 'ship', 'later-a1', 'w2', 'fixture', '{{}}');",
                file.display()
            ),
        );
    });
    let steps = after(&shots, &display, change);
    view(&fx, &["--job", "later", "--plan", "ship"], true, &display, steps).0.unwrap();
    let screen = shots.get(0);
    assert_eq!(screen[0], PLACEHOLDER, "{screen:?}");
    let screen = shots.get(1);
    assert_eq!(screen[0], "16:33:44 you", "{screen:?}");
    assert_eq!(screen.last().map(String::as_str), Some(" later-a1 running · working"), "{screen:?}");
}

#[test]
fn an_appended_entry_is_followed_at_the_end() {
    let fx = fx();
    let file = session_file(&fx.env);
    populate(&fx.env, Some(&file));
    let (display, shots) = (Display::new(40, 8), Shots::default());
    let appended = file.clone();
    let change = then(move || {
        let mut f = std::fs::OpenOptions::new().append(true).open(&appended).unwrap();
        for n in 1..=3 {
            f.write_all(entry("assistant", json!({"type": "text", "text": format!("Appended {n}.")})).as_bytes()).unwrap();
        }
    });
    let steps = after(&shots, &display, change);
    view(&fx, &["api-a1", "--plan", "ship"], true, &display, steps).0.unwrap();
    assert!(shots.get(0).iter().any(|r| r.contains("test result")), "the first screen: {:?}", shots.get(0));
    let screen = shots.get(1);
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
    let fx = fx();
    populate(&fx.env, None);
    let clock = &fx.fakes.clock;
    let (mut starts, mut said) = (0, Vec::new());
    // time-claim: over 6 s of virtual time, a view that fails at start is started at most 4 times (it backs off, no tight loop)
    while clock.elapsed() < Duration::from_secs(6) {
        starts += 1;
        let (failed, written) = view(&fx, &["nosuch-a9", "--plan", "ship"], true, &Display::new(80, 6), Vec::new());
        assert!(written.is_empty(), "it fails before it draws: {written:?}");
        let before = clock.elapsed();
        let e = backed_off_with(&fx.fakes.ports, true, &mut said, failed).unwrap_err();
        assert_eq!(clock.elapsed() - before, Duration::from_secs(3), "one back-off of 3 s before it exits");
        assert!(e.message().contains("nosuch-a9"), "{e:?}");
    }
    let said = String::from_utf8(said).unwrap();
    assert!(said.contains("nosuch-a9") && said.contains("Starting again in 3 s."), "{said}");
    assert!((1..=4).contains(&starts), "{starts} starts in 6 s: a tight loop");
}

#[test]
fn a_session_without_its_file_yet_says_so_and_shows_it_once_it_exists() {
    let fx = fx();
    let file = fx.env.root().join("later.jsonl");
    populate(&fx.env, Some(&file));
    let (display, shots) = (Display::new(60, 5), Shots::default());
    let created = file.clone();
    let change = then(move || std::fs::write(&created, entry("user", json!({"type": "text", "text": "Hello."}))).unwrap());
    let steps = after(&shots, &display, change);
    view(&fx, &["api-a1", "--plan", "ship"], true, &display, steps).0.unwrap();
    let screen = shots.get(0);
    assert!(screen[0].starts_with(" No session file at ") && screen.join("").replace(' ', "").contains("later.jsonl"), "{screen:?}"); // the path may wrap in a long test root
    let screen = shots.get(1);
    assert!(screen.iter().any(|r| r.contains("Hello.")), "{screen:?}");
}

const EVENT: &str = "INSERT INTO events (txn, at, kind, context, plan, attempt, session, text, data)
     VALUES (cpd_txn(), '2026-01-03T00:02:00.000Z', 'session-state', 'alpha', 'ship', 'api-a1', 'w1', 'fixture', '{}');";

/// The status at the foot follows the Events (commands.md §17): a Session that changed state shows its new one.
#[test]
fn the_status_follows_the_events_of_the_store() {
    let fx = fx();
    populate(&fx.env, None);
    let (display, shots) = (Display::new(70, 3), Shots::default());
    let home = fx.env.cpd_home();
    let change = then(move || sql_in(&home, &format!("UPDATE sessions SET state = 'idle' WHERE id = 'w1'; {EVENT}")));
    let steps = after(&shots, &display, change);
    view(&fx, &["api-a1", "--plan", "ship"], true, &display, steps).0.unwrap();
    assert!(shots.get(0).iter().any(|r| r == STATUS), "{:?}", shots.get(0));
    assert!(shots.get(1).iter().any(|r| r == " api-a1 running · idle"), "{:?}", shots.get(1));
}

/// A follow that fails is shown, the snapshot stays (tui-G4): the highest Event id fell below the cursor, which is
/// what a replaced database looks like.
#[test]
fn a_follow_that_fails_is_shown_and_the_status_stays() {
    let fx = fx();
    populate(&fx.env, None);
    fx.env.sql(EVENT);
    let (display, shots) = (Display::new(100, 3), Shots::default());
    let db = fx.env.cpd_home().join("cpd.db");
    // not through `Store`, which keeps the Events append-only
    let change = then(move || {
        rusqlite::Connection::open(&db).unwrap().execute_batch("DROP TRIGGER events_no_delete; DELETE FROM events;").unwrap();
    });
    let steps = after(&shots, &display, change);
    view(&fx, &["api-a1", "--plan", "ship"], true, &display, steps).0.unwrap();
    assert!(shots.get(0).iter().any(|r| r == STATUS), "{:?}", shots.get(0));
    let screen = shots.get(1);
    assert!(screen.iter().any(|r| r.contains("Event cursor")), "{screen:?}");
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
    let fx = fx();
    populate(&fx.env, None);
    let (result, written) = view(&fx, &["api-a1", "--plan", "ship"], false, &Display::new(60, 12), Vec::new());
    let e = result.unwrap_err();
    assert_eq!(e.exit().code(), 3, "{e:?}");
    assert_eq!(e.code(), ErrorCode::NotATerminal);
    assert_eq!(e.details()["command"], "tui view worker");
    assert!(written.is_empty(), "nothing is written without a terminal: {written:?}");
}
