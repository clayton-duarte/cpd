//! The `widgets` view (commands.md §13.2, §17): snapshot tests of the Dispatch
//! column and the key legend with ratatui's `TestBackend`, on a store prepared
//! through the store API (as `tui_read.rs` does, since Plans, Jobs and Attempts
//! belong to their areas), and the palette the two share.

mod common;

use common::TestEnv;
use cpd2::error::CpdError;
use cpd2::home::CpdHome;
use cpd2::store::Store;
use cpd2::tui::palette::{self, Meaning};
use cpd2::tui::read::{self, Widgets};
use cpd2::tui::widgets::{self, legend};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

// ---------- fixtures (the store API, as tui_read.rs prepares it) ----------

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn sql(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn context(id: &str, grounded: Option<&str>) -> String {
    let (lead, seq) = grounded.map_or(("NULL".to_owned(), "NULL"), |g| (format!("'{g}'"), "0"));
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on, lead_session, grounded_seq)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1', {lead}, {seq});"
    )
}

/// `phase` decides the columns the schema ties to it; `pr` is `(number, pr_seen)`.
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: Option<(i64, &str)>) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let (number, url, opened, seen, polled) = match pr {
        Some((n, seen)) => (
            n.to_string(),
            format!("'https://example.test/pull/{n}'"),
            "'2026-01-05T00:00:00.000Z'".to_owned(),
            seen.to_owned(),
            "'2026-01-06T00:00:00.000Z'".to_owned(),
        ),
        None => (
            "NULL".into(),
            "NULL".into(),
            "NULL".into(),
            r#"{"checks":{},"reviewIds":[],"commentIds":[]}"#.to_owned(),
            "NULL".into(),
        ),
    };
    format!(
        "INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, pr_number,
                            pr_url, pr_opened_at, pr_seen, pr_polled_at, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '/repo', 'main', 'cpd/{id}', {worktree}, '{phase}', '{lead}',
                 {number}, {url}, {opened}, '{seen}', {polled}, '{created}', {locked}, {done});"
    )
}

fn session_row(id: &str, role: &str, context: &str, plan: &str, attempt: Option<&str>, state: &str, label: Option<&str>, created: &str) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    let attempt = attempt.map_or("NULL".to_owned(), |a| format!("'{a}'"));
    let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
    let tier = if role == "lead" { "O3" } else { "S3" };
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {plan}, {attempt}, 'name of {id}', '/sessions/{id}.jsonl', '/repo', '{tier}',
                 NULL, '{state}', {label}, '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

fn job_row(plan: &str, key: &str, status: &str, tier: &str, needs: &[&str]) -> String {
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{FP}'") };
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let failed = if status == "failed" { "'crashed'" } else { "NULL" };
    let (reason, requested) =
        if status == "cancelled" { ("'not needed'", "'2026-01-04T00:00:00.000Z'") } else { ("NULL", "NULL") };
    let mut out = format!(
        "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', '{key}', 'Job {key}', '{tier}', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
                 cpd_txn(), {reason}, {requested}, {failed}, {merged}, '2026-01-02T00:00:00.000Z');"
    );
    for n in needs {
        out += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{key}', '{n}');");
    }
    out
}

/// An Attempt `<job>-a<number>`; `status` `ended` takes `outcome`.
fn attempt_row(plan: &str, job: &str, number: u32, tier: &str, status: &str, ended: Option<(&str, &str)>, worker: Option<&str>) -> String {
    let id = format!("{job}-a{number}");
    let (outcome, cause, merged, ended_at) = match ended {
        Some((outcome, cause)) => (
            format!("'{outcome}'"),
            format!("'{cause}'"),
            if outcome == "merged" { "'abc123'" } else { "NULL" }.to_owned(),
            "'2026-01-03T01:00:00.000Z'".to_owned(),
        ),
        None => ("NULL".into(), "NULL".into(), "NULL".into(), "NULL".into()),
    };
    let (sha, at) = if status == "merging" { ("'def456'", "'2026-01-03T00:30:00.000Z'") } else { ("NULL", "NULL") };
    let worker = worker.map_or("NULL".to_owned(), |w| format!("'{w}'"));
    let origin = if number == 1 { "ladder" } else { "retry" };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session,
                               submitted_sha, submitted_at, merged_sha, started_at, ended_at)
         VALUES ('{plan}', '{id}', '{job}', {number}, '{tier}', '{origin}', '{status}', {outcome}, {cause}, 'cpd/{plan}--{id}',
                 '/w/{id}', {worker}, {sha}, {at}, {merged}, '2026-01-03T00:{number:02}:00.000Z', {ended_at});"
    )
}

fn escalation_row(plan: &str, attempt: &str, number: u32, status: &str, asked: &str) -> String {
    let (answer, answered, expired) = match status {
        "answered" => ("'Use v2.'", "'2026-01-03T02:00:00.000Z'", "NULL"),
        "expired" => ("NULL", "NULL", "'2026-01-03T02:00:00.000Z'"),
        _ => ("NULL", "NULL", "NULL"),
    };
    format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at)
         VALUES ('{plan}', '{attempt}-q{number}', '{attempt}', {number}, 'Question {attempt}-q{number}?', '{status}', {answer}, 1,
                 '{asked}', {answered}, {expired});"
    )
}


/// An Event of `plan`, whose Context is `alpha` for `ship` and `beta` for the others.
fn event(at: &str, kind: &str, plan: Option<&str>, text: &str, data: &str) -> String {
    let context = plan.map_or("NULL", |p| if p == "ship" { "'alpha'" } else { "'beta'" });
    let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '{at}', '{kind}', {context}, {plan}, NULL, '{text}', '{data}');"
    )
}

/// Two Contexts and these Plans:
/// - `ship` (dispatch, no PR): Jobs `b` (its Attempt is merging) and `c` (a retry running, asking);
/// - `rel` (pr): PR 7 with a failing check, a review and two comments; Job `r1` running, asking;
/// - `old` (done): its merged PR 3, every check passed;
/// - `sketch` (plan): drafts, not in the column.
///
/// Three Attempts hold a slot. The engine runs (this test's pid), and the last Events include an `error`.
fn populate(store: &mut Store) {
    let mut s = String::new();
    s += &context("alpha", Some("g-alpha"));
    s += &context("beta", None);
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:00.000Z", None);
    s += &plan_row("sketch", "alpha", "plan", "l-sketch", "2026-01-02T00:00:01.000Z", None);
    s += &plan_row(
        "rel",
        "beta",
        "pr",
        "l-rel",
        "2026-01-02T00:00:02.000Z",
        Some((7, r#"{"checks":{"ci":"FAILURE","lint":"FAILURE","fmt":"SUCCESS"},"reviewIds":["r1"],"commentIds":["c1","c2"]}"#)),
    );
    s += &plan_row(
        "old",
        "beta",
        "done",
        "l-old",
        "2026-01-01T12:00:00.000Z",
        Some((3, r#"{"checks":{"ci":"SUCCESS"},"reviewIds":[],"commentIds":[]}"#)),
    );
    s += &session_row("g-alpha", "lead", "alpha", "", None, "idle", None, "2026-01-02T00:00:00.000Z");
    for (id, context, plan) in [("l-ship", "alpha", "ship"), ("l-sketch", "alpha", "sketch"), ("l-rel", "beta", "rel"), ("l-old", "beta", "old")] {
        s += &session_row(id, "lead", context, plan, None, "idle", None, "2026-01-02T00:00:00.000Z");
    }
    s += &job_row("ship", "b", "running", "S3", &[]);
    s += &job_row("ship", "c", "running", "S2", &[]);
    s += &session_row("w-b1", "worker", "alpha", "ship", Some("b-a1"), "idle", None, "2026-01-03T00:01:00.000Z");
    s += &session_row("w-c2", "worker", "alpha", "ship", Some("c-a2"), "working", None, "2026-01-03T00:02:00.000Z");
    s += &attempt_row("ship", "b", 1, "S3", "merging", None, Some("w-b1"));
    s += &attempt_row("ship", "c", 1, "S2", "ended", Some(("failed", "timed_out")), None);
    s += &attempt_row("ship", "c", 2, "S3", "running", None, Some("w-c2"));
    s += &escalation_row("ship", "c-a2", 1, "open", "2026-01-03T00:20:00.000Z");
    s += &job_row("rel", "r1", "running", "S3", &[]);
    s += &session_row("w-r1", "worker", "beta", "rel", Some("r1-a1"), "blocked", Some("cpd: question to the Lead"), "2026-01-03T00:03:00.000Z");
    s += &attempt_row("rel", "r1", 1, "S3", "starting", None, Some("w-r1"));
    s += &escalation_row("rel", "r1-a1", 1, "open", "2026-01-03T00:05:00.000Z");
    s += &escalation_row("rel", "r1-a1", 2, "answered", "2026-01-03T00:04:00.000Z");
    s += &event("2026-01-03T00:00:00.000Z", "watch-started", None, "The engine started.", &format!(r#"{{"pid":{}}}"#, std::process::id()));
    s += &event(
        "2026-01-03T00:10:00.000Z",
        "pr-feedback",
        Some("rel"),
        "PR 7 has failing checks.",
        r#"{"checks":[{"name":"ci","conclusion":"FAILURE"},{"name":"lint","conclusion":"FAILURE"}],"reviews":[{"id":"r1"}],"comments":[],"state":"OPEN"}"#,
    );
    s += &event("2026-01-03T00:25:00.000Z", "error", Some("ship"), "Attempt c-a2 could not push.", r#"{"code":"push_failed"}"#);
    sql(store, &s);
}

fn fixture() -> (TestEnv, Store) {
    let env = TestEnv::new();
    let mut store = open(&env);
    populate(&mut store);
    (env, store)
}

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-03T00:30:00.000Z", &Rfc3339).unwrap()
}

fn render(width: u16, height: u16, w: &Widgets, note: Option<&str>) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| widgets::draw(f, f.area(), w, now(), note)).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| row(buf, y).trim_end().to_owned()).collect()
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

/// The first cell of the first row at or after `from` that contains `needle` (every glyph here is one column wide).
fn find_after(buf: &Buffer, from: u16, needle: &str) -> (u16, u16) {
    (from..buf.area.height)
        .find_map(|y| row(buf, y).find(needle).map(|b| (row(buf, y)[..b].chars().count() as u16, y)))
        .unwrap_or_else(|| panic!("{needle:?} is not drawn:\n{}", rows(buf).join("\n")))
}

fn find(buf: &Buffer, needle: &str) -> (u16, u16) {
    find_after(buf, 0, needle)
}

// ---------- the Dispatch column ----------

const RULE: &str = "──────────────────────────────────────────────";

/// The column on the prepared store, as the User sees it: every Plan with a PR (oldest first), then the Plan in
/// Dispatch without one; the Events fill what is left.
fn column() -> (Vec<String>, Buffer) {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 50).unwrap();
    let buf = render(46, 25, &w, None);
    (rows(&buf), buf)
}

#[test]
fn the_column_shows_the_engine_slots_prs_escalations_and_events() {
    let (got, _) = column();
    let pid = std::process::id();
    let expected = [
        " Engine".to_owned(),
        format!(" ● running  pid {pid}  since 00:00:00"),
        " Slots 3/3".to_owned(),
        " rel/r1-a1 S3 starting 29m".to_owned(),
        " ship/b-a1 S3 merging 29m".to_owned(),
        " ship/c-a2 S3 running 28m".to_owned(),
        RULE.to_owned(),
        " PRs".to_owned(),
        " Plan old".to_owned(),
        "  #3 merged ✓ 0 reviews  0 comments".to_owned(),
        " Plan rel".to_owned(),
        "  #7 open ✗ 1 review  2 comments".to_owned(),
        "  ✗ ci, lint".to_owned(),
        " Plan ship  no PR".to_owned(),
        RULE.to_owned(),
        " Escalations 2".to_owned(),
        " ● rel/r1  Question r1-a1-q1?".to_owned(),
        " ● ship/c  Question c-a2-q1?".to_owned(),
        RULE.to_owned(),
        " Events".to_owned(),
        " 00:00:00 The engine started.".to_owned(),
        " 00:10:00 PR 7 has failing checks.".to_owned(),
        " 00:25:00 Attempt c-a2 could not push.".to_owned(),
        String::new(),
        widgets::HINT.trim_end().to_owned(),
    ];
    assert_eq!(got, expected, "\n{}", got.join("\n"));
}

#[test]
fn the_column_takes_every_colour_from_the_palette() {
    let (_, buf) = column();
    let fg = |at: (u16, u16)| buf[at].fg;
    let c = palette::color;
    assert_eq!(fg(find(&buf, "●")), c(Meaning::InProgress), "the running engine");
    assert_eq!(fg(find(&buf, "3/3")), c(Meaning::Review), "every slot is used");
    // Each Attempt's status, in the colour of its meaning.
    assert_eq!(fg(find(&buf, "starting")), c(Meaning::NotStarted));
    assert_eq!(fg(find(&buf, "merging")), c(Meaning::Review));
    assert_eq!(fg(find(&buf, "running 28m")), c(Meaning::InProgress));
    // The PRs: state and checks.
    assert_eq!(fg(find(&buf, "merged")), c(Meaning::Done));
    assert_eq!(fg(find(&buf, "✓")), c(Meaning::Done));
    assert_eq!(fg(find(&buf, "open")), c(Meaning::Review));
    assert_eq!(fg(find_after(&buf, find(&buf, "#7 open").1, "✗")), c(Meaning::Failed));
    assert_eq!(fg(find(&buf, "ci, lint")), c(Meaning::Failed), "the failing checks");
    // The Escalations, the failed Event and the separators.
    assert_eq!(fg(find(&buf, "● rel")), c(Meaning::Review));
    assert_eq!(fg(find(&buf, "Attempt c-a2 could")), c(Meaning::Failed), "an error Event");
    assert_eq!(fg(find(&buf, "The engine started")), Color::Reset, "a plain Event names no colour");
    assert_eq!(fg(find(&buf, RULE)), c(Meaning::Nothing));
    // Dim: the clocks, the questions, the hint.
    let dim = |at: (u16, u16)| buf[at].modifier.contains(Modifier::DIM);
    assert!(dim(find(&buf, "00:10:00")) && dim(find(&buf, "Question r1")) && dim(find(&buf, "? legend")));
    assert!(!dim(find(&buf, "PR 7 has failing")));
    assert!(buf[find(&buf, "Engine")].modifier.contains(Modifier::BOLD));
}

#[test]
fn an_empty_store_says_so_in_every_section() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let w = read::widgets(&mut store, 2, 50).unwrap();
    let got = rows(&render(30, 16, &w, None));
    let rule = "─".repeat(30);
    let expected = [
        " Engine",
        " ○ not started yet",
        " Slots 0/2",
        " none in use",
        &rule,
        " PRs",
        " none",
        &rule,
        " Escalations 0",
        " none open",
        &rule,
        " Events",
        " none yet",
        "",
        "",
        widgets::HINT.trim_end(),
    ];
    assert_eq!(got, expected, "\n{}", got.join("\n"));
}

fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

#[test]
fn the_engine_is_never_running_stopped_or_crashed_each_in_its_colour() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let engine = |store: &mut Store| {
        let w = read::widgets(store, 3, 0).unwrap();
        let buf = render(60, 20, &w, None);
        let at = find(&buf, "Engine");
        (row(&buf, at.1 + 1).trim_end().to_owned(), buf[(1, at.1 + 1)].fg)
    };
    let c = palette::color;
    assert_eq!(engine(&mut store), (" ○ not started yet".to_owned(), c(Meaning::Nothing)));

    let started = |pid: u32| event("2026-01-03T00:00:00.000Z", "watch-started", None, "The engine started.", &format!(r#"{{"pid":{pid}}}"#));
    sql(&mut store, &started(std::process::id()));
    assert_eq!(engine(&mut store), (format!(" ● running  pid {}  since 00:00:00", std::process::id()), c(Meaning::InProgress)));

    sql(&mut store, &event("2026-01-03T01:02:03.000Z", "watch-stopped", None, "The engine stopped.", "{}"));
    assert_eq!(engine(&mut store), (" ○ stopped  01:02:03".to_owned(), c(Meaning::Nothing)));

    let dead = dead_pid();
    sql(&mut store, &started(dead));
    assert_eq!(engine(&mut store), (format!(" ✗ crashed  pid {dead} is gone  since 00:00:00"), c(Meaning::Failed)));
}

#[test]
fn a_narrow_column_cuts_text_with_an_ellipsis_and_never_wraps() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 50).unwrap();
    let got = rows(&render(24, 25, &w, None));
    assert!(got.iter().all(|r| r.chars().count() <= 24), "{got:#?}");
    for cut in [
        " rel/r1… S3 starting 29m",
        "  #7 open ✗ 1 review  2…",
        " ● rel/r1  Question r1-…",
        " 00:25:00 Attempt c-a2 …",
    ] {
        assert!(got.contains(&cut.to_owned()), "{cut:?} in {got:#?}");
    }
}

#[test]
fn a_short_pane_keeps_the_latest_events_and_drops_the_plans_without_a_pr() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 50).unwrap();
    // 21 rows: the sections above the Events take 18 and the hint 1, so the Events keep their title and the latest one.
    let got = rows(&render(46, 21, &w, None));
    assert!(!got.iter().any(|r| r.contains("no PR")), "{got:#?}");
    let events = got.iter().position(|r| r == " Events").unwrap();
    assert_eq!(got[events + 1], " 00:25:00 Attempt c-a2 could not push.", "the latest Event, not the oldest");
    assert_eq!(got.last().unwrap(), widgets::HINT.trim_end());
}

#[test]
fn a_note_takes_the_hints_row_in_the_failed_colour() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 50).unwrap();
    let buf = render(46, 25, &w, Some("No tmux client is attached to the cockpit."));
    let at = find(&buf, "No tmux client");
    assert_eq!(at.1, 24, "the last row");
    assert_eq!(buf[at].fg, palette::color(Meaning::Failed));
    assert!(!rows(&buf).iter().any(|r| r.contains("? legend")));
}

#[test]
fn a_full_slot_set_is_yellow_a_partial_one_green_and_an_empty_one_dim() {
    let (_env, mut store) = fixture();
    for (limit, meaning) in [(3, Meaning::Review), (4, Meaning::InProgress)] {
        let w = read::widgets(&mut store, limit, 0).unwrap();
        let buf = render(46, 20, &w, None);
        assert_eq!(buf[find(&buf, &format!("3/{limit}"))].fg, palette::color(meaning), "{limit}");
    }
    let empty = TestEnv::new();
    let w = read::widgets(&mut open(&empty), 3, 0).unwrap();
    let buf = render(46, 20, &w, None);
    assert_eq!(buf[find(&buf, "0/3")].fg, palette::color(Meaning::Nothing));
}

// ---------- the legend ----------

fn legend_buffer() -> Buffer {
    let (w, h) = legend::size();
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| legend::draw(f, f.area())).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn every_meaning_is_listed_with_its_swatch_in_its_colour() {
    let buf = legend_buffer();
    for m in Meaning::ALL {
        let (x, y) = find(&buf, m.label());
        assert_eq!(buf[(x - 4, y)].symbol(), "█", "{m:?}");
        assert_eq!(buf[(x - 4, y)].fg, palette::color(m), "{m:?}");
    }
}

#[test]
fn every_job_status_is_a_pill_and_a_counter_from_the_palette() {
    let buf = legend_buffer();
    let jobs = find(&buf, "Jobs: Plan").1;
    for (name, status, merging) in palette::JOB_LEGEND {
        let (icon, m) = palette::job_status(status, merging);
        let (x, y) = find_after(&buf, jobs, name);
        let text = row(&buf, y);
        let pill_at = text.chars().position(|c| c.to_string() == icon).unwrap() as u16;
        assert!(pill_at > x, "{name}: the pill follows the name");
        assert_eq!(buf[(pill_at, y)].bg, palette::color(m), "{name}: pill");
        assert_eq!(buf[(pill_at, y)].fg, palette::pill(m).fg.unwrap(), "{name}: pill text");
        assert!(text.contains(" key S1 "), "{name}: {text}");
        let counter = text.chars().collect::<Vec<_>>().iter().rposition(|c| *c == '3').unwrap() as u16;
        assert_eq!(buf[(counter, y)].fg, palette::color(palette::job_status(status, merging).1), "{name}: counter");
    }
    let counters = row(&buf, find(&buf, "counters row").1);
    assert!(counters.contains("1⋅1⋅2⋅0⋅3"), "{counters}");
    assert!(counters.contains("queued⋅running⋅merging⋅failed⋅merged"), "{counters}");
}

#[test]
fn the_tree_dots_are_listed_for_every_phase() {
    let buf = legend_buffer();
    let dots = find(&buf, "Context dots").1;
    for (name, phase) in palette::PHASE_LEGEND {
        let (x, y) = find_after(&buf, dots, &format!(" {name}"));
        assert_eq!((buf[(x - 3, y)].symbol(), buf[(x - 1, y)].symbol()), ("●", "○"), "{name}");
        let want = palette::color(palette::phase(phase));
        assert_eq!((buf[(x - 3, y)].fg, buf[(x - 1, y)].fg), (want, want), "{name}");
    }
}

#[test]
fn the_pr_states_and_checks_are_listed_in_their_colours() {
    let buf = legend_buffer();
    for (name, state) in palette::PR_LEGEND {
        assert_eq!(buf[find(&buf, &format!("#2 {name}"))].fg, palette::color(palette::pr_state(state)), "{name}");
    }
    let checks = find(&buf, "PR checks").1;
    for (name, c) in palette::CHECKS_LEGEND {
        let (icon, m) = palette::checks(c);
        let (x, y) = find_after(&buf, checks, &format!(" {name}"));
        assert_eq!(buf[(x - 1, y)].symbol(), icon, "{name}");
        assert_eq!(buf[(x - 1, y)].fg, palette::color(m), "{name}");
    }
}

#[test]
fn the_legend_fits_its_own_size() {
    let buf = legend_buffer();
    assert!(row(&buf, 0).contains("any key closes"));
    let (w, h) = legend::size();
    assert_eq!((buf.area.width, buf.area.height), (w, h));
    assert!(row(&buf, h - 1).contains("passed"), "the last section is not cut");
}

#[test]
fn the_legend_popup_is_sized_to_the_legend_and_capped_at_the_client() {
    let (w, h) = legend::size();
    assert_eq!(
        legend::popup_args((215, 62)),
        ["display-popup", "-E", "-T", " legend ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd2 tui view legend"]
    );
    let small = legend::popup_args((40, 20));
    assert_eq!((small[5].as_str(), small[7].as_str()), ("40", "20"));
}

// ---------- the commands ----------

#[test]
fn the_views_need_a_terminal() {
    let env = TestEnv::new();
    for view in ["widgets", "legend"] {
        let run = env.cpd2(&["tui", "view", view]);
        assert_eq!(run.exit, 3, "{view}: {run:?}");
        let error = run.error();
        assert_eq!(error["code"], "not_a_terminal", "{view}");
        assert_eq!(error["details"]["command"], format!("tui view {view}"), "{view}");
    }
}
