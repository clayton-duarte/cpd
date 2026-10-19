//! The `logs` view and the StatusBar's `▤` button (commands.md §13.2, §17; R36): the Event log's snapshot,
//! following and scrolling with ratatui's `TestBackend`, the view itself and the button's click on a private tmux
//! server (the click is raw mouse bytes sent to the StatusBar's pane; `tmux` on `PATH` is a fake that answers
//! `list-clients` and records `display-popup`, and runs the real one for the rest).

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::{CPD, TestEnv};
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

fn render(width: u16, height: u16, log: &mut Log) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            log.fit(f.area().height);
            logs::draw(f, f.area(), log);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
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
    assert!(buf[(1, 0)].modifier.contains(Modifier::DIM) && !buf[(10, 0)].modifier.contains(Modifier::DIM));
    assert_eq!(buf[(10, 2)].fg, palette::fg(Meaning::Failed).fg.unwrap());
    assert_ne!(buf[(10, 1)].fg, palette::fg(Meaning::Failed).fg.unwrap());
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

/// Waits until `done` holds, at most 30 s.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
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

fn wait_screen(env: &TestEnv, pane: &str, what: &str, mut done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    let mut rows = Vec::new();
    wait_until(what, || {
        rows = screen(env, pane);
        done(&rows)
    });
    rows
}

#[test]
fn the_view_follows_live_scrolls_and_closes_with_q_or_esc() {
    let env = env();
    let mut store = open(&env);
    append_numbered(&mut store, 0, 10);
    let pane = start(&env, &["logs"], 30, 4);
    let rows = wait_screen(&env, &pane, "the latest Events", |r| r.get(3).is_some_and(|l| l == " 00:00:09 event 9"));
    assert_eq!(rows[0], " 00:00:06 event 6");

    // a new Event follows at the bottom
    append_numbered(&mut store, 10, 1);
    wait_screen(&env, &pane, "the new Event", |r| r.get(3).is_some_and(|l| l == " 00:00:10 event 10"));

    // k scrolls up and the view stops following; j comes back and follows again
    tmux_out(&env, &["send-keys", "-t", &pane, "k", "k"]);
    wait_screen(&env, &pane, "scrolled up", |r| r.first().is_some_and(|l| l == " 00:00:05 event 5"));
    append_numbered(&mut store, 11, 1);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(screen(&env, &pane)[0], " 00:00:05 event 5");
    tmux_out(&env, &["send-keys", "-t", &pane, "PageDown"]);
    wait_screen(&env, &pane, "following again", |r| r.get(3).is_some_and(|l| l == " 00:00:11 event 11"));
    tmux_out(&env, &["send-keys", "-t", &pane, "Up", "PageUp", "PageUp", "PageUp"]);
    wait_screen(&env, &pane, "the top", |r| r.first().is_some_and(|l| l == " 00:00:00 event 0"));

    // q closes it
    tmux_out(&env, &["send-keys", "-t", &pane, "q"]);
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());

    // so does Esc
    let pane = start(&env, &["logs"], 30, 4);
    wait_screen(&env, &pane, "the Events", |r| r.get(3).is_some_and(|l| l.contains("event 11")));
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

#[test]
fn the_logs_view_needs_a_terminal() {
    let env = env();
    let run = env.cpd(&["tui", "view", "logs"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
}

// ---------- the button ----------

#[test]
fn the_popup_is_sized_like_the_legends_and_at_most_80_percent_of_the_client() {
    let (w, h) = cpd_tui::widgets::legend::size();
    let big = logs::popup_args((1000, 1000));
    assert_eq!(
        big,
        ["display-popup", "-E", "-T", " logs ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd tui view logs"]
    );
    let small = logs::popup_args((80, 40));
    assert_eq!((small[5].as_str(), small[7].as_str()), ("64", "32"));
    assert_eq!(small[3], " logs ");
}

/// The StatusBar in a one-row window, with a fake `tmux` on `PATH`: `list-clients` says one client of 200x60,
/// `display-popup` does nothing, and the rest is the real tmux.
fn statusbar_with_fake_tmux(env: &TestEnv) -> String {
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    let pane = start(env, &["statusbar"], 80, 1);
    env.fake(
        "tmux",
        &format!(
            "case \"$*\" in\n *list-clients*) echo '100 200 60';;\n *display-popup*) ;;\n *) exec {} \"$@\";;\nesac",
            real.trim()
        ),
    );
    pane
}

fn popups(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-popup")).collect()
}

/// A left click at 1-based `column` of the pane's one row, as the terminal sends it (SGR mouse reporting).
fn click(env: &TestEnv, pane: &str, column: u16) {
    for end in ['M', 'm'] {
        tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<0;{column};1{end}")]);
    }
}

#[test]
fn a_click_on_the_logs_button_opens_the_popup_and_the_others_do_not() {
    let env = env();
    let pane = statusbar_with_fake_tmux(&env);
    wait_screen(&env, &pane, "the buttons", |r| r.first().is_some_and(|l| l.starts_with("⧉ ? ▤")));
    // `▤` is column 4 (0-based)
    click(&env, &pane, 5);
    wait_until("the popup", || !popups(&env).is_empty());
    let popup = popups(&env).remove(0);
    let at = popup.iter().position(|a| a == "display-popup").unwrap();
    // `tmux -L <socket> -f /dev/null display-popup -E -T ' logs ' -w <w> -h <h> cpd tui view logs`
    let (w, h) = cpd_tui::widgets::legend::size();
    assert_eq!(
        popup[at..],
        ["display-popup", "-E", "-T", " logs ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd tui view logs"]
    );
    assert!(popup[..at].starts_with(&["-L".to_owned(), env.tmux_socket().to_owned()]), "{popup:?}");

    // a click on the legend button opens the legend, not the log
    click(&env, &pane, 3);
    wait_until("the legend popup", || popups(&env).len() == 2);
    assert!(popups(&env)[1].contains(&" legend ".to_owned()));
}
