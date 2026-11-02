//! `cpd tui view tree` (commands.md §13.2, §17): the view runs in the test process on a `Tree` (`common/tree.rs`) over a
//! prepared store, on a `TestBackend`, with the fake `cpd` and the fake tmux of the `Fx`; a test reads the screen, presses
//! keys and clicks, and finds the `cpd` command lines and the tmux calls the view made. The rendering of single rows,
//! colours and keys is unit-tested in `tui/src/tree/`.

use crate::common;

use std::time::Duration;

use common::fx::Fx;
use common::screen::{Bytes, Display, FakeTty, Keys};
use common::tree::{self as drive, Gate, Tree, engine, fx, stamp};
use common::{TestEnv, sql};
use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;
use cpd_tui::palette::{IDLE, NEEDS_YOU, SPINNER, STOPPED, WAITING};
use ratatui::crossterm::Command;
use ratatui::crossterm::event::EnableMouseCapture;

// The tree's glyphs. `cpd_tui::tree` is private, so the tests repeat them (the unit tests of `tui/src/tree/` use the constants).
/// The closing row's arrows (md-arrow_down_drop_circle_outline, md-arrow_up_drop_circle_outline) and the pill's caps.
const DOWN: &str = "\u{F0B26}";
const UP: &str = "\u{F0B2C}";

/// The closing row of a block that folds, folded, after the pane's margin.
fn expand() -> String {
    format!(" ╰{DOWN} expand")
}

/// The same, expanded.
fn collapse() -> String {
    format!(" ╰{UP} collapse")
}

// ---------- the prepared store ----------

fn open(env: &TestEnv) -> Store {
    Store::open(
        &CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap()
}

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// `pr`: the Plan has a PR (the schema ties phases `deploy` and `done` to one).
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: bool) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late {
        "'2026-01-02T00:00:00.000Z'"
    } else {
        "NULL"
    };
    let done = if phase == "done" {
        "'2026-01-09T00:00:00.000Z'"
    } else {
        "NULL"
    };
    let (row, number, url, opened, pull_request) = if pr {
        let state = if phase == "done" { "merged" } else { "open" };
        let merged = if phase == "done" {
            "'2026-01-09T00:00:00.000Z'"
        } else {
            "NULL"
        };
        (
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/{id}', 7, 'https://github.com/acme/{id}/pull/7', 'PR of {id}', 0, '{state}', 'none', 'none',
                         '2026-01-05T00:00:00.000Z', {merged}, '2026-01-06T00:00:00.000Z');"
            ),
            "7",
            format!("'https://github.com/acme/{id}/pull/7'"),
            "'2026-01-05T00:00:00.000Z'",
            format!("(SELECT id FROM pull_requests WHERE repo = 'acme/{id}')"),
        )
    } else {
        (
            String::new(),
            "NULL",
            "NULL".to_owned(),
            "NULL",
            "NULL".to_owned(),
        )
    };
    let status = match phase {
        "grounding" | "plan" => "waiting",
        "dispatch" => "dispatch",
        "deploy" => "open",
        _ => "merged",
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked}, {done});
         {row}
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo/{id}', 'main', '{status}', 'cpd/{id}', {worktree}, {number}, {url}, {opened},
                 {pull_request}, cpd_txn(), '{created}', {locked}, {done});"
    )
}

/// `forked_from`: the Lead this row forks (contract §4.4: a Lead is a fork or has a Plan).
#[allow(clippy::too_many_arguments)]
fn session_row(
    id: &str,
    name: &str,
    context: &str,
    plan: &str,
    state: &str,
    label: Option<&str>,
    created: &str,
    forked_from: Option<&str>,
) -> String {
    let forked_from = forked_from.map_or("NULL".to_owned(), |f| format!("'{f}'"));
    let plan = if plan.is_empty() {
        "NULL".to_owned()
    } else {
        format!("'{plan}'")
    };
    let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at, forked_from)
         VALUES ('{id}', 'lead', '{context}', {plan}, NULL, '{name}', '/sessions/{id}.jsonl', '/repo', 'O3',
                 NULL, '{state}', {label}, '2026-01-03T00:00:00.000Z', '{created}', {forked_from});"
    )
}

/// A Context's Manager Session (contract §3.1): idle, never a row of the tree.
fn manager_row(id: &str, context: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'manager', '{context}', NULL, NULL, '{context}-manager', '/sessions/{id}.jsonl', '/repo', 'O4',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:07.000Z');"
    )
}

/// What the engine's E8 would open for `populate`'s store (the tree reads the Inbox, it does not compute it).
fn condition(
    kind: Kind,
    urgency: Urgency,
    session: &str,
    plan: Option<&str>,
    context: &str,
) -> Condition {
    Condition {
        kind,
        urgency,
        subject: format!("session:{session}"),
        label: format!("{session} label"),
        url: None,
        context: Some(ContextId::parse(context).unwrap()),
        plan: plan.map(|p| PlanId::parse(p).unwrap()),
        job: None,
        session: Some(SessionId::parse(session).unwrap()),
        record: None,
        event: None,
        opened_at: None,
    }
}

/// The items `populate` leaves open: red dialogs of the fork `f-blocked` and of the Lead `l-rel`, and, for the two
/// Contexts' dots, a white `unread` of alpha's Manager and a magenta `dialog` of beta's.
fn items() -> Vec<Condition> {
    vec![
        condition(
            Kind::Dialog,
            Urgency::Red,
            "f-blocked",
            Some("ship"),
            "alpha",
        ),
        condition(Kind::Dialog, Urgency::Red, "l-rel", Some("rel"), "beta"),
        condition(Kind::Unread, Urgency::White, "m-alpha", None, "alpha"),
        condition(Kind::Dialog, Urgency::Magenta, "m-beta", None, "beta"),
    ]
}

fn job_row(plan: &str, key: &str, status: &str) -> String {
    let fingerprint = if status == "draft" {
        "NULL".to_owned()
    } else {
        format!("'{}'", "0".repeat(64))
    };
    let merged = if status == "merged" {
        "'abc123'"
    } else {
        "NULL"
    };
    let failed = if status == "failed" {
        "'crashed'"
    } else {
        "NULL"
    };
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', 'repo', '{key}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
                 cpd_txn(), NULL, NULL, {failed}, {merged}, '2026-01-02T00:00:00.000Z');"
    )
}

fn moved(context: &str, plan: &str, session: &str, place: &str) -> String {
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-moved', '{context}', '{plan}', '{session}', 'fixture',
                 '{{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"{place}\"}}');"
    )
}

/// Contexts, Plans and Sessions in each state:
///
/// - `alpha` (also a Lead with no Plan, `g-alpha`, as migration 0010 leaves a grounded Lead): `ship` (dispatch; Lead `l-ship` working, in the chat slot; side forks
///   `f-idle` and `f-blocked`; Jobs queued, running, failed and two merged), `sketch` (plan; Lead idle; two drafts);
/// - `beta`: `rel` (pr; its Lead is blocked), `old` (done; its Lead is stopped);
/// - `gamma`: nothing.
///
/// Creation orders alpha's shiro rows `ship` (newest), `sketch`, then the fork `f-idle` (oldest), so that the tree's order
/// (tier, then creation newest first; hs-d686, pro-d48) is the seeded one: `f-blocked`, `ship`, `sketch`, `f-idle`.
///
/// The Inbox ([`items`]) gives the dots their filled dot: `f-blocked` and `l-rel` have a red dialog; the Contexts alpha and beta
/// have a Manager with an item, which alone fills a Context row's filled dot.
///
/// The Inbox items are opened at `opened` (contract §1.3 text).
fn populate_at(env: &TestEnv, opened: &str) {
    let mut store = open(env);
    let mut s = String::new();
    s += &context("alpha");
    s += &context("beta");
    s += &context("gamma");
    s += &plan_row(
        "ship",
        "alpha",
        "dispatch",
        "l-ship",
        "2026-01-02T00:00:10.000Z",
        false,
    );
    s += &plan_row(
        "sketch",
        "alpha",
        "plan",
        "l-sketch",
        "2026-01-02T00:00:09.000Z",
        false,
    );
    s += &plan_row(
        "rel",
        "beta",
        "deploy",
        "l-rel",
        "2026-01-02T00:00:02.000Z",
        true,
    );
    s += &plan_row(
        "old",
        "beta",
        "done",
        "l-old",
        "2026-01-02T00:00:03.000Z",
        true,
    );
    s += &session_row(
        "l-ship",
        "ship lead",
        "alpha",
        "ship",
        "working",
        None,
        "2026-01-02T00:00:00.000Z",
        None,
    );
    s += &session_row(
        "g-alpha",
        "grounded",
        "alpha",
        "",
        "idle",
        None,
        "2026-01-02T00:00:00.000Z",
        Some("l-ship"),
    );
    s += &session_row(
        "f-idle",
        "ship fork",
        "alpha",
        "ship",
        "idle",
        None,
        "2026-01-02T00:00:02.000Z",
        Some("l-ship"),
    );
    s += &session_row(
        "f-blocked",
        "ship review",
        "alpha",
        "ship",
        "blocked",
        Some("cpd: Worker question"),
        "2026-01-02T00:00:09.000Z",
        Some("l-ship"),
    );
    s += &session_row(
        "l-sketch",
        "sketch",
        "alpha",
        "sketch",
        "idle",
        None,
        "2026-01-02T00:00:01.000Z",
        None,
    );
    s += &session_row(
        "l-rel",
        "rel lead",
        "beta",
        "rel",
        "blocked",
        Some("cpd: PR feedback"),
        "2026-01-02T00:00:02.000Z",
        None,
    );
    s += &session_row(
        "l-old",
        "old lead",
        "beta",
        "old",
        "stopped",
        None,
        "2026-01-02T00:00:03.000Z",
        None,
    );
    for (key, status) in [
        ("a", "merged"),
        ("b", "merged"),
        ("c", "running"),
        ("d", "queued"),
        ("e", "failed"),
    ] {
        s += &job_row("ship", key, status);
    }
    for key in ["s1", "s2"] {
        s += &job_row("sketch", key, "draft");
    }
    s += &job_row("rel", "r1", "merged");
    s += &moved("alpha", "ship", "l-ship", "chat");
    s += &manager_row("m-alpha", "alpha");
    s += &manager_row("m-beta", "beta");
    sql(&mut store, &s);
    reconcile(&mut store, &items(), opened).unwrap();
}

// ---------- the screen ----------

/// What the legend (`?`) logs where no tmux client is attached (a view draws no note, commands.md §17 "No notes").
const NO_CLIENT: &str = "No tmux client is attached to the cockpit.";

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

// ---------- tests: the view in this process ----------
//
// The view runs in the test process on a `Tree` (`common/tree.rs`): the clock is the `ManualClock`, `cpd` is faked on the
// `FakeRunner` and a call returns when the loop waits for input again, so a screen and the calls are read at once.

/// [`populate`] for an `Fx`: its Inbox items were opened five and a half minutes before the clock's time.
fn populate_in(fx: &Fx) {
    populate_at(&fx.env, &stamp(fx, -330));
}

/// The first screen of `tree`: it has drawn the tree.
fn ready(tree: &Tree) -> Vec<String> {
    let screen = tree.screen();
    assert!(
        screen.iter().any(|r| r.contains("alpha")),
        "the tree: {screen:?}"
    );
    screen
}

/// The style of every cell of the row that shows `text`, when one does.
fn styled_row(tree: &Tree, text: &str) -> Option<Vec<common::screen::Cell>> {
    let screen = tree.styled();
    let (_, y) = screen.find(text)?;
    Some(
        (0..screen.width())
            .map(|x| screen.cell(x, y).clone())
            .collect(),
    )
}

#[test]
fn the_view_needs_a_terminal() {
    let fx = fx();
    let mut out = Bytes::default();
    let display = Display::new(10, 3);
    let error = cpd_tui::tree::run_on(
        &fx.fakes.ports,
        &mut FakeTty::new(false),
        &mut out,
        display.terminal(),
        &mut Keys::script([]),
    )
    .unwrap_err();
    assert_eq!(
        (error.exit().code(), error.code()),
        (3, ErrorCode::NotATerminal),
        "{error:?}"
    );
    assert_eq!(error.details()["command"], "tui view tree");
    assert!(out.bytes().is_empty(), "nothing is written to the terminal");
}

#[test]
fn the_tree_captures_the_mouse_so_tmuxs_default_right_click_binding_passes_it_on() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let tree = Tree::start(&fx, 46, 14);
    let mut on = String::new();
    EnableMouseCapture.write_ansi(&mut on).unwrap();
    let written = String::from_utf8_lossy(&tree.written()).into_owned();
    assert!(
        written.contains(&on),
        "the mouse capture is on: {written:?}"
    );
}

#[test]
fn an_invalid_folds_file_starts_the_tree_with_the_defaults_logs_it_and_the_next_fold_replaces_it() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    archive_for_folds(&fx.env);
    std::fs::create_dir_all(folds_file(&fx.env).parent().unwrap()).unwrap();
    std::fs::write(folds_file(&fx.env), "{not json").unwrap();
    let mut tree = Tree::start(&fx, 46, 26);
    let screen = ready(&tree);
    tree.until_logged("fold state reset: ");
    assert!(
        screen.iter().all(|r| !r.contains("fold state")),
        "no note: {screen:?}"
    );
    assert!(
        alpha_is_expanded(&screen),
        "the defaults: alpha is expanded: {screen:?}"
    );
    assert_eq!(
        std::fs::read_to_string(folds_file(&fx.env)).unwrap(),
        "{not json",
        "nothing is written until a fold changes"
    );
    let toggle = screen
        .iter()
        .position(|r| r.trim() == format!("╰{UP} collapse"))
        .unwrap() as u16;
    tree.click(5, toggle);
    tree.until("alpha folded", |s| {
        s.get(5)
            .is_some_and(|r| r.trim() == format!("╰{DOWN} expand"))
    });
    wait_for_folds(
        &mut tree,
        &fx.env,
        r#"{"collapsed":["alpha"],"expanded":[]}"#,
    );
}

#[test]
fn a_missing_folds_file_is_every_context_open() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    archive_for_folds(&fx.env);
    let mut tree = Tree::start(&fx, 46, 26);
    let screen = ready(&tree);
    assert!(alpha_is_expanded(&screen), "{screen:?}");
    // the archived Context beta is in the Archive section: the section starts folded and beta is not drawn
    assert!(
        !screen.iter().any(|r| r.contains(" beta")),
        "the folder starts folded: {screen:?}"
    );
    assert!(
        screen.iter().all(|r| !r.contains("fold state")),
        "{screen:?}"
    );
    assert!(!folds_file(&fx.env).exists(), "nothing is written");
    let archive = screen
        .iter()
        .position(|r| r.contains("Archive"))
        .expect("the Archive header");
    assert_eq!(
        archive, 24,
        "a folded Archive header is the pane's last row but one, a blank row below it (pro-d351): {screen:?}"
    );
    tree.click(5, archive as u16);
    let screen = tree.until("the Archive section open", |s| {
        s.iter().any(|r| r.contains(" beta"))
    });
    let beta = screen.iter().position(|r| r.contains(" beta")).unwrap();
    assert_eq!(
        screen[beta + 1].trim(),
        format!("╰{DOWN} expand"),
        "the archived Context starts folded: {screen:?}"
    );
}

#[test]
fn a_session_waiting_for_an_agent_shows_the_waiting_dot_and_not_the_idle_one() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let mut tree = Tree::start(&fx, 46, 22);
    ready(&tree);
    fx.cpd(&[
        "session",
        "state",
        "waiting",
        "--label",
        "cpd: Worker question",
        "--session",
        "l-sketch",
    ])
    .ok();
    fx.cpd(&["session", "state", "working", "--session", "f-idle"])
        .ok();
    let screen = tree.until("the waiting Lead", |s| {
        s.iter().any(|r| r.contains(&format!("{WAITING} sketch")))
    });
    assert!(
        !screen.iter().any(|r| r.contains(&format!("{IDLE} sketch"))),
        "not the idle glyph"
    );
}

#[test]
fn the_tree_follows_the_events_of_the_store() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let mut tree = Tree::start(&fx, 46, 22);
    ready(&tree);
    // session-state
    fx.cpd(&[
        "session",
        "state",
        "blocked",
        "--label",
        "cpd: Ask",
        "--session",
        "l-sketch",
    ])
    .ok();
    // a later change on the screen is the positive signal that the view has taken a snapshot with the blocked Session
    fx.cpd(&["session", "state", "working", "--session", "f-idle"])
        .ok();
    let screen = tree.until("the working fork", |s| {
        s.iter()
            .any(|r| r.contains("ship·f-id") && SPINNER.iter().any(|g| r.contains(g)))
    });
    assert!(
        screen
            .iter()
            .any(|r| r.contains(&format!("├─{IDLE} sketch"))),
        "a blocked Session is no filled dot until the Inbox has its dialog"
    );
    // inbox-opened: the engine's E8 opens the dialog
    let mut store = open(&fx.env);
    let mut now = items();
    now.push(condition(
        Kind::Dialog,
        Urgency::Magenta,
        "l-sketch",
        Some("sketch"),
        "alpha",
    ));
    reconcile(&mut store, &now, &stamp(&fx, -60)).unwrap();
    tree.until("the opened item", |s| {
        s.iter()
            .any(|r| r.contains(&format!("├─{NEEDS_YOU} sketch")))
    });
    // inbox-closed: its cause ends
    reconcile(&mut store, &items(), &stamp(&fx, -30)).unwrap();
    let screen = tree.until("the closed item", |s| {
        s.iter().any(|r| r.contains(&format!("├─{IDLE} sketch")))
    });
    // inbox-dismissed, by the command: the red dialog of the fork falls back to its glyph
    assert!(
        screen
            .iter()
            .any(|r| r.contains(&format!("├─{NEEDS_YOU} ship·f-bl")))
    );
    let id = store_item(&mut store, "session:f-blocked");
    fx.cpd(&["inbox", "dismiss", &id.to_string()]).ok();
    tree.until("the dismissed item", |s| {
        s.iter().any(|r| r.contains(&format!("├─{IDLE} ship·f-bl")))
    });
}

/// A failed snapshot keeps the tree on screen and shows the error in the footer; the next refresh takes the snapshot
/// again, with no Event needed, and the error goes when it succeeds (tui-W4). The read is made to fail by renaming the
/// table it reads; the snapshot is taken because an Event came (a store opened before the rename appends it).
#[test]
fn a_failed_snapshot_keeps_the_tree_and_shows_the_error_until_a_snapshot_succeeds() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    let mut store = open(&fx.env);
    // not through `Store`, which refuses a changed schema
    let raw = |text: &str| {
        rusqlite::Connection::open(fx.env.cpd_home().join("cpd.db"))
            .unwrap()
            .execute_batch(text)
            .unwrap()
    };
    raw("ALTER TABLE sessions RENAME TO sessions_gone;");
    sql(
        &mut store,
        "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '2026-02-01T00:00:00.000Z', 'session-state', 'fixture', '{}');",
    );
    let screen = tree.until("the error in the footer", |s| {
        s.iter().any(|r| r.contains("no such table"))
    });
    assert!(
        screen
            .iter()
            .any(|r| r.starts_with(&format!(" ╭{NEEDS_YOU} beta"))),
        "the tree stays: {screen:?}"
    );
    raw("ALTER TABLE sessions_gone RENAME TO sessions;");
    tree.until("the error to go", |s| {
        !s.iter().any(|r| r.contains("no such table"))
            && s.iter()
                .any(|r| r.starts_with(&format!(" ╭{NEEDS_YOU} beta")))
    });
}

/// The tree follows Events in its own process (commands.md §17): it starts no `cpd events follow` (tui-F4, tui-G4), whose
/// spawn could fail on a path that is gone and be swallowed (tui-W2). It runs no program at all: the `FakeRunner` has no call.
#[test]
fn the_tree_starts_no_subprocess() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let mut tree = Tree::start(&fx, 46, 22);
    ready(&tree);
    // follow an Event and take a new snapshot: the fork's red dialog is dismissed
    let id = store_item(&mut open(&fx.env), "session:f-blocked");
    fx.cpd(&["inbox", "dismiss", &id.to_string()]).ok();
    tree.until("the new state", |s| {
        s.iter().any(|r| r.contains(&format!("├─{IDLE} ship·f-bl")))
    });
    assert_eq!(fx.fakes.runner.calls(), [], "no program ran");
    assert!(fx.fakes.spawn.launches().is_empty(), "nothing was spawned");
}

/// A follow that fails is shown in the footer, not swallowed (tui-W2): the cursor is from another database (the highest
/// Event id fell below it), which the follow refuses as `cursor_ahead`.
#[test]
fn a_follow_that_fails_is_shown_in_the_footer() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let mut tree = Tree::start(&fx, 100, 14);
    ready(&tree);
    // not through `Store`, which keeps the Events append-only
    let raw = |text: &str| {
        rusqlite::Connection::open(fx.env.cpd_home().join("cpd.db"))
            .unwrap()
            .execute_batch(text)
            .unwrap()
    };
    raw("DROP TRIGGER events_no_delete; DELETE FROM events;");
    let screen = tree.until("the error in the footer", |s| {
        s.iter().any(|r| r.contains("Event cursor"))
    });
    assert!(
        screen
            .iter()
            .any(|r| r.starts_with(&format!(" ╭{NEEDS_YOU} beta"))),
        "the tree stays: {screen:?}"
    );
}

#[test]
fn the_tree_follows_the_archive_events() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let mut tree = Tree::start(&fx, 46, 24);
    ready(&tree);
    // an archived row is drawn muted: its colours change, its text does not (the working dots turn, so a row, not the screen)
    let before = styled_row(&tree, " old");
    // the kinds of migration 3, written as the commands will: the row, then its Event
    sql(
        &mut open(&fx.env),
        "UPDATE plans SET archived_at = '2026-02-02T00:00:00.000Z' WHERE id = 'old';
         INSERT INTO events (txn, at, kind, context, plan, text, data)
         VALUES (cpd_txn(), '2026-02-02T00:00:00.000Z', 'plan-archived', 'beta', 'old', 'fixture', '{\"archived\":true}');",
    );
    tree.until_that("the Plan muted", |t| styled_row(t, " old") != before);
    let screen = tree.screen();
    assert!(
        screen
            .iter()
            .any(|r| r.contains(&format!("├─{STOPPED} old"))),
        "an archived Plan stays in its Context's block: {screen:?}"
    );
    let before = styled_row(&tree, "gamma");
    sql(
        &mut open(&fx.env),
        "UPDATE contexts SET archived_at = '2026-02-02T00:00:00.000Z' WHERE id = 'gamma';
         INSERT INTO events (txn, at, kind, context, text, data)
         VALUES (cpd_txn(), '2026-02-02T00:00:01.000Z', 'context-archived', 'gamma', 'fixture', '{\"archived\":true}');",
    );
    tree.until_that("the Context muted", |t| styled_row(t, "gamma") != before);
    sql(
        &mut open(&fx.env),
        "UPDATE sessions SET archived_at = '2026-02-03T00:00:00.000Z' WHERE id = 'f-idle';
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-02-03T00:00:00.000Z', 'session-archived', 'alpha', 'ship', 'f-idle', 'fixture', '{\"archived\":true}');",
    );
    tree.until("the fork after the live ones", |s| {
        let at = |text: &str| s.iter().position(|r| r.contains(text));
        at("ship·f-bl")
            .zip(at("ship·f-id"))
            .is_some_and(|(live, archived)| live < archived)
    });
}

#[test]
fn enter_opens_the_lead_of_a_plan_and_a_fork_with_session_open() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::fake_cpd(
        &fx,
        r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#,
    );
    let mut tree = Tree::start(&fx, 46, 14);
    ready(&tree);
    tree.down(6); // the cursor appears on rel, then old, then the fork ship·f-bl, then ship, then sketch, then f-idle
    tree.keys(&["Enter"]);
    assert_eq!(drive::calls(&fx), [words(&["session", "open", "f-idle"])]);
    tree.keys(&["k", "k", "Enter"]);
    assert_eq!(drive::calls(&fx)[1], words(&["session", "open", "l-ship"]));
}

#[test]
fn a_click_on_a_context_title_opens_its_manager_and_only_its_closing_row_folds_its_archived_rows() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    sql(
        &mut open(&fx.env),
        "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';",
    );
    drive::fake_cpd(
        &fx,
        r#"{"ok":true,"manager":{"session":"m-alpha","name":"alpha-manager","state":"idle","pane":"%1","window":"@1"},"started":false,"session":"m-alpha","displaced":null}"#,
    );
    let mut tree = Tree::start(&fx, 46, 20);
    let start = ready(&tree);
    // beta is live before alpha: alpha's block starts below it, not at the top row.
    let alpha = start
        .iter()
        .position(|r| r.contains(" alpha"))
        .expect("alpha's title row") as u16;
    tree.click(8, alpha); // the title row of the Context `alpha`
    assert_eq!(
        drive::calls(&fx),
        [
            words(&["context", "manager", "--context=alpha", "--by-user"]),
            words(&["session", "open", "m-alpha"])
        ]
    );
    tree.idle();
    assert_eq!(drive::calls(&fx).len(), 2, "no more commands");
    let screen = tree.screen();
    assert!(
        screen[alpha as usize].starts_with(&format!(" ╭{NEEDS_YOU} alpha"))
            && screen.iter().any(|r| r.contains("ship")),
        "the title click folds nothing: {screen:?}"
    );
    assert_eq!(
        screen[alpha as usize + 5],
        collapse(),
        "alpha's closing row"
    );
    tree.click(1, alpha); // the first cell of the title row: its dot
    assert_eq!(
        drive::calls(&fx).len(),
        4,
        "the dot is the title row's: it opens the Manager too"
    );
    tree.click(8, alpha + 5); // its `╰󰬬 collapse` row
    let folded = tree.until("alpha's archived row hidden", |s| {
        !s.iter().any(|r| r.contains("ship·f-bl"))
    });
    assert_eq!(
        (
            folded[alpha as usize].as_str(),
            folded[alpha as usize + 3].as_str(),
            folded[alpha as usize + 4].as_str()
        ),
        (
            format!(" ╭{NEEDS_YOU} alpha").as_str(),
            format!(" ├─{IDLE} ship·f-id").as_str(),
            expand().as_str()
        ),
        "a folded Context draws its Manager's dot (its unread item), its live rows and a `╰󰬦 expand` row: {folded:?}"
    );
    assert!(
        folded.iter().any(|r| r.contains("ship")) && folded.iter().any(|r| r.contains("sketch")),
        "the live rows stay: {folded:?}"
    );
    assert_eq!(
        drive::calls(&fx).len(),
        4,
        "the closing row runs no command"
    );
    tree.click(8, alpha + 4);
    tree.until("alpha's archived row back", |s| {
        s.iter().any(|r| r.contains("ship·f-bl"))
    });
    assert_eq!(drive::calls(&fx).len(), 4);
}

#[test]
fn a_refused_context_manager_logs_its_error_and_opens_nothing() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::fake_cpd_fails(
        &fx,
        r#"{"ok":false,"error":{"code":"archived","message":"The Context is archived."}}"#,
    );
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    tree.click(8, 5); // the title row of the Context `alpha`
    tree.until_logged(
        "failed: context manager --context=alpha --by-user: archived: The Context is archived.",
    );
    assert!(!tree.shows("archived"), "no note: {:?}", tree.screen());
    assert_eq!(
        drive::calls(&fx),
        [words(&[
            "context",
            "manager",
            "--context=alpha",
            "--by-user"
        ])],
        "no session open"
    );
}

#[test]
fn no_key_but_the_references_does_anything() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, 46, 22);
    ready(&tree);
    tree.idle();
    let before = tree.screen();
    tree.down(2); // on sketch
    tree.keys(&["q", "f", "r", "p", "Space", "G", "Escape"]); // a `q` that ended the view would end the test here
    tree.idle();
    assert!(drive::calls(&fx).is_empty());
    assert_eq!(tree.screen(), before, "no prompt, nothing folded, no note");
}

#[test]
fn the_tree_keeps_drawing_and_reading_keys_while_a_command_runs() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    // the command answers when the test says so: the loop must go on without it
    let (gate, key) = Gate::new(Reply::exit(
        3,
        r#"{"ok":false,"error":{"code":"slow","message":"Took a while.","details":{}}}"#,
        "",
    ));
    let _ = (*fx.fakes.runner).clone().route("cpd", gate);
    drive::fake_cockpit(&fx);
    let mut tree = Tree::start(&fx, 80, 14);
    let key = key; // dropped before `tree`: a loop that waits on a command that waits on the gate cannot end
    ready(&tree);
    tree.down(4); // l-ship
    tree.eager(true);
    tree.keys(&["Enter"]);
    let before = styled_row(&tree, " sketch");
    tree.keys(&["j"]); // read and drawn while the command runs: the cursor moves to `sketch`
    assert_ne!(
        styled_row(&tree, " sketch"),
        before,
        "the key was read and the frame drawn, not after the command"
    );
    assert!(
        !tree.log().contains("slow:"),
        "the command still runs: {}",
        tree.log()
    );
    key.open();
    tree.idle();
    tree.until_logged("failed: session open l-ship: slow: Took a while.");
    // the command's call is read once it has ended: the loop dispatched it from its own task, so it is not there at once
    assert_eq!(
        drive::calls(&fx),
        [words(&["session", "open", "l-ship"])],
        "the command ran, and waited"
    );
}

#[test]
fn a_refused_command_logs_its_error_and_the_frame_shows_none() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::fake_cpd_fails(
        &fx,
        r#"{"ok":false,"error":{"code":"no_cockpit","message":"No cockpit.","details":{}}}"#,
    );
    let mut tree = Tree::start(&fx, 46, 14);
    ready(&tree);
    tree.down(4); // l-ship
    tree.keys(&["Enter"]);
    tree.until_logged("failed: session open l-ship: no_cockpit: No cockpit.");
    assert_eq!(drive::calls(&fx), [words(&["session", "open", "l-ship"])]);
    tree.idle();
    assert!(!tree.shows("no_cockpit"), "no note: {:?}", tree.screen());
}

#[test]
fn a_right_click_opens_the_menu_of_the_row_and_esc_or_a_click_outside_closes_it_running_nothing() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, 46, 22);
    ready(&tree);
    tree.right_click(8, 9); // the fork `ship fork`
    let screen = tree.until("the fork's menu", |s| menu_open(s, "Open"));
    for item in ["Open", "Rename…", "Fork…", "Close", "Archive"] {
        assert!(
            screen.iter().any(|r| r.contains(&format!("│ {item} "))),
            "{item} in\n{}",
            screen.join("\n")
        );
    }
    tree.keys(&["Escape"]);
    tree.until("the menu closed", the_menu_is_closed);
    tree.right_click(5, 5); // the Context `alpha`
    let screen = tree.until("the Context's menu", |s| menu_open(s, "New Plan…"));
    assert!(
        screen.iter().any(|r| r.contains("│ Rename… "))
            && screen.iter().any(|r| r.contains("│ Archive "))
    );
    tree.click(40, 12); // outside it
    tree.until("the menu closed", the_menu_is_closed);
    tree.right_click(8, 7); // the Plan `ship`
    tree.until("the Plan's menu", |s| menu_open(s, "New session…"));
    tree.right_click(45, 13); // a right click outside closes it too
    tree.until("the menu closed", the_menu_is_closed);
    tree.right_click(5, 14); // the empty area
    tree.until("the empty area's menu", |s| menu_open(s, "New Context…"));
    tree.keys(&["Escape"]);
    tree.right_click(5, 12); // the terminator closing row of the Context `gamma`, which has no row: no menu
    tree.idle();
    assert!(
        the_menu_is_closed(&tree.screen()),
        "a closing row has no menu"
    );
    assert!(drive::calls(&fx).is_empty(), "{:?}", drive::calls(&fx));
    assert!(!tree.shows("Archive"), "no prompt");
}

#[test]
fn a_click_on_an_archived_fork_unarchives_and_opens_it() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    sql(
        &mut open(&fx.env),
        "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';",
    );
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, 60, 22);
    let screen = ready(&tree);
    let at = |s: &[String], text: &str| s.iter().position(|r| r.contains(text)).expect(text) as u16;
    let fbl = at(&screen, "ship·f-bl");
    let fid = at(&screen, "ship·f-id");
    assert!(
        fid < fbl,
        "the archived fork is inside alpha's block, after the live rows: {screen:?}"
    );
    assert_eq!(
        screen[fbl as usize + 1],
        collapse(),
        "the archived fork closes alpha's block: {screen:?}"
    );
    tree.click(10, fbl);
    assert_eq!(
        drive::calls(&fx),
        [
            words(&["session", "unarchive", "f-blocked"]),
            words(&["session", "open", "f-blocked"])
        ]
    );
    tree.click(8, fid); // a live fork still opens
    assert_eq!(drive::calls(&fx)[2], words(&["session", "open", "f-idle"]));
}

/// hs-d686, pro-d48: at both levels the drawn rows come by dot-urgency tier (red 0 > yellow 1 > magenta 2 > cyan 3 > shiro 4,
/// the white dot and every row with no Inbox item, the red loader among them), then by creation, newest first; a newer
/// Event never moves a row; archived rows stay last, whatever their dot. Every seed gives the newest-created row the oldest Event.
#[test]
fn the_tree_orders_by_dot_tier_then_newest_first() {
    let row = |s: &[String], text: &str| {
        s.iter()
            .position(|r| r.contains(text))
            .unwrap_or_else(|| panic!("{text} on\n{}", s.join("\n")))
    };

    // Level 1: beta's Manager has a magenta dialog (tier 2), then the shiro Contexts `gamma` (created after alpha, no Event)
    // and `alpha` (a white unread and the newer Events).
    // Level 2: alpha's live rows as ONE list: the red fork `ship·f-bl`, then by creation, newest first, the fork
    // `ship·f-id`, `sketch` and `ship`, whose Events are the newest of the three.
    {
        let fx = fx();
        let _engine = engine(&fx);
        populate_in(&fx);
        sql(
            &mut open(&fx.env),
            "UPDATE contexts SET created_at = '2026-01-05T00:00:00.000Z' WHERE id = 'gamma';
             UPDATE plans SET created_at = '2026-01-03T00:00:00.000Z' WHERE id = 'ship';
             UPDATE plans SET created_at = '2026-01-04T00:00:00.000Z' WHERE id = 'sketch';
             UPDATE sessions SET created_at = '2026-01-05T00:00:00.000Z' WHERE id = 'f-idle';",
        );
        let tree = Tree::start(&fx, 46, 30);
        let screen = ready(&tree);
        let (beta, alpha, gamma) = (
            row(&screen, " beta"),
            row(&screen, " alpha"),
            row(&screen, " gamma"),
        );
        assert!(
            beta < gamma && gamma < alpha,
            "Contexts by tier then creation, newest first: {screen:#?}"
        );
        let (fbl, ship, sketch, fid) = (
            row(&screen, "ship·f-bl"),
            row(&screen, " ship "),
            row(&screen, " sketch"),
            row(&screen, "ship·f-id"),
        );
        assert!(
            fbl < fid && fid < sketch && sketch < ship,
            "alpha's live rows, one list, by tier then creation, newest first: {screen:#?}"
        );
    }

    // The reverse creation puts `alpha` above `gamma`: the order follows creation, not the Context id.
    {
        let fx = fx();
        let _engine = engine(&fx);
        populate_in(&fx);
        sql(
            &mut open(&fx.env),
            "UPDATE contexts SET created_at = '2026-01-06T00:00:00.000Z' WHERE id = 'alpha';",
        );
        let tree = Tree::start(&fx, 46, 30);
        let screen = ready(&tree);
        assert!(
            row(&screen, " beta") < row(&screen, " alpha")
                && row(&screen, " alpha") < row(&screen, " gamma"),
            "{screen:#?}"
        );
    }

    // A newer Event naming `sketch` never lifts it: `ship`, created later, stays above it.
    {
        let fx = fx();
        let _engine = engine(&fx);
        populate_in(&fx);
        sql(
            &mut open(&fx.env),
            "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
             VALUES (cpd_txn(), '2026-01-09T00:00:00.000Z', 'plan-phase', 'alpha', 'sketch', NULL, 'fixture', '{}');
             UPDATE plans SET created_at = '2026-01-03T00:00:00.000Z' WHERE id = 'sketch';
             UPDATE plans SET created_at = '2026-01-04T00:00:00.000Z' WHERE id = 'ship';",
        );
        let tree = Tree::start(&fx, 46, 30);
        let screen = ready(&tree);
        let (fbl, ship, sketch, fid) = (
            row(&screen, "ship·f-bl"),
            row(&screen, " ship "),
            row(&screen, " sketch"),
            row(&screen, "ship·f-id"),
        );
        assert!(
            fbl < ship && ship < sketch && sketch < fid,
            "a newer Event never moves a row: {screen:#?}"
        );
    }

    // Archived rows are last whatever their dot or creation: the archived Plan `old` comes after its live `rel`, and the
    // archived red fork `ship·f-bl` after every live row of alpha.
    {
        let fx = fx();
        let _engine = engine(&fx);
        populate_in(&fx);
        sql(
            &mut open(&fx.env),
            "UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'old';
             UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z', created_at = '2026-01-20T00:00:00.000Z' WHERE id = 'f-blocked';",
        );
        let tree = Tree::start(&fx, 46, 30);
        let screen = ready(&tree);
        let blade = row(&screen, "ship·f-bl");
        assert!(
            row(&screen, " rel") < row(&screen, " old")
                && row(&screen, " ship ") < blade
                && row(&screen, " sketch") < blade
                && row(&screen, "ship·f-id") < blade,
            "an archived Plan and an archived fork draw after every live row: {screen:#?}"
        );
    }
}

#[test]
fn a_click_that_shows_a_history_is_refused_in_the_log() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    sql(
        &mut open(&fx.env),
        "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';",
    );
    drive::fake_cockpit(&fx);
    fx.fake("cpd", Reply::exit(1, r#"{"ok":false,"error":{"code":"file_unreadable","message":"The session file is missing.","details":{}}}"#, ""));
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    let screen = tree.until("the archived fork", |s| {
        s.iter().any(|r| r.contains(&format!("├─{IDLE} ship·f-bl")))
    });
    let fbl = screen
        .iter()
        .position(|r| r.contains("ship·f-bl"))
        .expect("the archived fork") as u16;
    tree.click(10, fbl);
    tree.until_logged("file_unreadable: The session file is missing.");
    assert!(
        !tree.shows("file_unreadable"),
        "no note: {:?}",
        tree.screen()
    );
}

#[test]
fn unarchive_in_the_menu_runs_without_a_popup_and_a_refusal_is_logged() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    archive(&fx.env);
    drive::fake_cpd_fails(
        &fx,
        r#"{"ok":false,"error":{"code":"archived","message":"The Context is archived.","details":{}}}"#,
    );
    let mut tree = Tree::start(&fx, 46, 22);
    ready(&tree);
    tree.until("the archived Plan", |s| {
        s.iter().any(|r| r.contains(&format!("├─{STOPPED} old")))
    });
    tree.right_click(10, 3); // the archived Plan `old`
    let screen = tree.until("the menu", |s| menu_open(s, "Rename…"));
    assert!(
        screen.iter().any(|r| r.contains("│ Rename… ")),
        "{screen:?}"
    );
    assert!(
        screen
            .iter()
            .all(|r| !r.contains("New Plan") && !r.contains("Archive ")),
        "Rename…, Copy id and Unarchive only: {screen:?}"
    );
    tree.keys(&["Down", "Down", "Enter"]); // Unarchive
    assert_eq!(
        drive::calls(&fx),
        [words(&["plan", "unarchive", "--plan=old"])]
    );
    tree.until_logged("failed: plan unarchive --plan=old: archived: The Context is archived.");
    assert!(
        !drive::tmux_verbs(&fx).contains(&"display-popup".to_owned()),
        "no popup: {:?}",
        drive::tmux_verbs(&fx)
    );
}

#[test]
fn a_collapsed_context_and_an_expanded_archived_context_survive_a_restart_of_the_view() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    archive_for_folds(&fx.env);
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, 46, 26);
    let screen = ready(&tree);
    assert!(alpha_is_expanded(&screen), "{screen:#?}");
    assert!(
        !folds_file(&fx.env).exists(),
        "a start with no fold writes nothing"
    );
    // alpha's `╰󰬬 collapse` row (the fold widget's toggle row is the only one that folds, commands.md §13.2): its
    // archived row goes, its live rows stay
    let toggle = screen
        .iter()
        .position(|r| r.trim() == format!("╰{UP} collapse"))
        .unwrap() as u16;
    tree.click(5, toggle);
    let screen = tree.until("alpha folded", |s| {
        s.get(5)
            .is_some_and(|r| r.trim() == format!("╰{DOWN} expand"))
    });
    assert!(
        screen.iter().any(|r| r.contains("sketch"))
            && !screen.iter().any(|r| r.contains("ship·f-bl")),
        "{screen:#?}"
    );
    // the archived Context beta is in the Archive section: open it (a click on its header), then beta is folded in it
    let archive = screen
        .iter()
        .position(|r| r.contains("Archive"))
        .expect("the Archive header");
    assert_eq!(
        archive, 24,
        "a folded Archive header is the pane's last row but one, a blank row below it (pro-d351): {screen:?}"
    );
    tree.click(5, archive as u16);
    let screen = tree.until("the Archive section open", |s| {
        s.iter().any(|r| r.contains(" beta"))
    });
    let beta = screen.iter().position(|r| r.contains(" beta")).unwrap();
    assert_eq!(
        screen[beta + 1].trim(),
        format!("╰{DOWN} expand"),
        "{screen:#?}"
    );
    tree.click(5, beta as u16 + 1);
    let before = tree.until("beta expanded", |s| {
        s.iter()
            .position(|r| r.contains(" beta"))
            .is_some_and(|b| s[b + 1].contains("rel"))
    });
    wait_for_folds(
        &mut tree,
        &fx.env,
        r#"{"collapsed":["alpha"],"expanded":[["beta"]],"archived":true}"#,
    );

    // restart the view: the same screen comes back
    drop(tree);
    let mut tree = Tree::start(&fx, 46, 26);
    let after = tree.until("the folds restored", |s| s == before.as_slice());
    assert_eq!(after, before);
    assert_eq!(
        drive::calls(&fx),
        Vec::<Vec<String>>::new(),
        "no command ran"
    );
    let leftovers: Vec<_> = std::fs::read_dir(folds_file(&fx.env).parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["tree-folds.json"], "no temporary file is left");
}

#[test]
fn a_stopped_engine_is_not_restarted_by_the_tree_and_no_note_says_so_and_no_engine_down_shows_with_a_manager()
 {
    let fx = fx();
    populate_in(&fx); // its Contexts have Managers: the engine's absence is no Inbox item
    drive::fake_cpd(&fx, r#"{"ok":true,"started":true,"pane":"%3"}"#);
    let mut tree = Tree::start(&fx, 46, 20);
    ready(&tree);
    // hs-d363: one restart path, the supervisor's; the tree speaks only after 60 s down
    tree.advance(Duration::from_secs(6));
    assert_eq!(
        drive::calls(&fx),
        Vec::<Vec<String>>::new(),
        "no `watch --ensure`, no message to a Manager"
    );
    let screen = tree.screen();
    assert!(
        !screen
            .iter()
            .any(|r| r.contains("engine was down") || r.contains("engine is down")),
        "{screen:?}"
    );
}

/// hs-d514: the engine down past its supervisor goes as a message that wakes **one** Manager, the most recently active
/// (the newest `state_at`), not one per Manager. The 60 s of the tree's rule are the clock's.
#[test]
fn engine_down_wakes_only_the_most_recently_active_manager() {
    let fx = fx();
    populate_in(&fx);
    sql(
        &mut open(&fx.env),
        "UPDATE sessions SET state_at = '2026-01-05T00:00:00.000Z' WHERE id = 'm-beta';
         UPDATE sessions SET state_at = '2026-01-04T00:00:00.000Z' WHERE id = 'm-alpha';",
    );
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, 46, 20);
    ready(&tree);
    tree.advance(Duration::from_secs(66));
    tree.until_that("the engine-down message", |_| !drive::calls(&fx).is_empty());
    tree.idle();
    let sent: Vec<_> = drive::calls(&fx)
        .into_iter()
        .filter(|c| c.starts_with(&words(&["session", "send"])))
        .collect();
    assert_eq!(sent.len(), 1, "one message, to one Manager: {sent:?}");
    assert_eq!(
        sent[0][2], "m-beta",
        "the most recently active Manager: {sent:?}"
    );
    assert!(
        sent[0].iter().any(|a| a == "--unblocks"),
        "a message to a Manager says what it unblocks: {sent:?}"
    );
    assert!(
        !sent[0].iter().any(|a| a == "--wake"),
        "send always wakes, with no flag: {sent:?}"
    );
}

#[test]
fn s_and_d_do_nothing_on_a_session_row_or_before_an_arrow_key() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, 46, 20);
    ready(&tree);
    tree.keys(&["s", "d"]);
    tree.down(1); // on the first Session
    tree.keys(&["s", "d"]);
    tree.idle();
    assert!(the_menu_is_closed(&tree.screen()));
    assert!(drive::calls(&fx).is_empty(), "{:?}", drive::calls(&fx));
}

// ---------- tests ----------

#[test]
fn enter_n_and_x_before_any_arrow_key_do_nothing() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.confirm();
    let mut tree = Tree::start(&fx, 46, 14);
    ready(&tree);
    tree.keys(&["Enter", "x", "n"]);
    dialogs.settled(&mut tree);
    assert!(drive::calls(&fx).is_empty());
    assert_eq!(
        dialogs.raw().len(),
        1,
        "no popup but the legend's: {:?}",
        dialogs.raw()
    );
}

#[test]
fn n_opens_an_input_popup_for_the_plans_id_and_runs_plan_new_when_it_is_accepted() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(
        &fx,
        r#"{"ok":true,"plan":{"id":"pay-4521"},"lead":{"session":"l-new"}}"#,
    );
    let dialogs = drive::dialogs(&fx);
    dialogs.type_in("pay-4521");
    let mut tree = Tree::start(&fx, 80, 14);
    ready(&tree);
    tree.down(5); // rel, old, ship·f-bl, ship, sketch
    tree.keys(&["n"]);
    assert_eq!(
        dialogs.popups(),
        [
            "cpd tui view dialog input --title='New Plan' --prompt='Fork Plan sketch into a new Plan, name:' --initial='' --validate='plan-id'"
        ]
    );
    // the Context of the cursor's Session's Plan; the name is the id and the title
    assert_eq!(
        drive::calls(&fx),
        [
            words(&[
                "plan",
                "new",
                "pay-4521",
                "--context=alpha",
                "--title=pay-4521",
                "--by-user"
            ]),
            words(&["session", "open", "l-new"])
        ],
        "the new Lead is opened"
    );
    tree.until_logged("ok: forking pay-4521 in window park-pay-4521");
    assert!(!tree.shows("name:"), "no prompt row");
}

#[test]
fn n_runs_nothing_when_its_popup_is_cancelled() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.cancel();
    let mut tree = Tree::start(&fx, 80, 14);
    ready(&tree);
    tree.down(5); // sketch
    tree.keys(&["n"]);
    assert_eq!(dialogs.popups().len(), 1);
    dialogs.settled(&mut tree);
    assert!(drive::calls(&fx).is_empty(), "{:?}", drive::calls(&fx));
    assert!(!tree.shows("name:"), "no prompt row");
}

#[test]
fn x_runs_session_close_at_once_with_no_popup() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true,"session":"l-sketch","pane":"%4"}"#);
    let dialogs = drive::dialogs(&fx);
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    tree.down(5); // sketch
    tree.keys(&["x"]);
    assert!(
        dialogs.popups().is_empty(),
        "no confirm popup: {:?}",
        dialogs.popups()
    );
    assert_eq!(
        drive::calls(&fx),
        [words(&["session", "close", "l-sketch"])]
    );
    tree.until_logged("ok: closed");
}

#[test]
fn x_refuses_the_session_in_the_chat_slot_in_a_text_dialog() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    tree.down(4); // ship, whose Lead is in the chat slot
    tree.keys(&["x"]);
    let popups = dialogs.popups();
    assert!(
        popups[0].starts_with("cpd tui view dialog text --title='Close Session' ")
            && popups[0].ends_with("--body='switch to another session first'"),
        "a refusal is a `text` dialog, not a confirm: {popups:?}"
    );
    assert!(drive::calls(&fx).is_empty());
    dialogs.settled(&mut tree);
    assert!(
        !tree.shows("switch to another"),
        "no note: {:?}",
        tree.screen()
    );
}

/// The helper of the dialogs (`common/tree.rs`): the popup's command has a result file, which `popups` leaves out, and the
/// answer goes to that file.
#[test]
fn the_dialogs_helper_answers_the_popups_result_file_and_leaves_it_out_of_what_it_lists() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.cancel();
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    tree.down(5); // sketch
    tree.keys(&["n"]);
    assert!(drive::calls(&fx).is_empty(), "a cancel runs nothing");
    let raw = dialogs.raw();
    assert!(
        raw[0].last().unwrap().contains(" --out='"),
        "the popup's command has a result file: {raw:?}"
    );
    assert!(
        !dialogs.popups()[0].contains("--out"),
        "{:?}",
        dialogs.popups()
    );
    dialogs.type_in("plan-two");
    tree.keys(&["n"]);
    assert!(
        !drive::calls(&fx).is_empty(),
        "the answer was written to the file"
    );
}

#[test]
fn question_mark_opens_the_legend_popup() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    // a tmux server with no client: the popup has nowhere to open
    drive::fake_cockpit(&fx);
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    // `?` needs no cursor; with no client attached, the popup has nowhere to open: a line of the log
    tree.keys(&["?"]);
    tree.until_logged(NO_CLIENT);
    assert!(!tree.shows(NO_CLIENT), "no note: {:?}", tree.screen());
}

#[test]
fn a_failing_session_open_opens_a_text_dialog_and_draws_no_note() {
    // commands.md §17 "No notes" (a): a command that fails is something the User must see, a `text` dialog; the tree
    // shows no note row behind it
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_fails(
        &fx,
        r#"{"ok":false,"error":{"code":"no_cockpit","message":"No cockpit.","details":{}}}"#,
    );
    let dialogs = drive::dialogs(&fx);
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    tree.down(4); // l-ship
    tree.keys(&["Enter"]);
    assert_eq!(drive::calls(&fx), [words(&["session", "open", "l-ship"])]);
    let popups = dialogs.popups();
    assert!(
        popups[0].starts_with("cpd tui view dialog text "),
        "a `text` dialog: {popups:?}"
    );
    assert!(
        popups[0].contains("session open l-ship") && popups[0].contains("no_cockpit: No cockpit."),
        "`<what failed>: <code>: <message>`: {popups:?}"
    );
    dialogs.settled(&mut tree);
    assert!(!tree.shows("no_cockpit"), "no note: {:?}", tree.screen());
}

/// The id of the open item of `subject`.
fn store_item(store: &mut Store, subject: &str) -> i64 {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT id FROM inbox_items WHERE subject = ?1 AND closed_at IS NULL",
                [subject],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

/// `populate`, with the Context `gamma` and the Plan `old` of `beta` (and its Lead) archived.
fn archive(env: &TestEnv) {
    sql(
        &mut open(env),
        "UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'gamma';
         UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'old';
         UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'l-old';",
    );
}

#[test]
fn the_cursor_skips_archived_rows_and_enter_never_opens_one() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    archive(&fx.env);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    let mut tree = Tree::start(&fx, 46, 22);
    ready(&tree);
    tree.until("the archived Plan", |s| {
        s.iter().any(|r| r.contains(&format!("├─{STOPPED} old")))
    });
    tree.down(1); // rel, then a second step reaches ship·f-bl
    tree.keys(&["Enter"]);
    assert_eq!(drive::calls(&fx), [words(&["session", "open", "l-rel"])]);
    tree.keys(&["x"]);
    assert!(
        dialogs.popups().is_empty(),
        "x closes at once, with no popup"
    );
    assert_eq!(drive::calls(&fx)[1], words(&["session", "close", "l-rel"]));
    tree.down(1); // then nowhere: the archived Plan `old` is not a stop, the Inbox's first item is
    tree.keys(&["Enter"]);
    assert_eq!(
        drive::calls(&fx)[2],
        words(&["session", "open", "f-blocked"])
    );
}

// ---------- the right-click menu ----------

/// The screen has a menu box whose first inner row (after the top border) holds `first`.
fn menu_open(screen: &[String], first: &str) -> bool {
    screen
        .windows(2)
        .any(|w| w[0].contains('╮') && w[1].contains(&format!("│ {first}")))
}

fn the_menu_is_closed(screen: &[String]) -> bool {
    screen.iter().all(|r| !r.contains('╮') && !r.contains('╯'))
}

#[test]
fn archive_on_a_fork_runs_session_archive_at_once_with_no_popup() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(
        &fx,
        r#"{"ok":true,"archived":[{"kind":"session","id":"f-idle"}],"closed":[]}"#,
    );
    let dialogs = drive::dialogs(&fx);
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.right_click(8, 9);
    tree.until("the menu", |s| menu_open(s, "Open"));
    tree.keys(&["Down", "Down", "Down", "Down", "Down", "Enter"]); // Archive is the sixth item
    assert!(
        dialogs.popups().is_empty(),
        "Archive runs at once: {:?}",
        dialogs.popups()
    );
    assert_eq!(
        drive::calls(&fx),
        [words(&["session", "archive", "f-idle"])],
        "`f-idle` is not in the chat slot"
    );
    tree.until_logged("ok: archived");
    assert!(
        the_menu_is_closed(&tree.screen()),
        "the menu closes when an item is picked"
    );
}

#[test]
fn archive_on_the_chat_slots_lead_opens_the_next_session_first_then_archives() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.confirm();
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.right_click(5, 5); // the Context `alpha`, whose Lead `l-ship` holds the chat slot: Archive is its fourth item
    tree.until("the menu", |s| menu_open(s, "New Plan…"));
    tree.click(7, 9);
    assert!(
        dialogs.popups().is_empty(),
        "Archive runs at once: {:?}",
        dialogs.popups()
    );
    assert_eq!(
        drive::calls(&fx),
        [
            words(&["session", "open", "l-old"]),
            words(&["context", "archive", "alpha"])
        ],
        "no live Session follows alpha, so the previous one opens, then the archive"
    );
    tree.right_click(8, 7); // the Plan `ship`: its Lead is the chat slot's, so the Plan below it opens first
    tree.until("the menu", |s| menu_open(s, "New session…"));
    tree.click(10, 11);
    assert_eq!(
        drive::calls(&fx)[2..],
        [
            words(&["session", "open", "l-sketch"]),
            words(&["plan", "archive", "--plan=ship"])
        ]
    );
}

#[test]
fn a_refusal_of_a_step_is_a_text_dialog_and_stops_the_archive() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_fails(
        &fx,
        r#"{"ok":false,"error":{"code":"no_cockpit","message":"There is no cockpit.","details":{}}}"#,
    );
    let dialogs = drive::dialogs(&fx);
    dialogs.confirm();
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.right_click(8, 7);
    tree.until("the menu", |s| menu_open(s, "New session…"));
    tree.click(10, 11);
    // The dialog comes when the whole action is over (the view runs it in its loop), so the archive step is not still to come.
    let popups = dialogs.popups();
    assert_eq!(popups.len(), 1, "{popups:?}");
    assert!(
        popups[0].starts_with("cpd tui view dialog text --title='session open l-sketch' "),
        "the failed step: {popups:?}"
    );
    assert!(
        popups[0].ends_with("--body='no_cockpit: There is no cockpit.'"),
        "{popups:?}"
    );
    tree.until_logged("failed: session open l-sketch: no_cockpit: There is no cockpit.");
    assert_eq!(
        drive::calls(&fx),
        [words(&["session", "open", "l-sketch"])],
        "the archive did not run"
    );
}

#[test]
fn archive_on_the_only_live_lead_in_the_chat_slot_parks_it_first() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    sql(
        &mut open(&fx.env),
        "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id IN ('f-idle', 'f-blocked', 'l-sketch', 'l-rel', 'l-old');
         UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id IN ('sketch', 'rel', 'old');",
    );
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.confirm();
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.right_click(8, 7); // the Plan `ship`
    tree.until("the menu", |s| menu_open(s, "New session…"));
    tree.click(10, 11);
    assert_eq!(
        drive::calls(&fx),
        [
            words(&["session", "park"]),
            words(&["plan", "archive", "--plan=ship"])
        ]
    );
}

#[test]
fn a_click_on_an_item_picks_it_and_a_cancelled_popup_runs_nothing() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.cancel();
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    tree.right_click(5, 5); // the Context: New Plan…, Rename…, Archive below its title row
    tree.until("the menu", |s| menu_open(s, "New Plan…"));
    tree.click(7, 7); // Rename…
    assert_eq!(
        dialogs.popups(),
        [
            "cpd tui view dialog input --title='Rename' --prompt='Title:' --initial='Title of alpha' --validate='title'"
        ]
    );
    dialogs.settled(&mut tree);
    assert!(drive::calls(&fx).is_empty(), "{:?}", drive::calls(&fx));
    assert!(!tree.shows("Title:"), "no prompt row");
}

#[test]
fn open_and_close_in_the_menu_run_session_open_and_at_once_session_close() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(
        &fx,
        r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#,
    );
    let dialogs = drive::dialogs(&fx);
    dialogs.confirm();
    // 15 rows: the note `running: …` takes a row of the tree's after the first action, and the fork stays in view
    let mut tree = Tree::start(&fx, 60, 15);
    ready(&tree);
    tree.right_click(8, 6); // the fork `ship review`
    tree.until("the menu", |s| menu_open(s, "Open"));
    tree.keys(&["Enter"]);
    assert_eq!(
        drive::calls(&fx),
        [words(&["session", "open", "f-blocked"])]
    );
    tree.right_click(8, 6);
    tree.until("the menu", |s| menu_open(s, "Open"));
    tree.keys(&["Down", "Down", "Down", "Down", "Enter"]); // Close
    assert!(
        dialogs.popups().is_empty(),
        "Close runs at once: {:?}",
        dialogs.popups()
    );
    assert_eq!(
        drive::calls(&fx)[1],
        words(&["session", "close", "f-blocked"])
    );
}

#[test]
fn rename_on_an_archived_plan_runs_plan_rename() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    archive(&fx.env);
    drive::cpd_says(&fx, r#"{"ok":true,"plan":"old","title":"Older"}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.type_in("Plan older");
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.until("the archived Plan", |s| {
        s.iter().any(|r| r.contains(&format!("├─{STOPPED} old")))
    });
    tree.right_click(10, 3);
    tree.until("the menu", |s| menu_open(s, "Rename…"));
    tree.keys(&["Enter"]);
    assert_eq!(
        dialogs.popups(),
        [
            "cpd tui view dialog input --title='Rename' --prompt='Title:' --initial='Plan old' --validate='title'"
        ]
    );
    assert_eq!(
        drive::calls(&fx),
        [words(&[
            "plan",
            "rename",
            "--plan=old",
            "--title=Plan older"
        ])]
    );
}

#[test]
fn rename_on_a_plan_row_starts_from_its_title_and_runs_plan_rename() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true,"plan":"ship","title":"Plan ship!"}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.type_in("Plan ship!");
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.right_click(8, 7); // the Plan `ship`
    tree.until("the menu", |s| menu_open(s, "New session…"));
    tree.keys(&["Down", "Enter"]); // Rename…
    assert_eq!(
        dialogs.popups(),
        [
            "cpd tui view dialog input --title='Rename' --prompt='Title:' --initial='Plan ship' --validate='title'"
        ],
        "from the title"
    );
    tree.until("the menu closed", the_menu_is_closed);
    assert_eq!(
        drive::calls(&fx),
        [words(&[
            "plan",
            "rename",
            "--plan=ship",
            "--title=Plan ship!"
        ])]
    );
    tree.until_logged("ok: renamed");
}

#[test]
fn a_title_no_id_can_be_made_of_is_refused_in_a_text_dialog_and_runs_nothing() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.fill("!!", "acme");
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.right_click(5, 14); // the empty area, above the Inbox's margin row (row 15 is its upper margin, the widget's)
    tree.until("the menu", |s| menu_open(s, "New Context…"));
    tree.keys(&["Enter"]);
    let popups = dialogs.popups();
    assert_eq!(popups.len(), 2, "{popups:?}");
    assert!(
        popups[1].starts_with("cpd tui view dialog text --title='New Context' "),
        "the refusal is a `text` dialog: {popups:?}"
    );
    assert!(
        popups[1].contains("--body='A title needs at least two letters or digits"),
        "{popups:?}"
    );
    dialogs.settled(&mut tree);
    assert!(drive::calls(&fx).is_empty(), "{:?}", drive::calls(&fx));
    assert!(!tree.shows("A title needs"), "no note: {:?}", tree.screen());
}

#[test]
fn new_plan_and_fork_run_plan_new_without_a_repo_and_session_fork() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(
        &fx,
        r#"{"ok":true,"lead":{"session":"l-new"},"session":{"id":"l-new"}}"#,
    );
    let dialogs = drive::dialogs(&fx);
    dialogs.type_in("Ship");
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    tree.right_click(5, 5); // the Context `alpha`
    tree.until("the menu", |s| menu_open(s, "New Plan…"));
    tree.keys(&["Enter"]);
    assert_eq!(
        dialogs.popups(),
        [
            "cpd tui view dialog input --title='New Plan' --prompt='Title:' --initial='' --validate='title'"
        ]
    );
    // `ship` is taken in the tree; no repo (R34 A12)
    assert_eq!(
        drive::calls(&fx),
        [
            words(&[
                "plan",
                "new",
                "ship-2",
                "--context=alpha",
                "--title=Ship",
                "--by-user"
            ]),
            words(&["session", "open", "l-new"])
        ]
    );
    dialogs.type_in("ship fork (fork)");
    tree.right_click(8, 9); // the fork `ship fork`
    tree.until("the menu", |s| menu_open(s, "Open"));
    tree.keys(&["Down", "Down", "Enter"]); // Fork…
    assert_eq!(
        dialogs.popups()[1],
        "cpd tui view dialog input --title='Fork' --prompt='Name:' --initial='ship fork (fork)' --validate='name'"
    );
    let calls = drive::calls(&fx);
    assert_eq!(calls.len(), 4, "{calls:?}");
    assert_eq!(
        calls[2],
        words(&["session", "fork", "f-idle", "--name=ship fork (fork)"])
    );
    assert_eq!(
        calls[3],
        words(&["session", "open", "l-new"]),
        "the new fork is opened"
    );
}

/// hs-d688: a live Context with no row ends with the clickable ` ╰󰬪 new`; a click on it opens the same New Plan input
/// popup the context menu's New Plan… does, and the answer runs `plan new` for that Context and opens the new Lead.
/// Nothing runs before the popup is answered. `gamma`'s row is found by its text, not a fixed index.
#[test]
fn a_click_on_an_empty_contexts_new_row_opens_the_new_plan_popup_and_runs_plan_new() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(
        &fx,
        r#"{"ok":true,"plan":{"id":"fresh"},"lead":{"session":"l-new"}}"#,
    );
    let dialogs = drive::dialogs(&fx);
    dialogs.cancel();
    let mut tree = Tree::start(&fx, 60, 22);
    let screen = ready(&tree);
    // gamma, the live Context with no row, ends with ` ╰󰬪 new`
    let found = screen.iter().position(|r| r.contains("\u{F0B2A} new"));
    assert!(
        found.is_some(),
        "gamma's new row is on the screen:\n{}",
        screen.join("\n")
    );
    let row = found.expect("just asserted") as u16;
    let column = screen[row as usize]
        .find("new")
        .expect("the word `new` on the row") as u16;
    tree.click(column, row);
    // the click opens the popup and runs nothing until it is answered (the fake cancels it)
    assert_eq!(
        dialogs.popups(),
        [
            "cpd tui view dialog input --title='New Plan' --prompt='Title:' --initial='' --validate='title'"
        ],
        "the context menu's New Plan… popup"
    );
    assert!(
        drive::calls(&fx).is_empty(),
        "nothing runs before the popup is answered: {:?}",
        drive::calls(&fx)
    );
    // answering it runs `plan new` for `gamma` and opens the new Lead
    dialogs.type_in("Fresh");
    tree.click(column, row);
    tree.until_that("the new Plan to run", |_| drive::calls(&fx).len() >= 2);
    assert_eq!(
        drive::calls(&fx),
        [
            words(&[
                "plan",
                "new",
                "fresh",
                "--context=gamma",
                "--title=Fresh",
                "--by-user"
            ]),
            words(&["session", "open", "l-new"])
        ],
        "the new Plan's id derived from the title, in `gamma`"
    );
}

#[test]
fn a_refused_open_of_the_new_lead_shows_its_message_and_the_plan_stays_created() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    let session =
        |call: &cpd_core::host::runner::Call| call.args.first().is_some_and(|a| a == "session");
    let _ = (*fx.fakes.runner).clone().on(
        "cpd",
        session,
        Reply::exit(3, r#"{"ok":false,"error":{"code":"no_cockpit","message":"There is no cockpit.","details":{}}}"#, ""),
    );
    drive::cpd_says(&fx, r#"{"ok":true,"lead":{"session":"l-new"}}"#);
    let dialogs = drive::dialogs(&fx);
    dialogs.type_in("Pay");
    let mut tree = Tree::start(&fx, 60, 14);
    ready(&tree);
    tree.right_click(5, 5); // the Context `alpha`
    tree.until("the menu", |s| menu_open(s, "New Plan…"));
    tree.keys(&["Enter"]);
    assert_eq!(
        dialogs.popups().len(),
        2,
        "the input and the refusal: {:?}",
        dialogs.popups()
    );
    tree.until_logged("failed: session open l-new: no_cockpit: There is no cockpit.");
    assert_eq!(
        drive::calls(&fx),
        [
            words(&[
                "plan",
                "new",
                "pay",
                "--context=alpha",
                "--title=Pay",
                "--by-user"
            ]),
            words(&["session", "open", "l-new"])
        ]
    );
}

#[test]
fn copy_id_puts_the_shown_id_in_tmuxs_buffer_and_says_so() {
    let fx = fx();
    let _engine = engine(&fx);
    populate_in(&fx);
    drive::cpd_says(&fx, r#"{"ok":true}"#);
    // a fake tmux server with a client: it has the buffer `set-buffer` fills
    let _dialogs = drive::dialogs(&fx);
    let mut tree = Tree::start(&fx, 60, 22);
    ready(&tree);
    for (column, row, id) in [(5, 5, "alpha"), (8, 7, "ship"), (8, 9, "ship·f-id")] {
        tree.right_click(column, row);
        tree.until("the menu", |s| {
            menu_open(
                s,
                if id == "alpha" {
                    "New Plan…"
                } else if id == "ship" {
                    "New session…"
                } else {
                    "Open"
                },
            )
        });
        let downs = if id == "ship·f-id" { 3 } else { 2 };
        tree.keys(&[&["Down"; 3][..downs], &["Enter"]].concat());
        tree.until_logged(&format!("ok: copied {id}\n"));
        let buffer = fx.tmux(&["show-buffer"]);
        assert_eq!(String::from_utf8_lossy(&buffer.stdout), id);
    }
    assert!(
        drive::calls(&fx).is_empty(),
        "Copy id runs no cpd command: {:?}",
        drive::calls(&fx)
    );
}

// ---------- the folds survive a restart of the view (hs-d260) ----------

fn folds_file(env: &TestEnv) -> std::path::PathBuf {
    env.cpd_home()
        .join("run")
        .join("tui")
        .join("tree-folds.json")
}

/// The tree's rows above the Inbox section (whose own fold row is a `╰󰬦 expand` too).
fn above_inbox(screen: &[String]) -> &[String] {
    &screen[..screen
        .iter()
        .position(|r| r.contains("Inbox"))
        .unwrap_or(screen.len())]
}

/// `populate`, with `alpha`'s fork `f-blocked` archived (alpha has an archived row, so its closing row is a toggle) and
/// the Context `beta` archived with its Plans (its rows are all archived: it starts folded, with a toggle). `gamma` has
/// no row to fold and stays plain.
fn archive_for_folds(env: &TestEnv) {
    sql(
        &mut open(env),
        "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';
         UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'beta';",
    );
}

/// `alpha`, the live Context with an archived row, is expanded: its block shows the archived row and ends in `╰󰬬 collapse`.
fn alpha_is_expanded(screen: &[String]) -> bool {
    let tree = above_inbox(screen);
    tree.iter().any(|r| r.trim() == format!("╰{UP} collapse"))
        && tree.iter().any(|r| r.contains("ship·f-bl"))
}

/// Naps `tree` until `tree-folds.json` holds `want` (it is written at most once a second).
fn wait_for_folds(tree: &mut Tree, env: &TestEnv, want: &str) {
    tree.until_that(&format!("the folds file to hold {want}"), |_| {
        std::fs::read_to_string(folds_file(env)).is_ok_and(|text| text == want)
    });
}

// ---------- the Inbox section (commands.md §13.2) ----------

// ---------- the Inbox's menu and keys (commands.md §13.2, §21) ----------
