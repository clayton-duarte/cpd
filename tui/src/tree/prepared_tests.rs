//! The tree view over a prepared store (`read::tree`, `View::draw_at`) on a `TestBackend`, no terminal and no process:
//! the rows of Contexts, Plans and Sessions, the blank margins, the colours of the
//! selected row. The view's loop (follow, keys of a
//! pane, commands) is the view's own and is not run here; keys and clicks reach `View::mouse` as the loop gives them.

use std::fs::File;

use nix::fcntl::{Flock, FlockArg};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;

use super::View;
use crate::palette::numbered::{C10, C15};
use crate::palette::{self, IDLE, NEEDS_YOU, SPINNER, STOPPED};
use crate::read::{self, Tree};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;

const WORKING: &str = SPINNER[0];

// The tree's glyphs, as the unit tests of this directory have them.
const ACTIVE_MARK: &str = "\u{F040A}"; // md-play
/// The closing row's arrows (md-arrow_down_drop_circle_outline, md-arrow_up_drop_circle_outline).
const UP: &str = "\u{F0B2C}";

// ---------- the prepared store ----------

/// When `populate`'s Inbox items were opened (they colour the dots; no row shows them).
const OPENED: &str = "2026-01-09T23:54:30.000Z";

/// A store in a temporary home, with an engine running unless built without one (`engine-down` is no row of the Inbox):
/// the engine's lock is held by the env and let go when it drops.
pub(super) struct Env {
    store: Store,
    _engine: Option<Flock<File>>,
    _dir: tempfile::TempDir,
}

impl Env {
    pub(super) fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let store = Store::open(&home, 5000).unwrap();
        let path = home.watch_lock();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let lock = Flock::lock(File::create(path).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
        Env { store, _engine: Some(lock), _dir: dir }
    }

    pub(super) fn store(&mut self) -> &mut Store {
        &mut self.store
    }

    pub(super) fn sql(&mut self, text: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(text)?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
    }

    /// The tree as the view reads it.
    pub(super) fn tree(&mut self) -> Tree {
        read::tree(&mut self.store).unwrap()
    }

    /// A view of the store's tree, not focused: as the loop starts it.
    pub(super) fn view(&mut self) -> View {
        View::new(self.tree(), false)
    }
}

/// What the view draws in a `width` x `height` pane, at the first frame of the spinner.
pub(super) fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    terminal.backend().buffer().clone()
}

/// The rows of `buf` as text, each without trailing blanks: a link's cells carry their OSC 8 escapes (`crate::link`), which are
/// not text.
pub(super) fn text_rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            let (mut row, mut x) = (String::new(), 0);
            while x < buf.area.width {
                let symbol = buf[(x, y)].symbol();
                // a piece of a link is one cell holding its text; the backend never writes the cell(s) after it
                let text = symbol.strip_prefix("\x1b]8;;").and_then(|s| s.split_once('\x07')).map(|(_, t)| t.trim_end_matches("\x1b]8;;\x07"));
                row += text.unwrap_or(symbol);
                x += text.map_or(1, |t| t.chars().count() as u16);
            }
            row.trim_end().to_owned()
        })
        .collect()
}

/// The screen: its rows without trailing blanks, without the empty rows at the end.
pub(super) fn screen(buf: &Buffer) -> Vec<String> {
    let mut rows = text_rows(buf);
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

fn click(view: &mut View, column: u16, row: u16) {
    view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE });
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
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let (row, number, url, opened, pull_request) = if pr {
        let state = if phase == "done" { "merged" } else { "open" };
        let merged = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
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
        (String::new(), "NULL", "NULL".to_owned(), "NULL", "NULL".to_owned())
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
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo/{id}', 'main', '{status}', 'cpd/{id}', {worktree}, {number}, {url}, {opened},
                 {pull_request}, cpd_txn(), '{created}', {locked}, {done});"
    )
}

fn session_row(id: &str, name: &str, context: &str, plan: &str, state: &str, label: Option<&str>, created: &str) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'lead', '{context}', {plan}, NULL, '{name}', '/sessions/{id}.jsonl', '/repo', 'O3',
                 NULL, '{state}', {label}, '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

/// A Context's Manager Session (contract §3.1): idle, never a row of the tree.
fn manager_row(id: &str, context: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'manager', '{context}', NULL, NULL, '{context}-manager', '/sessions/{id}.jsonl', '/repo', 'O5',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:07.000Z');"
    )
}

/// What the engine's E8 would open for `populate`'s store (the tree reads the Inbox, it does not compute it).
fn condition(kind: Kind, urgency: Urgency, session: &str, plan: Option<&str>, context: &str) -> Condition {
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
        condition(Kind::Dialog, Urgency::Red, "f-blocked", Some("ship"), "alpha"),
        condition(Kind::Dialog, Urgency::Red, "l-rel", Some("rel"), "beta"),
        condition(Kind::Unread, Urgency::White, "m-alpha", None, "alpha"),
        condition(Kind::Dialog, Urgency::Magenta, "m-beta", None, "beta"),
    ]
}

fn job_row(plan: &str, key: &str, status: &str) -> String {
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{}'", "0".repeat(64)) };
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let failed = if status == "failed" { "'crashed'" } else { "NULL" };
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
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
pub(super) fn populate(env: &mut Env) {
    let mut s = String::new();
    s += &context("alpha");
    s += &context("beta");
    s += &context("gamma");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z", false);
    s += &plan_row("sketch", "alpha", "plan", "l-sketch", "2026-01-02T00:00:09.000Z", false);
    s += &plan_row("rel", "beta", "deploy", "l-rel", "2026-01-02T00:00:02.000Z", true);
    s += &plan_row("old", "beta", "done", "l-old", "2026-01-02T00:00:03.000Z", true);
    s += &session_row("g-alpha", "grounded", "alpha", "", "idle", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("l-ship", "ship lead", "alpha", "ship", "working", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("f-idle", "ship fork", "alpha", "ship", "idle", None, "2026-01-02T00:00:02.000Z");
    s += &session_row("f-blocked", "ship review", "alpha", "ship", "blocked", Some("φ: Worker question"), "2026-01-02T00:00:09.000Z");
    s += &session_row("l-sketch", "sketch", "alpha", "sketch", "idle", None, "2026-01-02T00:00:01.000Z");
    s += &session_row("l-rel", "rel lead", "beta", "rel", "blocked", Some("cpd: PR feedback"), "2026-01-02T00:00:02.000Z");
    s += &session_row("l-old", "old lead", "beta", "old", "stopped", None, "2026-01-02T00:00:03.000Z");
    for (key, status) in [("a", "merged"), ("b", "merged"), ("c", "running"), ("d", "queued"), ("e", "failed")] {
        s += &job_row("ship", key, status);
    }
    for key in ["s1", "s2"] {
        s += &job_row("sketch", key, "draft");
    }
    s += &job_row("rel", "r1", "merged");
    s += &moved("alpha", "ship", "l-ship", "chat");
    s += &manager_row("m-alpha", "alpha");
    s += &manager_row("m-beta", "beta");
    env.sql(&s);
    reconcile(&mut env.store, &items(), OPENED).unwrap();
}


// ---------- tests ----------

/// The tree taller than its share of the column (hs-d307): forty Plans more under `gamma`, in a pane of 20
/// rows with the Inbox's items. The tree keeps the top of the column and scrolls inside it, with the wheel, between a
/// `▲` and a `▼` cue; the Inbox stays below it (the bug: the tree did not scroll, its rows past the column were lost).
#[test]
fn a_tree_taller_than_its_share_scrolls_with_the_wheel_between_its_cues() {
    let mut env = Env::new();
    populate(&mut env);
    let mut s = String::new();
    for i in 0..40 {
        let (plan, lead) = (format!("p{i:02}"), format!("l-p{i:02}"));
        let created = format!("2026-01-03T00:00:{i:02}.000Z");
        s += &plan_row(&plan, "gamma", "plan", &lead, &created, false);
        s += &session_row(&lead, &plan, "gamma", &plan, "idle", None, &created);
    }
    env.sql(&s);
    let mut view = env.view();
    let cue = |r: &String, glyph: char| r.trim_start().starts_with(glyph) && r.ends_with(" more");
    let screen = screen(&draw(&mut view, 46, 20));
    assert!(screen.last().is_some_and(|r| cue(r, '▼')), "the tree's ▼ cue on the column's last row: {screen:?}");
    assert!(!screen.iter().any(|r| cue(r, '▲')), "nothing is hidden above yet: {screen:?}");
    view.mouse(MouseEvent { kind: MouseEventKind::ScrollDown, column: 10, row: 3, modifiers: KeyModifiers::NONE });
    let screen = self::screen(&draw(&mut view, 46, 20));
    assert!(screen.get(1).is_some_and(|r| cue(r, '▲')), "the tree's ▲ cue after a notch: {screen:?}");
    assert!(!screen[1..].iter().any(|r| r.contains(" beta")), "the first Context scrolled away: {screen:?}");
}

#[test]
fn the_tree_of_a_prepared_store() {
    let mut env = Env::new();
    populate(&mut env);
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 42 - right.chars().count());
    let expected = vec![
        String::new(),
        format!(" ╭{NEEDS_YOU} beta"),
        row(&format!("├─{NEEDS_YOU} rel"), "1"),
        format!(" ╰─{STOPPED} old"),
        String::new(),
        format!(" ╭{NEEDS_YOU} alpha"),
        format!(" ├─{NEEDS_YOU} ship·f-bl"),
        row(&format!("├─{WORKING} ship"), "1⋅1⋅1⋅2"),
        row(&format!("├─{IDLE} sketch"), "2"),
        format!(" ╰─{IDLE} ship·f-id"),
        String::new(),
        format!(" ╭{STOPPED} gamma"),
        " ╰\u{F0B2A} new".to_owned(),
    ]; // each Context is a fold: its rows behind `├`, the last one behind `╰─` (nothing archived, so no closing row; a live Context with no row ends with the clickable `╰󰬪 new`), one empty row between two Contexts; the Inbox section is pinned below
    let screen = screen(&draw(&mut env.view(), 46, 20));
    assert_eq!(
        screen,
        expected,
        "the Contexts by dot tier then creation, newest first (hs-d686, pro-d48: beta's magenta Manager, then alpha and gamma tied on shiro and creation, in Context id order); alpha's live Plans and forks as one list by tier then creation, newest first (the red fork `ship·f-bl`, then shiro: `ship` created last, `sketch`, then the fork `ship·f-id` created first); the last row of each Context `╰─` (nothing archived) and the terminator `╰󰬪` of the Context with no row; the Lead with no Plan, g-alpha, is no row; the Jobs: queued, running, merging, failed, merged; no state text; the Inbox section at the bottom: an empty row, its pill title, three items with their icon and age, `╰󰬦 expand`"
    );
}

/// The tree orders by dot-urgency tier first and creation, newest first, second (hs-d686, pro-d48): a magenta Context still
/// leads a shiro one, whatever its age; within one tier the newest-created row comes first (`contexts.created_at` for a Context,
/// `plans.created_at` for a Plan row, the Session's `created_at` for a fork), whichever row an Event names last: Events never
/// move a row. Every seed gives the newest-created row the oldest Event.
#[test]
fn the_tree_orders_live_rows_by_dot_tier_then_newest_first() {
    let mut env = Env::new();
    populate(&mut env);
    // Events the opposite way to creation: the older-created `alpha`, `ship` get the newest Events
    env.sql(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-09T00:00:00.000Z', 'plan-phase', 'alpha', 'ship', NULL, 'fixture', '{}');
         UPDATE contexts SET created_at = '2026-01-05T00:00:00.000Z' WHERE id = 'gamma';
         UPDATE plans SET created_at = '2026-01-03T00:00:00.000Z' WHERE id = 'ship';
         UPDATE plans SET created_at = '2026-01-04T00:00:00.000Z' WHERE id = 'sketch';
         UPDATE sessions SET created_at = '2026-01-05T00:00:00.000Z' WHERE id = 'f-idle';",
    );
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 42 - right.chars().count());
    let beta = || {
        vec![
            String::new(),
            format!(" ╭{NEEDS_YOU} beta"),
            row(&format!("├─{NEEDS_YOU} rel"), "1"),
            format!(" ╰─{STOPPED} old"),
        ]
    };
    let gamma = || vec![String::new(), format!(" ╭{STOPPED} gamma"), " ╰\u{F0B2A} new".to_owned()];
    // within shiro: the fork (newest), then `sketch`, then `ship`: one list of Plans and forks
    let alpha = || {
        vec![
            String::new(),
            format!(" ╭{NEEDS_YOU} alpha"),
            format!(" ├─{NEEDS_YOU} ship·f-bl"),
            format!(" ├─{IDLE} ship·f-id"),
            row(&format!("├─{IDLE} sketch"), "2"),
            row(&format!("╰─{WORKING} ship"), "1⋅1⋅1⋅2"),
        ]
    };
    let expected = [beta(), gamma(), alpha()].concat();
    let screen = screen(&draw(&mut env.view(), 46, 20));
    assert_eq!(
        screen,
        expected,
        "beta's magenta Context stays first; within shiro `gamma`, created later, is above `alpha` though alpha has the newer Event; within alpha the red fork `ship·f-bl` is first, then the fork `ship·f-id`, `sketch` and `ship` by creation, newest first, as one list (hs-d686, pro-d48)"
    );
    // the reverse creation puts `alpha` above `gamma`: the order follows creation, not the Context id
    env.sql("UPDATE contexts SET created_at = '2026-01-06T00:00:00.000Z' WHERE id = 'alpha';");
    let expected = [beta(), alpha(), gamma()].concat();
    let screen = self::screen(&draw(&mut env.view(), 46, 20));
    assert_eq!(screen, expected, "`alpha`, created after `gamma`, is above it (hs-d686, pro-d48)");
}

/// An archived Plan and an archived fork stay after every live row, whatever their dot (hs-d686): `sketch` and the fork
/// `ship·f-id`, both shiro, follow the live red fork `ship·f-bl` and the live `ship`, themselves by creation, newest first (hs-d686, pro-d48).
#[test]
fn an_archived_row_is_last_after_the_live_rows() {
    let mut env = Env::new();
    populate(&mut env);
    env.sql(
        "UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'sketch';
         UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-idle';",
    );
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 42 - right.chars().count());
    let expected = vec![
        String::new(),
        format!(" ╭{NEEDS_YOU} beta"),
        row(&format!("├─{NEEDS_YOU} rel"), "1"),
        format!(" ╰─{STOPPED} old"),
        String::new(),
        format!(" ╭{NEEDS_YOU} alpha"),
        format!(" ├─{NEEDS_YOU} ship·f-bl"),
        row(&format!("├─{WORKING} ship"), "1⋅1⋅1⋅2"),
        row(&format!("├─{IDLE} sketch"), "2"),
        format!(" ├─{IDLE} ship·f-id"),
        format!(" ╰{UP} collapse"),
        String::new(),
        format!(" ╭{STOPPED} gamma"),
        " ╰\u{F0B2A} new".to_owned(),
    ];
    let screen = screen(&draw(&mut env.view(), 46, 21));
    assert_eq!(
        screen,
        expected,
        "the archived Plan `sketch` and the archived fork `ship·f-id` are after the live rows though shiro; the Context's toggle is expanded, so it closes with `╰󰬬 collapse`"
    );
}

#[test]
fn the_tree_leaves_the_first_and_the_last_column_blank() {
    let mut env = Env::new();
    populate(&mut env);
    let screen = screen(&draw(&mut env.view(), 46, 19));
    assert!(screen.last().is_some_and(|r| r.contains("new")), "the whole tree: {screen:?}");
    // `screen` is trimmed at the right, so the last column is blank when no row is longer than 45 columns
    assert!(screen.iter().all(|r| r.is_empty() || r.starts_with(' ')), "column 0: {screen:?}");
    assert!(screen.iter().all(|r| r.chars().count() <= 45), "the last column: {screen:?}");
    assert!(screen.iter().any(|r| r.chars().count() == 43), "the counters end at width − 4: {screen:?}");
}

#[test]
fn an_empty_store_says_so() {
    let mut env = Env::new();
    assert_eq!(screen(&draw(&mut env.view(), 40, 5)), ["", "  no Contexts"]);
}

// ---------- blue: the selected row's name (hs-d351; commands.md §13.2 "Rows", §17 "Blue") ----------

/// Blue (colour 4), as `tree/rows.rs`' tests have it: parsed, so that no colour is named outside the palette
/// (`no_colour_outside_the_palette`).
fn blue() -> Color {
    "blue".parse().unwrap()
}

/// What `buf` draws in blue: per row, the glyphs whose foreground is blue.
fn blue_text(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .filter(|&x| buf[(x, y)].fg == blue())
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

/// The foreground of the first cell of `glyph` in the screen row that says `row`.
fn fg_of(buf: &Buffer, row: &str, glyph: &str) -> Color {
    let y = text_rows(buf).iter().position(|r| r.contains(row)).unwrap_or_else(|| panic!("no row says {row:?}")) as u16;
    let x = (0..buf.area.width).find(|&x| !buf[(x, y)].symbol().is_empty() && glyph.starts_with(buf[(x, y)].symbol())).expect("the glyph on the row");
    buf[(x, y)].fg
}

#[test]
fn the_selected_plan_rows_name_is_blue_and_no_chat_mark_is_drawn() {
    let mut env = Env::new();
    populate(&mut env); // `l-ship`, the Lead of `ship`, is in the chat slot
    let buf = draw(&mut env.view(), 46, 19);
    let rows = text_rows(&buf);
    assert!(!rows.iter().any(|r| r.contains(ACTIVE_MARK)), "no marker in column 0: {rows:?}");
    assert_eq!(blue_text(&buf), ["ship"], "the one blue thing is the name of the Plan whose Lead is in the chat slot: {rows:?}");
    assert_eq!(fg_of(&buf, " sketch", "sketch"), C15, "an unselected row's name is clickable text, colour 15");
    assert_eq!(fg_of(&buf, " ship ", "1"), C15, "the row keeps its counters' colours (the queued Job: 15)");
    assert_eq!(fg_of(&buf, " ship ", "2"), C10, "(the merged Jobs: bright green)");
}

#[test]
fn the_selected_context_rows_name_is_blue_when_its_manager_is_in_the_chat_slot() {
    let mut env = Env::new();
    populate(&mut env);
    // the chat slot moves from `l-ship` to the Manager of `alpha`
    env.sql(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-04T00:00:00.000Z', 'session-moved', 'alpha', 'ship', 'l-ship', 'fixture', '{\"place\":\"parked\"}');
         INSERT INTO events (txn, at, kind, context, session, text, data)
         VALUES (cpd_txn(), '2026-01-04T00:00:01.000Z', 'session-moved', 'alpha', 'm-alpha', 'fixture', '{\"pane\":\"%2\",\"window\":\"@1\",\"place\":\"chat\"}');",
    );
    let buf = draw(&mut env.view(), 46, 19);
    let rows = text_rows(&buf);
    assert!(!rows.iter().any(|r| r.contains(ACTIVE_MARK)), "no marker: {rows:?}");
    assert_eq!(blue_text(&buf), ["alpha"], "the Context's name, and no Plan's: {rows:?}");
    assert_ne!(fg_of(&buf, "╭", NEEDS_YOU), blue(), "its dot keeps the dot's colour");
}

// ---------- the empty Context's clickable ` ╰󰬪 new` (hs-d688) ----------

/// A live Context with no Plan or fork row ends with the clickable ` ╰\u{F0B2A} new` (hs-d688): the corner and the glyph
/// muted (colour 8), the word `new` in the clickable colour 15. An ARCHIVED empty Context keeps the plain muted
/// ` ╰\u{F0B2A}`, since `plan new` is refused there. The row is found by its text: the order PR may have moved `gamma`.
#[test]
fn an_empty_live_context_draws_the_clickable_new_row() {
    let mut env = Env::new();
    populate(&mut env);
    let muted = palette::muted_text().fg.unwrap();
    let buf = draw(&mut env.view(), 46, 20);
    let rows = text_rows(&buf);
    let live = rows.iter().find(|r| r.contains('\u{F0B2A}'));
    assert_eq!(live.map(String::as_str), Some(" ╰\u{F0B2A} new"), "a live empty Context ends with the clickable new row:\n{}", rows.join("\n"));
    assert_eq!(fg_of(&buf, "\u{F0B2A}", "╰"), muted, "the corner is muted (colour 8)");
    assert_eq!(fg_of(&buf, "\u{F0B2A}", "\u{F0B2A}"), muted, "the glyph is muted (colour 8)");
    assert_eq!(fg_of(&buf, "\u{F0B2A}", "n"), C15, "the word `new` is clickable (colour 15)");
    // `plan new` is refused in an archived Context, so its empty block keeps the plain terminator
    env.sql("UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'gamma';");
    let mut view = env.view();
    let closed = draw(&mut view, 46, 30);
    let archive = text_rows(&closed).iter().position(|r| r.contains("archive")).expect("the archive row") as u16;
    click(&mut view, 3, archive);
    let buf = draw(&mut view, 46, 30);
    let rows = text_rows(&buf);
    let archived = rows.iter().find(|r| r.contains('\u{F0B2A}'));
    assert_eq!(archived.map(String::as_str), Some(" ╰\u{F0B2A}"), "an archived empty Context keeps the plain terminator:\n{}", rows.join("\n"));
    assert_eq!(fg_of(&buf, "\u{F0B2A}", "\u{F0B2A}"), muted, "its glyph stays muted (colour 8)");
}
