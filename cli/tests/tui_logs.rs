//! The `logs` view and the StatusBar's `▤` button (commands.md §13.2, §17; R36): the Event log's snapshot,
//! following and scrolling with ratatui's `TestBackend`, the view itself on a private tmux server, and the button's
//! `cpd tui action logs` (`tmux` on `PATH` is a fake that answers the client's size and records `display-popup`).

mod common;

use std::fs;

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use cpd_tui::logs::{self, Log};
use cpd_tui::palette::{self, Meaning};
use cpd_tui::read;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

/// Appends one Event per `(kind, text)`, one second apart from 00:00:00 on.
fn append(store: &mut Store, first: usize, events: &[(&str, &str)]) {
    let mut sql = String::new();
    for (i, (kind, text)) in events.iter().enumerate() {
        let n = first + i;
        sql += &format!(
            "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '2026-01-03T{:02}:{:02}:{:02}.000Z', '{kind}', '{text}', '{{}}');",
            n / 3600,
            n / 60 % 60,
            n % 60
        );
    }
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// The log in an area of `height` rows of Events under the title's row.
fn render(width: u16, height: u16, log: &mut Log) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height + 1)).unwrap();
    terminal
        .draw(|f| {
            log.fit(f.area().height);
            logs::draw(f, f.area(), log);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

/// The rows under the title.
fn rows(buf: &Buffer) -> Vec<String> {
    (1..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned())
        .collect()
}

/// `n` Events `event <first>` …, so each says which one it is.
fn append_numbered(store: &mut Store, first: usize, n: usize) {
    let texts: Vec<String> = (first..first + n).map(|i| format!("event {i}")).collect();
    let events: Vec<(&str, &str)> = texts.iter().map(|t| ("plan-phase", t.as_str())).collect();
    append(store, first, &events);
}

// ---------- the snapshot ----------

#[test]
fn the_log_lists_the_events_one_line_each_newest_last() {
    let env = TestEnv::new();
    let mut store = open(&env);
    append(&mut store, 0, &[("context-created", ""), ("plan-phase", "Plan ship: dispatch"), ("error", "store busy")]);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events);
    let buf = render(34, 5, &mut log);
    assert_eq!(
        rows(&buf),
        [" 00:00:00 context-created", " 00:00:01 Plan ship: dispatch", " 00:00:02 store busy", "", ""]
    );
    // the time is dim, an `error` Event is in the failed colour and the others are not
    assert!(buf[(1, 1)].modifier.contains(Modifier::DIM) && !buf[(10, 1)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(10, 3)].fg, palette::fg(Meaning::Failed).fg.unwrap());
    assert_ne!(buf[(10, 2)].fg, palette::fg(Meaning::Failed).fg.unwrap());
}

#[test]
fn a_long_event_is_cut_with_an_ellipsis_and_an_empty_log_says_so() {
    let env = TestEnv::new();
    let mut store = open(&env);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events);
    assert_eq!(rows(&render(20, 2, &mut log)), [" none yet", ""]);
    append(&mut store, 0, &[("plan-phase", "a very long event text indeed")]);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events);
    assert_eq!(rows(&render(20, 1, &mut log)), [" 00:00:00 a very lo…"]);
}

#[test]
fn the_snapshot_is_the_latest_500_events_oldest_first_and_follows_from_its_cursor() {
    let env = TestEnv::new();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 600);
    let snapshot = read::logs(&mut store, 500).unwrap();
    assert_eq!(snapshot.events.len(), 500);
    assert_eq!((snapshot.events[0].text.as_str(), snapshot.events[499].text.as_str()), ("event 100", "event 599"));
    assert_eq!(snapshot.events_cursor, 600);
    assert!(read::logs_after(&mut store, 600).unwrap().events.is_empty());
    append_numbered(&mut store, 600, 2);
    let next = read::logs_after(&mut store, snapshot.events_cursor).unwrap();
    assert_eq!(next.events.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["event 600", "event 601"]);
    assert_eq!(next.events_cursor, 602);
}

// ---------- following and scrolling ----------

#[test]
fn the_log_follows_while_it_shows_the_bottom_and_stops_when_scrolled_up() {
    let env = TestEnv::new();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 10);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events);
    let last = |buf: &Buffer| rows(buf).last().unwrap().clone();
    assert_eq!(rows(&render(20, 4, &mut log)), [" 00:00:06 event 6", " 00:00:07 event 7", " 00:00:08 event 8", " 00:00:09 event 9"]);
    assert!(log.follows());

    // at the bottom: a new Event scrolls in
    append_numbered(&mut store, 10, 1);
    log.push(read::logs_after(&mut store, 10).unwrap().events);
    assert_eq!(last(&render(20, 4, &mut log)), " 00:00:10 event 10");

    // scrolled up: new Events do not move what is shown
    log.scroll_up(2);
    assert!(!log.follows());
    let before = render(20, 4, &mut log);
    assert_eq!(rows(&before), [" 00:00:05 event 5", " 00:00:06 event 6", " 00:00:07 event 7", " 00:00:08 event 8"]);
    append_numbered(&mut store, 11, 2);
    log.push(read::logs_after(&mut store, 11).unwrap().events);
    assert_eq!(rows(&render(20, 4, &mut log)), rows(&before));

    // back at the bottom, it follows again
    log.scroll_down(4);
    assert!(log.follows());
    assert_eq!(last(&render(20, 4, &mut log)), " 00:00:12 event 12");
    append_numbered(&mut store, 13, 1);
    log.push(read::logs_after(&mut store, 13).unwrap().events);
    assert_eq!(last(&render(20, 4, &mut log)), " 00:00:13 event 13");
}

#[test]
fn scrolling_moves_a_line_or_a_page_and_stops_at_both_ends() {
    let env = TestEnv::new();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 10);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events);
    let first = |log: &mut Log| rows(&render(20, 4, log))[0].clone();
    assert_eq!(first(&mut log), " 00:00:06 event 6");
    log.scroll_up(1);
    assert_eq!(first(&mut log), " 00:00:05 event 5");
    log.scroll_up(log.page());
    assert_eq!(first(&mut log), " 00:00:01 event 1");
    log.scroll_up(100);
    assert_eq!(first(&mut log), " 00:00:00 event 0");
    log.scroll_down(log.page());
    assert_eq!(first(&mut log), " 00:00:04 event 4");
    log.scroll_down(100);
    assert_eq!(first(&mut log), " 00:00:06 event 6");
    assert!(log.follows());
}

// ---------- the view, on a private tmux server ----------

fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

fn start(env: &TestEnv, view: &[&str], width: u16, height: u16) -> String {
    let mut args = vec!["new-session", "-d", "-s", "view", "-x", "", "-y", "", "-P", "-F", "#{pane_id}", CPD, "tui", "view"];
    let (w, h) = (width.to_string(), height.to_string());
    args[5] = &w;
    args[7] = &h;
    args.extend(view);
    tmux_out(env, &args).trim().to_owned()
}

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

fn wait_screen(env: &TestEnv, pane: &str, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), done)
}

/// The log view in a pane showing `events` rows of Events under the title's row.
fn start_log(env: &TestEnv, args: &[&str], width: u16, events: u16) -> String {
    let view: Vec<&str> = ["logs"].into_iter().chain(args.iter().copied()).collect();
    start(env, &view, width, events + 1)
}

/// Waits for the rows under the title; the title itself is asserted to be the section header.
fn wait_log(env: &TestEnv, pane: &str, what: &str, mut done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    let rows = common::wait_screen(what, || screen(env, pane), |r| r.len() > 1 && done(&r[1..]));
    assert!(rows[0].starts_with("── logs ─"), "the title is a section header: {:?}", rows[0]);
    rows[1..].to_vec()
}

/// One notch of the wheel at 1-based `column`, `row` of the pane, as the terminal sends it (SGR: 64 up, 65 down).
fn wheel(env: &TestEnv, pane: &str, up: bool, column: u16, row: u16) {
    tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<{};{column};{row}M", if up { 64 } else { 65 })]);
}

#[test]
fn the_view_follows_live_scrolls_and_closes_with_q_or_esc() {
    let env = env();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 10);
    let pane = start_log(&env, &[], 30, 4);
    let rows = wait_log(&env, &pane, "the latest Events", |r| r.get(3).is_some_and(|l| l == " 00:00:09 event 9"));
    assert_eq!(rows[0], " 00:00:06 event 6");

    // a new Event follows at the bottom
    append_numbered(&mut store, 10, 1);
    wait_log(&env, &pane, "the new Event", |r| r.get(3).is_some_and(|l| l == " 00:00:10 event 10"));

    // k scrolls up and the view stops following; j comes back and follows again
    tmux_out(&env, &["send-keys", "-t", &pane, "k", "k"]);
    wait_log(&env, &pane, "scrolled up", |r| r.first().is_some_and(|l| l == " 00:00:05 event 5"));
    append_numbered(&mut store, 11, 1);
    // The view reads the store after each key it handles, so once `k` has scrolled it one more row, it has read
    // Event 11: it did not follow it.
    tmux_out(&env, &["send-keys", "-t", &pane, "k"]);
    let rows = wait_log(&env, &pane, "scrolled up once more", |r| r.first().is_some_and(|l| l == " 00:00:04 event 4"));
    assert_eq!(rows[3], " 00:00:07 event 7", "the new Event did not move it: {rows:?}");
    tmux_out(&env, &["send-keys", "-t", &pane, "PageDown"]);
    wait_log(&env, &pane, "following again", |r| r.get(3).is_some_and(|l| l == " 00:00:11 event 11"));
    tmux_out(&env, &["send-keys", "-t", &pane, "Up", "PageUp", "PageUp", "PageUp"]);
    wait_log(&env, &pane, "the top", |r| r.first().is_some_and(|l| l == " 00:00:00 event 0"));

    // the wheel scrolls too, three rows a notch, and the bottom follows again
    wheel(&env, &pane, false, 5, 3);
    wait_log(&env, &pane, "the wheel down", |r| r.first().is_some_and(|l| l == " 00:00:03 event 3"));
    wheel(&env, &pane, false, 5, 3);
    wait_log(&env, &pane, "the wheel down again", |r| r.first().is_some_and(|l| l == " 00:00:06 event 6"));
    wheel(&env, &pane, true, 5, 3);
    let rows = wait_log(&env, &pane, "the wheel up", |r| r.first().is_some_and(|l| l == " 00:00:03 event 3"));
    assert_eq!(rows[3], " 00:00:06 event 6");
    append_numbered(&mut store, 12, 1);
    wheel(&env, &pane, false, 5, 3);
    wait_log(&env, &pane, "the wheel down", |r| r.first().is_some_and(|l| l == " 00:00:06 event 6"));
    for _ in 0..4 {
        wheel(&env, &pane, false, 5, 3);
    }
    wait_log(&env, &pane, "the bottom", |r| r.get(3).is_some_and(|l| l == " 00:00:12 event 12"));
    append_numbered(&mut store, 13, 1);
    wait_log(&env, &pane, "following again", |r| r.get(3).is_some_and(|l| l == " 00:00:13 event 13"));

    // q closes it
    tmux_out(&env, &["send-keys", "-t", &pane, "q"]);
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());

    // so does Esc
    let pane = start_log(&env, &[], 30, 4);
    wait_log(&env, &pane, "the Events", |r| r.get(3).is_some_and(|l| l.contains("event 13")));
    tmux_out(&env, &["send-keys", "-t", &pane, "Escape"]);
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());
}

/// A failed read keeps the Events on screen and shows the error in the last row; the next poll reads again, and the
/// error goes when a read succeeds (tui-W4). The read is made to fail by renaming the table it reads.
#[test]
fn a_failed_read_keeps_the_events_and_shows_the_error_until_a_read_succeeds() {
    let env = env();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 10);
    let pane = start(&env, &["logs"], 60, 5);
    wait_screen(&env, &pane, "the latest Events", |r| r.get(4).is_some_and(|l| l == " 00:00:09 event 9"));
    // not through `Store`, which refuses a changed schema
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("ALTER TABLE events RENAME TO events_gone;");
    let rows = wait_screen(&env, &pane, "the error in the last row", |r| r.get(4).is_some_and(|l| l.contains("no such table")));
    assert_eq!(rows[3], " 00:00:09 event 9", "the Events stay, one row less: {rows:?}");
    raw("ALTER TABLE events_gone RENAME TO events;");
    wait_screen(&env, &pane, "the error to go", |r| r.get(4).is_some_and(|l| l == " 00:00:09 event 9"));
}

/// A follow that fails is shown in the last row, as a failed read is (tui-G4): the highest Event id fell below the
/// cursor, which is what a replaced database looks like. The Events stay.
#[test]
fn a_follow_that_fails_is_shown_and_the_events_stay() {
    let env = env();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 10);
    let pane = start(&env, &["logs"], 80, 5);
    wait_screen(&env, &pane, "the latest Events", |r| r.get(4).is_some_and(|l| l == " 00:00:09 event 9"));
    // not through `Store`, which keeps the Events append-only
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("DROP TRIGGER events_no_delete; DELETE FROM events WHERE id > 5;");
    let rows = wait_screen(&env, &pane, "the error in the last row", |r| r.get(4).is_some_and(|l| l.contains("Event cursor")));
    assert_eq!(rows[3], " 00:00:09 event 9", "the Events stay: {rows:?}");
}

#[test]
fn the_logs_view_needs_a_terminal() {
    let env = env();
    let run = env.cpd(&["tui", "view", "logs"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
}

// ---------- the legend popup scrolls too ----------

#[test]
fn a_legend_taller_than_its_pane_scrolls_with_the_wheel_and_any_other_key_closes_it() {
    let env = env();
    let pane = start(&env, &["legend"], 100, 10);
    let rows = wait_screen(&env, &pane, "the legend", |r| r.first().is_some_and(|l| l.starts_with("── Legend: what")));
    assert!(rows[0].contains(" 1-9/"), "{:?}", rows[0]);
    assert!(rows[2].contains("Meanings"), "{rows:?}");
    wheel(&env, &pane, false, 5, 5);
    let rows = wait_screen(&env, &pane, "the wheel down", |r| r.first().is_some_and(|l| l.contains(" 4-12/")));
    assert!(rows[0].starts_with("── Legend: what"), "the title stays");
    assert!(!rows.iter().any(|l| l.contains("Meanings")), "{rows:?}");
    wheel(&env, &pane, true, 5, 5);
    wait_screen(&env, &pane, "the wheel up", |r| r.first().is_some_and(|l| l.contains(" 1-9/")));
    tmux_out(&env, &["send-keys", "-t", &pane, "PageDown"]);
    wait_screen(&env, &pane, "a page down", |r| r.first().is_some_and(|l| l.contains(" 10-18/")));
    tmux_out(&env, &["send-keys", "-t", &pane, "x"]);
    wait_until("the legend to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());
}

// ---------- the button ----------

#[test]
fn the_popup_is_sized_like_the_legends_and_at_most_80_percent_of_the_client() {
    let (w, h) = cpd_tui::widgets::legend::size();
    let big = logs::popup_args((1000, 1000), None);
    assert_eq!(
        big,
        ["display-popup", "-E", "-T", " logs ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd tui view logs"]
    );
    let small = logs::popup_args((80, 40), None);
    assert_eq!((small[5].as_str(), small[7].as_str()), ("64", "32"));
    assert_eq!(small[3], " logs ");
}

/// A fake `tmux` on `PATH`: client `/dev/pts/9` is 200x60, `display-popup` does nothing.
fn fake_tmux(env: &TestEnv) {
    env.fake(
        "tmux",
        "case \"$*\" in\n *'display -p -c /dev/pts/9 '*) echo '200 60';;\n *display-popup*) ;;\n *) exit 1;;\nesac",
    );
}

fn popups(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-popup")).collect()
}

#[test]
fn the_logs_action_opens_the_popup_on_the_clicking_client() {
    let env = env();
    fake_tmux(&env);
    let run = env.cpd(&["tui", "action", "logs", "--client", "/dev/pts/9"]);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");
    let popup = popups(&env).remove(0);
    let at = popup.iter().position(|a| a == "display-popup").unwrap();
    // `tmux -L <socket> -f /dev/null display-popup -c <client> -E -T ' logs ' -w <w> -h <h> cpd tui view logs`
    // Sized like the legend, at most 80 % of the 200x60 client (`logs::popup_args`): 160x48.
    let (w, h) = cpd_tui::widgets::legend::size();
    let (w, h) = ((w + 2).min(160), (h + 2).min(48));
    assert_eq!(
        popup[at..],
        ["display-popup", "-c", "/dev/pts/9", "-E", "-T", " logs ", "-w", &w.to_string(), "-h", &h.to_string(), "cpd tui view logs"]
    );
    assert!(popup[..at].starts_with(&["-L".to_owned(), env.tmux_socket().to_owned()]), "{popup:?}");

    // the legend action opens the legend, not the log
    let run = env.cpd(&["tui", "action", "legend", "--client", "/dev/pts/9"]);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");
    assert_eq!(popups(&env).len(), 2);
    assert!(popups(&env)[1].contains(&" legend ".to_owned()));
}

// ---------- opening at an Event (`--event <id>`, the Inbox click of an engine-error or engine-down item) ----------

/// The row indexes (under the title) of `buf` whose first and last cell are reversed.
fn reversed_rows(buf: &Buffer) -> Vec<u16> {
    let width = buf.area.width;
    (1..buf.area.height)
        .filter(|&y| buf[(0, y)].modifier.contains(Modifier::REVERSED) && buf[(width - 1, y)].modifier.contains(Modifier::REVERSED))
        .map(|y| y - 1)
        .collect()
}

#[test]
fn opened_at_an_event_its_line_is_reversed_in_the_middle_of_the_view_and_the_view_does_not_follow() {
    let env = TestEnv::new();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 30);
    let events = read::logs(&mut store, 500).unwrap().events;
    let id = events.iter().find(|e| e.text == "event 10").unwrap().id;
    let mut log = Log::at(events, id);
    let buf = render(20, 5, &mut log);
    assert_eq!(
        rows(&buf),
        [" 00:00:08 event 8", " 00:00:09 event 9", " 00:00:10 event 10", " 00:00:11 event 11", " 00:00:12 event 12"]
    );
    assert_eq!(reversed_rows(&buf), [2], "its whole line, and only it");
    assert!(!log.follows());
    // new Events do not move what is shown; scrolling to the bottom follows again
    append_numbered(&mut store, 30, 2);
    log.push(read::logs_after(&mut store, 30).unwrap().events);
    assert_eq!(rows(&render(20, 5, &mut log))[2], " 00:00:10 event 10");
    log.scroll_down(1000);
    assert!(log.follows());
    assert_eq!(rows(&render(20, 5, &mut log))[4], " 00:00:31 event 31");
}

#[test]
fn an_event_at_either_end_fills_the_view_and_one_that_does_not_exist_opens_at_the_bottom() {
    let env = TestEnv::new();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 30);
    let events = read::logs(&mut store, 500).unwrap().events;
    let (first, last) = (events[0].id, events[29].id);
    let mut log = Log::at(events.clone(), first);
    let buf = render(20, 5, &mut log);
    assert_eq!((rows(&buf)[0].as_str(), rows(&buf)[4].as_str()), (" 00:00:00 event 0", " 00:00:04 event 4"));
    assert_eq!(reversed_rows(&buf), [0]);
    let mut log = Log::at(events.clone(), last);
    let buf = render(20, 5, &mut log);
    assert_eq!(rows(&buf)[4], " 00:00:29 event 29");
    assert_eq!(reversed_rows(&buf), [4]);
    assert!(log.follows(), "its line is the bottom one");
    let mut log = Log::at(events, 9999);
    let buf = render(20, 5, &mut log);
    assert_eq!(rows(&buf)[4], " 00:00:29 event 29");
    assert!(reversed_rows(&buf).is_empty() && log.follows());
}

#[test]
fn the_popup_runs_the_view_at_the_event_of_the_item() {
    let at = logs::popup_args((1000, 1000), Some(42));
    assert_eq!(at.last().unwrap(), "cpd tui view logs --event 42");
    assert_eq!(at[..at.len() - 1], logs::popup_args((1000, 1000), None)[..at.len() - 1], "sized as the button's");
}

#[test]
fn the_view_opens_at_the_event_even_when_it_is_older_than_the_latest_500_and_does_not_follow() {
    let env = env();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 600);
    // `event 2` is Event 3: older than the 500 the view starts with, so it is read on its own and shown first
    let pane = start_log(&env, &["--event", "3"], 30, 5);
    let rows = wait_log(&env, &pane, "the Event", |r| r.first().is_some_and(|l| l == " 00:00:02 event 2"));
    assert_eq!(rows[1], " 00:01:40 event 100", "then the latest 500");
    append_numbered(&mut store, 600, 1);
    // The view reads the store after each key it handles, so once `j` has scrolled it one row, it has read Event 600:
    // it did not follow it.
    tmux_out(&env, &["send-keys", "-t", &pane, "j"]);
    let rows = wait_log(&env, &pane, "scrolled down once", |r| r.first().is_some_and(|l| l != " 00:00:02 event 2"));
    assert_eq!(rows[0], " 00:01:40 event 100", "it does not follow: {rows:?}");
    tmux_out(&env, &["send-keys", "-t", &pane, "q"]);
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());

    // an Event among the latest 500 is in the middle of the view
    let pane = start_log(&env, &["--event", "301"], 30, 5);
    let rows = wait_log(&env, &pane, "the Event", |r| r.get(2).is_some_and(|l| l == " 00:05:00 event 300"));
    assert_eq!((rows[0].as_str(), rows[4].as_str()), (" 00:04:58 event 298", " 00:05:02 event 302"));
    tmux_out(&env, &["send-keys", "-t", &pane, "Escape"]);
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());

    // an Event that does not exist: the bottom, as without the option
    let pane = start_log(&env, &["--event", "99999"], 30, 5);
    wait_log(&env, &pane, "the bottom", |r| r.get(4).is_some_and(|l| l == " 00:10:00 event 600"));
}
