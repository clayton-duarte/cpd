//! The tree view over a prepared store (`read::tree`, `View::draw_at`) on a `TestBackend`, no terminal and no process:
//! the rows of Contexts, Plans and Sessions, the Inbox section pinned below them, the blank margins, the colours of the
//! selected row. The Inbox items' ages are taken at [`NOW`], so no test reads the wall clock. The view's loop (follow, keys of a
//! pane, commands) is the view's own and is not run here; keys and clicks reach `View::mouse` as the loop gives them.

use std::fs::File;

use nix::fcntl::{Flock, FlockArg};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;
use time::OffsetDateTime;

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
const DOWN: &str = "\u{F0B26}";
const UP: &str = "\u{F0B2C}";

/// The closing row of a block that folds, folded, after the pane's margin.
fn expand() -> String {
    format!(" ╰{DOWN} expand")
}

/// The Inbox's pill title for `count` items in `inner` columns (the pane less its margins), after the margin: the collapsible form
/// (hs-d478), the count a pill too (hs-d480).
fn pill(inner: usize, count: usize) -> String {
    let (left, right) = ("\u{E0B6}", "\u{E0B4}");
    let count = count.to_string();
    let used = 1 + 2 + 2 + "Inbox".len() + 1 + 2 + count.len();
    format!(" ╭─{left}Inbox{right}{}{left}{count}{right}", "─".repeat(inner + 2 - used))
}

/// An Inbox item row in `inner` columns: the muted `│`, one space, the tier `glyph`, the label and the age (`5m`) at the last column.
fn item_row_of(inner: usize, glyph: &str, label: &str) -> String {
    format!(" │ {glyph} {label}{}5m", " ".repeat(inner - 2 - 4 - label.len() - 2))
}

// ---------- the prepared store ----------

/// The clock of the drawing: 2026-01-10T00:00:00Z.
pub(super) fn now() -> OffsetDateTime {
    OffsetDateTime::parse(NOW, &time::format_description::well_known::Rfc3339).unwrap()
}

const NOW: &str = "2026-01-10T00:00:00.000Z";

/// When `populate`'s Inbox items were opened: five minutes and a half before [`NOW`], so that their age reads `5m`.
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

/// What the view draws in a `width` x `height` pane, at the first frame of the spinner and at [`NOW`].
pub(super) fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw_at(f, 0, now())).unwrap();
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
/// The Inbox ([`items`]) gives the dots their filled dot: `f-blocked` and `l-rel` have a red dialog; the Contexts alpha and beta
/// have a Manager with an item, which alone fills a Context row's filled dot.
pub(super) fn populate(env: &mut Env) {
    let mut s = String::new();
    s += &context("alpha");
    s += &context("beta");
    s += &context("gamma");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:00.000Z", false);
    s += &plan_row("sketch", "alpha", "plan", "l-sketch", "2026-01-02T00:00:01.000Z", false);
    s += &plan_row("rel", "beta", "deploy", "l-rel", "2026-01-02T00:00:02.000Z", true);
    s += &plan_row("old", "beta", "done", "l-old", "2026-01-02T00:00:03.000Z", true);
    s += &session_row("g-alpha", "grounded", "alpha", "", "idle", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("l-ship", "ship lead", "alpha", "ship", "working", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("f-idle", "ship fork", "alpha", "ship", "idle", None, "2026-01-02T00:00:08.000Z");
    s += &session_row("f-blocked", "ship review", "alpha", "ship", "blocked", Some("cpd: Worker question"), "2026-01-02T00:00:09.000Z");
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

/// The Inbox section of `populate`'s four items in a pane `width` wide: the empty row, the pill title, three rows and the
/// fold widget's `╰󰬦 expand`.
fn inbox_section(width: usize) -> Vec<String> {
    let inner = width - 2;
    vec![
        String::new(),
        pill(inner, 4),
        item_row_of(inner, palette::TIER_RED, "f-blocked label"),
        item_row_of(inner, palette::TIER_RED, "l-rel label"),
        item_row_of(inner, palette::TIER_MAGENTA, "m-beta label"),
        expand(),
    ]
}

/// `populate`'s items and one of each kind that jumps or links; the old engine-error row is caused by Event `event`.
fn jumping_items(event: i64) -> Vec<Condition> {
    let other = |kind, urgency, subject: &str, label: &str| Condition {
        subject: subject.to_owned(),
        label: label.to_owned(),
        session: None,
        ..condition(kind, urgency, "none", Some("ship"), "alpha")
    };
    let mut items = items();
    items.extend([
        Condition { event: Some(event), plan: None, context: None, ..other(Kind::EngineError, Urgency::Red, "engine:store", "engine: store busy") },
        Condition {
            url: Some("https://github.com/acme/api/pull/12".into()),
            ..other(Kind::PrFailing, Urgency::Yellow, "pr:acme/api#12", "acme/api#12 failing")
        },
        other(Kind::JobStuck, Urgency::Yellow, "job:ship/c", "ship: c is stuck"),
        Condition { url: Some("http://10.0.0.5:20000/".into()), ..other(Kind::PlanReview, Urgency::Magenta, "review:ship/r1", "ship plan review") },
        Condition { url: Some("https://x.io/ready".into()), ..other(Kind::PrReady, Urgency::White, "pr:acme/api#3", "acme/api#3 ready") },
        Condition {
            url: Some("https://x.io/changes".into()),
            plan: None,
            ..other(Kind::PrChanges, Urgency::Yellow, "pr:acme/web#4", "acme/web#4 changes requested")
        },
        Condition {
            url: Some("https://x.io/review".into()),
            plan: None,
            context: None,
            ..other(Kind::ReviewRequested, Urgency::Yellow, "pr:acme/web#5", "acme/web#5 review requested")
        },
        Condition { plan: None, ..other(Kind::Question, Urgency::Magenta, "question:q1", "alpha: a question") },
    ]);
    items
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
    let inbox = screen.iter().position(|r| r.contains("Inbox"));
    assert!(
        inbox.is_some_and(|i| i > 2 && screen[i - 1].trim().is_empty() && cue(&screen[i - 2], '▼')),
        "the tree's ▼ cue above the Inbox: {screen:?}"
    );
    assert!(!screen.iter().any(|r| cue(r, '▲')), "nothing is hidden above yet: {screen:?}");
    view.mouse(MouseEvent { kind: MouseEventKind::ScrollDown, column: 10, row: 3, modifiers: KeyModifiers::NONE });
    let screen = self::screen(&draw(&mut view, 46, 20));
    assert!(screen.get(1).is_some_and(|r| cue(r, '▲')), "the tree's ▲ cue after a notch: {screen:?}");
    assert!(!screen[1..].iter().any(|r| r.contains(" beta")), "the first Context scrolled away: {screen:?}");
    assert!(screen.iter().any(|r| r.contains("Inbox")), "the Inbox stays: {screen:?}");
}

#[test]
fn the_tree_of_a_prepared_store() {
    let mut env = Env::new();
    populate(&mut env);
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 42 - right.chars().count());
    let mut expected = vec![
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
        " ╰\u{F0B2A}".to_owned(),
    ]; // each Context is a fold: its rows behind `├`, the last one behind `╰─` (nothing archived, so no closing row; a Context with no row ends with the terminator `╰󰬪`), one empty row between two Contexts; the Inbox section is pinned below
    expected.extend(inbox_section(46));
    let screen = screen(&draw(&mut env.view(), 46, 20));
    assert_eq!(
        screen,
        expected,
        "the Contexts by dot tier then last activity (beta's magenta Manager, then alpha's shiro Manager with an Event, then gamma with no Manager); alpha's live Plans and forks as one list by tier then last activity (the red fork `ship·f-bl`, the red loader of `ship` in shiro, then `sketch` and the fork `ship·f-id` tied on shiro and creation order); the last row of each Context `╰─` (nothing archived) and the terminator `╰󰬪` of the Context with no row; the Lead with no Plan, g-alpha, is no row; the Jobs: queued, running, merging, failed, merged; no state text; the Inbox section at the bottom: an empty row, its pill title, three items with their icon and age, `╰󰬦 expand`"
    );
}

/// The tree reorders by dot-urgency tier first and last activity second (hs-d686): a magenta Context still leads a shiro
/// one, whatever its activity; within one tier the row the newer Event names comes first (`context` for a Context,
/// `plan` for a Plan row).
#[test]
fn the_tree_orders_live_rows_by_dot_tier_then_last_activity() {
    let mut env = Env::new();
    populate(&mut env);
    // a newer Event names the Plan `sketch` (and the Context `alpha`), and a newer one still the Context `gamma`, whose
    // own Event outranks alpha's Manager's.
    env.sql(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-09T00:00:00.000Z', 'plan-phase', 'alpha', 'sketch', NULL, 'fixture', '{}');
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-09T00:00:01.000Z', 'context-created', 'gamma', NULL, NULL, 'fixture', '{}');",
    );
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 42 - right.chars().count());
    let mut expected = vec![
        String::new(),
        format!(" ╭{NEEDS_YOU} beta"),
        row(&format!("├─{NEEDS_YOU} rel"), "1"),
        format!(" ╰─{STOPPED} old"),
        String::new(),
        format!(" ╭{STOPPED} gamma"),
        " ╰\u{F0B2A}".to_owned(),
        String::new(),
        format!(" ╭{NEEDS_YOU} alpha"),
        format!(" ├─{NEEDS_YOU} ship·f-bl"),
        row(&format!("├─{IDLE} sketch"), "2"),
        row(&format!("├─{WORKING} ship"), "1⋅1⋅1⋅2"),
        format!(" ╰─{IDLE} ship·f-id"),
    ];
    expected.extend(inbox_section(46));
    let screen = screen(&draw(&mut env.view(), 46, 20));
    assert_eq!(
        screen,
        expected,
        "beta's magenta Context stays first though `gamma`'s newer Event outranks it; within shiro `gamma`'s Event puts it above `alpha`, and within alpha the newer `sketch` Event puts it above `ship` (the red fork `ship·f-bl` keeps tier 0)"
    );
}

/// An archived Plan and an archived fork stay after every live row, whatever their dot (hs-d686): `sketch` and the fork
/// `ship·f-id`, both shiro, follow the live red fork `ship·f-bl` and the live `ship`.
#[test]
fn an_archived_row_is_last_after_the_live_rows() {
    let mut env = Env::new();
    populate(&mut env);
    env.sql(
        "UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'sketch';
         UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-idle';",
    );
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 42 - right.chars().count());
    let mut expected = vec![
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
        " ╰\u{F0B2A}".to_owned(),
    ];
    expected.extend(inbox_section(46));
    let screen = screen(&draw(&mut env.view(), 46, 21));
    assert_eq!(
        screen,
        expected,
        "the archived Plan `sketch` and the archived fork `ship·f-id` are after the live rows though shiro; the Context's toggle is expanded, so it closes with `╰󰬬 collapse`"
    );
}

#[test]
fn an_inbox_of_two_items_is_the_pill_title_and_two_rows_with_no_toggle_and_the_terminator_close() {
    let mut env = Env::new();
    populate(&mut env);
    reconcile(&mut env.store, &items()[..2], OPENED).unwrap();
    let expected = [
        String::new(),
        pill(44, 2),
        item_row_of(44, palette::TIER_RED, "f-blocked label"),
        item_row_of(44, palette::TIER_RED, "l-rel label"),
        " ╰\u{F0B2A}".to_owned(),
    ];
    // the screen has no empty row at its end: the section's own empty row below is not in it
    let screen = screen(&draw(&mut env.view(), 46, 18));
    assert!(screen.len() == 17 && screen[12..] == expected, "the two items: {screen:?}");
    assert!(screen.iter().all(|r| !r.contains("expand")), "two items have nothing to expand: {screen:?}");
}

#[test]
fn the_tree_leaves_the_first_and_the_last_column_blank() {
    let mut env = Env::new();
    populate(&mut env);
    let screen = screen(&draw(&mut env.view(), 46, 19));
    assert!(screen.last().is_some_and(|r| *r == expand()), "the whole tree: {screen:?}");
    // `screen` is trimmed at the right, so the last column is blank when no row is longer than 45 columns
    assert!(screen.iter().all(|r| r.is_empty() || r.starts_with(' ')), "column 0: {screen:?}");
    assert!(screen.iter().all(|r| r.chars().count() <= 45), "the last column: {screen:?}");
    assert!(screen.iter().any(|r| r.chars().count() == 43), "the counters end at width − 4: {screen:?}");
}

#[test]
fn an_empty_store_says_so() {
    let mut env = Env::new();
    let header = pill(38, 0);
    let close = " ╰\u{F0B2A} up to date, drink some water"; // hs-d487: the empty Inbox's one closing row, below its title
    assert_eq!(screen(&draw(&mut env.view(), 40, 5)), ["", "  no Contexts", "", header.as_str(), close], "the section is there with no item: the title (count 0), then the one closing row");
}

#[test]
fn a_link_kinds_label_reads_whole_on_the_screen_through_tmux() {
    let mut env = Env::new();
    populate(&mut env);
    let event = env.store.read(cpd_core::events::highest_id).unwrap();
    reconcile(&mut env.store, &jumping_items(event), OPENED).unwrap();
    let mut view = env.view();
    let screen = screen(&draw(&mut view, 46, 26));
    assert!(screen.last().is_some_and(|r| *r == expand()), "the more row: {screen:?}");
    click(&mut view, 10, screen.len() as u16 - 1);
    let expected = item_row_of(44, palette::TIER_YELLOW, "acme/api#12 failing");
    let buf = draw(&mut view, 46, 26);
    let screen = self::screen(&buf);
    assert!(screen.iter().any(|r| *r == expected), "the link's whole row: {screen:?}");
    let row = screen.iter().find(|r| r.contains("acme/api#12")).unwrap();
    assert_eq!(row, &expected, "the OSC 8 pieces leave no stray cell on the row");
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
