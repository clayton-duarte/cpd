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
use cpd_tui::read::{self, Place, Widgets};
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
        "INSERT INTO contexts (id, title, jira, created_at, created_on, lead_session, grounded_seq)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1', NULL, NULL);"
    )
}

/// A Plan; `pr` is `(number, polled)`, where `polled` is `pr poll`'s `(title, review decision, checks)`, `None` before the
/// first poll.
fn plan_row(id: &str, phase: &str, created: &str, pr: Option<(i64, Option<(&str, &str, &str)>)>) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let (number, url, opened, seen, polled_at) = match pr {
        Some((n, polled)) => (
            n.to_string(),
            format!("'https://example.test/pull/{n}'"),
            "'2026-01-05T00:00:00.000Z'".to_owned(),
            r#"'{"checks":{},"reviewIds":[],"commentIds":[]}'"#.to_owned(),
            if polled.is_some() { "'2026-01-06T00:00:00.000Z'" } else { "NULL" }.to_owned(),
        ),
        None => ("NULL".into(), "NULL".into(), "NULL".into(), r#"'{"checks":{},"reviewIds":[],"commentIds":[]}'"#.to_owned(), "NULL".into()),
    };
    let (title, decision, checks) = match pr.and_then(|p| p.1) {
        Some((t, d, c)) => (format!("'{t}'"), format!("'{d}'"), format!("'{c}'")),
        None => ("NULL".into(), "NULL".into(), "NULL".into()),
    };
    let status = match phase {
        "grounding" | "plan" => "waiting",
        "dispatch" => "dispatch",
        "pr" => "open",
        _ => "merged",
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', 'alpha', 'Plan {id}', '{phase}', 'l-{id}', '{created}', {locked}, {done});
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pr_seen,
                          pr_polled_at, pr_title, pr_review_decision, pr_checks, created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo', 'main', '{status}', 'cpd/{id}', {worktree}, {number}, {url}, {opened}, {seen},
                 {polled_at}, {title}, {decision}, {checks}, cpd_txn(), '{created}', {locked}, {done});"
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
/// - `fresh` ("fresh lead"): PR 11, not polled yet; the Lead parked;
/// - `ship` ("v1"): in dispatch, no PR; the Lead in the chat slot;
/// - `sketch` ("sketch lead"): no PR; the Lead has no pane (`none`);
/// - `away` ("away lead"): PR 9, merged; the Lead's pane is in the `work` window, so the Plan is not in the cockpit;
/// - `never` ("never lead"): no PR; the Lead was never moved.
fn populate(store: &mut Store) {
    let mut s = context("alpha");
    let plans: [(&str, &str, &str, Option<(i64, Option<(&str, &str, &str)>)>, &str, Option<&str>); 7] = [
        ("old", "cpd-ui", "done", Some((3, Some(("cpd-ui: cockpit rows", "none", "passed")))), "2026-01-01T12:00:00.000Z", Some("chat")),
        (
            "rel",
            "cpd visual lead",
            "pr",
            Some((7, Some(("cpd-tui cockpit column and its long title", "changes_requested", "failed")))),
            "2026-01-02T00:00:00.000Z",
            Some("parked"),
        ),
        ("fresh", "fresh lead", "pr", Some((11, None)), "2026-01-02T00:00:01.000Z", Some("parked")),
        ("ship", "v1", "dispatch", None, "2026-01-02T00:00:02.000Z", Some("chat")),
        ("sketch", "sketch lead", "plan", None, "2026-01-02T00:00:03.000Z", Some("none")),
        ("away", "away lead", "done", Some((9, Some(("away", "approved", "pending")))), "2026-01-02T00:00:04.000Z", Some("work")),
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

/// A section header on the 46-column test pane: `──`, ` <title> `, then `─` to the edge.
fn header(title: &str) -> String {
    format!("── {title} {}", "─".repeat(46 - 4 - title.chars().count()))
}

/// The column on the prepared store as the User sees it: the Plans with a PR (oldest first), then the Plans without
/// one, of the Plans whose Lead has a pane in the cockpit.
fn column(height: u16) -> (Vec<String>, Buffer) {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let buf = render(46, height, &w);
    (rows(&buf), buf)
}

/// The reference's own test, `the_dispatch_sections_are_titled_with_a_separator_between_each_pair` (`tui/src/main.rs:1053–1062`):
/// the same lines on the same 30×14 pane, for a machine with no Plan in the cockpit.
#[test]
fn the_dispatch_sections_are_titled_with_a_separator_between_each_pair() {
    let env = TestEnv::new();
    let w = read::widgets(&mut open(&env), 3, 0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(30, 20)).unwrap();
    let machine = Machine { name: "homeserver".to_owned(), load: ["0.1", "0.2", "0.3"].map(str::to_owned).to_vec() };
    terminal.draw(|f| widgets::draw(f, f.area(), &w, &machine)).unwrap();
    let lines = rows(terminal.backend().buffer());
    let head = |title: &str| format!("── {title} {}", "─".repeat(30 - 4 - title.chars().count()));
    assert_eq!(lines[..3], ["", " homeserver", " load 0.1 0.2 0.3"]);
    assert_eq!(lines[3..6], ["", head("PRs").as_str(), ""]);
    assert_eq!(lines[6], " no Plans in the cockpit");
    assert_eq!(lines[7..], ["", "", "", "", "", "", head("git").as_str(), "", " (future)", "", head("Jira").as_str(), "", " (future)"]);
}

#[test]
fn the_column_shows_the_machine_the_cockpits_prs_git_and_jira() {
    let (got, _) = column(29);
    let (prs, git, jira) = (header("PRs"), header("git"), header("Jira"));
    let expected = [
        "",
        " homeserver",
        " load 3.70 3.05 4.84",
        "",
        prs.as_str(),
        "",
        " cpd-ui",
        "  #3 merged ✓ no review  cpd-ui: cockpit rows",
        " cpd visual lead",
        "  #7 open ✗ changes requested  cpd-tui cockpi…",
        " fresh lead",
        "  #11 loading…",
        " v1  no PR",
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
fn only_plans_whose_lead_is_in_the_chat_slot_or_parked_are_listed() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let place = |plan: &str| w.plans.iter().find(|c| c.head.id.as_str() == plan).unwrap().lead_place;
    assert_eq!(
        ["old", "rel", "fresh", "ship", "sketch", "away", "never"].map(place),
        [Place::Chat, Place::Parked, Place::Parked, Place::Chat, Place::None, Place::Work, Place::None]
    );
    let got = rows(&render(46, 29, &w)).join("\n");
    for listed in ["cpd-ui", "cpd visual lead", "fresh lead", "v1"] {
        assert!(got.contains(listed), "{listed}");
    }
    for hidden in ["sketch lead", "away lead", "never lead", "#9"] {
        assert!(!got.contains(hidden), "{hidden}:\n{got}");
    }
    // The latest move wins: `ship` leaves the cockpit.
    sql(&mut store, &moved("2026-01-04T00:00:00.000Z", "ship", "work"));
    let w = read::widgets(&mut store, 3, 0).unwrap();
    assert!(!rows(&render(46, 29, &w)).join("\n").contains("v1"));
}

#[test]
fn the_column_has_no_engine_slots_escalations_events_or_hint() {
    let (got, _) = column(29);
    let all = got.join("\n");
    for gone in ["Engine", "Slots", "Escalations", "Events", "legend", "quit"] {
        assert!(!all.contains(gone), "{gone}:\n{all}");
    }
}

#[test]
fn the_column_takes_every_colour_from_the_palette() {
    let (_, buf) = column(29);
    let fg = |at: (u16, u16)| buf[at].fg;
    let c = palette::color;
    // The PRs: state and checks.
    assert_eq!(fg(find(&buf, "merged")), c(Meaning::Done));
    assert_eq!(fg(find(&buf, "✓")), c(Meaning::Done));
    assert_eq!(fg(find(&buf, "open")), c(Meaning::Review));
    assert_eq!(fg(find(&buf, "✗")), c(Meaning::Failed));
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
    // Bold: the machine and the labels (the section titles are reversed, not bold); dim: the title of the PR, `no PR`, `loading…`, `(future)`.
    for title in ["homeserver", "cpd-ui", "cpd visual lead", "v1"] {
        assert!(bold(find(&buf, title)), "{title}");
        assert!(!dim(find(&buf, title)), "{title}");
    }
    for faint in ["cpd-ui: cockpit", "no PR", "loading…", "(future)"] {
        assert!(dim(find(&buf, faint)), "{faint}");
        assert!(!bold(find(&buf, faint)), "{faint}");
    }
    // Plain: the number, the review decision and the load.
    for plain in ["#3", "no review", "changes requested", "load 3.70"] {
        let at = find(&buf, plain);
        assert!(!dim(at) && !bold(at), "{plain}");
    }
}

#[test]
fn a_narrow_column_cuts_the_title_first_then_the_label_and_never_wraps() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let wide = rows(&render(33, 29, &w));
    assert!(wide.contains(&"  #7 open ✗ changes requested".to_owned()), "no room for a title of 4 characters:\n{wide:#?}");
    let got = rows(&render(24, 29, &w));
    assert!(got.iter().all(|r| r.chars().count() <= 24), "{got:#?}");
    for cut in [" cpd visual lead", "  #7 open ✗ changes requested", " fresh lead", " v1  no PR"] {
        assert!(got.iter().any(|r| r.starts_with(cut.chars().take(24).collect::<String>().as_str())), "{cut:?} in {got:#?}");
    }
    let narrow = rows(&render(10, 29, &w));
    assert!(narrow.contains(&" cpd visu…".to_owned()), "{narrow:#?}");
    assert!(narrow.contains(&" v1  no PR".to_owned()), "{narrow:#?}");
}

#[test]
fn a_short_pane_drops_the_plans_without_a_pr_before_the_rest() {
    let (_env, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    // Beside the 14 rows of the blank row, the machine, the headers and their blank rows, the three Plans with a PR take 6; `v1` needs a 7th.
    let full = rows(&render(46, 14 + 7, &w));
    assert!(full.iter().any(|r| r.contains("v1  no PR")), "{full:#?}");
    let short = rows(&render(46, 14 + 6, &w));
    assert!(!short.iter().any(|r| r.contains("no PR")), "{short:#?}");
    assert!(short.iter().any(|r| r.contains("#11 loading…")), "the Plans with a PR stay:\n{short:#?}");
}

#[test]
fn a_plan_of_several_prs_takes_one_line_per_pull_request_in_key_order() {
    let env = TestEnv::new();
    let mut store = open(&env);
    // `web` (#21, open) and `api` (#20, merged) have pull requests; `docs` waits and has none
    let mut s = context("alpha") + &lead_row("multi", "multi lead");
    s += r#"INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
            VALUES ('multi', 'alpha', 'Plan multi', 'dispatch', 'l-multi', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
            INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                             pr_polled_at, pr_title, pr_review_decision, pr_checks, created_txn, created_at, started_at, merged_at)
            VALUES ('multi', 'web', '/src/web', 'main', 'open', 'cpd/multi/web', '/wt/web', 21, 'https://example.test/pull/21',
                    '2026-01-05T00:00:00.000Z', '2026-01-06T00:00:00.000Z', 'web side', 'none', 'pending', cpd_txn(),
                    '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', NULL),
                   ('multi', 'api', '/src/api', 'main', 'merged', 'cpd/multi/api', '/wt/api', 20, 'https://example.test/pull/20',
                    '2026-01-05T00:00:00.000Z', '2026-01-06T00:00:00.000Z', 'api side', 'approved', 'passed', cpd_txn(),
                    '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-07T00:00:00.000Z');
            INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
            VALUES ('multi', 'docs', '/src/docs', 'main', 'waiting', 'cpd/multi/docs', cpd_txn(), '2026-01-02T00:00:00.000Z');"#;
    s += &moved("2026-01-03T00:00:00.000Z", "multi", "chat");
    sql(&mut store, &s);
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let keys: Vec<&str> = w.plans[0].prs.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(keys, ["api", "web"], "the PRs with a pull request, in key order");
    let got = rows(&render(46, 22, &w));
    assert_eq!(got[4..8], [header("PRs").as_str(), " multi lead", "  #20 merged ✓ approved  api side", "  #21 open … no review  web side"], "{got:#?}");
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
