//! The `widgets` view (commands.md §13.2, §17): snapshot tests of the Dispatch
//! column and the key legend with ratatui's `TestBackend`, on a store prepared
//! through the store API (as `tui_read.rs` does, since Plans and Sessions belong to
//! their areas), and the palette the two share.

use crate::common;
use crate::tui_titles;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use common::fx::{Fx, TmuxProbe};
use common::screen::{NoInput, Screen, SharedBackend, Steps, left_click, right_click, wheel};
use common::{TestEnv, sql};
use cpd_core::error::CpdError;
use cpd_core::host::runner::{FakeRunner, Reply};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use cpd_tui::ViewError;
use cpd_tui::pump::{Feed, Signal};
use cpd_tui::view_ports::Ctx;
use ratatui::crossterm::event::{Event, KeyCode, MouseEventKind};
use cpd_tui::link::CUE;
use cpd_tui::palette::{self, Meaning};
use cpd_tui::read::{self, Widgets};
use cpd_tui::widgets::{self, Hit, Machine, Usage};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;

// ---------- fixtures (the store API, as tui_read.rs prepares it) ----------

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// A Plan; `pr` is `(number, polled)`, where `polled` is the pull request row's `(title, review decision, checks)`: what `pr open`
/// writes (the Plan's title, no review, no checks) until the first poll changes it.
fn plan_row(id: &str, phase: &str, created: &str, pr: Option<(i64, (&str, &str, &str))>) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let status = match phase {
        "grounding" | "plan" => "waiting",
        "dispatch" => "dispatch",
        "deploy" => "open",
        _ => "merged",
    };
    let (row, number, url, opened, pull_request) = match pr {
        Some((n, (title, decision, checks))) => {
            let (state, merged) = if status == "merged" { ("merged", "'2026-01-09T00:00:00.000Z'") } else { ("open", "NULL") };
            (
                format!(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                     VALUES ('acme/repo', {n}, 'https://github.com/acme/repo/pull/{n}', '{title}', 0, '{state}', '{decision}', '{checks}',
                             '2026-01-05T00:00:00.000Z', {merged}, '2026-01-06T00:00:00.000Z');"
                ),
                n.to_string(),
                format!("'https://github.com/acme/repo/pull/{n}'"),
                "'2026-01-05T00:00:00.000Z'",
                format!("(SELECT id FROM pull_requests WHERE number = {n})"),
            )
        }
        None => (String::new(), "NULL".into(), "NULL".into(), "NULL", "NULL".into()),
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', 'alpha', 'Plan {id}', '{phase}', 'l-{id}', '{created}', {locked}, {done});
         {row}
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo', 'main', '{status}', 'cpd/{id}', {worktree}, {number}, {url}, {opened}, {pull_request},
                 cpd_txn(), '{created}', {locked}, {done});"
    )
}

fn lead_row(plan: &str, name: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('l-{plan}', 'lead', 'alpha', '{plan}', NULL, '{name}', '/sessions/l-{plan}.jsonl', '/repo', 'O3',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    )
}

/// The Lead of `plan` moved to `place` (`session-moved`, contract §5.2).
fn moved(at: &str, plan: &str, place: &str) -> String {
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '{at}', 'session-moved', 'alpha', '{plan}', 'l-{plan}', 'Session moved.',
                 '{{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"{place}\"}}');"
    )
}

/// One Context and these Plans, each named by its Lead Session (the label), oldest first:
/// - `old` ("cpd-ui"): done, its merged PR 3, every check passed, no review; the Lead in the chat slot;
/// - `rel` ("cpd visual lead"): PR 7, open, failing checks, changes requested; the Lead parked;
/// - `fresh` ("fresh lead"): PR 11, as `pr open` wrote its row (no review, no checks); the Lead parked;
/// - `ship` ("v1"): in dispatch, no PR; the Lead in the chat slot;
/// - `sketch` ("sketch lead"): no PR; the Lead has no pane (`none`);
/// - `away` ("away lead"): PR 9, merged; the Lead's pane is in the `work` window, so the Plan is not in the cockpit;
/// - `never` ("never lead"): no PR; the Lead was never moved.
fn populate(store: &mut Store) {
    let mut s = context("alpha");
    let plans: [(&str, &str, &str, Option<(i64, (&str, &str, &str))>, &str, Option<&str>); 7] = [
        ("old", "cpd-ui", "done", Some((3, ("cpd-ui: cockpit rows", "none", "passed"))), "2026-01-01T12:00:00.000Z", Some("chat")),
        (
            "rel",
            "cpd visual lead",
            "deploy",
            Some((7, ("cpd-tui cockpit column and its long title", "changes_requested", "failed"))),
            "2026-01-02T00:00:00.000Z",
            Some("parked"),
        ),
        ("fresh", "fresh lead", "deploy", Some((11, ("Plan fresh", "none", "none"))), "2026-01-02T00:00:01.000Z", Some("parked")),
        ("ship", "v1", "dispatch", None, "2026-01-02T00:00:02.000Z", Some("chat")),
        ("sketch", "sketch lead", "plan", None, "2026-01-02T00:00:03.000Z", Some("none")),
        ("away", "away lead", "done", Some((9, ("away", "approved", "pending"))), "2026-01-02T00:00:04.000Z", Some("work")),
        ("never", "never lead", "dispatch", None, "2026-01-02T00:00:05.000Z", None),
    ];
    for (n, (id, name, phase, pr, created, place)) in plans.into_iter().enumerate() {
        s += &lead_row(id, name);
        s += &plan_row(id, phase, created, pr);
        if let Some(place) = place {
            s += &moved(&format!("2026-01-03T00:00:0{n}.000Z"), id, place);
        }
    }
    sql(store, &s);
}

fn fixture() -> (Fx, Store) {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    populate(&mut store);
    (fx, store)
}

fn machine() -> Machine {
    Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } }
}

fn render(width: u16, height: u16, w: &Widgets) -> Buffer {
    render_hits(width, height, w).0
}

/// The column, and where a click reaches an owned PR.
fn render_hits(width: u16, height: u16, w: &Widgets) -> (Buffer, Vec<Hit>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut hits = vec![];
    terminal.draw(|f| hits = widgets::draw(f, f.area(), &w.prs, &machine(), 0, 0, &w.inbox, (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default()).hits).unwrap();
    (terminal.backend().buffer().clone(), hits)
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| row(buf, y).trim_end().to_owned()).collect()
}

/// The row as the terminal shows it: a linked cell's symbol carries its OSC 8 escapes (`ESC ] 8 ; ; <url> BEL`), which take no column.
fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| plain(buf[(x, y)].symbol())).collect()
}

fn plain(symbol: &str) -> String {
    let mut out = String::new();
    let mut chars = symbol.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(|c| *c == '\x07');
        } else {
            out.push(c);
        }
    }
    out
}

/// A PR's first line on a pane of `width` columns: its name, then `󱆭` in the last column.
fn first_line(name: &str, width: usize) -> String {
    format!("{name:<w$}{CUE}", w = width - 1)
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

/// A section title on a `width`-column pane (hs-d475, hs-d476, hs-d478, hs-d480): the Deploy column's titles are not collapsible.
fn pill(width: usize, title: &str, count: Option<usize>) -> String {
    tui_titles::title(width, title, count)
}

/// A section title on the 46-column test pane.
fn header(title: &str, count: Option<usize>) -> String {
    pill(46, title, count)
}

/// A scroll cue row on the 120-column pane of [`start`]: the cue right-aligned at the last column.
fn cue_row(arrow: char, n: usize) -> String {
    format!("{:>120}", format!("{arrow} {n} more"))
}

/// The column on the prepared store as the User sees it: the open pull requests, oldest first.
fn column(height: u16) -> (Vec<String>, Buffer) {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let buf = render(46, height, &w);
    (rows(&buf), buf)
}

/// One row of `pull_requests` of `repo`, created at `created` (GitHub's time).
fn pull_request(repo: &str, number: i64, title: &str, draft: bool, state: &str, decision: &str, checks: &str, created: &str) -> String {
    let merged = if state == "merged" { "'2026-02-01T00:00:00.000Z'" } else { "NULL" };
    format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('{repo}', {number}, 'https://github.com/{repo}/pull/{number}', '{title}', {}, '{state}', '{decision}', '{checks}',
                 '{created}', {merged}, '2026-02-02T00:00:00.000Z');",
        u8::from(draft)
    )
}

/// Pull requests in two repos, none created in the order they are listed here; `mine` (its Lead in the chat slot) owns
/// `acme/repo#20`, `bare` (its Lead parked) owns none:
/// - `acme/web#4`: draft, changes requested, failed checks, created 01-03;
/// - `acme/repo#20`: pending, no review, created 01-05 (the Plan `mine`'s);
/// - `acme/repo#9`: approved, passed, created 01-04;
/// - `acme/web#2` closed and `acme/api#1` merged: not open.
fn prs_store(env: &TestEnv) -> Store {
    let mut store = open(env);
    let mut s = context("alpha") + &lead_row("mine", "mine lead") + &lead_row("bare", "bare lead");
    s += &plan_row("mine", "deploy", "2026-01-02T00:00:00.000Z", Some((20, ("Mine to review", "none", "pending"))));
    s += &plan_row("bare", "dispatch", "2026-01-02T00:00:01.000Z", None);
    s += &pull_request("acme/web", 4, "Older web fix", true, "open", "changes_requested", "failed", "2026-01-03T00:00:00.000Z");
    s += &pull_request("acme/repo", 9, "Middle one", false, "open", "approved", "passed", "2026-01-04T00:00:00.000Z");
    s += &pull_request("acme/web", 2, "Closed web", false, "closed", "none", "none", "2026-01-01T00:00:00.000Z");
    s += &pull_request("acme/api", 1, "Merged api", false, "merged", "approved", "passed", "2026-01-01T00:00:00.000Z");
    s += &moved("2026-01-03T00:00:00.000Z", "mine", "chat");
    s += &moved("2026-01-03T00:00:01.000Z", "bare", "parked");
    sql(&mut store, &s);
    store
}

/// The reference's own test, `the_dispatch_sections_are_titled_with_a_separator_between_each_pair` (`tui/src/main.rs:1053–1062`):
/// the same lines on the same 30×14 pane, for a machine with no open pull request.
#[test]
fn the_dispatch_sections_are_titled_with_a_separator_between_each_pair() {
    let fx = Fx::new();
    let _engine = common::tree::engine(&fx);
    let w = read::widgets(&mut open(&fx.env), 3, 0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(30, 20)).unwrap();
    let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: None, gpus: vec![], mem: Some(7) } };
    terminal.draw(|f| drop(widgets::draw(f, f.area(), &w.prs, &machine, 0, 0, &w.inbox, (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
    let lines = rows(terminal.backend().buffer());
    let prs_head = pill(30, "PRs", Some(0));
    assert_eq!(lines[..3], ["", " homeserver", " \u{F4BC} –%  \u{EFC5} 7%"], "no CPU % until a second sample");
    assert_eq!(lines[3..6], ["", prs_head.as_str(), ""]);
    assert_eq!(lines[6], " no open PRs");
    // the Inbox section, with no item, is the last four rows (an empty one, its title, the up-to-date row, an empty one)
    assert_eq!(lines[7..16], [""; 9], "the PRs take the rest of the column: no git, no Jira");
    assert!(lines[17].contains("Inbox") && lines[18].contains("up to date"), "{lines:?}");
    assert_eq!((lines[16].as_str(), lines[19].as_str()), ("", ""));
}

#[test]
fn three_open_prs_in_two_repos_come_out_oldest_first_in_two_lines_each() {
    let fx = Fx::new();
    let mut store = prs_store(&fx.env);
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let listed: Vec<(&str, i64)> = w.prs.iter().map(|p| (p.repo.as_str(), p.number)).collect();
    assert_eq!(listed, [("acme/web", 4), ("acme/repo", 9), ("acme/repo", 20)]);
    let got = rows(&render(46, 25, &w));
    let draft_failed = format!("  draft {} changes requested  Older web fix", palette::CHECKS_FAILED);
    let open_approved = format!("  open {} approved  Middle one", palette::CHECKS_PASSED);
    let open_pending = format!("  open {} no review  Mine to review", palette::CHECKS_PENDING);
    let (web4, repo9, repo20) =
        (first_line(" acme/web#4", 46), first_line(" acme/repo#9", 46), first_line(" acme/repo#20", 46));
    assert_eq!(
        got[4..12],
        [
            header("PRs", Some(3)).as_str(),
            "",
            web4.as_str(),
            draft_failed.as_str(),
            repo9.as_str(),
            open_approved.as_str(),
            repo20.as_str(),
            open_pending.as_str(),
        ],
        "{got:#?}"
    );
    assert_eq!(got[12], "", "nothing else is listed");
}

#[test]
fn a_conflicting_pr_shows_conflict_on_its_second_line_and_the_others_do_not() {
    let fx = Fx::new();
    let mut store = prs_store(&fx.env);
    sql(&mut store, "UPDATE pull_requests SET mergeable = 'conflicting' WHERE number = 9; UPDATE pull_requests SET mergeable = 'mergeable' WHERE number = 4;");
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let buf = render(46, 25, &w);
    let got = rows(&buf);
    assert_eq!(got[7], format!("  draft {} changes requested  Older web fix", palette::CHECKS_FAILED));
    assert_eq!(got[9], format!("  open {} approved conflict  Middle one", palette::CHECKS_PASSED));
    assert_eq!(got[11], format!("  open {} no review  Mine to review", palette::CHECKS_PENDING));
    let (x, y) = find(&buf, "conflict");
    assert_eq!(buf[(x, y)].fg, palette::color(Meaning::Failed));
}

#[test]
fn a_draft_shows_draft_and_the_others_open() {
    let fx = Fx::new();
    let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
    assert_eq!(w.prs.iter().map(|p| p.draft).collect::<Vec<_>>(), [true, false, false]);
    let got = rows(&render(46, 25, &w)).join("\n");
    assert_eq!(got.matches("draft").count(), 1, "{got}");
}

#[test]
fn the_pr_of_the_plan_in_the_chat_slot_is_the_blue_block_and_bold_and_the_others_are_not() {
    let fx = Fx::new();
    let mut store = prs_store(&fx.env);
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(w.prs.iter().map(|p| p.active).collect::<Vec<_>>(), [false, false, true]);
    let buf = render(46, 25, &w);
    let style = |needle: &str| {
        let (x, y) = find(&buf, needle);
        let m = buf[(x, y)].modifier;
        (buf[(x, y)].fg == palette::selected().fg.unwrap() && m.contains(Modifier::REVERSED), m.contains(Modifier::BOLD))
    };
    // Both lines of the active one: the blue block (background 4) and bold.
    assert_eq!(style("acme/repo#20"), (true, true));
    assert_eq!(style("Mine to review"), (true, true));
    assert_eq!(style("no review"), (true, true));
    // The others: the name bold, the second line neither the block nor bold.
    assert_eq!(style("acme/web#4"), (false, true));
    assert_eq!(style("acme/repo#9"), (false, true));
    for line_two in ["Older web fix", "approved", "Middle one"] {
        assert_eq!(style(line_two), (false, false), "{line_two}");
    }
    // The chat slot moves to `bare`, which owns none: nothing is active.
    sql(&mut store, &moved("2026-01-04T00:00:00.000Z", "bare", "chat"));
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert!(w.prs.iter().all(|p| !p.active));
    let buf = render(46, 25, &w);
    let at = find(&buf, "Mine to review");
    assert!(!buf[at].modifier.contains(Modifier::REVERSED));
}

#[test]
fn closed_and_merged_pull_requests_are_not_listed() {
    {
        let fx = Fx::new();
        let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
        let got = rows(&render(46, 25, &w)).join("\n");
        for hidden in ["acme/web#2", "Closed web", "acme/api#1", "Merged api"] {
            assert!(!got.contains(hidden), "{hidden}:\n{got}");
        }
    }
    // (one `Fx` at a time: the second is the fixture's, in `column`)
    // The fixture's merged ones (a Plan's own, `old` #3 and `away` #9) are not listed either.
    let (got, _) = column(25);
    let got = got.join("\n");
    assert!(!got.contains("#3") && !got.contains("#9"), "{got}");
}

#[test]
fn an_open_review_request_is_not_listed_but_one_that_is_also_authored_is() {
    let fx = Fx::new();
    let mut store = prs_store(&fx.env);
    let mut s = pull_request("acme/web", 31, "A colleague change", false, "open", "none", "passed", "2026-01-06T00:00:00.000Z");
    s += &pull_request("acme/web", 32, "My own, and asked of me", false, "open", "none", "passed", "2026-01-07T00:00:00.000Z");
    s += "UPDATE pull_requests SET authored = 0, review_requested = 1 WHERE number = 31;
          UPDATE pull_requests SET review_requested = 1 WHERE number = 32;";
    sql(&mut store, &s);

    let w = read::widgets(&mut store, 3, 0).unwrap();

    let listed: Vec<(&str, i64)> = w.prs.iter().map(|p| (p.repo.as_str(), p.number)).collect();
    assert!(listed.contains(&("acme/web", 32)), "{listed:?}");
    assert!(listed.iter().all(|(_, n)| *n != 31), "{listed:?}");
    let got = rows(&render(46, 30, &w)).join("\n");
    assert!(got.contains("acme/web#32") && !got.contains("acme/web#31"), "{got}");
}

#[test]
fn no_open_pull_request_says_so() {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    let mut s = context("alpha") + &lead_row("ship", "v1");
    s += &plan_row("ship", "dispatch", "2026-01-02T00:00:00.000Z", None);
    s += &pull_request("acme/web", 2, "Closed web", false, "closed", "none", "none", "2026-01-01T00:00:00.000Z");
    sql(&mut store, &s);
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert!(w.prs.is_empty());
    let buf = render(46, 25, &w);
    let got = rows(&buf);
    assert_eq!(got[4..7], [header("PRs", Some(0)).as_str(), "", " no open PRs"], "{got:#?}");
    assert_eq!(buf[find(&buf, "no open PRs")].fg, palette::muted_text().fg.unwrap(), "muted, colour 8");
}

/// `acme/web#4` (failed checks) is owned by no Plan, `acme/repo#20` (pending) by `mine`: only the unowned one is red.
#[test]
fn failed_checks_are_red_on_an_unowned_pull_request() {
    let fx = Fx::new();
    let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
    let buf = render(46, 25, &w);
    assert_eq!(buf[find(&buf, palette::CHECKS_FAILED)].fg, palette::color(Meaning::Failed));
}

#[test]
fn a_long_title_is_cut_with_an_ellipsis_first_and_a_line_never_wraps() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let wide = rows(&render(30, 25, &w));
    assert!(wide.contains(&format!("  open {} changes requested", palette::CHECKS_FAILED)), "no room for a title of 4 characters:\n{wide:#?}");
    let got = rows(&render(36, 25, &w));
    assert!(got.contains(&format!("  open {} changes requested  cpd-tui…", palette::CHECKS_FAILED)), "{got:#?}");
    let got = rows(&render(24, 25, &w));
    assert!(got.iter().all(|r| r.chars().count() <= 24), "{got:#?}");
    assert!(got.contains(&format!("  open {} changes request", palette::CHECKS_FAILED)), "the pane clips what is left:\n{got:#?}");
    let narrow = rows(&render(8, 25, &w));
    assert!(narrow.contains(&format!(" acme… {CUE}")), "the name is cut before the arrow when it must be:\n{narrow:#?}");
}

#[test]
fn a_short_pane_hides_the_pull_requests_that_do_not_fit_beside_the_cue_below() {
    let fx = Fx::new();
    let _engine = common::tree::engine(&fx);
    let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
    // Beside the 6 rows of the blank row, the machine, the header and its blank rows, and the 4 of the Inbox section (the
    // up-to-date row: no item, the engine's lock is held), each pull request takes 2, and when some are hidden the `▼` row takes one more.
    for (height, listed) in [(6 + 6, 3), (6 + 5, 2), (6 + 4, 1), (6 + 3, 1), (6 + 2, 0), (6 + 1, 0)] {
        let got = rows(&render(46, height + 4, &w)).join("\n");
        let shown = ["acme/web#4", "acme/repo#9", "acme/repo#20"].iter().filter(|n| got.contains(**n)).count();
        assert_eq!(shown, listed, "height {height}:\n{got}");
        assert!(listed == 0 || got.contains("acme/web#4"), "the oldest stay:\n{got}");
        assert_eq!(got.contains(&format!("▼ {} more", 3 - listed)), listed < 3, "height {height}:\n{got}");
    }
}

// ---------- the 󱆭 links and the owned rows ----------

#[test]
fn every_pr_row_ends_its_first_line_in_an_arrow_linking_to_the_pull_request() {
    let fx = Fx::new();
    let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
    let buf = render(46, 25, &w);
    for (y, number, repo) in [(6, 4, "acme/web"), (8, 9, "acme/repo"), (10, 20, "acme/repo")] {
        let _ = (number, repo);
        assert_eq!(buf[(45, y)].symbol(), CUE, "row {y}");
        assert_eq!(row(&buf, y).chars().count(), 46, "the arrow is the last column of row {y}");
        assert_eq!(plain(buf[(45, y + 1)].symbol()), " ", "the second line has no arrow");
    }
}

#[test]
fn an_unowned_pr_is_read_only_text_on_both_lines_with_its_arrow_clickable_and_an_owned_one_is_clickable_text() {
    let fx = Fx::new();
    let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
    assert_eq!(
        w.prs.iter().map(|p| p.lead.as_ref().map(|l| l.as_str())).collect::<Vec<_>>(),
        [None, None, Some("l-mine")],
        "`mine` owns acme/repo#20; no Plan owns the other two"
    );
    let buf = render(46, 25, &w);
    let (read_only, clickable) = (palette::read_only_text().fg.unwrap(), palette::clickable_text().fg.unwrap());
    let fg = |x: u16, y: u16| buf[(x, y)].fg;
    for y in [6, 8] {
        assert!([1, 2, 10].iter().all(|x| fg(*x, y) == read_only), "row {y} is 7: the name");
        assert_eq!(fg(45, y), clickable, "row {y}: the arrow is 15, a modifier-click target");
        // the state word: `draft` is magenta 5 (base, the PR is no Plan's), `open` is 7 (it is not ready to merge)
        let word = if y == 6 { palette::color(Meaning::Planning) } else { read_only };
        assert_eq!(fg(2, y + 1), word, "row {}: the second line's state word", y + 1);
    }
    assert_eq!(plain(buf[(45, 6)].symbol()), CUE, "the arrow stays on an unowned PR");
    // The owned one (here also the active one) is the blue block: text 0 on background 4 across both lines.
    let block = palette::selected();
    assert!([1, 2, 12, 45].iter().all(|x| (Some(buf[(*x, 10)].bg), Some(fg(*x, 10))) == (block.bg, block.fg)));
    assert!([2, 20].iter().all(|x| (Some(buf[(*x, 11)].bg), Some(fg(*x, 11))) == (block.bg, block.fg)));
}

/// The column drawn at spinner frame `frame`.
fn render_at(width: u16, height: u16, w: &Widgets, frame: usize) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| drop(widgets::draw(f, f.area(), &w.prs, &machine(), 0, frame, &w.inbox, (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn a_pr_whose_lead_works_shows_the_spinner_beside_its_state_and_the_frame_advances_with_the_clock() {
    let fx = Fx::new();
    let mut store = prs_store(&fx.env);
    // `mine` owns acme/repo#20 (third row pair); `acme/web#4` and `acme/repo#9` have no owner.
    let second = |w: &Widgets, frame: usize| row(&render_at(46, 25, w, frame), 11).trim_end().to_owned();
    let idle = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(second(&idle, 0), format!("  open {} no review  Mine to review", palette::CHECKS_PENDING), "an idle Lead: no dot");
    sql(&mut store, "UPDATE sessions SET state = 'working' WHERE id = 'l-mine'");
    let working = read::widgets(&mut store, 3, 0).unwrap();
    for frame in 0..palette::SPINNER.len() {
        assert_eq!(second(&working, frame), format!("  open {} {} no review  Mine to review", palette::SPINNER[frame], palette::CHECKS_PENDING), "frame {frame}");
    }
    // the unowned rows show none, whatever works
    let buf = render_at(46, 25, &working, 3);
    for y in [7, 9] {
        assert!(!palette::SPINNER.iter().any(|g| row(&buf, y).contains(g)), "row {y}: {}", row(&buf, y));
    }
}

#[test]
fn only_an_owned_pr_has_a_hit_area_its_two_rows_except_the_arrow() {
    let fx = Fx::new();
    let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
    let (_, hits) = render_hits(46, 25, &w);
    let [hit] = &hits[..] else { panic!("{hits:?}") };
    assert_eq!(hit.lead.as_str(), "l-mine");
    let inside = |x, y| hit.contains(ratatui::layout::Position::new(x, y));
    assert!(inside(0, 10) && inside(44, 10) && inside(0, 11) && inside(45, 11));
    assert!(!inside(45, 10), "the arrow cell is the terminal's");
    assert!(!inside(0, 9) && !inside(0, 12), "the unowned rows above and the blank row below");
}

#[test]
fn a_snapshot_error_is_the_last_row_in_red() {
    let fx = Fx::new();
    let _engine = common::tree::engine(&fx);
    let w = read::widgets(&mut prs_store(&fx.env), 3, 0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(46, 25)).unwrap();
    terminal
        .draw(|f| {
            drop(widgets::draw(f, f.area(), &w.prs, &machine(), 0, 0, &w.inbox, (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default()));
            widgets::draw_error(f, f.area(), "no cockpit");
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    assert_eq!(row(&buf, 24).trim_end(), "no cockpit");
    assert_eq!(buf[(0, 24)].fg, palette::color(Meaning::Failed), "red, colour 1");
    // the Inbox section's last rows: the up-to-date row, as without the error
    assert_eq!(row(&buf, 23), row(&render(46, 25, &w), 23), "the rest is as without it");
    assert!(row(&buf, 23).contains("up to date"), "{}", row(&buf, 23));
}

// ---------- the view's loop, in this process ----------

/// A second of virtual time, on the `ManualClock` the view runs on.
const ONE_SECOND: Duration = Duration::from_secs(1);

/// The root command tree, in this process, on the ports the view runs on (`show` scopes them): what `session open` runs through.
fn run_command(argv: Vec<OsString>) -> Result<Output, CpdError> {
    cpd::cli::run_with(&Ports::current(), argv)
}

/// A press of the character `c`, as the scripted pump hands it to the loop.
fn press(c: char) -> Signal {
    Signal::Input(common::screen::ch(c))
}

/// An event of the terminal, as the scripted pump hands it to the loop.
fn input(event: Event) -> Signal {
    Signal::Input(event)
}

/// The end of a script: the terminal fails.
fn gone() -> Signal {
    Signal::InputFailed(io::Error::other("gone"))
}

/// Runs the view's loop (`widgets::show_with`) on the store of `fx` in a 120 x 40 pane: one thread, the `ManualClock` and a
/// scripted pump that hands `signals` over in order (an idle wait advances the clock, no sleep, no watcher), and `steps`
/// as what the world does while it draws. The script ends the view with the terminal's failure (`gone()`); every step
/// must have run by then.
fn show(fx: &Fx, signals: Vec<Signal>, steps: &Steps) -> SharedBackend {
    let screen = SharedBackend::new(120, 40);
    steps.on(&screen);
    let ports = &fx.fakes.ports;
    let ended = Ports::scope(ports, || {
        let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
        let prepared = widgets::prepare(ports, home).unwrap();
        let mut input = NoInput;
        let mut ctx = Ctx::new(ports, screen.terminal(), &mut input);
        widgets::show_with(&mut ctx, Feed::scripted(ports.clock.clone(), signals), prepared, run_command)
    });
    match ended {
        Err(ViewError::Terminal(e)) => assert_eq!(e.to_string(), "gone", "the view's terminal failed"),
        Err(ViewError::Cpd(e)) => panic!("the view's own work failed: {}", e.message()),
        Ok(()) => panic!("the view ended with no terminal failure: no key ends it"),
    }
    steps.finish();
    screen
}

/// An `Fx` whose config is the one a view needs (machine `m1`, the private tmux server): `Config::load` refuses a home without one.
fn view_fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    // The machine header: the host's name, and no reading of its usage (every probe fails: the view shows none, no error).
    fx.fake("hostname", Reply::ok("homeserver\n"));
    for probe in ["sysctl", "ioreg"] {
        fx.fake(probe, Reply::exit(1, "", ""));
    }
    // `run-shell -b` hands the merge to `sh -c`: the record is what tmux would start in the background
    fx.fake("sh", Reply::ok(""));
    fx
}

/// Whether the screen shows the PR `name` (` acme/repo#9`) at the start of a row.
fn shows(screen: &Screen, name: &str) -> bool {
    screen.shown_rows().iter().any(|r| r.starts_with(&format!(" {name} ")))
}

/// `text` on the database `db` through a connection of its own (not through `Store`, which refuses a changed schema and keeps the Events append-only).
fn raw(db: &Path, text: &str) {
    rusqlite::Connection::open(db).unwrap().execute_batch(text).unwrap();
}

// ---------- the cockpit: the view beside the chat in the fake tmux server; the clicks and the keys are the script's ----------

/// The client the fake server's `list-clients` names ([`client`]).
const CLIENT: &str = "/dev/pts/9";

/// The pane the view draws on ([`show`]), in columns.
const WIDTH: u16 = 120;

/// The cockpit (session `cpd`, window `main`) in the fake server of `fx`: the view and the chat placeholder beside it, and the
/// Lead `l-mine` alone in a park window, as `session open` finds it (`@cpd_session`); its row records that pane. No pane
/// runs a program: the fake only records.
fn cockpit(fx: &Fx) {
    let ok = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let view = ok(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "120", "-y", "40", "-P", "-F", "#{pane_id}", "cpd tui view widgets"]);
    ok(&["set", "-p", "-t", &view, "@cpd_view", "widgets"]);
    let chat = ok(&["split-window", "-h", "-d", "-t", &view, "-P", "-F", "#{pane_id}", "cpd tui view chat"]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    let parked = ok(&["new-window", "-d", "-t", "=cpd:", "-n", "park-mine", "-P", "-F", "#{pane_id}", "cpd tui view chat"]);
    ok(&["set", "-p", "-t", &parked, "@cpd_session", "l-mine"]);
    sql(&mut open(&fx.env), &format!("UPDATE sessions SET tmux_pane = '{parked}' WHERE id = 'l-mine';"));
}

/// The client [`CLIENT`] of 200 x 60 on the fake server: what `list-clients` answers.
fn client(fx: &Fx) {
    fx.tmux_route.fake().add_client(CLIENT, 200, 60);
}

/// What a step looks at while the view runs: the fake server, the Events and the calls of `fx`. It owns its handles, so a step
/// (which owns what it captures) holds a clone.
#[derive(Clone)]
struct Seen {
    probe: TmuxProbe,
    runner: Arc<FakeRunner>,
    home: PathBuf,
}

impl Seen {
    fn of(fx: &Fx) -> Seen {
        Seen { probe: fx.tmux_probe(), runner: fx.fakes.runner.clone(), home: fx.env.cpd_home() }
    }

    /// `tmux <args>` on the fake server of the env's socket (no call of the product's).
    fn tmux(&self, args: &[&str]) -> String {
        self.probe.run(args)
    }

    /// The argv (`display-popup` first) of every popup the fake server opened.
    fn popups(&self) -> Vec<Vec<String>> {
        self.probe.state().popups()
    }

    /// The command of every `run-shell` the product sent to tmux: the merges it started at once (pro-d111).
    fn merges(&self) -> Vec<String> {
        let calls = self.runner.calls().into_iter().filter(|c| c.program.rsplit('/').next() == Some("tmux"));
        calls.map(|c| c.args).filter(|args| args.iter().any(|a| a == "run-shell")).map(|args| args.last().unwrap().clone()).collect()
    }

    /// The arguments of every `set-buffer` the product sent to tmux.
    fn copies(&self) -> Vec<Vec<String>> {
        let calls = self.runner.calls().into_iter().filter(|c| c.program.rsplit('/').next() == Some("tmux"));
        calls.map(|c| c.args).filter(|args| args.iter().any(|a| a == "set-buffer")).collect()
    }

    /// The `session-moved` Events, `(session, place)`, in order.
    fn moved_events(&self) -> Vec<(String, String)> {
        Store::open(&CpdHome::resolve(Some(self.home.clone().into()), None).unwrap(), 5000)
            .unwrap()
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT session, json_extract(data, '$.place') FROM events WHERE kind = 'session-moved' ORDER BY id")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// The pane of the cockpit's `main` window that has the focus: `1|<@cpd_session>|<@cpd_view>` (empty for the view and the placeholder).
    fn focused_session(&self) -> String {
        let panes = self.tmux(&["list-panes", "-t", "=cpd:main", "-F", "#{pane_active}|#{@cpd_session}|#{@cpd_view}"]);
        let active: Vec<&str> = panes.lines().filter(|l| l.starts_with("1|")).collect();
        assert_eq!(active.len(), 1, "{panes}");
        active[0].to_owned()
    }
}

/// A left click at 0-based `column`, `row`, as the scripted pump hands it to the loop.
fn click(column: u16, row: u16) -> Signal {
    input(left_click(column, row))
}

/// A right click at 0-based `column`, `row`.
fn rclick(column: u16, row: u16) -> Signal {
    input(right_click(column, row))
}

/// A press of `code`.
fn key(code: KeyCode) -> Signal {
    input(common::screen::key(code))
}

/// The rows of the first frame the view draws over the store of `fx`: where the clicks of a script go.
fn first_rows(fx: &Fx) -> Vec<String> {
    rows(&render(WIDTH, 40, &read::widgets(&mut open(&fx.env), 3, 0).unwrap()))
}

#[test]
fn in_a_pane_each_arrow_is_a_link_at_the_right_edge_and_the_view_captures_the_mouse() {
    // The column on the 120-column pane of the old test. Whether the terminal's mouse reporting is on while a view runs is
    // `tui_runner`'s (`run_view`'s `Capture::Mouse`): a real pty's flag has no in-process subject.
    let fx = Fx::new();
    let mut store = prs_store(&fx.env);
    let width = 120;
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let buf = render(width, 40, &w);
    for (name, number, repo) in [(" acme/web#4", 4, "acme/web"), (" acme/repo#9", 9, "acme/repo"), (" acme/repo#20", 20, "acme/repo")] {
        let y = (0..buf.area.height).find(|y| row(&buf, *y).starts_with(name)).unwrap_or_else(|| panic!("{name}: {:#?}", rows(&buf)));
        let line = row(&buf, y);
        assert_eq!(line.chars().count(), usize::from(width), "the arrow ends {name}'s line at the right edge: {line:?}");
        assert!(line.ends_with(CUE), "{line:?}");
        // the cell at the right edge is the plain cue: no escape in any cell of the row
        let _ = (number, repo);
        assert_eq!(buf[(width - 1, y)].symbol(), CUE, "{name}");
        assert!((0..width).all(|x| !buf[(x, y)].symbol().contains('\x1b')), "{name}");
    }
}

/// `q` does nothing in the column: with the cockpit's `pane-died` hook a view that exits is started again, so a `q` that
/// ended the program would be a restart (tui-W1).
#[test]
fn q_does_not_end_the_view() {
    let fx = view_fx();
    prs_store(&fx.env);
    let clock = fx.fakes.clock.clone();
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), |_| {})
        // the frame the `q` made: two seconds go by with the view still running
        .next("the frame after the q", move |_| clock.advance(2 * ONE_SECOND));
    // the script ends the view after the `q` and the time: a `q` that ended it would have returned before the script's end
    let shown = show(&fx, vec![press('q'), Signal::Due, gone()], &steps).screen();
    assert!(shown.contains("acme/repo#20"));
}

/// A failed read keeps the column on screen and shows the error in its last row; the next refresh reads again, and the
/// error goes when a read succeeds (tui-W4). The read is made to fail by renaming the table it reads.
#[test]
fn a_failed_read_keeps_the_column_and_shows_the_error_until_a_read_succeeds() {
    let fx = view_fx();
    prs_store(&fx.env);
    let (db, clock) = (fx.env.cpd_home().join("cpd.db"), fx.fakes.clock.clone());
    let failed = |s: &Screen| s.contains("no such table");
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), {
            let (db, clock) = (db.clone(), clock.clone());
            move |_| {
                // not through `Store`, which refuses a changed schema
                raw(&db, "ALTER TABLE pull_requests RENAME TO pull_requests_gone;");
                clock.advance(ONE_SECOND);
            }
        })
        .at("the error in the last row", failed, move |screen| {
            assert!(screen.contains("acme/repo#20"), "the column stays: {}", screen.text());
            raw(&db, "ALTER TABLE pull_requests_gone RENAME TO pull_requests;");
            clock.advance(ONE_SECOND);
        })
        .at("the error to go", move |s| !failed(s) && s.contains("acme/repo#20"), |_| {});
    // a commit makes the view read: it fails; the next commit makes it read again: it succeeds
    show(&fx, vec![Signal::Commit, Signal::Commit, gone()], &steps);
}

/// A follow that fails is shown in the column's last row, the column stays (tui-G4): the highest Event id fell below
/// the cursor, which is what a replaced database looks like.
#[test]
fn a_follow_that_fails_is_shown_and_the_column_stays() {
    let fx = view_fx();
    prs_store(&fx.env);
    let (db, clock) = (fx.env.cpd_home().join("cpd.db"), fx.fakes.clock.clone());
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), move |_| {
            // not through `Store`, which keeps the Events append-only
            raw(&db, "DROP TRIGGER events_no_delete; DELETE FROM events;");
            clock.advance(ONE_SECOND);
        })
        .at("the error in the last row", |s| s.contains("Event cursor"), |screen| {
            assert!(screen.contains("acme/repo#20"), "the column stays: {}", screen.text());
        });
    show(&fx, vec![Signal::Commit, gone()], &steps);
}

#[test]
fn a_click_on_an_owned_row_opens_its_lead_and_focuses_the_chat_and_the_others_run_nothing() {
    let fx = view_fx();
    fx.fake("open", Reply::ok(""));
    let mut store = prs_store(&fx.env);
    sql(&mut store, &moved("2026-01-04T00:00:00.000Z", "mine", "parked"));
    drop(store);
    cockpit(&fx);
    let seen = Seen::of(&fx);
    assert_eq!(seen.focused_session(), "1||widgets", "the view has the focus");
    let before = seen.moved_events();
    let quiet = |what: &'static str| {
        let (seen, before) = (seen.clone(), before.clone());
        move |_: &Screen| assert_eq!(seen.moved_events(), before, "{what}: nothing ran")
    };
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), |_| {})
        // The unowned rows, and the owned row's cue (it opens the page, never the Lead): no `session open` runs.
        .next("the click on an unowned row", quiet("an unowned row"))
        .next("the click on another unowned row", quiet("another unowned row"))
        .next("the click on the arrow", quiet("the arrow"));
    // The owned row, on its second line: `session open l-mine`, then the chat slot has the focus.
    show(&fx, vec![click(3, 6), click(3, 9), click(WIDTH - 1, 10), click(3, 11), gone()], &steps);
    let mut expected = before;
    expected.push(("l-mine".to_owned(), "chat".to_owned()));
    assert_eq!(seen.moved_events(), expected, "one `session open`, for the owned PR's Lead");
    let opened: Vec<_> = fx.calls("open").into_iter().map(|c| c.args).collect();
    assert_eq!(opened, [["https://github.com/acme/repo/pull/20"]], "the cue click ran `open` once, with its row's url");
    assert!(seen.focused_session().contains("|l-mine|"), "{}", seen.focused_session());
    let chat = seen.tmux(&["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_session}|#{pane_id}"]);
    assert!(chat.lines().any(|l| l.starts_with("l-mine|")), "the Lead is in `main`: {chat}");
}

#[test]
fn the_wheel_over_the_prs_scrolls_them_one_at_a_time_between_muted_cue_rows() {
    let fx = view_fx();
    let _engine = common::tree::engine(&fx);
    let mut s = String::new();
    for n in 1..=21 {
        s += &pull_request("acme/repo", n, "A title", false, "open", "none", "passed", &format!("2026-01-03T00:{n:02}:00.000Z"));
    }
    fx.env.sql(&s);
    let has = |s: &Screen, name: &str| shows(s, name);
    let rows = |s: &Screen, row: String| s.shown_rows().iter().any(|r| *r == row);
    let down = || input(wheel(MouseEventKind::ScrollDown, 3, 10));
    let up = || input(wheel(MouseEventKind::ScrollUp, 3, 10));
    let steps = Steps::new()
        // 40 rows less the 6 of the rest of the column and the 4 of the Inbox section leave 30 for the PRs: 14 whole ones beside the `▼` row.
        .at(
            "the first PRs and the cue below",
            move |s| {
                rows(s, cue_row('▼', 7)) && has(s, "acme/repo#1") && has(s, "acme/repo#14") && !has(s, "acme/repo#15") && !s.contains("▲")
            },
            |screen| assert_eq!(screen.cell_of("▼ 7 more").fg, palette::muted_text().fg.unwrap(), "the cue is muted, colour 8"),
        )
        // A notch outside the section does nothing: its frame is the first one
        .next("a notch outside the section", move |screen| {
            assert!(rows(screen, cue_row('▼', 7)) && has(screen, "acme/repo#1") && !screen.contains("▲"), "{}", screen.text());
        })
        // one inside moves by one pull request
        .at("one pull request down", move |s| rows(s, cue_row('▲', 1)) && rows(s, cue_row('▼', 6)) && has(s, "acme/repo#2") && !has(s, "acme/repo#1"), |_| {})
        // Past the end it stops, with the last pull request at the bottom and no cue below.
        .at("the end", move |s| rows(s, cue_row('▲', 7)) && has(s, "acme/repo#21") && has(s, "acme/repo#8") && !has(s, "acme/repo#7") && !s.contains("▼"), |_| {})
        // And back up, one at a time.
        .at("one back up", move |s| rows(s, cue_row('▲', 6)) && rows(s, cue_row('▼', 1)) && has(s, "acme/repo#7") && !has(s, "acme/repo#21"), |_| {})
        .at("the top", move |s| rows(s, cue_row('▼', 7)) && has(s, "acme/repo#1") && !s.contains("▲"), |_| {});
    let mut signals = vec![input(wheel(MouseEventKind::ScrollDown, 3, 1)), down()];
    signals.extend((0..20).map(|_| down()));
    signals.push(up());
    signals.extend((0..20).map(|_| up()));
    signals.push(gone());
    show(&fx, signals, &steps);
}

/// The view's log, `run/tui/widgets.log` (a view draws no note, commands.md §17 "No notes"): empty before its first line.
fn widgets_log(home: &Path) -> String {
    std::fs::read_to_string(home.join("run").join("tui").join("widgets.log")).unwrap_or_default()
}

#[test]
fn a_refused_session_open_is_logged_and_draws_no_note() {
    let fx = view_fx();
    fx.fake("open", Reply::ok(""));
    prs_store(&fx.env);
    let home = fx.env.cpd_home();
    let log = move || widgets_log(&home);
    // No cockpit session: `session open` is refused with `no_cockpit`.
    let (nothing, then, refused) = (log.clone(), log.clone(), log.clone());
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), |_| {})
        // The unowned rows run nothing and the cue opens the page without a failure, so nothing is logged: not after the first click, not after the second.
        .next("the click on an unowned row", move |_| assert_eq!(nothing(), "", "nothing was refused"))
        .next("the click on the cue", move |_| assert_eq!(then(), "", "nothing was refused"))
        // the owned row: refused, logged, and the view has drawn again since; no tmux client is attached, so no dialog opened either
        .next("the click on the owned row", move |screen| {
            assert!(refused().contains("failed: Go to Lead: no_cockpit: "), "{}", refused());
            assert!(refused().contains("cpd tui"), "{}", refused());
            let rows = screen.rows();
            assert!(!rows[39].contains("cockpit"), "no note: {:?}", rows[39]);
            assert!(rows[..39].iter().any(|r| r.contains("acme/repo#20")), "the PRs are as before");
        });
    let click = |column, row| input(left_click(column, row));
    show(&fx, vec![click(3, 6), click(119, 10), click(3, 10), gone()], &steps);
}

#[test]
fn the_merge_icon_is_drawn_and_hit_only_on_clean_pull_requests() {
    let fx = Fx::new();
    let mut store = prs_store(&fx.env);
    sql(&mut store, "UPDATE pull_requests SET merge_state = 'clean' WHERE number IN (9, 20); UPDATE pull_requests SET merge_state = 'blocked' WHERE number = 4;");
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(w.prs.iter().map(|p| (p.number, p.clean)).collect::<Vec<_>>(), [(4, false), (9, true), (20, true)]);
    let mut terminal = Terminal::new(TestBackend::new(46, 25)).unwrap();
    let mut drawn = None;
    terminal.draw(|f| drawn = Some(widgets::draw(f, f.area(), &w.prs, &machine(), 0, 0, &w.inbox, (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
    let widgets::Drawn { hits, icons, .. } = drawn.unwrap();
    let buf = terminal.backend().buffer().clone();
    for (y, icon) in [(7, false), (9, true), (11, true)] {
        assert_eq!(buf[(45, y)].symbol(), if icon { "\u{F062D}" } else { " " }, "row {y}");
    }
    assert_eq!(
        icons.iter().map(|i| (i.area, i.pull.as_str())).collect::<Vec<_>>(),
        [(ratatui::layout::Rect::new(45, 9, 1, 1), "acme/repo#9"), (ratatui::layout::Rect::new(45, 11, 1, 1), "acme/repo#20")]
    );
    let [hit] = &hits[..] else { panic!("{hits:?}") };
    assert!(!hit.contains(ratatui::layout::Position::new(45, 11)), "the owned row's session open excludes the icon");
    assert!(hit.contains(ratatui::layout::Position::new(44, 11)));
}

#[test]
fn a_clean_pr_shows_the_merge_icon_and_a_click_on_it_merges_at_once_not_the_lead() {
    const ICON: char = '\u{F062D}';
    let fx = view_fx();
    // `acme/repo#20` (owned by `mine`) and `acme/repo#9` (no owner) are clean, `acme/web#4` is blocked.
    clean_prs(&fx.env);
    cockpit(&fx);
    client(&fx);
    let seen = Seen::of(&fx);
    let before = seen.moved_events();
    let first = first_rows(&fx);
    let second_line = |name: &str| first_line_of(&first, name) + 1;
    let steps = Steps::new().at("the merge icons", |s| s.shown_rows().iter().any(|r| r.contains(ICON)), |screen| {
        let shown = screen.shown_rows();
        // The icon is the last column of the second line, under the arrow; only the clean ones have it.
        for (name, icon) in [(" acme/web#4", false), (" acme/repo#9", true), (" acme/repo#20", true)] {
            let first = shown.iter().position(|r| r.starts_with(name)).unwrap_or_else(|| panic!("{name}: {shown:#?}"));
            let second = &shown[first + 1];
            assert_eq!(second.ends_with(ICON), icon, "{name}: {second:?}");
            if icon {
                assert_eq!(second.chars().count(), usize::from(WIDTH), "the icon is the last column: {second:?}");
            }
        }
        assert_eq!(shown.iter().filter(|r| r.contains(ICON)).count(), 2, "{shown:#?}");
    });
    // The unowned row's icon, then the owned row's: each starts its merge and neither runs `session open`. The redraws after both
    // clicks (a commit, the time): no third merge starts.
    let signals = vec![click(WIDTH - 1, second_line(" acme/repo#9")), click(WIDTH - 1, second_line(" acme/repo#20")), Signal::Commit, Signal::Due, gone()];
    show(&fx, signals, &steps);
    let merges = seen.merges();
    assert_eq!(merges.len(), 2, "{merges:?}");
    for (command, pull) in merges.iter().zip(["acme/repo##9", "acme/repo##20"]) {
        assert_eq!(command, &format!("cpd tui action merge '{pull}' --client '{CLIENT}' 2>/dev/null"));
    }
    assert!(seen.popups().is_empty(), "no popup opens: {:?}", seen.popups());
    assert_eq!(seen.moved_events(), before, "no `session open` ran");
    assert_eq!(seen.focused_session(), "1||widgets", "the focus did not move");
}

// ---------- the right-click menu of a PR row (hs-d296) ----------

/// The labels of the menu box on `screen`: the `│ <label> │` rows between the `╮` and the `╯` rows.
fn menu_labels(screen: &[String]) -> Vec<String> {
    let top = screen.iter().position(|r| r.contains('╮'));
    let Some(top) = top else { return vec![] };
    screen[top + 1..].iter().take_while(|r| !r.contains('╯')).filter_map(|r| r.split('│').nth(1)).map(|l| l.trim().to_owned()).collect()
}

fn the_menu_is_closed(screen: &[String]) -> bool {
    screen.iter().all(|r| !r.contains('╮') && !r.contains('╯'))
}

/// The row of the first line of the PR named `name` (` acme/repo#9`) on `screen`.
fn first_line_of(screen: &[String], name: &str) -> u16 {
    u16::try_from(screen.iter().position(|r| r.starts_with(name)).unwrap_or_else(|| panic!("{name}: {screen:#?}"))).unwrap()
}

/// `acme/repo#20` (owned by `mine`) and `acme/repo#9` (no owner) are clean, `acme/web#4` (no owner) is blocked.
fn clean_prs(env: &TestEnv) {
    let mut store = prs_store(env);
    sql(
        &mut store,
        "UPDATE pull_requests SET merge_state = 'clean' WHERE number IN (9, 20);
         UPDATE pull_requests SET merge_state = 'blocked' WHERE number = 4;",
    );
    sql(&mut store, &moved("2026-01-04T00:00:00.000Z", "mine", "parked"));
}

#[test]
fn a_right_click_on_a_pr_row_opens_its_menu_merge_only_when_clean_and_go_to_lead_only_when_owned() {
    let fx = view_fx();
    clean_prs(&fx.env);
    cockpit(&fx);
    let first = first_rows(&fx);
    let mut steps = Steps::new().at("the PRs", |s| s.contains("acme/repo#20"), |_| {});
    let mut signals = Vec::new();
    for (name, second_line, labels) in [
        (" acme/web#4", false, &["Copy Link"][..]),
        (" acme/repo#9", true, &["Merge", "Copy Link"]),
        (" acme/repo#20", false, &["Merge", "Copy Link", "Go to Lead"]),
    ] {
        signals.push(rclick(3, first_line_of(&first, name) + u16::from(second_line)));
        signals.push(key(KeyCode::Esc));
        steps = steps
            .at(name, |s| !menu_labels(&s.shown_rows()).is_empty(), move |screen| assert_eq!(menu_labels(&screen.shown_rows()), labels, "{name}"))
            .at("the menu closed", |s| the_menu_is_closed(&s.shown_rows()), |_| {});
    }
    // Not on a PR row: no menu; the redraws that follow (the time) open none.
    signals.extend([rclick(3, 1), Signal::Due, gone()]);
    steps = steps.next("a right click that opens nothing", |screen| assert!(the_menu_is_closed(&screen.shown_rows()), "{}", screen.text()));
    show(&fx, signals, &steps);
}

#[test]
fn an_open_menu_takes_every_click_and_go_to_lead_opens_the_lead_and_focuses_the_chat() {
    let fx = view_fx();
    clean_prs(&fx.env);
    cockpit(&fx);
    let first = first_rows(&fx);
    let (nine, twenty) = (first_line_of(&first, " acme/repo#9"), first_line_of(&first, " acme/repo#20"));
    let seen = Seen::of(&fx);
    let before = seen.moved_events();
    let mut expected = before.clone();
    expected.push(("l-mine".to_owned(), "chat".to_owned()));
    let (after_click, after_lead) = (seen.clone(), seen);
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), |_| {})
        .at("the menu", |s| !menu_labels(&s.shown_rows()).is_empty(), |_| {})
        // A left click outside the menu, on the owned row, closes it and does not open the Lead.
        .at("the menu closed", |s| the_menu_is_closed(&s.shown_rows()), move |_| {
            assert_eq!(after_click.moved_events(), before, "no `session open` ran");
            assert_eq!(after_click.focused_session(), "1||widgets");
        })
        // Go to Lead: what a click on the row does.
        .at("the menu of the owned row", |s| menu_labels(&s.shown_rows()).len() == 3, |_| {})
        .at("the Lead in the chat slot", |s| the_menu_is_closed(&s.shown_rows()), move |_| {
            assert_eq!(after_lead.moved_events(), expected);
            assert!(after_lead.focused_session().contains("|l-mine|"), "{}", after_lead.focused_session());
        });
    let signals = vec![rclick(3, nine), click(0, twenty), rclick(3, twenty), key(KeyCode::Down), key(KeyCode::Down), key(KeyCode::Enter), gone()];
    show(&fx, signals, &steps);
}

#[test]
fn copy_link_puts_the_url_in_tmuxs_buffer_with_set_buffer_w_and_says_nothing() {
    let fx = view_fx();
    clean_prs(&fx.env);
    cockpit(&fx);
    let seen = Seen::of(&fx);
    let url = "https://github.com/acme/repo/pull/9";
    let nine = first_line_of(&first_rows(&fx), " acme/repo#9");
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), |_| {})
        .at("the menu", |s| menu_labels(&s.shown_rows()) == ["Merge", "Copy Link"], |_| {})
        .at("the menu closed", |s| the_menu_is_closed(&s.shown_rows()), |screen| {
            assert!(!screen.rows().iter().any(|r| r.contains("copied")), "no note: {}", screen.text());
        });
    show(&fx, vec![rclick(3, nine), key(KeyCode::Down), key(KeyCode::Enter), gone()], &steps);
    let copies = seen.copies();
    let [copy] = &copies[..] else { panic!("{copies:?}") };
    assert_eq!(&copy[copy.len() - 4..], ["set-buffer", "-w", "--", url], "{copy:?}");
    assert_eq!(seen.tmux(&["show-buffer"]), url);
    assert!(seen.popups().is_empty(), "no popup opened");
}

#[test]
fn merge_in_the_menu_merges_at_once_as_the_merge_icon_does() {
    let fx = view_fx();
    clean_prs(&fx.env);
    cockpit(&fx);
    client(&fx);
    let nine = first_line_of(&first_rows(&fx), " acme/repo#9");
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), |_| {})
        .at("the menu", |s| menu_labels(&s.shown_rows()) == ["Merge", "Copy Link"], |_| {});
    show(&fx, vec![rclick(3, nine), key(KeyCode::Enter), gone()], &steps);
    let seen = Seen::of(&fx);
    assert_eq!(seen.merges(), [format!("cpd tui action merge 'acme/repo##9' --client '{CLIENT}' 2>/dev/null")]);
    assert!(seen.popups().is_empty(), "no popup opens: {:?}", seen.popups());
}

// ---------- a merge in progress (hs-d495): the marker, the spinner and no click ----------

/// The second line of the PR named `name` (` acme/repo#9`) on `screen`, if the screen has it.
fn second_line_of(screen: &[String], name: &str) -> Option<String> {
    let first = screen.iter().position(|r| r.starts_with(name))?;
    screen.get(first + 1).cloned()
}

/// The cell at the end of the second line of `name` is a spinner frame.
fn spinning(screen: &[String], name: &str) -> bool {
    second_line_of(screen, name).is_some_and(|line| palette::SPINNER.iter().any(|frame| line.ends_with(frame)))
}

/// The cell at the end of the second line of `name` is the merge icon.
fn has_icon(screen: &[String], name: &str) -> bool {
    second_line_of(screen, name).is_some_and(|line| line.ends_with('\u{F062D}'))
}

#[test]
fn a_live_merge_marker_shows_the_spinner_for_that_pull_request_alone_and_it_cannot_be_clicked_a_stale_one_changes_nothing() {
    let fx = view_fx();
    clean_prs(&fx.env);
    cockpit(&fx);
    client(&fx);
    let seen = Seen::of(&fx);
    let first = first_rows(&fx);
    let (nine, twenty) = (first_line_of(&first, " acme/repo#9"), first_line_of(&first, " acme/repo#20"));
    // A merge of #9 runs (a live marker, written after the view started: no Event of the store tells it) and the marker of
    // #20 is stale (a dead pid).
    let (live, dead) = (crate::tui_merge::live_pid(&fx), crate::tui_merge::ended_pid(&fx));
    let (home, clock) = (fx.env.cpd_home(), fx.fakes.clock.clone());
    let before = seen.moved_events();
    let (popups, copies) = (seen.clone(), seen.clone());
    let (sees_none, sees_one, sees_two) = (seen.clone(), seen.clone(), seen.clone());
    let steps = Steps::new()
        .at("the merge icons", |s| has_icon(&s.shown_rows(), " acme/repo#9") && has_icon(&s.shown_rows(), " acme/repo#20"), {
            let (home, clock) = (home.clone(), clock.clone());
            move |_| {
                crate::tui_merge::put_marker_at(&home, "acme__repo__9.json", live);
                crate::tui_merge::put_marker_at(&home, "acme__repo__20.json", dead);
                clock.advance(3 * ONE_SECOND);
            }
        })
        .at("the spinner of #9", |s| spinning(&s.shown_rows(), " acme/repo#9"), |screen| {
            let shown = screen.shown_rows();
            assert!(!second_line_of(&shown, " acme/repo#9").unwrap().contains('\u{F062D}'), "the spinner is instead of the icon: {shown:?}");
            assert!(has_icon(&shown, " acme/repo#20"), "a stale marker is ignored: the icon stays: {shown:?}");
            assert!(!spinning(&shown, " acme/web#4") && !has_icon(&shown, " acme/web#4"), "#4 is blocked: no icon, no spinner");
        })
        // A click on the spinner does nothing: no popup, no Lead (no `session open`)
        .next("the click on the spinner", move |_| {
            assert_eq!(sees_none.merges(), Vec::<String>::new());
            assert_eq!(sees_none.moved_events(), before, "no `session open` ran");
        })
        // The icon of the stale one starts its merge, as ever: a positive signal that the click on the spinner was handled first
        .next("the click on the stale one's icon", move |_| {
            let merges = sees_one.merges();
            assert_eq!(merges, [format!("cpd tui action merge 'acme/repo##20' --client '{CLIENT}' 2>/dev/null")]);
        })
        // The menu of #9: Merge is shown with the spinner and cannot be selected; the stale one's menu is as ever
        .at("the menu of #9", |s| menu_labels(&s.shown_rows()).len() == 2, |screen| {
            let labels = menu_labels(&screen.shown_rows());
            assert!(labels[0].contains("Merge") && palette::SPINNER.iter().any(|f| labels[0].contains(f)), "Merge with the spinner: {labels:?}");
            assert_eq!(labels[1], "Copy Link", "{labels:?}");
        })
        // Copy Link's Down and Enter is the positive signal that the Enter on Merge was handled before it
        .at("the menu closed", |s| the_menu_is_closed(&s.shown_rows()), move |_| {
            assert_eq!(copies.copies().len(), 1, "the copy: {:?}", copies.copies());
            assert_eq!(popups.merges().len(), 1, "Enter on the busy Merge started no merge: {:?}", popups.merges());
        })
        .at("the menu of #20", |s| !menu_labels(&s.shown_rows()).is_empty(), |screen| {
            assert_eq!(menu_labels(&screen.shown_rows()), ["Merge", "Copy Link", "Go to Lead"], "the stale marker changes nothing");
        })
        // The merge ends (the action removes its marker): the icon is back, and its click starts the merge
        .at("the menu closed again", |s| the_menu_is_closed(&s.shown_rows()), {
            let (home, clock) = (home.clone(), clock.clone());
            move |_| {
                std::fs::remove_file(crate::tui_merge::markers_at(&home).join("acme__repo__9.json")).unwrap();
                clock.advance(3 * ONE_SECOND);
            }
        })
        .at("the icon of #9 back", |s| has_icon(&s.shown_rows(), " acme/repo#9") && !spinning(&s.shown_rows(), " acme/repo#9"), |_| {})
        .next("the click on the icon of #9", move |_| {
            let merges = sees_two.merges();
            assert_eq!(merges.len(), 2, "{merges:?}");
            assert_eq!(merges[1], format!("cpd tui action merge 'acme/repo##9' --client '{CLIENT}' 2>/dev/null"));
        });
    let signals = vec![
        Signal::Due,
        click(WIDTH - 1, nine + 1),
        click(WIDTH - 1, twenty + 1),
        rclick(3, nine),
        key(KeyCode::Enter),
        key(KeyCode::Down),
        key(KeyCode::Enter),
        rclick(3, twenty),
        key(KeyCode::Esc),
        Signal::Due,
        click(WIDTH - 1, nine + 1),
        gone(),
    ];
    show(&fx, signals, &steps);
}

#[test]
fn a_view_started_while_a_merge_runs_shows_its_spinner_at_once_on_the_owned_row_too() {
    let fx = view_fx();
    clean_prs(&fx.env);
    crate::tui_merge::put_marker(&fx.env, "acme__repo__20.json", crate::tui_merge::live_pid(&fx));
    cockpit(&fx);
    client(&fx);
    let seen = Seen::of(&fx);
    let before = seen.moved_events();
    let first = first_rows(&fx);
    let (twenty, nine) = (first_line_of(&first, " acme/repo#20"), first_line_of(&first, " acme/repo#9"));
    let steps = Steps::new().at("the spinner of #20", |s| spinning(&s.shown_rows(), " acme/repo#20"), |screen| {
        assert!(has_icon(&screen.shown_rows(), " acme/repo#9"), "only #20 merges: {:?}", screen.shown_rows());
    });
    // the owned row's click on the spinner starts no merge and opens no Lead; the redraws that follow add none
    show(&fx, vec![click(WIDTH - 1, twenty + 1), click(WIDTH - 1, nine + 1), Signal::Commit, Signal::Due, gone()], &steps);
    let merges = seen.merges();
    assert_eq!(merges, [format!("cpd tui action merge 'acme/repo##9' --client '{CLIENT}' 2>/dev/null")]);
    assert_eq!(seen.moved_events(), before, "no `session open` ran");
}

// ---------- the commands ----------

#[test]
fn the_views_need_a_terminal() {
    let fx = Fx::with_ports();
    for view in ["widgets", "legend"] {
        let run = fx.cpd_without_a_terminal(&["tui", "view", view]);
        assert_eq!(run.exit, 3, "{view}: {run:?}");
        let error = run.error();
        assert_eq!(error["code"], "not_a_terminal", "{view}");
        assert_eq!(error["details"]["command"], format!("tui view {view}"), "{view}");
    }
}

/// The columns (by character) of the menu box's `╭` and `╮` on `screen`.
fn menu_columns(screen: &[String]) -> (usize, usize) {
    let top = screen.iter().find(|r| r.contains('╮')).unwrap_or_else(|| panic!("no menu: {screen:#?}"));
    let chars: Vec<char> = top.chars().collect();
    let right = chars.iter().position(|&c| c == '╮').unwrap();
    (chars[..right].iter().rposition(|&c| c == '╭').unwrap(), right)
}

#[test]
fn the_deploy_columns_menu_keeps_one_blank_column_on_each_side_of_the_pane() {
    let fx = view_fx();
    clean_prs(&fx.env);
    let width = 120;
    // the row of `acme/web#4` in the 120-column pane: the first frame shows it there
    let blocked = first_line_of(&rows(&render(width, 40, &read::widgets(&mut open(&fx.env), 3, 0).unwrap())), " acme/web#4");
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), move |screen| assert!(screen.shown_row(blocked).starts_with(" acme/web#4"), "{}", screen.text()))
        // A right click in the pane's last column: the box ends one column before the edge, which stays blank.
        .at("the menu", |s| !menu_labels(&s.shown_rows()).is_empty(), move |screen| {
            let open = screen.shown_rows();
            let (_, right) = menu_columns(&open);
            assert_eq!(right, usize::from(width) - 2, "the box's right wall is one column from the pane's edge: {open:#?}");
        })
        .at("the menu closed", |s| the_menu_is_closed(&s.shown_rows()), |_| {})
        // A right click in column 0: the box starts in column 1.
        .at("the menu again", |s| !menu_labels(&s.shown_rows()).is_empty(), |screen| {
            let open = screen.shown_rows();
            let (left, _) = menu_columns(&open);
            assert_eq!(left, 1, "the box's left wall is one column from the pane's edge: {open:#?}");
        });
    let signals = vec![input(common::screen::right_click(width - 1, blocked)), input(common::screen::key(KeyCode::Esc)), input(common::screen::right_click(0, blocked)), gone()];
    show(&fx, signals, &steps);
}

#[test]
fn a_click_on_a_pr_rows_cue_runs_open_with_that_rows_url_and_nothing_else() {
    let fx = view_fx();
    fx.fake("open", Reply::ok(""));
    prs_store(&fx.env);
    let home = fx.env.cpd_home();
    let steps = Steps::new()
        .at("the PRs", |s| s.contains("acme/repo#20"), |_| {})
        .next("the click on the cue", move |s| {
            assert_eq!(widgets_log(&home), "", "no failure logged");
            assert!(s.shown_rows().iter().any(|r| r.contains("acme/repo#20")), "the column is as before");
        });
    show(&fx, vec![input(left_click(119, 6)), gone()], &steps);
    let opened: Vec<_> = fx.calls("open").into_iter().map(|c| c.args).collect();
    assert_eq!(opened, [["https://github.com/acme/web/pull/4"]]);
}
