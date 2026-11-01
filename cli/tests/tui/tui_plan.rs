//! The `plan` view (commands.md §13.2, §17), the centre-top pane of the cockpit: the flow chart of the Plan of the Lead in
//! the chat slot, and the view's refresh from Events.
//!
//! The store is prepared through the store API (`Store::write`), as the other areas' tests do, because Plans, Jobs
//! and Attempts belong to their areas. What the chart draws is the view's first frame (`cpd_tui::draw_first_frame`) on a
//! `TestBackend` over the store of an `Fx`; the tests of the view's loop (keys, the follow of Events, the Worker pane)
//! run it in this process on a scripted pump (`show`), the Worker pane's on the fake tmux server of the `Fx`.

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::tui_plan_cancelled::{cells_of, draw, draw_sized, plain, rows};
use common::fx::{Fx, TmuxProbe};
use common::screen::{NoInput, Screen, SharedBackend, Steps};
use common::TestEnv;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use cpd_tui::plan;
use cpd_tui::pump::{Feed, Signal};
use cpd_tui::view_ports::Ctx;
use cpd_tui::ViewError;
use cpd_tui::link::CUE;
use cpd_tui::palette::{self, DRAFT, FAILED, MERGED, MERGING, Meaning, QUEUED, SPINNER};
use cpd_tui::read::Kind;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, MouseEventKind};
use ratatui::style::Color;
use serde_json::json;

/// The expected chart of the store `cpd_jobs()` makes: the fixture of the cockpit of branch `cpd/cpd-ui`
/// (`tui/tests/fixtures/cpd-chart.txt`), kept beside the view.

/// The 27 Jobs of the cockpit's `cpd-jobs.json` fixture (cpd 1's `workflow show` of cpd-rust's `cpd` Workflow), as
/// cpd 2 stores them: `(key, status, tier, needs, Attempts)`. cpd 1's `closed` is `merged`, and its `in_review` is a
/// `running` Job whose Attempt is `merging` (here `merging`).
const CPD_JOBS: &[(&str, &str, &str, &[&str], u32)] = &[
    ("contract-core", "merged", "O3", &[], 2),
    ("contract-commands", "merged", "O3", &["contract-core"], 1),
    ("review-in-tmux", "merged", "S3", &[], 1),
    ("contract-amend", "merged", "S3", &[], 1),
    ("rs-workspace", "merging", "O3", &[], 1),
    ("rs-store", "queued", "O3", &["rs-workspace"], 0),
    ("rs-events", "queued", "S3", &["rs-store"], 0),
    ("rs-config", "queued", "S3", &["rs-workspace"], 0),
    ("rs-git", "queued", "S3", &["rs-config"], 0),
    ("rs-host", "queued", "O3", &["rs-config"], 0),
    ("rs-context", "queued", "S3", &["rs-events"], 0),
    ("rs-session", "queued", "O3", &["rs-events", "rs-host"], 0),
    ("rs-plan", "queued", "O3", &["rs-context", "rs-git", "rs-session"], 0),
    ("rs-job", "queued", "S3", &["rs-plan"], 0),
    ("rs-merge", "queued", "O3", &["rs-events", "rs-git"], 0),
    ("rs-attempt-cmds", "queued", "S3", &["rs-git", "rs-job"], 0),
    ("rs-ask", "queued", "S3", &["rs-job"], 0),
    ("rs-review", "queued", "S3", &["rs-host", "rs-job"], 0),
    ("rs-spec-metrics", "queued", "S3", &["rs-job"], 0),
    ("rs-driver", "queued", "O3", &["rs-attempt-cmds", "rs-merge", "rs-review", "rs-session", "rs-spec-metrics"], 0),
    ("rs-pr", "queued", "S3", &["rs-merge", "rs-plan"], 0),
    ("rs-watch", "queued", "O3", &["rs-ask", "rs-context", "rs-driver", "rs-pr"], 0),
    ("rs-doctor", "queued", "S3", &["rs-config", "rs-git", "rs-host", "rs-store"], 0),
    ("pi-adapter-v2", "queued", "S3", &["rs-events", "rs-plan", "rs-session"], 0),
    ("skill-v2", "queued", "S3", &["pi-adapter-v2", "rs-watch"], 0),
    ("gate-at-pr", "running", "S3", &[], 1),
    ("contract-gate-pr", "running", "O3", &["contract-amend"], 1),
];

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

// ---------- the environment ----------

/// An `Fx` with the config of the chart tests: the store the chart is drawn from, no process.
fn fx() -> Fx {
    crate::tui_plan_cancelled::env()
}

/// The chart rows inside the one PR box of a screen: below one row of padding and the box's top border, indented by two columns of padding and the box's left border; the box's right border is left out. A running pill spins on the clock (`SPINNER`): its frame is whatever the screen was captured at, so every frame reads as the first.
fn chart_rows(screen: &[String], rows: usize) -> String {
    screen[2..2 + rows]
        .iter()
        .map(|l| {
            let inside: String = l.chars().skip(2).map(|c| if SPINNER[1..].iter().any(|g| g.starts_with(c)) { SPINNER[0].chars().next().unwrap() } else { c }).collect();
            let inside = inside.trim_end();
            inside.strip_suffix('│').unwrap_or_else(|| panic!("no right border: {l:?}")).trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------- the view's loop, in this process ----------

/// A second of virtual time, on the `ManualClock` the view runs on.
const ONE_SECOND: Duration = Duration::from_secs(1);

/// A press of the character `c`, as the scripted pump hands it to the loop.
fn press(c: char) -> Signal {
    Signal::Input(common::screen::ch(c))
}

/// One notch of the wheel down over the chart, as the scripted pump hands it to the loop.
fn scroll_down() -> Signal {
    Signal::Input(common::screen::wheel(MouseEventKind::ScrollDown, 3, 5))
}

/// The end of a script: the terminal fails.
fn gone() -> Signal {
    Signal::InputFailed(io::Error::other("gone"))
}

/// `cpd` is never run by these views' loops (no Worker pane opens): any run is a bug of the test.
fn no_command(_: Vec<OsString>) -> Result<Output, CpdError> {
    Err(CpdError::new(ErrorCode::StoreBusy, "the loop of this test runs no command"))
}

/// Runs the view's loop (`plan::show_with`) on the store of `fx` in a 300 × 40 pane: one thread, the `ManualClock` and a
/// scripted pump that hands `signals` over in order (an idle wait advances the clock, no sleep, no watcher), and `steps`
/// as what the world does while it draws. The script ends the view with the terminal's failure (`gone()`); every step
/// must have run by then.
fn show(fx: &Fx, signals: Vec<Signal>, steps: &Steps) -> SharedBackend {
    let screen = SharedBackend::new(300, 40);
    steps.on(&screen);
    let ports = &fx.fakes.ports;
    let ended = Ports::scope(ports, || {
        let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
        let prepared = plan::prepare(ports, home, no_command, || Ok(300)).unwrap();
        let mut input = NoInput;
        let mut ctx = Ctx::new(ports, screen.terminal(), &mut input);
        plan::show_with(&mut ctx, Feed::scripted(ports.clock.clone(), signals), prepared)
    });
    match ended {
        Err(ViewError::Terminal(e)) => assert_eq!(e.to_string(), "gone", "the view's terminal failed"),
        Err(ViewError::Cpd(e)) => panic!("the view's own work failed: {}", e.message()),
        Ok(()) => panic!("the view ended with no terminal failure: no key ends it"),
    }
    steps.finish();
    screen
}

/// A change of the store a test makes while the view runs, from the view's thread: it opens the store of `home` for each.
#[derive(Clone)]
struct World {
    home: PathBuf,
}

impl World {
    fn of(fx: &Fx) -> World {
        World { home: fx.env.cpd_home() }
    }

    fn write(&self, text: &str) {
        common::sql(&mut Store::open(&CpdHome::resolve(Some(self.home.clone().into()), None).unwrap(), 5000).unwrap(), text);
    }
}

/// `text` on the database `db` through a connection of its own (not through `Store`, which refuses a changed schema and keeps the Events append-only).
fn raw(db: &Path, text: &str) {
    rusqlite::Connection::open(db).unwrap().execute_batch(text).unwrap();
}

/// Whether `cell` is the blue block of a selection (hs-d534, hs-d594, §6.1.1): the selected style, reversed.
fn is_block(cell: &common::screen::Cell) -> bool {
    let block = palette::selected();
    (Some(cell.bg), Some(cell.fg)) == (block.bg, block.fg) && cell.modifier.contains(ratatui::style::Modifier::REVERSED)
}

/// Whether a cell of the screen is the selection's block.
fn any_selected(screen: &Screen) -> bool {
    (0..screen.height()).any(|y| (0..screen.width()).any(|x| is_block(screen.cell(x, y))))
}

/// Whether the row that has `text` has a cell of the selection's block.
fn row_selected(screen: &Screen, text: &str) -> bool {
    screen.find(text).is_some_and(|(_, y)| (0..screen.width()).any(|x| is_block(screen.cell(x, y))))
}

// ---------- rows ----------

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// A Plan in `phase` (`dispatch` or `deploy`) with its Lead; `pr` is `(number, checks)`.
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: Option<(i64, &str)>) -> String {
    let (row, number, url, opened, pull_request) = match pr {
        Some((n, checks)) => (
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/repo', {n}, 'https://github.com/acme/repo/pull/{n}', 'PR {n}', 0, 'open', 'none', '{checks}',
                         '2026-01-05T00:00:00.000Z', NULL, '2026-01-06T00:00:00.000Z');"
            ),
            n.to_string(),
            format!("'https://github.com/acme/repo/pull/{n}'"),
            "'2026-01-05T00:00:00.000Z'",
            format!("(SELECT id FROM pull_requests WHERE number = {n})"),
        ),
        None => (String::new(), "NULL".into(), "NULL".into(), "NULL", "NULL".into()),
    };
    let status = if pr.is_some() { "open" } else { "dispatch" };
    bare_plan_row(id, context, phase, lead, created)
        + &format!(
            "{row}
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at)
         VALUES ('{id}', 'repo', '/repo', 'main', '{status}', 'cpd/{id}', '/wt', {number}, {url}, {opened}, {pull_request},
                 cpd_txn(), '{created}', '2026-01-02T00:00:00.000Z');"
        )
}

/// A Plan in `phase` with its Lead, and no PR.
fn bare_plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str) -> String {
    let locked = if matches!(phase, "grounding" | "plan") { "NULL" } else { "'2026-01-02T00:00:00.000Z'" };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked});"
    )
}

/// One Context `ctx` and one Plan `ship` in `phase`, with no PR, whose Lead `lead-ship` is in the chat slot.
fn bare_plan(phase: &str) -> String {
    context("ctx")
        + &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z")
        + &bare_plan_row("ship", "ctx", phase, "lead-ship", "2026-01-02T00:00:00.000Z")
        + &lead_row("lead-ship", "ctx", "ship", "2026-01-02T00:00:00.000Z")
        + &in_chat("ctx", "ship", "lead-ship")
}

/// A Lead; `plan` is empty for a Context's grounded Lead.
fn lead_row(id: &str, context: &str, plan: &str, created: &str) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'lead', '{context}', {plan}, NULL, 'name of {id}', '/sessions/{id}.jsonl', '/repo', 'O3',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

fn job_row(plan: &str, key: &str, status: &str, tier: &str) -> String {
    job_row_in(plan, "repo", key, status, tier)
}

/// A Job of the PR `pr`.
fn job_row_in(plan: &str, pr: &str, key: &str, status: &str, tier: &str) -> String {
    // `merging` is a `running` Job whose Attempt is `merging`.
    let status = if status == "merging" { "running" } else { status };
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{FP}'") };
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let failed = if status == "failed" { "'crashed'" } else { "NULL" };
    let (reason, requested) =
        if status == "cancelled" { ("'not needed'", "'2026-01-04T00:00:00.000Z'") } else { ("NULL", "NULL") };
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', '{pr}', '{key}', 'Job {key}', '{tier}', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
                 cpd_txn(), {reason}, {requested}, {failed}, {merged}, '2026-01-02T00:00:00.000Z');"
    )
}

fn need_row(plan: &str, job: &str, needs: &str) -> String {
    format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{job}', '{needs}');")
}

/// The Attempts of one Job: `count` of them, the last one in the Job's status, the earlier ones ended `crashed`.
fn attempt_rows(plan: &str, job: &str, status: &str, tier: &str, count: u32) -> String {
    let mut out = String::new();
    for number in 1..=count {
        let last = number == count;
        let (state, outcome, cause, merged, ended) = match (last, status) {
            (false, _) => ("ended", "'failed'", "'crashed'", "NULL", "'2026-01-03T01:00:00.000Z'"),
            (true, "merged") => ("ended", "'merged'", "'merged'", "'abc123'", "'2026-01-03T01:00:00.000Z'"),
            (true, "failed") => ("ended", "'failed'", "'crashed'", "NULL", "'2026-01-03T01:00:00.000Z'"),
            (true, "merging") => ("merging", "NULL", "NULL", "NULL", "NULL"),
            (true, _) => ("running", "NULL", "NULL", "NULL", "NULL"),
        };
        let (sha, at) = if state == "merging" { ("'def456'", "'2026-01-03T00:30:00.000Z'") } else { ("NULL", "NULL") };
        let origin = if number == 1 { "ladder" } else { "retry" };
        out += &format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session,
                                   submitted_sha, submitted_at, merged_sha, started_at, ended_at)
             VALUES ('{plan}', '{job}-a{number}', '{job}', {number}, '{tier}', '{origin}', '{state}', {outcome}, {cause},
                     'cpd/{plan}--{job}-a{number}', '/w/{job}-a{number}', NULL, {sha}, {at}, {merged},
                     '2026-01-03T00:{number:02}:00.000Z', {ended});"
        );
    }
    out
}

fn event(kind: &str, context: &str, plan: Option<&str>, session: Option<&str>, data: &str) -> String {
    let q = |v: Option<&str>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', '{kind}', '{context}', {}, {}, 'fixture {kind}', '{data}');",
        q(plan),
        q(session)
    )
}

/// `session-moved` to the chat slot: the Lead is the one the view shows.
fn in_chat(context: &str, plan: &str, session: &str) -> String {
    event("session-moved", context, Some(plan), Some(session), r#"{"pane":"%1","window":"@1","place":"chat"}"#)
}

/// One Context `ctx`, one Plan `ship` in `phase` whose Lead `lead-ship` is in the chat slot, and `jobs`
/// as `(key, status, tier, needs, Attempts)`; no Attempt for the statuses that have none.
fn plan_with(phase: &str, pr: Option<(i64, &str)>, jobs: &[(&str, &str, &str, &[&str], u32)]) -> String {
    let mut s = context("ctx");
    s += &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z");
    s += &plan_row("ship", "ctx", phase, "lead-ship", "2026-01-02T00:00:00.000Z", pr);
    s += &lead_row("lead-ship", "ctx", "ship", "2026-01-02T00:00:00.000Z");
    for (key, status, tier, _, _) in jobs {
        s += &job_row("ship", key, status, tier);
    }
    for (key, _, _, needs, _) in jobs {
        for need in *needs {
            s += &need_row("ship", key, need);
        }
    }
    for (key, status, tier, _, attempts) in jobs {
        s += &attempt_rows("ship", key, status, tier, *attempts);
    }
    s + &in_chat("ctx", "ship", "lead-ship")
}

// ---------- tests ----------

#[test]
fn the_chart_of_a_store_equivalent_to_the_cockpits_fixture_has_all_its_jobs_in_one_box() {
    let fx = fx();
    fx.env.sql(&plan_with("dispatch", None, CPD_JOBS));
    // The chart is wide: 1500 columns.
    let screen = rows(&draw_sized(&fx, 1500, 40));
    let text = screen.join("\n");
    for (key, ..) in CPD_JOBS {
        assert!(text.contains(key), "{key} is drawn:\n{text}");
    }
    assert!(text.contains(&format!("{MERGING} rs-workspace")), "the merging Job has its icon:\n{text}");
    // a Plan of one PR is one box, titled on its top border
    assert!(screen[1].starts_with(" ╭─ repo · repo 󱦰 main · dispatch ─"), "{:?}", screen[1]);
    assert_eq!(screen.iter().filter(|l| l.trim().starts_with("╭─ ") && l.contains(" · ") && l.trim_end().ends_with('╮')).count(), 1, "{text}");
    assert!(screen.iter().all(|l| !l.contains(['┌', '┐', '└', '┘'])), "the corners are rounded:\n{text}");
}

#[test]
fn a_plan_of_two_prs_is_two_titled_boxes_one_below_the_other_linked_by_an_arrow() {
    let fx = fx();
    // `api` has its open pull request #12; `web` needs it and waits
    let mut s = bare_plan("dispatch");
    s += r#"INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
            VALUES ('acme/api-server', 12, 'https://github.com/acme/api-server/pull/12', 'api', 0, 'open', 'none', 'none',
                    '2026-01-05T00:00:00.000Z', NULL, '2026-01-05T00:00:00.000Z');
            INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at)
            VALUES ('ship', 'api', '/src/api-server', 'main', 'open', 'cpd/ship/api', '/wt/api', 12, 'https://github.com/acme/api-server/pull/12',
                    '2026-01-05T00:00:00.000Z', (SELECT id FROM pull_requests WHERE number = 12), cpd_txn(), '2026-01-02T00:00:00.000Z',
                    '2026-01-02T00:00:00.000Z');
            INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
            VALUES ('ship', 'web', '/src/web', 'develop', 'waiting', 'cpd/ship/web', cpd_txn(), '2026-01-02T00:00:00.000Z');
            INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('ship', 'web', 'api');"#;
    s += &job_row_in("ship", "api", "a", "merged", "O3");
    s += &job_row_in("ship", "api", "b", "merged", "S3");
    s += &need_row("ship", "b", "a");
    s += &job_row_in("ship", "web", "c", "queued", "S3");
    fx.env.sql(&s);
    let screen = rows(&draw(&fx));
    // web below api, the line from api's bottom border into web's top border
    // `web` waits for `api`, which has not merged (hs-d521): its title says on what
    let (merged_row, queued_row) = (
        format!(" │ \u{e0b6}{MERGED} a│O3\u{e0b4}─────\u{e0b6}{MERGED} b│S3\u{e0b4}                  │"),
        format!(" │ \u{e0b6}{QUEUED} c│S3\u{e0b4}                               │"),
    );
    let title = format!(" ╭─ api · api-server 󱦰 main · #12 open {CUE} ─╮");
    let want = [
        title.as_str(),
        merged_row.as_str(),
        " ╰───────────────────┬────────────────────╯",
        "                     ╰───────────────────╮",
        "                                         │",
        " ╭─ web · web 󱦰 develop · waiting on api ┴╮",
        queued_row.as_str(),
        " ╰────────────────────────────────────────╯",
    ];
    assert_eq!(screen[1..9], want, "{}", screen.join("\n"));
}

#[test]
fn a_plan_with_no_pr_yet_says_so() {
    let fx = fx();
    fx.env.sql(&bare_plan("grounding"));
    let buf = draw(&fx);
    let screen = rows(&buf);
    assert!(screen.iter().any(|l| l.trim() == "no Workflow yet: cpd workflow add"), "{}", screen.join("\n"));
    assert!(!screen.iter().any(|l| l.contains('╭') || l.contains("No Plan")), "{}", screen.join("\n"));
    // muted, colour 8
    let text = cells_of(&buf, "no Workflow yet: cpd workflow add").expect("the message");
    assert!(text.iter().all(|at| buf[*at].fg == palette::muted_text().fg.unwrap()), "{text:?}");
}

#[test]
fn each_job_status_has_its_icon_and_a_cancelled_job_is_drawn_muted_after_them() {
    let fx = fx();
    let jobs: &[(&str, &str, &str, &[&str], u32)] = &[
        ("draft", "draft", "S3", &[], 0),
        ("queued", "queued", "S3", &[], 0),
        ("running", "running", "S3", &[], 1),
        ("merging", "merging", "S3", &[], 1),
        ("merged", "merged", "S3", &[], 1),
        ("failed", "failed", "S3", &[], 3),
        ("gone", "cancelled", "S3", &[], 0),
    ];
    fx.env.sql(&plan_with("dispatch", None, jobs));
    let screen = rows(&draw(&fx));
    // a column is as wide as its widest pill (the failed pill, `failed│#3│S3`); the tier sits at its right, the attempt `#N` (from the second Attempt) before it
    let want = format!(
        "
 \u{e0b6}{DRAFT} draft    │S3\u{e0b4}

 \u{e0b6}{QUEUED} queued   │S3\u{e0b4}

 \u{e0b6}{} running  │S3\u{e0b4}

 \u{e0b6}{MERGING} merging  │S3\u{e0b4}

 \u{e0b6}{MERGED} merged   │S3\u{e0b4}

 \u{e0b6}{FAILED} failed│#3│S3\u{e0b4}",
        SPINNER[0]
    );
    assert_eq!(chart_rows(&screen, 11), want.strip_prefix('\n').unwrap(), "{}", screen.join("\n"));
    // the cancelled Job keeps its place, with its status (hs-d518; `tui_plan_cancelled` pins how)
    assert!(screen.iter().any(|l| l.contains("gone") && !l.contains("cancelled")), "a cancelled Job is drawn as a pill with no note:\n{}", screen.join("\n"));
}

#[test]
fn a_plan_without_jobs_says_no_plan_yet() {
    let fx = fx();
    fx.env.sql(&plan_with("dispatch", None, &[]));
    let screen = rows(&draw(&fx));
    assert!(screen.iter().any(|l| l.trim() == "no Plan yet"), "{}", screen.join("\n"));
}

#[test]
fn escalations_and_a_decided_plan_review_are_not_drawn() {
    let fx = fx();
    let mut s = plan_with("dispatch", None, &[("a", "running", "S3", &[], 1)]);
    s += &format!("INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at)
          VALUES ('ship', 'a-a1-q1', 'a-a1', 1, 'Which?', 'open', NULL, 1, '2026-01-03T00:20:00.000Z', NULL, NULL);
          INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
          VALUES ('ship', 'r1', 1, '{FP}', 'approved', '2026-01-02T01:00:00.000Z', '2026-01-02T02:00:00.000Z');");
    fx.env.sql(&s);
    let screen = rows(&draw(&fx));
    assert!(screen[2].contains(" a│"), "{}", screen.join("\n"));
    assert!(screen.iter().all(|l| !l.contains("Escalation") && !l.contains("review")), "{}", screen.join("\n"));
}

/// A Plan of one PR `repo` with its pull request #7 and one Job, and a Plan review `r<number>` in `status`.
fn plan_under_review(number: u32, status: &str) -> String {
    let decided = if status == "pending" { "NULL" } else { "'2026-01-02T02:00:00.000Z'" };
    plan_with("dispatch", Some((7, "none")), &[("a", "queued", "S3", &[], 0)])
        + &format!(
            "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
             VALUES ('ship', 'r{number}', {number}, '{FP}', '{status}', '2026-01-02T01:00:00.000Z', {decided});"
        )
}

#[test]
fn a_pull_request_box_title_ends_in_a_link_to_it() {
    let fx = fx();
    fx.env.sql(&plan_under_review(2, "pending"));
    let buf = draw(&fx);
    let screen = rows(&buf);
    assert!(screen.iter().any(|l| *l == format!(" ╭─ repo · repo 󱦰 main · #7 open {CUE} ─╮")), "{}", screen.join("\n"));
    // plain cells: the `Link` widget writes no escape
    let cells = (0..buf.area.height).flat_map(|y| (0..buf.area.width).map(move |x| (x, y)));
    assert!(cells.into_iter().all(|at| !buf[at].symbol().contains('\x1b')), "{}", screen.join("\n"));
}

#[test]
fn a_conflict_shows_in_the_box_title_and_clears_with_no_event() {
    let fx = fx();
    fx.env.sql(&plan_with("deploy", Some((7, "none")), &[("a", "merged", "S3", &[], 1)]));
    let title = |word: &str| rows(&draw(&fx)).iter().any(|l| l.contains(&format!("#7 {word} {CUE}")));
    assert!(title("open"), "the open title");

    // the poll stored a conflict: no Event is appended, so only the change of the row shows it
    // (the poll sets `changed_at` with every change, contract §4.4)
    fx.env.sql("UPDATE pull_requests SET mergeable = 'conflicting', changed_at = '2026-02-01T00:00:01.000Z' WHERE number = 7;");
    assert!(title("conflict"), "conflict");

    // `unknown` is not a conflict
    fx.env.sql("UPDATE pull_requests SET mergeable = 'unknown', changed_at = '2026-02-01T00:00:02.000Z' WHERE number = 7;");
    assert!(title("open"), "open again");
}

/// A failed snapshot keeps the chart on screen and shows the error in the row below it, for longer than one poll; the
/// view takes the snapshot again at the next refresh, and the error goes when it succeeds (tui-W4). The read is made to
/// fail by renaming the table it reads.
#[test]
fn a_failed_snapshot_keeps_the_chart_and_shows_the_error_until_a_snapshot_succeeds() {
    let fx = fx();
    fx.env.sql(&plan_with("deploy", Some((7, "none")), &[("a", "merged", "S3", &[], 1)]));
    let title = |s: &Screen| s.shown_rows().iter().any(|l| l.contains(&format!("#7 open {CUE}")));
    let failed = |s: &Screen| s.contains("no such table");
    let mut store = fx.env.store();
    let (db, clock) = (fx.env.cpd_home().join("cpd.db"), fx.fakes.clock.clone());
    let steps = Steps::new()
        .at("the chart", title, {
            let (db, clock) = (db.clone(), clock.clone());
            move |_| {
                // not through `Store`, which refuses a changed schema
                raw(&db, "ALTER TABLE jobs RENAME TO jobs_gone;");
                // the snapshot is taken because an Event of the Plan came (the store was opened before the rename)
                store
                    .write("fixture", |tx| {
                        tx.execute_batch("INSERT INTO events (txn, at, kind, context, plan, text, data)
                             VALUES (cpd_txn(), '2026-02-01T00:00:00.000Z', 'plan-phase', 'ctx', 'ship', 'fixture', '{}');")?;
                        Ok::<(), CpdError>(())
                    })
                    .unwrap();
                clock.advance(ONE_SECOND);
            }
        })
        .at("the error", failed, {
            let clock = clock.clone();
            // the refresh is every 2 s: past two of them the snapshot was taken again, and still fails
            move |_| clock.advance(3 * ONE_SECOND)
        })
        .next("a frame after the next snapshot", {
            let (db, clock) = (db.clone(), clock.clone());
            move |screen| {
                assert!(failed(screen) && title(screen), "{}", screen.text());
                raw(&db, "ALTER TABLE jobs_gone RENAME TO jobs;");
                clock.advance(3 * ONE_SECOND);
            }
        })
        .at("the error to go", move |s| !failed(s) && title(s), |_| {});
    // a commit, the refresh that comes with it, a key for a frame, the refresh that succeeds
    show(&fx, vec![Signal::Commit, Signal::Commit, press('x'), Signal::Commit, gone()], &steps);
}

/// A follow that fails is shown below the chart, for longer than one snapshot (tui-G4): the highest Event id fell below
/// the cursor, which is what a replaced database looks like. The chart stays.
#[test]
fn a_follow_that_fails_is_shown_below_the_chart() {
    let fx = fx();
    fx.env.sql(&plan_with("deploy", Some((7, "none")), &[("a", "merged", "S3", &[], 1)]));
    fx.env.sql(&event("plan-phase", "ctx", Some("ship"), None, "{}"));
    let title = |s: &Screen| s.shown_rows().iter().any(|l| l.contains(&format!("#7 open {CUE}")));
    let (db, clock) = (fx.env.cpd_home().join("cpd.db"), fx.fakes.clock.clone());
    let steps = Steps::new()
        .at("the chart", title, {
            let clock = clock.clone();
            move |_| {
                // not through `Store`, which keeps the Events append-only
                raw(&db, "DROP TRIGGER events_no_delete; DELETE FROM events WHERE id > (SELECT min(id) FROM events);");
                clock.advance(ONE_SECOND);
            }
        })
        .at("the error", |s| s.contains("Event cursor"), move |_| clock.advance(3 * ONE_SECOND))
        // a snapshot is retaken within 3 s (the refresh): the error stays
        .next("a frame after the next refresh", move |screen| assert!(screen.contains("Event cursor") && title(screen), "{}", screen.text()));
    show(&fx, vec![Signal::Commit, Signal::Commit, press('x'), gone()], &steps);
}

/// P1: a pending Plan review is no row: the screen is the one of a Plan without it.
#[test]
fn a_pending_plan_review_draws_no_row_above_the_boxes() {
    {
        let fx = fx();
        fx.env.sql(&plan_under_review(2, "pending"));
        let buf = draw(&fx);
        let screen = rows(&buf);
        let shown = screen.join("\n");
        assert_eq!(screen[0], "", "{shown}");
        assert_eq!(screen[1], format!(" ╭─ repo · repo 󱦰 main · #7 open {CUE} ─╮"), "{shown}");
        assert!(screen.iter().all(|l| !l.contains("Plan review")), "{shown}");
        assert!(!(0..buf.area.height).any(|y| (0..buf.area.width).any(|x| buf[(x, y)].symbol().contains("localhost"))));
    }
}

#[test]
fn no_lead_in_the_chat_slot_says_so() {
    let fx = fx();
    fx.env.sql(&(context("ctx") + &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z")));
    let buf = draw(&fx);
    let screen = rows(&buf);
    // in the message row, the pane's last row, indented by the margin; nothing above it
    assert_eq!(screen.iter().filter(|l| !l.is_empty()).collect::<Vec<_>>(), [&"   no Lead session in the centre".to_owned()], "{}", screen.join("\n"));
    let text = cells_of(&buf, " no Lead session in the centre").expect("the message");
    assert!(text.iter().all(|at| buf[*at].fg == palette::muted_text().fg.unwrap()), "{text:?}");
}

#[test]
fn the_first_rows_of_the_pane_are_the_top_of_the_first_box_with_no_tab() {
    let fx = fx();
    fx.env.sql(&plan_with("dispatch", None, &[("a", "queued", "S3", &[], 0)]));
    // a second Lead of the Context with a pane: it used to be a tab
    fx.env.sql(&(lead_row("fork-ship", "ctx", "ship", "2026-01-02T00:00:09.000Z") + &event(
        "session-moved", "ctx", Some("ship"), Some("fork-ship"), r#"{"pane":"%2","window":"@2","place":"parked"}"#)));
    let screen = rows(&draw(&fx));
    // one row of margin, then the box's top border, its one chart row and its bottom border
    assert_eq!(screen[0], "", "{}", screen.join("\n"));
    assert_eq!(screen[1], " ╭─ repo · repo 󱦰 main · dispatch ─╮");
    assert!(screen[2].starts_with(&format!(" │ \u{e0b6}{QUEUED} a│S3\u{e0b4}")), "{:?}", screen[2]);
    assert!(screen[3].starts_with(" ╰─"), "{:?}", screen[3]);
    assert!(screen.iter().all(|l| !l.contains('\u{F0142}' /* md-chevron_right */) && !l.contains("name of") && !l.starts_with('─')), "{}", screen.join("\n"));
}

#[test]
fn the_view_follows_events_and_switches_plan_when_the_chat_slot_changes() {
    let fx = fx();
    let mut s = plan_with("dispatch", None, &[("a", "queued", "S3", &[], 0)]);
    s += &plan_row("side", "ctx", "dispatch", "lead-side", "2026-01-02T00:00:05.000Z", None);
    s += &lead_row("lead-side", "ctx", "side", "2026-01-02T00:00:05.000Z");
    s += &job_row("side", "other", "queued", "O3");
    fx.env.sql(&s);
    let (world, clock) = (World::of(&fx), fx.fakes.clock.clone());
    let row2 = |word: &'static str| move |s: &Screen| s.height() > 2 && s.shown_row(2).contains(word);
    let steps = Steps::new()
        .at("Job a", row2(" a│"), {
            let (world, clock) = (world.clone(), clock.clone());
            move |_| {
                // a Job is appended to the shown Plan: its Event makes the view take a new snapshot
                world.write(
                    &(job_row("ship", "b", "queued", "S3")
                        + &need_row("ship", "b", "a")
                        + &event("job-added", "ctx", Some("ship"), None, r#"{"status":"queued","appended":true,"tier":"S3","needs":["a"]}"#)),
                );
                clock.advance(ONE_SECOND);
            }
        })
        .at("Job b", row2(" b│"), move |screen| {
            // the pills and the edge between them; what a note after them was is pinned by `tui_plan_waiting`
            assert!(chart_rows(&screen.shown_rows(), 1).starts_with(&format!(" \u{e0b6}{QUEUED} a│S3\u{e0b4}─────\u{e0b6}{QUEUED} b│S3\u{e0b4}")), "{}", screen.text());
            // the other Lead moves into the chat slot: the view shows its Plan
            world.write(&in_chat("ctx", "side", "lead-side"));
            clock.advance(ONE_SECOND);
        })
        .at("the other Plan", row2("other"), |screen| {
            let rows = screen.shown_rows();
            assert_eq!(chart_rows(&rows, 1), format!(" \u{e0b6}{QUEUED} other│O3\u{e0b4}"));
            assert!(!rows.iter().any(|l| l.contains(&format!("{QUEUED} a│"))), "{}", screen.text());
        });
    show(&fx, vec![Signal::Commit, Signal::Commit, gone()], &steps);
}

#[test]
fn each_plan_keeps_its_scroll_offset_while_the_view_runs() {
    let fx = fx();
    let keys: Vec<String> = (10..60).map(|n| format!("j{n}")).collect();
    let jobs: Vec<(&str, &str, &str, &[&str], u32)> = keys.iter().map(|k| (k.as_str(), "queued", "S3", &[][..], 0)).collect();
    let mut s = plan_with("dispatch", None, &jobs);
    s += &plan_row("side", "ctx", "dispatch", "lead-side", "2026-01-02T00:00:05.000Z", None);
    s += &lead_row("lead-side", "ctx", "side", "2026-01-02T00:00:05.000Z");
    s += &job_row("side", "other", "queued", "O3");
    fx.env.sql(&s);
    let (world, clock) = (World::of(&fx), fx.fakes.clock.clone());
    let (top, scrolled) = (Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(Vec::new())));
    let steps = Steps::new()
        .at("the top of the chart", |s| s.height() > 2 && s.shown_row(2).contains(" j10│"), {
            let top = top.clone();
            move |screen| *top.borrow_mut() = screen.shown_rows()
        })
        // the wheel scrolls the chart, a notch scrolls it three rows: j10 leaves the top
        .at("the chart scrolled by a notch", |s| s.height() > 2 && !s.shown_row(2).contains(" j10│"), {
            let (scrolled, world, clock) = (scrolled.clone(), world.clone(), clock.clone());
            move |screen| {
                *scrolled.borrow_mut() = screen.shown_rows();
                assert!(scrolled.borrow().iter().all(|l| !l.contains(" j10│")), "{}", screen.text());
                // another Plan starts at its corner
                world.write(&in_chat("ctx", "side", "lead-side"));
                clock.advance(ONE_SECOND);
            }
        })
        .at("the other Plan", |s| s.height() > 2 && s.shown_row(2).contains("other"), {
            let (world, clock) = (world.clone(), clock.clone());
            // coming back shows the chart where it was left
            move |_| {
                world.write(&in_chat("ctx", "ship", "lead-ship"));
                clock.advance(ONE_SECOND);
            }
        })
        .at("the chart where it was left", { let scrolled = scrolled.clone(); move |s| s.shown_rows() == *scrolled.borrow() }, move |screen| {
            assert_ne!(screen.shown_rows(), *top.borrow());
        });
    let mut signals = vec![scroll_down()];
    signals.extend([Signal::Commit, Signal::Commit, gone()]);
    show(&fx, signals, &steps);
}

/// A Plan `ship` of 50 independent Jobs, `j10` to `j59`: a chart taller than the pane.
fn tall_plan() -> String {
    let keys: Vec<String> = (10..60).map(|n| format!("j{n}")).collect();
    let jobs: Vec<(&str, &str, &str, &[&str], u32)> = keys.iter().map(|k| (k.as_str(), "queued", "S3", &[][..], 0)).collect();
    plan_with("dispatch", None, &jobs)
}

/// The view's log, `run/tui/plan.log` (a view draws no note, commands.md §17 "No notes"): empty before its first line.
fn plan_log(home: &Path) -> String {
    fs::read_to_string(home.join("run").join("tui").join("plan.log")).unwrap_or_default()
}

fn scroll_file(env: &TestEnv) -> std::path::PathBuf {
    env.cpd_home().join("run").join("tui").join("plan-scroll.json")
}

#[test]
fn the_scroll_offset_survives_a_restart_of_the_view() {
    let fx = fx();
    fx.env.sql(&tall_plan());
    let clock = fx.fakes.clock.clone();
    let (top, scrolled) = (Rc::new(RefCell::new(Vec::new())), Rc::new(RefCell::new(Vec::new())));
    let steps = Steps::new()
        .at("the top of the chart", |s| s.height() > 2 && s.shown_row(2).contains(" j10│"), {
            let top = top.clone();
            move |screen| *top.borrow_mut() = screen.shown_rows()
        })
        // the wheel scrolls down the chart, a notch three rows
        .at("the chart scrolled by a notch", |s| s.height() > 2 && !s.shown_row(2).contains(" j10│"), {
            let scrolled = scrolled.clone();
            move |screen| {
                *scrolled.borrow_mut() = screen.shown_rows();
                assert!(scrolled.borrow().iter().all(|l| !l.contains(" j10│")), "{}", screen.text());
                // the view writes the file at most once a second (SAVE_EVERY): the write held back for the last scroll is due now
                clock.advance(3 * ONE_SECOND / 2);
            }
        });
    let mut signals = vec![scroll_down()];
    signals.extend([Signal::Due, gone()]);
    show(&fx, signals, &steps);
    // the offset is in the file; restart the view
    let offset = || -> Option<serde_json::Value> {
        let text = fs::read_to_string(scroll_file(&fx.env)).ok()?;
        serde_json::from_str::<serde_json::Value>(&text).ok()?.get("ship").cloned()
    };
    let written = offset().expect("the offset is written");
    assert!(written[1].as_u64().unwrap_or(0) > 0, "{written}");

    let steps = Steps::new().at("the offset restored", { let scrolled = scrolled.clone(); move |s| s.shown_rows() == *scrolled.borrow() }, move |screen| {
        assert_ne!(screen.shown_rows(), *top.borrow());
    });
    show(&fx, vec![gone()], &steps);
    assert_eq!(offset().unwrap(), written, "the restart did not move the offset");
    let leftovers: Vec<_> = fs::read_dir(scroll_file(&fx.env).parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(leftovers, ["plan-scroll.json"], "no temporary file is left");
}

#[test]
fn an_invalid_scroll_file_starts_the_view_at_the_top_logs_it_and_the_next_write_replaces_it() {
    let fx = fx();
    fx.env.sql(&tall_plan());
    fs::create_dir_all(scroll_file(&fx.env).parent().unwrap()).unwrap();
    fs::write(scroll_file(&fx.env), "{not json").unwrap();
    let home = fx.env.cpd_home();
    let steps = Steps::new().at("the chart", |s| s.height() > 2 && s.shown_row(2).contains(" j10│"), move |screen| {
        assert!(plan_log(&home).contains("scroll positions reset: "), "the log line");
        assert!(screen.shown_rows().iter().all(|l| !l.contains("scroll positions reset")), "no note: {}", screen.text());
        assert_eq!(fs::read_to_string(home.join("run").join("tui").join("plan-scroll.json")).unwrap(), "{not json", "nothing is written until an offset changes");
    });
    // scroll down the chart with the wheel: the file becomes a valid one; the view is alive until the script ends it
    let mut signals: Vec<Signal> = (0..15).map(|_| scroll_down()).collect();
    signals.push(gone());
    show(&fx, signals, &steps);
    assert!(
        fs::read_to_string(scroll_file(&fx.env)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).is_some_and(|v| v["ship"][1].as_u64().unwrap_or(0) > 0),
        "the file replaced"
    );
}

#[test]
fn the_view_starts_no_subprocess_and_asks_gh_nothing() {
    let fx = fx();
    fx.env.sql(&plan_with("deploy", Some((7, "failed")), &[("a", "running", "S3", &[], 1)]));
    let world = World::of(&fx);
    // let it draw, follow an Event and take a new snapshot: the Event comes with its Job, so when the new pill shows, the
    // view has followed the Event and read the store again
    let steps = Steps::new()
        .at("the chart", |s| s.shown_row(2).contains(" a│"), move |_| {
            world.write(&format!(
                "{}{}",
                job_row("ship", "b", "queued", "S3"),
                event("job-added", "ctx", Some("ship"), None, r#"{"status":"queued","appended":true,"tier":"S3","needs":[]}"#)
            ));
        })
        .at("the new Job", |s| s.rows().iter().any(|l| l.contains(" b│")), |_| {});
    show(&fx, vec![Signal::Commit, gone()], &steps);
    // the view is in this process and asks nothing of the world: no `Runner` call (`gh` and `pi` included), no child
    assert_eq!(fx.fakes.runner.calls(), Vec::new(), "the view ran a program");
    assert_eq!(fx.fakes.spawn.launches(), Vec::new(), "the view started a process");
}

/// An `Fx` on fake ports whose config is the chart tests' plus the private tmux server (the fake): the Worker pane's tests.
fn tmux_fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_models();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n\
             [events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
            fx.env.tmux_socket()
        ),
    )
    .unwrap();
    fx
}

/// The cockpit's centre on the fake server: the plan view above the chat slot. Only records: no pane runs a program.
fn cockpit_centre(fx: &Fx) {
    let ok = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let plan = ok(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "200", "-y", "40", "-P", "-F", "#{pane_id}", "cpd tui view plan"]);
    ok(&["set", "-p", "-t", &plan, "@cpd_view", "plan"]);
    let chat = ok(&["split-window", "-d", "-v", "-t", &plan, "-P", "-F", "#{pane_id}", "sleep 120"]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
}

/// What `format` says of each pane of the cockpit's `main` window, one line each.
fn panes(tmux: &TmuxProbe, format: &str) -> String {
    tmux.run(&["list-panes", "-t", "=cpd:main", "-F", format])
}

/// A press of `code`, as the scripted pump hands it to the loop.
fn press_key(code: KeyCode) -> Signal {
    Signal::Input(common::screen::key(code))
}

#[test]
fn a_click_on_a_pill_opens_no_pane_and_selects_nothing() {
    let fx = tmux_fx();
    fx.env.sql(&plan_with("dispatch", None, &[("a", "merged", "O3", &[], 1), ("b", "queued", "S3", &["a"], 0)]));
    cockpit_centre(&fx);
    let layout = "#{pane_id} #{@cpd_view} #{pane_active}";
    let (before, after) = (fx.tmux_probe().run(&["list-panes", "-t", "=cpd:main", "-F", layout]), fx.tmux_probe());
    // the pill of `a` is the chart's first row (row 2, below the box's top edge); the click lands on its icon
    let click = || Signal::Input(common::screen::left_click(3, 2));
    // after each click: nothing is selected, no pane is open
    let (one, two) = (fx.tmux_probe(), fx.tmux_probe());
    let steps = Steps::new()
        .at("the chart", |s| s.shown_row(2).contains(" a│"), |_| {})
        .next("the first click", move |screen| {
            assert!(!any_selected(screen), "a pill is selected:\n{}", screen.text());
            assert_eq!(panes(&one, "#{@cpd_view}"), "plan\nchat", "no pane opened");
        })
        .next("the second click", move |screen| {
            assert!(!any_selected(screen), "a pill is selected:\n{}", screen.text());
            assert_eq!(panes(&two, "#{@cpd_view}"), "plan\nchat", "no pane opened");
        });
    show(&fx, vec![click(), click(), gone()], &steps);
    assert_eq!(panes(&after, layout), before, "the panes are as the cockpit left them");
}

#[test]
fn no_key_selects_a_pill_or_opens_a_pane() {
    let fx = tmux_fx();
    fx.env.sql(&plan_with("dispatch", None, &[("a", "merged", "O3", &[], 1), ("b", "queued", "S3", &["a"], 0)]));
    cockpit_centre(&fx);
    let after = fx.tmux_probe();
    let mut keys: Vec<Signal> = [KeyCode::Right, KeyCode::Left, KeyCode::Down, KeyCode::Up].into_iter().map(press_key).collect();
    keys.extend(['h', 'j', 'k', 'l'].map(press));
    keys.extend([press_key(KeyCode::Enter), press_key(KeyCode::Esc), press_key(KeyCode::Enter)]);
    // each key draws a frame: after it nothing is selected, no Worker pane is open, and the chart still draws
    let mut steps = Steps::new().at("the chart", |s| s.shown_row(2).contains(" a│"), |_| {});
    for _ in &keys {
        let probe = fx.tmux_probe();
        steps = steps.next("a key", move |screen| {
            assert!(!any_selected(screen), "a pill is selected:\n{}", screen.text());
            assert_eq!(panes(&probe, "#{@cpd_view}"), "plan\nchat", "no pane opened");
            assert!(screen.shown_row(2).contains(" a│"), "the chart still draws:\n{}", screen.text());
        });
    }
    keys.push(gone());
    show(&fx, keys, &steps);
    assert_eq!(panes(&after, "#{@cpd_view}"), "plan\nchat", "no pane opened");
}

// ---------- Pulse (a Manager in the chat slot; User, hs-d327) ----------

/// An Event of `plan` in `ctx` that happened `minutes` minutes ago by the clock (the view reads the real time).
fn event_ago(kind: &str, plan: &str, minutes: u32) -> String {
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{minutes} minutes'), '{kind}', 'ctx', '{plan}', NULL, 'fixture {kind}', '{{}}');"
    )
}

/// The bars of row `y` of `buf` with their foreground.
fn bars_with_colours(buf: &Buffer, y: u16) -> Vec<(char, Color)> {
    (0..buf.area.width)
        .filter_map(|x| {
            let cell = &buf[(x, y)];
            let c = cell.symbol().chars().next()?;
            ('▁'..='█').contains(&c).then_some((c, cell.fg))
        })
        .collect()
}

#[test]
fn pulse_rows_have_no_icon_and_bars_take_the_colour_of_their_kind_with_a_grey_low_bar_for_zero() {
    let fx = fx();
    let mut s = context("ctx") + &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z");
    s += &lead_row("mgr", "ctx", "", "2026-01-01T00:00:01.000Z").replace("'lead'", "'manager'");
    s += &plan_row("p1", "ctx", "dispatch", "lead-p1", "2026-01-02T00:00:00.000Z", None);
    s += &lead_row("lead-p1", "ctx", "p1", "2026-01-02T00:00:00.000Z");
    // five slots, ten minutes apart (two buckets), oldest first; each with a lower-precedence kind beside the winner
    for (minutes, kinds) in [
        (52, &["job-added"][..]),
        (42, &["pr-opened", "job-merged"]),
        (32, &["error", "plan-phase"]),
        (22, &["plan-phase", "job-merged", "job-added"]),
        (12, &["escalation", "job-merged"]),
        (2, &["job-merged"]),
    ] {
        for kind in kinds {
            s += &event_ago(kind, "p1", minutes);
        }
    }
    s += &event("session-moved", "ctx", None, Some("mgr"), r#"{"pane":"%1","window":"@1","place":"chat"}"#);
    fx.env.sql(&s);

    let buf = draw(&fx);
    let screen = rows(&buf);
    let row = &screen[1];
    assert!(row.starts_with("  p1  "), "the id first, no dot: {row:?}");
    assert!(!row.contains('󰗠') && !row.contains('󰪥') && !row.contains('󰝦'), "no icon: {row:?}");
    let age = row.chars().rev().take(9).collect::<String>();
    assert!(age.trim().chars().next().is_some(), "the age cell stays: {row:?}");
    // the pane is 300 wide (`draw`); 2 columns of margin on each side
    let n = 300 - 4 - ("p1".len() + 2 + 2 + 9);
    assert_eq!(row.chars().filter(|c| ('▁'..='█').contains(c)).count(), n, "the sparkline takes the width the icons freed: {row:?}");

    let bars = bars_with_colours(&buf, 1);
    assert_eq!(bars.len(), n);
    let grey = palette::color(Meaning::Nothing);
    let coloured: Vec<Color> = bars.iter().filter(|(_, c)| *c != grey).map(|(_, c)| *c).collect();
    let kinds = [Kind::Other, Kind::PrOpened, Kind::Error, Kind::Plan, Kind::Escalation, Kind::Merged].map(palette::pulse_kind);
    assert_eq!(coloured, kinds, "other, PR opened, error, plan, escalation, merged (the bright pairs): {bars:?}");
    assert!(bars.iter().filter(|(_, c)| *c == grey).all(|(g, _)| *g == '▁'), "an empty slot is a grey ▁: {bars:?}");
    assert!(bars.iter().any(|(_, c)| *c == grey), "{bars:?}");
}

#[test]
fn q_does_not_quit_the_view() {
    let fx = fx();
    fx.env.sql(&tall_plan());
    // The view handles input in order: once the wheel has scrolled the chart, `q` before it has been handled; the script ends the
    // view after both, so a `q` that quit it would have ended it before the wheel.
    let steps = Steps::new().at("the chart scrolled", |s| s.height() > 2 && !s.shown_row(2).contains(" j10│"), |screen| {
        assert!(screen.shown_rows().iter().all(|l| !l.contains(" j10│")), "{}", screen.text());
    });
    show(&fx, vec![press('q'), scroll_down(), gone()], &steps);
}

#[test]
fn without_a_terminal_it_is_refused_before_anything_is_read() {
    let fx = Fx::with_ports();
    let run = fx.cpd_without_a_terminal(&["tui", "view", "plan"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (3, json!("not_a_terminal")), "{run:?}");
}
