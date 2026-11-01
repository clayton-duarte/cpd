//! The Dispatch column and the key legend drawn on ratatui's `TestBackend`: the unit tests that `cli/tests/tui/tui_widgets.rs` ran
//! with a store prepared through the store API (as `tui_read.rs` does, since Plans and Sessions belong to their areas) and
//! no pane, and the palette they share. The store is a temp directory (a library call, no process).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;

use super::{Machine, Usage, draw, legend};
use crate::palette::{self, Meaning};
use crate::read::{self, Dot, Widgets};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

// ---------- fixtures (the store API, as tui_read.rs prepares it) ----------

/// A store in a temp directory; the directory lives as long as the returned guard.
fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&CpdHome::resolve(Some(dir.path().into()), None).unwrap(), 5000).unwrap();
    (dir, store)
}

/// Runs the SQL `text` in one write.
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

/// One Plan's fixture row: lead label, name, phase, its PR (number and checks), its time and its pane place.
type PlanFixture = (&'static str, &'static str, &'static str, Option<(i64, (&'static str, &'static str, &'static str))>, &'static str, Option<&'static str>);

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
    let plans: [PlanFixture; 7] = [
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

fn fixture() -> (tempfile::TempDir, Store) {
    let (dir, mut store) = open();
    populate(&mut store);
    (dir, store)
}

// ---------- the Dispatch column ----------

fn machine() -> Machine {
    Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } }
}

fn render(width: u16, height: u16, w: &Widgets) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| drop(draw(f, f.area(), &machine(), 0, &w.inbox, 0, time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
    terminal.backend().buffer().clone()
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


/// The first cell of the first row at or after `from` that contains `needle` (every glyph here is one column wide).
fn find_after(buf: &Buffer, from: u16, needle: &str) -> (u16, u16) {
    (from..buf.area.height)
        .find_map(|y| row(buf, y).find(needle).map(|b| (row(buf, y)[..b].chars().count() as u16, y)))
        .unwrap_or_else(|| panic!("{needle:?} is not drawn:\n{}", rows(buf).join("\n")))
}

fn find(buf: &Buffer, needle: &str) -> (u16, u16) {
    find_after(buf, 0, needle)
}

/// The column on the prepared store as the User sees it: the open pull requests, oldest first.
fn column(height: u16) -> (Vec<String>, Buffer) {
    let (_dir, mut store) = fixture();
    let w = read::widgets(&mut store, 3, 0).unwrap();
    let buf = render(46, height, &w);
    (rows(&buf), buf)
}

#[test]
fn the_column_shows_the_machine_and_the_inbox_under_it_and_nothing_else() {
    let (got, _) = column(25);
    let expected = ["", " homeserver", " \u{F4BC} 23%  \u{EFC5} 41%", ""];
    assert_eq!(got[..4], expected, "\n{}", got.join("\n"));
    assert!(!got.iter().any(|r| r.contains("PRs") || r.contains("acme/repo#")), "\n{}", got.join("\n"));
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
    let c = palette::color;
    let dim = |at: (u16, u16)| buf[at].fg == c(Meaning::Nothing);
    let bold = |at: (u16, u16)| buf[at].modifier.contains(Modifier::BOLD);
    // Bold: the machine's name.
    assert!(bold(find(&buf, "homeserver")));
    assert!(!dim(find(&buf, "homeserver")));
    // Plain: the CPU and memory %: not bold, not muted.
    for plain in ["23%", "41%"] {
        let at = find(&buf, plain);
        assert!(!dim(at) && !bold(at), "{plain}");
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
        let (icon, _m) = palette::job_status(status, merging);
        let (x, y) = find_after(&buf, jobs, name);
        let text = row(&buf, y);
        let pill_col = text.chars().position(|c| c.to_string() == icon).unwrap() as u16;
        assert!(pill_col > x, "{name}: the pill follows the name");
        let pill = palette::job_pill(status, merging, false);
        assert_eq!(buf[(pill_col, y)].bg, pill.bg.unwrap(), "{name}: pill");
        assert_eq!(buf[(pill_col, y)].fg, pill.fg.unwrap(), "{name}: pill text");
        // the compact pill (hs-d474): no padding inside the caps
        assert!(text.contains(&format!("\u{e0b6}{icon} key│S1\u{e0b4}")), "{name}: {text}");
        // a rounded cap on each end, in the pill's colour on the default background
        let (left, right) = (pill_col - 1, pill_col + 1 + " key│S1".chars().count() as u16);
        for (cap_at, glyph) in [(left, "\u{e0b6}"), (right, "\u{e0b4}")] {
            let cap = &buf[(cap_at, y)];
            assert_eq!((cap.symbol(), cap.fg, cap.bg), (glyph, pill.fg.unwrap(), Default::default()), "{name}: cap at {cap_at}: {text}");
        }
        let counter = text.chars().collect::<Vec<_>>().iter().rposition(|c| *c == '3').unwrap() as u16;
        assert_eq!(buf[(counter, y)].fg, palette::count_style(status, merging, 3).fg.unwrap(), "{name}: counter");
    }
    let counters = row(&buf, find(&buf, "counters row").1);
    assert!(counters.contains("1⋅1⋅1⋅1"), "{counters}");
    assert!(counters.contains("planned⋅active⋅failed⋅done"), "{counters}");
}

#[test]
fn the_session_dots_are_listed_in_their_colours() {
    let buf = legend_buffer();
    let dots = find(&buf, "Session Dots").1;
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
        ("cyan:", palette::NEEDS_YOU, Dot::Cyan),
        ("white:", palette::NEEDS_YOU, Dot::White),
    ] {
        let cell = &buf[glyph_at(text)];
        let want = palette::dot_span(dot, 0, false);
        assert_eq!(cell.symbol(), glyph, "{text}");
        assert_eq!(cell.symbol(), want.content, "{text}");
        assert_eq!(cell.fg, want.style.fg.unwrap_or_default(), "{text}");
    }
    let precedence = format!(
        "red > yellow > magenta > cyan > white > {} > {} > {} > {}",
        palette::SPINNER[0],
        palette::WAITING,
        palette::IDLE,
        palette::STOPPED
    );
    find_after(&buf, dots, &precedence);
    let red = row(&buf, find_after(&buf, dots, "red:").1);
    assert!(red.contains("a dialog in dispatch or deploy, a stuck Job, an engine error, your own broken pull request that nobody is fixing"), "{red}");
    assert!(!red.contains("Escalation") && !red.contains("or pr"), "{red}");
    let yellow = row(&buf, find_after(&buf, dots, "yellow:").1);
    assert!(yellow.contains("yellow: ready to post, ready to merge, review requested"), "{yellow}");
    let waiting = row(&buf, find_after(&buf, dots, "waiting:").1);
    assert!(waiting.contains("an agent waits for another agent"), "{waiting}");
}

#[test]
fn the_pr_states_and_checks_are_listed_in_their_colours() {
    let buf = legend_buffer();
    for (name, state) in palette::PR_LEGEND {
        assert_eq!(buf[find(&buf, &format!("#2 {name}"))].fg, palette::color_clickable(palette::pr_state(state)), "{name}");
    }
    let checks = find(&buf, "PR Checks").1;
    for (name, c) in palette::CHECKS_LEGEND {
        let (icon, m) = palette::checks(c);
        let (x, y) = find_after(&buf, checks, &format!(" {name}"));
        assert_eq!(buf[(x - 1, y)].symbol(), icon, "{name}");
        assert_eq!(buf[(x - 1, y)].fg, palette::color_clickable(m), "{name}");
    }
}

#[test]
fn the_legend_fits_its_own_size() {
    let buf = legend_buffer();
    assert!(row(&buf, 1).contains("Any Key Closes"));
    let (w, h) = legend::size();
    assert_eq!((buf.area.width, buf.area.height), (w, h));
    assert!(row(&buf, h - 1).contains("passed"), "the last section is not cut");
}

#[test]
fn the_legend_popup_is_sized_to_the_legend_and_capped_at_the_client() {
    let (w, h) = legend::size();
    assert_eq!(
        legend::popup_args((215, 100)),
        ["display-popup", "-E", "-T", " legend ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd tui view legend"]
    );
    let small = legend::popup_args((40, 20));
    assert_eq!((small[5].as_str(), small[7].as_str()), ("40", "20"));
}
