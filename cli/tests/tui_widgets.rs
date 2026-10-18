//! The `widgets` view (commands.md §13.2, §17): snapshot tests of the Dispatch
//! column and the key legend with ratatui's `TestBackend`, on a store prepared
//! through the store API (as `tui_read.rs` does, since Plans and Sessions belong to
//! their areas), and the palette the two share.

mod common;

use common::TestEnv;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use cpd_tui::palette::{self, Meaning};
use cpd_tui::read::{self, Widgets};
use cpd_tui::widgets::{self, Machine, legend};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;

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

fn fixture() -> (TestEnv, Store) {
    let env = TestEnv::new();
    let mut store = open(&env);
    populate(&mut store);
    (env, store)
}

fn machine() -> Machine {
    Machine { name: "homeserver".to_owned(), load: ["3.70", "3.05", "4.84"].map(str::to_owned).to_vec() }
}

fn render(width: u16, height: u16, w: &Widgets) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| widgets::draw(f, f.area(), w, &machine())).unwrap();
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
    let env = TestEnv::new();
    let w = read::widgets(&mut open(&env), 3, 0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(30, 14)).unwrap();
    let machine = Machine { name: "homeserver".to_owned(), load: ["0.1", "0.2", "0.3"].map(str::to_owned).to_vec() };
    terminal.draw(|f| widgets::draw(f, f.area(), &w, &machine)).unwrap();
    let lines = rows(terminal.backend().buffer());
    let rule = "─".repeat(30);
    assert_eq!(lines[..3], [" homeserver", " load 0.1 0.2 0.3", rule.as_str()]);
    assert_eq!(lines[3], " PRs");
    assert_eq!(lines[4], " no open PRs");
    assert_eq!(lines[5..], ["", "", "", rule.as_str(), " git", " (future)", rule.as_str(), " Jira", " (future)"]);
}

#[test]
fn the_column_shows_the_machine_the_open_prs_git_and_jira() {
    let (got, _) = column(25);
    let expected = [
        " homeserver",
        " load 3.70 3.05 4.84",
        RULE,
        " PRs",
        " acme/repo#7",
        "  open ✗ changes requested  cpd-tui cockpit c…",
        " acme/repo#11",
        "  open – no review  Plan fresh",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        RULE,
        " git",
        " (future)",
        RULE,
        " Jira",
        " (future)",
    ];
    assert_eq!(got, expected, "\n{}", got.join("\n"));
}

#[test]
fn three_open_prs_in_two_repos_come_out_oldest_first_in_two_lines_each() {
    let env = TestEnv::new();
    let mut store = prs_store(&env);
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let listed: Vec<(&str, i64)> = w.prs.iter().map(|p| (p.repo.as_str(), p.number)).collect();
    assert_eq!(listed, [("acme/web", 4), ("acme/repo", 9), ("acme/repo", 20)]);
    let got = rows(&render(46, 25, &w));
    assert_eq!(
        got[3..10],
        [
            " PRs",
            " acme/web#4",
            "  draft ✗ changes requested  Older web fix",
            " acme/repo#9",
            "  open ✓ approved  Middle one",
            " acme/repo#20",
            "  open … no review  Mine to review",
        ],
        "{got:#?}"
    );
    assert_eq!(got[10], "", "nothing else is listed");
}

#[test]
fn a_draft_shows_draft_and_the_others_open() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    assert_eq!(w.prs.iter().map(|p| p.draft).collect::<Vec<_>>(), [true, false, false]);
    let got = rows(&render(46, 25, &w)).join("\n");
    assert_eq!(got.matches("draft").count(), 1, "{got}");
}

#[test]
fn the_pr_of_the_plan_in_the_chat_slot_is_reversed_and_bold_and_the_others_are_not() {
    let env = TestEnv::new();
    let mut store = prs_store(&env);
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(w.prs.iter().map(|p| p.active).collect::<Vec<_>>(), [false, false, true]);
    let buf = render(46, 25, &w);
    let style = |needle: &str| {
        let (x, y) = find(&buf, needle);
        let m = buf[(x, y)].modifier;
        (m.contains(Modifier::REVERSED), m.contains(Modifier::BOLD))
    };
    // Both lines of the active one: reversed and bold.
    assert_eq!(style("acme/repo#20"), (true, true));
    assert_eq!(style("Mine to review"), (true, true));
    assert_eq!(style("no review"), (true, true));
    // The others: the name bold, the second line neither reversed nor bold.
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
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    let got = rows(&render(46, 25, &w)).join("\n");
    for hidden in ["acme/web#2", "Closed web", "acme/api#1", "Merged api"] {
        assert!(!got.contains(hidden), "{hidden}:\n{got}");
    }
    // The fixture's merged ones (a Plan's own, `old` #3 and `away` #9) are not listed either.
    let (got, _) = column(25);
    let got = got.join("\n");
    assert!(!got.contains("#3") && !got.contains("#9"), "{got}");
}

#[test]
fn a_plan_without_a_pr_is_not_listed_and_no_label_is_drawn() {
    let (got, _) = column(25);
    let got = got.join("\n");
    for hidden in ["v1", "sketch lead", "never lead", "no PR", "cpd-ui", "cpd visual lead", "fresh lead", "Plans in the cockpit"] {
        assert!(!got.contains(hidden), "{hidden}:\n{got}");
    }
}

#[test]
fn no_open_pull_request_says_so() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let mut s = context("alpha") + &lead_row("ship", "v1");
    s += &plan_row("ship", "dispatch", "2026-01-02T00:00:00.000Z", None);
    s += &pull_request("acme/web", 2, "Closed web", false, "closed", "none", "none", "2026-01-01T00:00:00.000Z");
    sql(&mut store, &s);
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert!(w.prs.is_empty());
    let buf = render(46, 25, &w);
    let got = rows(&buf);
    assert_eq!(got[3..5], [" PRs", " no open PRs"], "{got:#?}");
    assert!(buf[find(&buf, "no open PRs")].modifier.contains(Modifier::DIM));
}

#[test]
fn the_column_has_no_engine_slots_escalations_events_or_hint() {
    let (got, _) = column(25);
    let all = got.join("\n");
    for gone in ["Engine", "Slots", "Escalations", "Events", "legend", "quit"] {
        assert!(!all.contains(gone), "{gone}:\n{all}");
    }
}

#[test]
fn the_column_takes_every_colour_from_the_palette() {
    let (_, buf) = column(25);
    let fg = |at: (u16, u16)| buf[at].fg;
    let c = palette::color;
    // The PRs: state and checks.
    assert_eq!(fg(find(&buf, "open")), c(Meaning::Review));
    assert_eq!(fg(find(&buf, "✗")), c(Meaning::Failed));
    assert_eq!(fg(find(&buf, "–")), c(Meaning::Nothing));
    assert_eq!(fg(find(&buf, RULE)), c(Meaning::Nothing));
    let dim = |at: (u16, u16)| buf[at].modifier.contains(Modifier::DIM);
    let bold = |at: (u16, u16)| buf[at].modifier.contains(Modifier::BOLD);
    // Bold: the machine, the section titles and the pull request names; dim: the title of the PR, `(future)`.
    for title in ["homeserver", "PRs", "acme/repo#7", "git", "Jira"] {
        assert!(bold(find(&buf, title)), "{title}");
        assert!(!dim(find(&buf, title)), "{title}");
    }
    for faint in ["cpd-tui cockpit", "Plan fresh", "(future)"] {
        assert!(dim(find(&buf, faint)), "{faint}");
        assert!(!bold(find(&buf, faint)), "{faint}");
    }
    // Plain: the review decision and the load.
    for plain in ["no review", "changes requested", "load 3.70"] {
        let at = find(&buf, plain);
        assert!(!dim(at) && !bold(at), "{plain}");
    }
}

#[test]
fn a_long_title_is_cut_with_an_ellipsis_first_and_a_line_never_wraps() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let wide = rows(&render(30, 25, &w));
    assert!(wide.contains(&"  open ✗ changes requested".to_owned()), "no room for a title of 4 characters:\n{wide:#?}");
    let got = rows(&render(36, 25, &w));
    assert!(got.contains(&"  open ✗ changes requested  cpd-tui…".to_owned()), "{got:#?}");
    let got = rows(&render(24, 25, &w));
    assert!(got.iter().all(|r| r.chars().count() <= 24), "{got:#?}");
    assert!(got.contains(&"  open ✗ changes request".to_owned()), "the pane clips what is left:\n{got:#?}");
    let narrow = rows(&render(8, 25, &w));
    assert!(narrow.contains(&" acme/r…".to_owned()), "the name is cut when it must be:\n{narrow:#?}");
}

#[test]
fn a_short_pane_drops_the_pull_requests_that_do_not_fit_from_the_end() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    // Beside the 9 rows of the other sections and the PRs title, each pull request takes 2.
    for (height, listed) in [(9 + 1 + 6, 3), (9 + 1 + 5, 2), (9 + 1 + 2, 1), (9 + 1 + 1, 0)] {
        let got = rows(&render(46, height, &w)).join("\n");
        let shown = ["acme/web#4", "acme/repo#9", "acme/repo#20"].iter().filter(|n| got.contains(**n)).count();
        assert_eq!(shown, listed, "height {height}:\n{got}");
        assert!(listed == 0 || got.contains("acme/web#4"), "the oldest stay:\n{got}");
    }
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
        ["display-popup", "-E", "-T", " legend ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd tui view legend"]
    );
    let small = legend::popup_args((40, 20));
    assert_eq!((small[5].as_str(), small[7].as_str()), ("40", "20"));
}

// ---------- the commands ----------

#[test]
fn the_views_need_a_terminal() {
    let env = TestEnv::new();
    for view in ["widgets", "legend"] {
        let run = env.cpd(&["tui", "view", view]);
        assert_eq!(run.exit, 3, "{view}: {run:?}");
        let error = run.error();
        assert_eq!(error["code"], "not_a_terminal", "{view}");
        assert_eq!(error["details"]["command"], format!("tui view {view}"), "{view}");
    }
}
