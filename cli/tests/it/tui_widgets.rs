//! The `widgets` view (commands.md §13.2, §17): snapshot tests of the Dispatch
//! column and the key legend with ratatui's `TestBackend`, on a store prepared
//! through the store API (as `tui_read.rs` does, since Plans and Sessions belong to
//! their areas), and the palette the two share.

use crate::common;


use common::{CPD, SLEEP_SECONDS, TestEnv, sql, wait_until};
use cpd_core::home::CpdHome;
use cpd_core::host::tmux::PANE_DIED_HOOK;
use cpd_core::store::Store;
use cpd_tui::palette::{self, Meaning};
use cpd_tui::read::{self, Dot, Widgets};
use cpd_tui::widgets::{self, Hit, Machine, legend};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

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
    render_hits(width, height, w, "").0
}

/// The column, and where a click reaches an owned PR; `note` is the last row's message.
fn render_hits(width: u16, height: u16, w: &Widgets, note: &str) -> (Buffer, Vec<Hit>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut hits = vec![];
    terminal.draw(|f| hits = widgets::draw(f, f.area(), &w.prs, &machine(), note, 0, 0).hits).unwrap();
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

/// A PR's first line on a pane of `width` columns: its name, then `󰏌` in the last column.
fn first_line(name: &str, width: usize) -> String {
    format!("{name:<w$}󰏌", w = width - 1)
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

/// A section header on the 46-column test pane: `──`, ` <title> `, then `─` to the edge.
fn header(title: &str) -> String {
    format!("── {title} {}", "─".repeat(46 - 4 - title.chars().count()))
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
    let env = TestEnv::new();
    let w = read::widgets(&mut open(&env), 3, 0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(30, 20)).unwrap();
    let machine = Machine { name: "homeserver".to_owned(), load: ["0.1", "0.2", "0.3"].map(str::to_owned).to_vec() };
    terminal.draw(|f| drop(widgets::draw(f, f.area(), &w.prs, &machine, "", 0, 0))).unwrap();
    let lines = rows(terminal.backend().buffer());
    let head = |title: &str| format!("── {title} {}", "─".repeat(30 - 4 - title.chars().count()));
    assert_eq!(lines[..3], ["", " homeserver", " load 0.1 0.2 0.3"]);
    assert_eq!(lines[3..6], ["", head("PRs").as_str(), ""]);
    assert_eq!(lines[6], " no open PRs");
    assert_eq!(lines[7..], ["", "", "", "", "", "", head("git").as_str(), "", " (future)", "", head("Jira").as_str(), "", " (future)"]);
}

#[test]
fn the_column_shows_the_machine_the_open_prs_git_and_jira() {
    let (got, _) = column(25);
    let (prs, git, jira) = (header("PRs"), header("git"), header("Jira"));
    let (seven, eleven) = (first_line(" acme/repo#7", 46), first_line(" acme/repo#11", 46));
    let failed_open = format!("  open {} changes requested  cpd-tui cockpit c…", palette::CHECKS_FAILED);
    let none_open = format!("  open {} no review  Plan fresh", palette::CHECKS_NONE);
    let expected = [
        "",
        " homeserver",
        " load 3.70 3.05 4.84",
        "",
        prs.as_str(),
        "",
        seven.as_str(),
        failed_open.as_str(),
        eleven.as_str(),
        none_open.as_str(),
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        git.as_str(),
        "",
        " (future)",
        "",
        jira.as_str(),
        "",
        " (future)",
    ];
    assert_eq!(got, expected, "\n{}", got.join("\n"));
}

#[test]
fn each_section_header_has_a_blank_row_above_and_below() {
    for height in [20, 25, 29] {
        let (got, _) = column(height);
        for title in ["PRs", "git", "Jira"] {
            let at = got.iter().position(|r| *r == header(title)).unwrap_or_else(|| panic!("{title} at {height}:\n{got:#?}"));
            assert_eq!(got[at - 1], "", "above {title} at {height}:\n{got:#?}");
            assert_eq!(got[at + 1], "", "below {title} at {height}:\n{got:#?}");
        }
    }
}

#[test]
fn three_open_prs_in_two_repos_come_out_oldest_first_in_two_lines_each() {
    let env = TestEnv::new();
    let mut store = prs_store(&env);
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
            header("PRs").as_str(),
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
    let env = TestEnv::new();
    let mut store = prs_store(&env);
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
fn an_open_review_request_is_not_listed_but_one_that_is_also_authored_is() {
    let env = TestEnv::new();
    let mut store = prs_store(&env);
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
    assert_eq!(got[4..7], [header("PRs").as_str(), "", " no open PRs"], "{got:#?}");
    assert!(buf[find(&buf, "no open PRs")].modifier.contains(Modifier::DIM));
}

#[test]
fn the_column_has_no_engine_slots_escalations_events_or_hint() {
    let (got, _) = column(29);
    let all = got.join("\n");
    for gone in ["Engine", "Slots", "Escalations", "Events", "legend", "quit"] {
        assert!(!all.contains(gone), "{gone}:\n{all}");
    }
}

/// `acme/web#4` (failed checks) is owned by no Plan, `acme/repo#20` (pending) by `mine`: only the unowned one is red.
#[test]
fn failed_checks_are_red_on_an_unowned_pull_request() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    let buf = render(46, 25, &w);
    assert_eq!(buf[find(&buf, palette::CHECKS_FAILED)].fg, palette::color(Meaning::Failed));
}

#[test]
fn the_column_takes_every_colour_from_the_palette() {
    let (_, buf) = column(29);
    let fg = |at: (u16, u16)| buf[at].fg;
    let c = palette::color;
    // The PRs: state and checks.
    assert_eq!(fg(find(&buf, "open")), c(Meaning::Review));
    assert_eq!(fg(find(&buf, palette::CHECKS_FAILED)), c(Meaning::Waiting), "`acme/repo#7` is a Plan's: its Lead fixes failed checks (hs-d302)");
    assert_eq!(fg(find(&buf, palette::CHECKS_NONE)), c(Meaning::Nothing));
    // The headers: grey, the title a reversed block, not bold.
    for title in ["PRs", "git", "Jira"] {
        let (x, y) = find(&buf, &format!("── {title} "));
        assert_eq!(buf[(x, y)].fg, c(Meaning::Nothing), "{title}");
        let block = buf[(x + 3, y)].clone();
        assert_eq!(block.fg, c(Meaning::Nothing), "{title}");
        assert!(block.modifier.contains(Modifier::REVERSED) && !block.modifier.contains(Modifier::BOLD), "{title}");
        assert!(!buf[(x, y)].modifier.contains(Modifier::REVERSED), "{title}");
    }
    let dim = |at: (u16, u16)| buf[at].modifier.contains(Modifier::DIM);
    let bold = |at: (u16, u16)| buf[at].modifier.contains(Modifier::BOLD);
    // Bold: the machine and the pull request names (the section titles are reversed, not bold); dim: the title of the PR, `(future)`.
    for title in ["homeserver", "acme/repo#7"] {
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
    assert!(wide.contains(&format!("  open {} changes requested", palette::CHECKS_FAILED)), "no room for a title of 4 characters:\n{wide:#?}");
    let got = rows(&render(36, 25, &w));
    assert!(got.contains(&format!("  open {} changes requested  cpd-tui…", palette::CHECKS_FAILED)), "{got:#?}");
    let got = rows(&render(24, 25, &w));
    assert!(got.iter().all(|r| r.chars().count() <= 24), "{got:#?}");
    assert!(got.contains(&format!("  open {} changes request", palette::CHECKS_FAILED)), "the pane clips what is left:\n{got:#?}");
    let narrow = rows(&render(8, 25, &w));
    assert!(narrow.contains(&" acme… 󰏌".to_owned()), "the name is cut before the arrow when it must be:\n{narrow:#?}");
}

#[test]
fn a_short_pane_hides_the_pull_requests_that_do_not_fit_beside_the_cue_below() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    // Beside the 14 rows of the blank row, the machine, the headers and their blank rows, each pull request takes 2, and
    // when some are hidden the `▼` row takes one more.
    for (height, listed) in [(14 + 6, 3), (14 + 5, 2), (14 + 4, 1), (14 + 3, 1), (14 + 2, 0), (14 + 1, 0)] {
        let got = rows(&render(46, height, &w)).join("\n");
        let shown = ["acme/web#4", "acme/repo#9", "acme/repo#20"].iter().filter(|n| got.contains(**n)).count();
        assert_eq!(shown, listed, "height {height}:\n{got}");
        assert!(listed == 0 || got.contains("acme/web#4"), "the oldest stay:\n{got}");
        assert_eq!(got.contains(&format!("▼ {} more", 3 - listed)), listed < 3, "height {height}:\n{got}");
    }
}

// ---------- the 󰏌 links and the owned rows ----------

/// The cell of a linked `󰏌`: its symbol is wrapped in the OSC 8 escapes of its link.
fn linked_arrow(url: &str) -> String {
    format!("\x1b]8;;{url}\x07󰏌\x1b]8;;\x07")
}

#[test]
fn every_pr_row_ends_its_first_line_in_an_arrow_linking_to_the_pull_request() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    let buf = render(46, 25, &w);
    for (y, number, repo) in [(6, 4, "acme/web"), (8, 9, "acme/repo"), (10, 20, "acme/repo")] {
        assert_eq!(buf[(45, y)].symbol(), linked_arrow(&format!("https://github.com/{repo}/pull/{number}")), "row {y}");
        assert_eq!(plain(buf[(45, y)].symbol()), "󰏌", "row {y}");
        assert_eq!(row(&buf, y).chars().count(), 46, "the arrow is the last column of row {y}");
        assert_eq!(plain(buf[(45, y + 1)].symbol()), " ", "the second line has no arrow");
    }
}

#[test]
fn an_unowned_pr_is_dim_on_both_lines_with_its_arrow_and_an_owned_one_is_not() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    assert_eq!(
        w.prs.iter().map(|p| p.lead.as_ref().map(|l| l.as_str())).collect::<Vec<_>>(),
        [None, None, Some("l-mine")],
        "`mine` owns acme/repo#20; no Plan owns the other two"
    );
    let buf = render(46, 25, &w);
    let dim = |x: u16, y: u16| buf[(x, y)].modifier.contains(Modifier::DIM);
    for y in [6, 8] {
        assert!([1, 2, 12, 45].iter().all(|x| dim(*x, y)), "row {y} is dim: the name and the arrow");
        assert!([2, 3, 12, 20].iter().all(|x| dim(*x, y + 1)), "row {} is dim: the second line", y + 1);
    }
    assert_eq!(plain(buf[(45, 6)].symbol()), "󰏌", "the arrow stays on an unowned PR");
    // The owned one (here also the active one) is not dim.
    assert!([1, 2, 12, 45].iter().all(|x| !dim(*x, 10)) && [2, 20].iter().all(|x| !dim(*x, 11)));
}

/// The column drawn at spinner frame `frame`.
fn render_at(width: u16, height: u16, w: &Widgets, frame: usize) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| drop(widgets::draw(f, f.area(), &w.prs, &machine(), "", 0, frame))).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn a_pr_whose_lead_works_shows_the_spinner_beside_its_state_and_the_frame_advances_with_the_clock() {
    let env = TestEnv::new();
    let mut store = prs_store(&env);
    // `mine` owns acme/repo#20 (third row pair); `acme/web#4` and `acme/repo#9` have no owner.
    let second = |w: &Widgets, frame: usize| row(&render_at(46, 25, w, frame), 11).trim_end().to_owned();
    let idle = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(second(&idle, 0), "  open 󰦖 no review  Mine to review", "an idle Lead: no dot");
    sql(&mut store, "UPDATE sessions SET state = 'working' WHERE id = 'l-mine'");
    let working = read::widgets(&mut store, 3, 0).unwrap();
    for frame in 0..palette::SPINNER.len() {
        assert_eq!(second(&working, frame), format!("  open {} 󰦖 no review  Mine to review", palette::SPINNER[frame]), "frame {frame}");
    }
    // the unowned rows show none, whatever works
    let buf = render_at(46, 25, &working, 3);
    for y in [7, 9] {
        assert!(!palette::SPINNER.iter().any(|g| row(&buf, y).contains(g)), "row {y}: {}", row(&buf, y));
    }
}

#[test]
fn only_an_owned_pr_has_a_hit_area_its_two_rows_except_the_arrow() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    let (_, hits) = render_hits(46, 25, &w, "");
    let [hit] = &hits[..] else { panic!("{hits:?}") };
    assert_eq!(hit.lead.as_str(), "l-mine");
    let inside = |x, y| hit.contains(ratatui::layout::Position::new(x, y));
    assert!(inside(0, 10) && inside(44, 10) && inside(0, 11) && inside(45, 11));
    assert!(!inside(45, 10), "the arrow cell is the terminal's");
    assert!(!inside(0, 9) && !inside(0, 12), "the unowned rows above and the blank row below");
}

#[test]
fn a_note_is_the_last_row_in_the_note_colour() {
    let env = TestEnv::new();
    let w = read::widgets(&mut prs_store(&env), 3, 0).unwrap();
    let (buf, _) = render_hits(46, 25, &w, "no cockpit");
    assert_eq!(row(&buf, 24).trim_end(), "no cockpit");
    assert_eq!(buf[(0, 24)].fg, palette::note().fg.unwrap());
    assert_eq!(row(&buf, 23).trim_end(), "", "the rest is as without it");
}

// ---------- the pane: links and clicks, with the view in a pane of a private tmux server ----------

/// An env whose config has the private tmux server, as the tree's tests have.
fn pane_env() -> TestEnv {
    let env = TestEnv::new();
    let config = format!(
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    std::fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// `cpd tui view widgets` in session `name`, window `main`, 120 columns by 40 rows; returns its pane.
fn start(env: &TestEnv, name: &str) -> String {
    tmux_out(env, &["new-session", "-d", "-s", name, "-n", "main", "-x", "120", "-y", "40", "-P", "-F", "#{pane_id}", &format!("{CPD} tui view widgets")])
}

/// The cockpit (session `cpd`, the config's default): the view, the chat placeholder beside it, and the Lead `l-mine` alone in
/// a park window, as `session open` finds it (`@cpd_session`); its row records that pane. Returns the view's pane.
fn cockpit(env: &TestEnv) -> String {
    let view = start(env, "cpd");
    tmux_out(env, &["set", "-p", "-t", &view, "@cpd_view", "widgets"]);
    let chat = tmux_out(env, &["split-window", "-h", "-d", "-t", &view, "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    tmux_out(env, &["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    let parked =
        tmux_out(env, &["new-window", "-d", "-t", "=cpd:", "-n", "park-mine", "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    tmux_out(env, &["set", "-p", "-t", &parked, "@cpd_session", "l-mine"]);
    sql(&mut open(env), &format!("UPDATE sessions SET tmux_pane = '{parked}' WHERE id = 'l-mine';"));
    view
}

/// The pane's rows, each without trailing blanks; `capture-pane` leaves out the empty rows at the end.
fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    let out = env.tmux(&["capture-pane", "-p", "-t", pane]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().lines().map(|l| l.trim_end().to_owned()).collect()
}

fn wait_for_screen(env: &TestEnv, pane: &str, what: &str, ok: impl Fn(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), ok)
}

/// A left click at 0-based `column` and `row`, as the terminal sends it (SGR mouse reporting, 1-based).
fn click(env: &TestEnv, pane: &str, column: u16, row: u16) {
    for end in ['M', 'm'] {
        tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<0;{};{}{end}", column + 1, row + 1)]);
    }
}

fn moved_events(env: &TestEnv) -> Vec<(String, String)> {
    open(env)
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT session, json_extract(data, '$.place') FROM events WHERE kind = 'session-moved' ORDER BY id")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
}

/// The pane of the cockpit's `main` window that has the focus: its `@cpd_session` (empty for the view and the placeholder).
fn focused_session(env: &TestEnv) -> String {
    let panes = tmux_out(env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_active}|#{@cpd_session}|#{@cpd_view}"]);
    let active: Vec<&str> = panes.lines().filter(|l| l.starts_with("1|")).collect();
    assert_eq!(active.len(), 1, "{panes}");
    active[0].to_owned()
}

#[test]
fn in_a_pane_each_arrow_is_a_link_at_the_right_edge_and_the_view_captures_the_mouse() {
    let env = pane_env();
    prs_store(&env);
    let pane = start(&env, "view");
    let screen = wait_for_screen(&env, &pane, "the PRs", |s| s.iter().any(|r| r.contains("acme/repo#20")));
    let width = tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_width}"]).parse::<usize>().unwrap();
    for name in [" acme/web#4", " acme/repo#9", " acme/repo#20"] {
        let line = screen.iter().find(|r| r.starts_with(name)).unwrap_or_else(|| panic!("{name}: {screen:#?}"));
        assert_eq!(line.chars().count(), width, "the arrow ends {name}'s line at the right edge: {line:?}");
        assert!(line.ends_with('󰏌'), "{line:?}");
    }
    let styled = tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]);
    for url in ["https://github.com/acme/web/pull/4", "https://github.com/acme/repo/pull/9", "https://github.com/acme/repo/pull/20"] {
        assert!(styled.lines().any(|l| l.contains(&format!("]8;;{url}\x1b\\󰏌"))), "{url}: {styled:?}");
    }
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", &pane, "#{mouse_any_flag}"]), "1", "mouse capture is on");
}

/// `q` does nothing in the column: with the cockpit's `pane-died` hook a view that exits is started again, so a `q` that
/// ended the program would be a restart (tui-W1).
#[test]
fn q_does_not_end_the_view() {
    let env = pane_env();
    prs_store(&env);
    let pane = start(&env, "view");
    tmux_out(&env, &["set", "-p", "-t", &pane, "@cpd_view", "widgets"]);
    tmux_out(&env, &["set", "-g", "remain-on-exit", "on"]);
    tmux_out(&env, &["set-hook", "-g", "pane-died", PANE_DIED_HOOK]);
    wait_for_screen(&env, &pane, "the PRs", |s| s.iter().any(|r| r.contains("acme/repo#20")));
    let pid = tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_pid}"]);
    tmux_out(&env, &["send-keys", "-t", &pane, "q"]);
    // time-claim: for 2 s after the `q`, the program has not ended (the same pid, not dead).
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_pid} #{pane_dead}"]), format!("{pid} 0"), "the program ended");
    assert!(screen(&env, &pane).iter().any(|r| r.contains("acme/repo#20")));
}

/// A failed read keeps the column on screen and shows the error in its last row; the next refresh reads again, and the
/// error goes when a read succeeds (tui-W4). The read is made to fail by renaming the table it reads.
#[test]
fn a_failed_read_keeps_the_column_and_shows_the_error_until_a_read_succeeds() {
    let env = pane_env();
    prs_store(&env);
    let pane = start(&env, "view");
    wait_for_screen(&env, &pane, "the PRs", |s| s.iter().any(|r| r.contains("acme/repo#20")));
    // not through `Store`, which refuses a changed schema
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("ALTER TABLE pull_requests RENAME TO pull_requests_gone;");
    let screen = wait_for_screen(&env, &pane, "the error in the last row", |s| s.iter().any(|r| r.contains("no such table")));
    assert!(screen.iter().any(|r| r.contains("acme/repo#20")), "the column stays: {screen:?}");
    raw("ALTER TABLE pull_requests_gone RENAME TO pull_requests;");
    wait_for_screen(&env, &pane, "the error to go", |s| {
        !s.iter().any(|r| r.contains("no such table")) && s.iter().any(|r| r.contains("acme/repo#20"))
    });
}

/// A follow that fails is shown in the column's last row, the column stays (tui-G4): the highest Event id fell below
/// the cursor, which is what a replaced database looks like.
#[test]
fn a_follow_that_fails_is_shown_and_the_column_stays() {
    let env = pane_env();
    prs_store(&env);
    let pane = start(&env, "view");
    wait_for_screen(&env, &pane, "the PRs", |s| s.iter().any(|r| r.contains("acme/repo#20")));
    // not through `Store`, which keeps the Events append-only
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("DROP TRIGGER events_no_delete; DELETE FROM events;");
    let screen = wait_for_screen(&env, &pane, "the error in the last row", |s| s.iter().any(|r| r.contains("Event cursor")));
    assert!(screen.iter().any(|r| r.contains("acme/repo#20")), "the column stays: {screen:?}");
}

#[test]
fn a_click_on_an_owned_row_opens_its_lead_and_focuses_the_chat_and_the_others_run_nothing() {
    let env = pane_env();
    let mut store = prs_store(&env);
    sql(&mut store, &moved("2026-01-04T00:00:00.000Z", "mine", "parked"));
    drop(store);
    let view = cockpit(&env);
    wait_for_screen(&env, &view, "the PRs", |s| s.iter().any(|r| r.contains("acme/repo#20")));
    let width = tmux_out(&env, &["display", "-p", "-t", &view, "#{pane_width}"]).parse::<u16>().unwrap();
    assert_eq!(focused_session(&env), "1||widgets", "the view has the focus");
    let before = moved_events(&env);

    // The unowned rows, and the owned row's arrow (the terminal's): nothing runs.
    click(&env, &view, 3, 6);
    click(&env, &view, 3, 9);
    click(&env, &view, width - 1, 10);
    // The owned row, on its second line: `session open l-mine`, then the chat slot has the focus.
    click(&env, &view, 3, 11);
    wait_until("the Lead in the chat slot", || moved_events(&env).len() > before.len());
    let mut expected = before;
    expected.push(("l-mine".to_owned(), "chat".to_owned()));
    assert_eq!(moved_events(&env), expected, "one `session open`, for the owned PR's Lead");
    wait_until("the focus", || focused_session(&env).contains("|l-mine|"));
    let chat = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_session}|#{pane_id}"]);
    assert!(chat.lines().any(|l| l.starts_with("l-mine|")), "the Lead is in `main`: {chat}");
}

/// A wheel notch (`down` or up) at 0-based `column` and `row`, as the terminal sends it (SGR mouse reporting: 65 down, 64 up).
fn wheel(env: &TestEnv, pane: &str, down: bool, column: u16, row: u16) {
    let button = if down { 65 } else { 64 };
    tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<{button};{};{}M", column + 1, row + 1)]);
}

#[test]
fn the_wheel_over_the_prs_scrolls_them_one_at_a_time_between_dim_cue_rows() {
    let env = pane_env();
    let mut store = open(&env);
    let mut s = String::new();
    for n in 1..=21 {
        s += &pull_request("acme/repo", n, "A title", false, "open", "none", "passed", &format!("2026-01-03T00:{n:02}:00.000Z"));
    }
    sql(&mut store, &s);
    drop(store);
    let pane = start(&env, "view");
    let has = |s: &[String], name: &str| s.iter().any(|r| r.starts_with(&format!(" {name} ")));
    // 40 rows less the 14 of the rest of the column leave 26 for the PRs: 12 whole ones beside the `▼` row.
    // (polling for the whole screen: a capture can catch the pane mid-draw)
    wait_for_screen(&env, &pane, "the first PRs and the cue below", |s| {
        s.iter().any(|r| r == " ▼ 9 more")
            && has(s, "acme/repo#1")
            && has(s, "acme/repo#12")
            && !has(s, "acme/repo#13")
            && !s.iter().any(|r| r.contains('▲'))
    });
    let styled = tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]);
    let cue = styled.lines().find(|l| l.contains("▼ 9 more")).unwrap();
    assert!(cue.contains("\x1b[2m"), "the cue is dim: {cue:?}");

    // A notch outside the section does nothing; one inside moves by one pull request.
    wheel(&env, &pane, true, 3, 1);
    wheel(&env, &pane, true, 3, 10);
    wait_for_screen(&env, &pane, "one pull request down", |s| {
        s.iter().any(|r| r == " ▲ 1 more") && s.iter().any(|r| r == " ▼ 8 more") && has(s, "acme/repo#2") && !has(s, "acme/repo#1")
    });

    // Past the end it stops, with the last pull request at the bottom and no cue below.
    for _ in 0..20 {
        wheel(&env, &pane, true, 3, 10);
    }
    wait_for_screen(&env, &pane, "the end", |s| {
        s.iter().any(|r| r == " ▲ 9 more")
            && has(s, "acme/repo#21")
            && has(s, "acme/repo#10")
            && !has(s, "acme/repo#9")
            && !s.iter().any(|r| r.contains('▼'))
    });

    // And back up, one at a time.
    wheel(&env, &pane, false, 3, 10);
    wait_for_screen(&env, &pane, "one back up", |s| s.iter().any(|r| r == " ▲ 8 more") && s.iter().any(|r| r == " ▼ 1 more") && has(s, "acme/repo#9") && !has(s, "acme/repo#21"));
    for _ in 0..20 {
        wheel(&env, &pane, false, 3, 10);
    }
    wait_for_screen(&env, &pane, "the top", |s| s.iter().any(|r| r == " ▼ 9 more") && has(s, "acme/repo#1") && !s.iter().any(|r| r.contains('▲')));
}

#[test]
fn a_refused_session_open_shows_its_message_in_the_last_row_until_the_next_click() {
    let env = pane_env();
    prs_store(&env);
    // No cockpit session: `session open` is refused with `no_cockpit`.
    let pane = start(&env, "view");
    wait_for_screen(&env, &pane, "the PRs", |s| s.iter().any(|r| r.contains("acme/repo#20")));
    let height = tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_height}"]).parse::<usize>().unwrap();
    let width = tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_width}"]).parse::<u16>().unwrap();
    let last_row = |s: &[String]| s.get(height - 1).cloned().unwrap_or_default();

    // The unowned rows and the arrow run nothing, so no note appears.
    click(&env, &pane, 3, 6);
    click(&env, &pane, width - 1, 10);
    // time-claim: the clicks on the unowned rows and the arrow leave no note, over a few redraws of the view; a click
    // that runs nothing leaves no signal (the view opens sessions in its own process, not through a recorded `cpd`).
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!(last_row(&screen(&env, &pane)), " (future)", "nothing was refused: Jira's row is still the last");

    click(&env, &pane, 3, 10);
    let shown = wait_for_screen(&env, &pane, "the refusal", |s| last_row(s).contains("cockpit"));
    assert!(last_row(&shown).contains("cpd tui"), "{:?}", last_row(&shown));
    let styled = tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]);
    let last = styled.lines().last().unwrap();
    assert!(last.contains("\x1b[") && last.contains("cockpit"), "{last:?}");
    assert!(shown[..height - 1].iter().any(|r| r.contains("(future)")), "the git section is as before");

    click(&env, &pane, 3, 6);
    wait_for_screen(&env, &pane, "the note gone", |s| last_row(s) == " (future)");
}

#[test]
fn the_merge_icon_is_drawn_and_hit_only_on_clean_pull_requests() {
    let env = TestEnv::new();
    let mut store = prs_store(&env);
    sql(&mut store, "UPDATE pull_requests SET merge_state = 'clean' WHERE number IN (9, 20); UPDATE pull_requests SET merge_state = 'blocked' WHERE number = 4;");
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert_eq!(w.prs.iter().map(|p| (p.number, p.clean)).collect::<Vec<_>>(), [(4, false), (9, true), (20, true)]);
    let mut terminal = Terminal::new(TestBackend::new(46, 25)).unwrap();
    let mut drawn = None;
    terminal.draw(|f| drawn = Some(widgets::draw(f, f.area(), &w.prs, &machine(), "", 0, 0))).unwrap();
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

/// The Deploy column in the cockpit with a fake `tmux` on `PATH`: `list-clients` says one client of 200x60, `display-popup`
/// does nothing (the call is recorded), and the rest is the real tmux.
fn fake_tmux(env: &TestEnv) {
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake(
        "tmux",
        &format!("case \"$*\" in\n *list-clients*) echo '100 200 60';;\n *display-popup*) ;;\n *) exec {} \"$@\";;\nesac", real.trim()),
    );
}

fn popups(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-popup")).collect()
}

#[test]
fn a_clean_pr_shows_the_merge_icon_and_a_click_on_it_opens_the_popup_not_the_lead() {
    const ICON: char = '\u{F062D}';
    let env = pane_env();
    let mut store = prs_store(&env);
    // `acme/repo#20` (owned by `mine`) and `acme/repo#9` (no owner) are clean, `acme/web#4` is blocked.
    sql(
        &mut store,
        "UPDATE pull_requests SET merge_state = 'clean' WHERE number IN (9, 20);
         UPDATE pull_requests SET merge_state = 'blocked' WHERE number = 4;",
    );
    sql(&mut store, &moved("2026-01-04T00:00:00.000Z", "mine", "parked"));
    drop(store);
    let view = cockpit(&env);
    let shown = wait_for_screen(&env, &view, "the merge icons", |s| s.iter().any(|r| r.contains(ICON)));
    let width = tmux_out(&env, &["display", "-p", "-t", &view, "#{pane_width}"]).parse::<usize>().unwrap();
    // The icon is the last column of the second line, under the arrow; only the clean ones have it.
    for (name, icon) in [(" acme/web#4", false), (" acme/repo#9", true), (" acme/repo#20", true)] {
        let first = shown.iter().position(|r| r.starts_with(name)).unwrap_or_else(|| panic!("{name}: {shown:#?}"));
        let second = &shown[first + 1];
        assert_eq!(second.ends_with(ICON), icon, "{name}: {second:?}");
        if icon {
            assert_eq!(second.chars().count(), width, "the icon is the last column: {second:?}");
        }
    }
    assert_eq!(shown.iter().filter(|r| r.contains(ICON)).count(), 2, "{shown:#?}");

    fake_tmux(&env);
    let before = moved_events(&env);
    let second_line = |name: &str| u16::try_from(shown.iter().position(|r| r.starts_with(name)).unwrap() + 1).unwrap();
    // The unowned row's icon, then the owned row's: each opens its popup and neither runs `session open`.
    for (n, (name, pull)) in [(" acme/repo#9", "acme/repo#9"), (" acme/repo#20", "acme/repo#20")].into_iter().enumerate() {
        click(&env, &view, u16::try_from(width).unwrap() - 1, second_line(name));
        wait_until("the popup", || popups(&env).len() == n + 1);
        let popup = &popups(&env)[n];
        let at = popup.iter().position(|a| a == "display-popup").unwrap();
        assert_eq!(popup.last().unwrap(), &format!("cpd tui view merge '{pull}'"), "{popup:?}");
        assert_eq!(popup[at + 1], "-E", "{popup:?}");
    }
    // time-claim: over the next redraws after both clicks, no third popup opens and no `session open` runs.
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!(popups(&env).len(), 2);
    assert_eq!(moved_events(&env), before, "no `session open` ran");
    assert_eq!(focused_session(&env), "1||widgets", "the focus did not move");
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
        assert!(text.contains(" key │S1 "), "{name}: {text}");
        let counter = text.chars().collect::<Vec<_>>().iter().rposition(|c| *c == '3').unwrap() as u16;
        assert_eq!(buf[(counter, y)].fg, palette::color(palette::job_status(status, merging).1), "{name}: counter");
    }
    let counters = row(&buf, find(&buf, "counters row").1);
    assert!(counters.contains("1⋅1⋅2⋅0⋅3"), "{counters}");
    assert!(counters.contains("queued⋅running⋅merging⋅failed⋅merged"), "{counters}");
}

#[test]
fn the_session_dots_are_listed_in_their_colours() {
    let buf = legend_buffer();
    let dots = find(&buf, "Session dots").1;
    // The glyph column is as wide as the spinner's frames, then two cells of spacing.
    let glyph_at = |text: &str| {
        let (x, y) = find_after(&buf, dots, text);
        (x - (palette::SPINNER.len() as u16 + 2), y)
    };
    let needs_you = glyph_at("the Session needs you");
    assert_eq!(buf[needs_you].symbol(), palette::NEEDS_YOU);
    let working = glyph_at("the Lead is working");
    let frames: String = (0..palette::SPINNER.len() as u16).map(|i| buf[(working.0 + i, working.1)].symbol().to_string()).collect();
    assert_eq!(frames, palette::SPINNER.concat());
    for (text, glyph, dot) in [
        ("waiting:", palette::WAITING, Dot::Waiting),
        ("idle, nothing needed", palette::IDLE, Dot::Idle),
        ("pi stopped", palette::STOPPED, Dot::Stopped),
        ("red:", palette::NEEDS_YOU, Dot::Red),
        ("yellow:", palette::NEEDS_YOU, Dot::Yellow),
        ("magenta:", palette::NEEDS_YOU, Dot::Magenta),
        ("white:", palette::NEEDS_YOU, Dot::White),
    ] {
        let cell = &buf[glyph_at(text)];
        let want = palette::dot_span(dot, 0, false);
        assert_eq!(cell.symbol(), glyph, "{text}");
        assert_eq!(cell.symbol(), want.content, "{text}");
        assert_eq!(cell.fg, want.style.fg.unwrap_or(Color::Reset), "{text}");
    }
    let precedence = format!(
        "red > yellow > magenta > white > {} > {} > {} > {}",
        palette::SPINNER[0],
        palette::WAITING,
        palette::IDLE,
        palette::STOPPED
    );
    find_after(&buf, dots, &precedence);
    let red = row(&buf, find_after(&buf, dots, "red:").1);
    assert!(red.contains("a dialog in dispatch or deploy, a stuck Job, an engine error, your own failing or changed pull request"), "{red}");
    assert!(!red.contains("Escalation") && !red.contains("or pr"), "{red}");
    let yellow = row(&buf, find_after(&buf, dots, "yellow:").1);
    assert!(yellow.contains("yellow: ready to merge, review requested"), "{yellow}");
    let waiting = row(&buf, find_after(&buf, dots, "waiting:").1);
    assert!(waiting.contains("an agent waits for another agent"), "{waiting}");
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
