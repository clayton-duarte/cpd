//! The worker view's screen over a prepared store and a fixture pi session file: the first frame the view draws
//! (`read::worker`, `refresh`, `View::draw`) on a `TestBackend`, no terminal and no process. The loop (follow, keys of a
//! pane, `q`) is the view's own and is not run here.

use std::path::{Path, PathBuf};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use serde_json::json;

use super::view::View;
use super::{Stamp, refresh};
use crate::palette::numbered::{C6, C7, C8};
use crate::read::{self, Target};
use crate::test_text::rows as buffer_rows;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, JobKey, PlanId};
use cpd_core::store::Store;

/// A store in a temporary home, which lives as long as `dir`.
struct Env {
    dir: tempfile::TempDir,
    store: Store,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&CpdHome::resolve(Some(dir.path().into()), None).unwrap(), 5000).unwrap();
    Env { dir, store }
}

impl Env {
    fn sql(&mut self, text: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(text)?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
    }

    /// The view as the loop starts it: the first snapshot of `target` taken and shown.
    fn view(&mut self, target: &Target) -> View {
        let worker = read::worker(&mut self.store, &PlanId::parse("ship").unwrap(), target).unwrap();
        let (mut view, mut shown): (View, Option<Stamp>) = (View::new(), None);
        refresh(&mut view, &mut shown, Some(&worker));
        view
    }

    fn attempt(&mut self) -> View {
        self.view(&Target::Attempt(AttemptId::parse("api-a1").unwrap()))
    }

    fn job(&mut self, key: &str) -> View {
        self.view(&Target::Job(JobKey::parse(key).unwrap()))
    }
}

/// Plan `ship` with Job `api` and its first Attempt `api-a1`, running. Its Worker Session `w1`, `working`, has
/// the session file `file`, or none registered yet.
fn populate(env: &mut Env, file: Option<&Path>) {
    let file = file.map_or("NULL".to_owned(), |f| format!("'{}'", f.display()));
    env.sql(&format!(
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
    ));
}

fn entry(role: &str, part: serde_json::Value) -> String {
    json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z", "message": {"role": role, "content": [part]}}).to_string() + "\n"
}

fn session_file(env: &Env) -> PathBuf {
    let path = env.dir.path().join("worker.jsonl");
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

/// What the view draws in a `width` × `height` pane.
fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// The screen, each row without trailing blanks, without the empty rows at the end.
fn screen(view: &mut View, width: u16, height: u16) -> Vec<String> {
    let mut rows = buffer_rows(&draw(view, width, height));
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn press(view: &mut View, code: KeyCode) {
    view.key(KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE });
}

const STATUS: &str = " api-a1 running · working";

#[test]
fn the_session_file_of_the_worker_is_rendered_read_only() {
    let mut env = env();
    let file = session_file(&env);
    populate(&mut env, Some(&file));
    let screen = screen(&mut env.attempt(), 40, 12);
    assert_eq!(
        screen,
        [
            "16:33:44 you",
            "      Implement the API.",
            "16:33:44 agent",
            "      On it.",
            "16:33:44 → bash cargo test",
            "        \u{F060C} test result: ok",
            "",
            "",
            "",
            "",
            "",
            STATUS,
        ]
    );
}

/// `you` and the User's text are grey (7), `agent` and `→ tool` cyan (6): contract §6.1.1.
#[test]
fn the_roles_are_in_the_palettes_colours() {
    let mut env = env();
    let file = session_file(&env);
    populate(&mut env, Some(&file));
    let buf = draw(&mut env.attempt(), 40, 12);
    let rows = buffer_rows(&buf);
    let row_with = |text: &str| rows.iter().position(|r| r.contains(text)).unwrap_or_else(|| panic!("no row with {text}:\n{rows:?}")) as u16;
    let fg = |x: u16, y: u16| buf[(x, y)].fg;
    // the cell of the first character of `text` on its row, and the cell `before` columns left of it
    let at = |text: &str, before: u16| {
        let y = row_with(text);
        let x = rows[y as usize].find(text).map(|i| rows[y as usize][..i].chars().count() as u16).unwrap();
        fg(x - before, y)
    };
    assert_eq!(at("you", 0), C7, "you: {rows:?}");
    assert_eq!((at("Implement", 6), at("Implement", 0)), (C7, C7), "the User's text: {rows:?}");
    assert_eq!(at("agent", 0), C6, "agent: {rows:?}");
    assert_eq!(at("→ bash", 0), C6, "tool: {rows:?}");
}

/// A Worker that asked its Lead waits for an agent, not for the User (hs-d303): the foot reads `waiting <label>`.
#[test]
fn the_foot_of_a_worker_waiting_for_its_lead_reads_waiting_and_its_label() {
    let mut env = env();
    let file = session_file(&env);
    populate(&mut env, Some(&file));
    env.sql("UPDATE sessions SET state = 'waiting', state_label = 'cpd: question to the Lead' WHERE id = 'w1';");
    let screen = screen(&mut env.attempt(), 60, 12);
    assert_eq!(screen.last().map(String::as_str), Some(" api-a1 running · waiting cpd: question to the Lead"), "{screen:?}");
}

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

fn need(job: &str, needs: &str) -> String {
    format!("INSERT INTO dependencies (plan, job, needs) VALUES ('ship', '{job}', '{needs}');")
}

fn rows(text: &[&str]) -> Vec<String> {
    text.iter().map(|r| (*r).to_owned()).collect()
}

/// The Job form of a cancelled Job without an Attempt: the reason at the top, in colour 8 (P4, P5).
#[test]
fn the_job_form_of_a_cancelled_job_without_an_attempt_says_why_at_the_top() {
    let mut env = env();
    populate(&mut env, None);
    env.sql(&job("dead", "cancelled", Some("the vendor API is gone for good so the export was dropped by the User")));
    let mut view = env.job("dead");
    let screen = screen(&mut view, 44, 8);
    assert_eq!(
        screen,
        rows(&[
            " cancelled: the vendor API is gone for good",
            " so the export was dropped by the User",
            "",
            PLACEHOLDER,
            "",
            "",
            "",
            " dead cancelled",
        ]),
        "{screen:?}"
    );
    let buf = draw(&mut view, 44, 8);
    assert_eq!(buf[(0, 0)].fg, C8, "the header is colour 8: {:?}", buffer_rows(&buf)[0]);
    assert!((0.." cancelled: the vendor".len() as u16).all(|x| buf[(x, 0)].fg == C8), "the header is colour 8 from its first cell");
}

/// The Job form of a queued Job: what it waits for, or nothing when its needs are merged.
#[test]
fn the_job_form_of_a_waiting_job_says_what_it_waits_for_at_the_top() {
    let mut env = env();
    populate(&mut env, None);
    env.sql(
        &[
            job("a", "queued", None),
            job("b", "queued", None),
            job("m", "merged", None),
            job("web", "queued", None),
            job("ok", "queued", None),
            need("web", "a"),
            need("web", "b"),
            need("ok", "m"),
        ]
        .concat(),
    );
    let screen = screen(&mut env.job("web"), 60, 5);
    assert_eq!(screen, rows(&[" waiting on a, b", "", PLACEHOLDER, "", " web waiting"]), "{screen:?}");

    let ok = self::screen(&mut env.job("ok"), 60, 3);
    assert_eq!(ok, rows(&[PLACEHOLDER, "", " ok queued"]), "{ok:?}");
}

/// An ended Attempt that failed says why at the top, pinned (P5), not at the foot: what the User reads when the
/// Worker of a failed Job is opened from its pill in the Plan view.
#[test]
fn the_attempt_form_of_a_failed_job_says_why_at_the_top_and_not_at_the_foot() {
    let mut env = env();
    let file = session_file(&env);
    populate(&mut env, Some(&file));
    env.sql(
        "UPDATE sessions SET state = 'stopped' WHERE id = 'w1';
         UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = '2026-01-03T00:05:00.000Z';
         UPDATE jobs SET status = 'failed', failed_cause = 'crashed';",
    );
    let mut view = env.attempt();
    let first = screen(&mut view, 60, 12);
    assert_eq!(first[..3], rows(&[" failed: crashed", "", "16:33:44 you"])[..], "{first:?}");
    assert_eq!(first.last().map(String::as_str), Some(" api-a1 ended · stopped"), "{first:?}");
    for _ in 0..8 {
        press(&mut view, KeyCode::Char('k'));
    }
    press(&mut view, KeyCode::PageUp);
    let scrolled = screen(&mut view, 60, 12);
    assert_eq!(scrolled[..2], rows(&[" failed: crashed", ""])[..], "pinned: {scrolled:?}");
    assert_eq!(scrolled.last().map(String::as_str), Some(" api-a1 ended · stopped"), "{scrolled:?}");
}

/// A running or merged Job has no header.
#[test]
fn the_attempt_form_of_a_merged_job_has_no_header() {
    let mut env = env();
    let file = session_file(&env);
    populate(&mut env, Some(&file));
    env.sql(
        "UPDATE sessions SET state = 'stopped' WHERE id = 'w1';
         UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = 'abc123', ended_at = '2026-01-03T00:05:00.000Z';
         UPDATE jobs SET status = 'merged', merged_sha = 'abc123';",
    );
    let screen = screen(&mut env.attempt(), 60, 12);
    assert_eq!(screen[0], "16:33:44 you", "{screen:?}");
}

/// P5: at most 5 header rows, `…` ends the 5th, then one empty row.
#[test]
fn a_long_reason_wraps_and_stops_at_five_rows() {
    let mut env = env();
    populate(&mut env, None);
    let reason = (1..=39).map(|i| format!("word{i:02}")).collect::<Vec<_>>().join(" ");
    env.sql(&job("dead", "cancelled", Some(&reason)));
    let screen = screen(&mut env.job("dead"), 44, 10);
    assert_eq!(
        screen[..4],
        rows(&[
            " cancelled: word01 word02 word03 word04",
            " word05 word06 word07 word08 word09 word10",
            " word11 word12 word13 word14 word15 word16",
            " word17 word18 word19 word20 word21 word22",
        ])[..],
        "{screen:?}"
    );
    assert!(screen[4].starts_with(" word23") && screen[4].ends_with('…'), "the fifth row ends in …: {screen:?}");
    assert_eq!((screen[5].as_str(), screen[6].as_str()), ("", PLACEHOLDER), "{screen:?}");
}

#[test]
fn a_session_with_no_file_registered_says_so() {
    let mut env = env();
    populate(&mut env, None);
    let screen = screen(&mut env.attempt(), 70, 3);
    assert_eq!(screen[0], " No session file yet: the Worker has not started its Session.");
}
