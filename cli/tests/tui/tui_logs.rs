//! The `logs` view and the StatusBar's `▤` button (commands.md §13.2, §17; R36): the Event log's snapshot,
//! following and scrolling with ratatui's `TestBackend` over a store in this process (`Fx`: no process), the view
//! loop itself (`logs::show_with`, the legend's too) on a `SharedBackend` with a scripted input and the `ManualClock`
//! (`common::screen::Script`: the keys, the polls that find nothing, what another process commits between two polls and
//! a look at the screen), and the button's
//! `cpd tui action logs`, in this process through `run_with` on `Fx::with_ports` (the fake tmux route answers the client's
//! size and the `FakeRunner` records `display-popup`).

use crate::common;

use std::path::PathBuf;
use std::time::Duration;

use common::TestEnv;
use common::fx::Fx;
use common::screen::{Screen, Script, SharedBackend, Step, wheel};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use cpd_tui::logs::{self, Log};
use cpd_tui::palette::{self, Meaning};
use cpd_tui::read;
use cpd_tui::view_ports::Ctx;
use cpd_tui::widgets::legend;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, MouseEventKind};

/// The zone the rows are drawn in: `ManualClock`'s (EST in January, EDT in October), never the machine's.
fn zone() -> cpd_core::clock::TimeZone {
    cpd_core::clock::Clock::zone(&cpd_core::clock::ManualClock::new())
}

fn open(env: &TestEnv) -> Store {
    Store::open(
        &CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap()
}

/// Appends one Event per `(kind, text)`, one second apart from 00:00:00 UTC on (19:00:00 the day before in the EST of `ManualClock`'s zone: the rows are drawn there).
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

/// The rows above the Events: an empty row, the title and an empty row.
const HEAD: u16 = 3;

/// The log in an area of `height` rows of Events under the title's rows.
fn render(width: u16, height: u16, log: &mut Log) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height + HEAD)).unwrap();
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
    (HEAD..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
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
    let fx = Fx::new();
    let mut store = open(&fx.env);
    append(
        &mut store,
        0,
        &[
            ("context-created", ""),
            ("plan-phase", "Plan ship: dispatch"),
            ("error", "store busy"),
        ],
    );
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events, zone());
    let buf = render(34, 5, &mut log);
    assert_eq!(
        rows(&buf),
        [
            " 19:00:00 context-created",
            " 19:00:01 Plan ship: dispatch",
            " 19:00:02 store busy",
            "",
            ""
        ]
    );
    // the time is muted (colour 8), an `error` Event is in the failed colour and the others are read-only text (7)
    assert!(
        buf[(1, 3)].fg == palette::muted_text().fg.unwrap()
            && buf[(10, 3)].fg == palette::read_only_text().fg.unwrap()
    );
    assert_eq!(buf[(10, 5)].fg, palette::fg(Meaning::Failed).fg.unwrap());
    assert_ne!(buf[(10, 4)].fg, palette::fg(Meaning::Failed).fg.unwrap());
}

#[test]
fn a_long_event_is_cut_with_an_ellipsis_and_an_empty_log_says_so() {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events, zone());
    assert_eq!(rows(&render(20, 2, &mut log)), [" none yet", ""]);
    append(
        &mut store,
        0,
        &[("plan-phase", "a very long event text indeed")],
    );
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events, zone());
    assert_eq!(rows(&render(20, 1, &mut log)), [" 19:00:00 a very l…"]);
}

#[test]
fn the_snapshot_is_the_latest_500_events_oldest_first_and_follows_from_its_cursor() {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    append_numbered(&mut store, 0, 600);
    let snapshot = read::logs(&mut store, 500).unwrap();
    assert_eq!(snapshot.events.len(), 500);
    assert_eq!(
        (
            snapshot.events[0].text.as_str(),
            snapshot.events[499].text.as_str()
        ),
        ("event 100", "event 599")
    );
    assert_eq!(snapshot.events_cursor, 600);
    assert!(read::logs_after(&mut store, 600).unwrap().events.is_empty());
    append_numbered(&mut store, 600, 2);
    let next = read::logs_after(&mut store, snapshot.events_cursor).unwrap();
    assert_eq!(
        next.events
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>(),
        ["event 600", "event 601"]
    );
    assert_eq!(next.events_cursor, 602);
}

// ---------- following and scrolling ----------

#[test]
fn the_log_follows_while_it_shows_the_bottom_and_stops_when_scrolled_up() {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    append_numbered(&mut store, 0, 10);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events, zone());
    let last = |buf: &Buffer| rows(buf).last().unwrap().clone();
    assert_eq!(
        rows(&render(20, 4, &mut log)),
        [
            " 19:00:06 event 6",
            " 19:00:07 event 7",
            " 19:00:08 event 8",
            " 19:00:09 event 9"
        ]
    );
    assert!(log.follows());

    // at the bottom: a new Event scrolls in
    append_numbered(&mut store, 10, 1);
    log.push(read::logs_after(&mut store, 10).unwrap().events);
    assert_eq!(last(&render(20, 4, &mut log)), " 19:00:10 event 10");

    // scrolled up: new Events do not move what is shown
    log.scroll_up(2);
    assert!(!log.follows());
    let before = render(20, 4, &mut log);
    assert_eq!(
        rows(&before),
        [
            " 19:00:05 event 5",
            " 19:00:06 event 6",
            " 19:00:07 event 7",
            " 19:00:08 event 8"
        ]
    );
    append_numbered(&mut store, 11, 2);
    log.push(read::logs_after(&mut store, 11).unwrap().events);
    assert_eq!(rows(&render(20, 4, &mut log)), rows(&before));

    // back at the bottom, it follows again
    log.scroll_down(4);
    assert!(log.follows());
    assert_eq!(last(&render(20, 4, &mut log)), " 19:00:12 event 12");
    append_numbered(&mut store, 13, 1);
    log.push(read::logs_after(&mut store, 13).unwrap().events);
    assert_eq!(last(&render(20, 4, &mut log)), " 19:00:13 event 13");
}

#[test]
fn scrolling_moves_a_line_or_a_page_and_stops_at_both_ends() {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    append_numbered(&mut store, 0, 10);
    let mut log = Log::new(read::logs(&mut store, 500).unwrap().events, zone());
    let first = |log: &mut Log| rows(&render(20, 4, log))[0].clone();
    assert_eq!(first(&mut log), " 19:00:06 event 6");
    log.scroll_up(1);
    assert_eq!(first(&mut log), " 19:00:05 event 5");
    log.scroll_up(log.page());
    assert_eq!(first(&mut log), " 19:00:01 event 1");
    log.scroll_up(100);
    assert_eq!(first(&mut log), " 19:00:00 event 0");
    log.scroll_down(log.page());
    assert_eq!(first(&mut log), " 19:00:04 event 4");
    log.scroll_down(100);
    assert_eq!(first(&mut log), " 19:00:06 event 6");
    assert!(log.follows());
}

// ---------- the view loop, in this process ----------

/// How often the view looks at the store when nothing else wakes it (`events.poll_ms` of the old test's config).
const POLL: Duration = Duration::from_millis(50);

/// The log view over the store of `fx`, as `logs::run` opens it (the latest 500 Events, at `event` when there is one), run
/// on `steps` in a terminal of `screen`; what it returned. The script must be over when the view closed.
fn run_log(
    fx: &Fx,
    screen: &SharedBackend,
    event: Option<i64>,
    steps: Vec<Step>,
) -> Result<(), cpd_tui::ViewError> {
    let mut store = open(&fx.env);
    let first = read::logs(&mut store, logs::RECENT).unwrap();
    let log = match event {
        Some(id) => Log::at(
            logs::with_event(&mut store, first.events, id).unwrap(),
            id,
            zone(),
        ),
        None => Log::new(first.events, zone()),
    };
    let follow = read::Follow::new(&mut store, &[], first.events_cursor).unwrap();
    let mut input = Script::on_screen(fx.fakes.clock.clone(), screen, steps);
    let mut ctx = Ctx::new(&fx.fakes.ports, screen.terminal(), &mut input);
    let result = logs::show_with(&mut ctx, &mut store, POLL, follow, log);
    drop(ctx);
    assert_eq!(
        input.remaining(),
        0,
        "the view closed before its script was over"
    );
    result
}

/// The rows under the title of `screen`; the title itself is asserted to be a section title between two empty rows.
fn log_rows(screen: &Screen) -> Vec<String> {
    let rows = screen.rows();
    assert_eq!(
        (rows[0].as_str(), rows[2].as_str()),
        ("", ""),
        "an empty row above and below the title: {rows:?}"
    );
    assert!(
        rows[1].starts_with(" ──\u{E0B6}Logs\u{E0B4}─"),
        "the title is a section title: {:?}",
        rows[1]
    );
    rows[3..].to_vec()
}

/// A look at the rows under the title: `check` holds for them.
fn look_log(what: &'static str, check: impl Fn(&[String]) -> bool + 'static) -> Step {
    Step::look(move |screen| {
        let rows = log_rows(screen);
        assert!(check(&rows), "{what}: {rows:?}");
    })
}

/// A look at the whole screen: `check` holds for its rows.
fn look(what: &'static str, check: impl Fn(&[String]) -> bool + 'static) -> Step {
    Step::look(move |screen| {
        let rows = screen.rows();
        assert!(check(&rows), "{what}: {rows:?}");
    })
}

/// What another process does: appends `n` Events `event <first>` … to the store at `home`.
fn commit_later(home: PathBuf, first: usize, n: usize) -> Step {
    Step::then(move || {
        let mut store = Store::open(
            &CpdHome::resolve(Some(home.clone().into()), None).unwrap(),
            5000,
        )
        .unwrap();
        append_numbered(&mut store, first, n);
    })
}

/// What another process does: runs `text` on the database file of the store at `home`, not through `Store`.
fn raw_later(home: PathBuf, text: &'static str) -> Step {
    Step::then(move || {
        rusqlite::Connection::open(home.join("cpd.db"))
            .unwrap()
            .execute_batch(text)
            .unwrap()
    })
}

#[test]
fn the_view_follows_live_scrolls_and_closes_with_q_or_esc() {
    let fx = Fx::new();
    let home = fx.env.cpd_home();
    append_numbered(&mut open(&fx.env), 0, 10);
    let screen = SharedBackend::new(30, 4 + HEAD);
    let first = |row: &'static str| move |r: &[String]| r.first().is_some_and(|l| l == row);
    let last = |row: &'static str| move |r: &[String]| r.get(3).is_some_and(|l| l == row);
    let steps = vec![
        Step::look(|s| {
            let rows = log_rows(s);
            assert_eq!(rows[0], " 19:00:06 event 6", "the latest Events");
            assert_eq!(rows[3], " 19:00:09 event 9");
        }),
        // a new Event follows at the bottom
        commit_later(home.clone(), 10, 1),
        Step::Idle,
        look_log("the new Event", last(" 19:00:10 event 10")),
        // k scrolls up and the view stops following; j comes back and follows again
        Step::ch('k'),
        Step::ch('k'),
        look_log("scrolled up", first(" 19:00:05 event 5")),
        commit_later(home.clone(), 11, 1),
        // The view reads the store after each key it handles, so once `k` has scrolled it one more row, it has read
        // Event 11: it did not follow it.
        Step::ch('k'),
        Step::look(|s| {
            let rows = log_rows(s);
            assert_eq!(rows[0], " 19:00:04 event 4", "scrolled up once more");
            assert_eq!(
                rows[3], " 19:00:07 event 7",
                "the new Event did not move it: {rows:?}"
            );
        }),
        Step::key(KeyCode::PageDown),
        look_log("following again", last(" 19:00:11 event 11")),
        Step::key(KeyCode::Up),
        Step::key(KeyCode::PageUp),
        Step::key(KeyCode::PageUp),
        Step::key(KeyCode::PageUp),
        look_log("the top", first(" 19:00:00 event 0")),
        // the wheel scrolls too, three rows a notch, and the bottom follows again
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 2)),
        look_log("the wheel down", first(" 19:00:03 event 3")),
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 2)),
        look_log("the wheel down again", first(" 19:00:06 event 6")),
        Step::Event(wheel(MouseEventKind::ScrollUp, 4, 2)),
        Step::look(|s| {
            let rows = log_rows(s);
            assert_eq!(rows[0], " 19:00:03 event 3", "the wheel up");
            assert_eq!(rows[3], " 19:00:06 event 6");
        }),
        commit_later(home.clone(), 12, 1),
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 2)),
        look_log("the wheel down", first(" 19:00:06 event 6")),
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 2)),
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 2)),
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 2)),
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 2)),
        look_log("the bottom", last(" 19:00:12 event 12")),
        commit_later(home.clone(), 13, 1),
        Step::Idle,
        look_log("following again", last(" 19:00:13 event 13")),
        // q closes it
        Step::ch('q'),
    ];
    run_log(&fx, &screen, None, steps).unwrap();

    // so does Esc
    let screen = SharedBackend::new(30, 4 + HEAD);
    let steps = vec![
        look_log("the Events", |r| {
            r.get(3).is_some_and(|l| l.contains("event 13"))
        }),
        Step::key(KeyCode::Esc),
    ];
    run_log(&fx, &screen, None, steps).unwrap();
}

/// A failed read keeps the Events on screen and shows the error in the last row; the next poll reads again, and the
/// error goes when a read succeeds (tui-W4). The read is made to fail by renaming the table it reads.
#[test]
fn a_failed_read_keeps_the_events_and_shows_the_error_until_a_read_succeeds() {
    let fx = Fx::new();
    let home = fx.env.cpd_home();
    append_numbered(&mut open(&fx.env), 0, 10);
    let screen = SharedBackend::new(60, 7);
    let steps = vec![
        look("the latest Events", |r| {
            r.get(6).is_some_and(|l| l == " 19:00:09 event 9")
        }),
        // not through `Store`, which refuses a changed schema
        raw_later(home.clone(), "ALTER TABLE events RENAME TO events_gone;"),
        Step::Idle,
        Step::look(|s| {
            let rows = s.rows();
            assert!(
                rows[6].contains("no such table"),
                "the error in the last row: {rows:?}"
            );
            assert_eq!(
                rows[5], " 19:00:09 event 9",
                "the Events stay, one row less: {rows:?}"
            );
        }),
        raw_later(home, "ALTER TABLE events_gone RENAME TO events;"),
        Step::Idle,
        look("the error to go", |r| {
            r.get(6).is_some_and(|l| l == " 19:00:09 event 9")
        }),
        Step::ch('q'),
    ];
    run_log(&fx, &screen, None, steps).unwrap();
}

/// A follow that fails is shown in the last row, as a failed read is (tui-G4): the highest Event id fell below the
/// cursor, which is what a replaced database looks like. The Events stay.
#[test]
fn a_follow_that_fails_is_shown_and_the_events_stay() {
    let fx = Fx::new();
    let home = fx.env.cpd_home();
    append_numbered(&mut open(&fx.env), 0, 10);
    let screen = SharedBackend::new(80, 7);
    let steps = vec![
        look("the latest Events", |r| {
            r.get(6).is_some_and(|l| l == " 19:00:09 event 9")
        }),
        // not through `Store`, which keeps the Events append-only
        raw_later(
            home,
            "DROP TRIGGER events_no_delete; DELETE FROM events WHERE id > 5;",
        ),
        Step::Idle,
        Step::look(|s| {
            let rows = s.rows();
            assert!(
                rows[6].contains("Event cursor"),
                "the error in the last row: {rows:?}"
            );
            assert_eq!(rows[5], " 19:00:09 event 9", "the Events stay: {rows:?}");
        }),
        Step::ch('q'),
    ];
    run_log(&fx, &screen, None, steps).unwrap();
}

#[test]
fn the_logs_view_needs_a_terminal() {
    let fx = Fx::with_ports();
    let run = fx.cpd_without_a_terminal(&["tui", "view", "logs"]);
    assert_eq!(
        (run.exit, run.error()["code"].as_str()),
        (3, Some("not_a_terminal")),
        "{run:?}"
    );
}

// ---------- the legend popup scrolls too ----------

#[test]
fn a_legend_taller_than_its_pane_scrolls_with_the_wheel_and_any_other_key_closes_it() {
    let fx = Fx::new();
    let screen = SharedBackend::new(100, 10);
    let title = |rows: &[String]| rows[1].starts_with(" ──\u{E0B6}Legend: What");
    let steps = vec![
        Step::look(move |s| {
            let rows = s.rows();
            assert!(title(&rows), "the legend: {rows:?}");
            assert!(rows[1].contains(" 1-8/"), "{:?}", rows[1]);
            assert_eq!(rows[0], "", "an empty row above the title");
            assert!(rows[3].contains("Meanings"), "{rows:?}");
        }),
        Step::Event(wheel(MouseEventKind::ScrollDown, 4, 4)),
        Step::look(move |s| {
            let rows = s.rows();
            assert!(rows[1].contains(" 4-11/"), "the wheel down: {rows:?}");
            assert!(title(&rows), "the title stays");
            assert!(!rows.iter().any(|l| l.contains("Meanings")), "{rows:?}");
        }),
        Step::Event(wheel(MouseEventKind::ScrollUp, 4, 4)),
        look("the wheel up", |r| r[1].contains(" 1-8/")),
        Step::key(KeyCode::PageDown),
        look("a page down", |r| r[1].contains(" 9-16/")),
        Step::ch('x'),
    ];
    let mut input = Script::on_screen(fx.fakes.clock.clone(), &screen, steps);
    let mut ctx = Ctx::new(&fx.fakes.ports, screen.terminal(), &mut input);
    legend::show_with(&mut ctx).unwrap();
    drop(ctx);
    assert_eq!(
        input.remaining(),
        0,
        "the legend closed on the key, the last step"
    );
}

// ---------- the button ----------

/// An `Fx` on fake ports with the fake tmux server of the cockpit and client `/dev/pts/9`, 200x60 (the popup is at most
/// 48 rows).
fn fx_with_client() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cockpit();
    fx.tmux_route.fake().add_client("/dev/pts/9", 200, 60);
    fx
}

/// The `display-popup` calls of the fake tmux, as the runner saw them (the whole argv, global flags first).
fn popups(fx: &Fx) -> Vec<Vec<String>> {
    fx.calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|c| c.iter().any(|a| a == "display-popup"))
        .collect()
}

#[test]
fn the_logs_action_opens_the_popup_on_the_clicking_client() {
    let fx = fx_with_client();
    fx.cpd_silent(&["tui", "action", "logs", "--client", "/dev/pts/9"]);
    let popup = popups(&fx).remove(0);
    let at = popup.iter().position(|a| a == "display-popup").unwrap();
    // `tmux -u -L <socket> -f /dev/null display-popup -c <client> -E -T ' logs ' -w <w> -h <h> cpd tui view logs`
    // Sized like the legend, at most 80 % of the 200x60 client (`logs::popup_args`): 160x48.
    let (w, h) = cpd_tui::widgets::legend::size();
    let (w, h) = ((w + 2).min(160), (h + 2).min(48));
    assert_eq!(
        popup[at..],
        [
            "display-popup",
            "-c",
            "/dev/pts/9",
            "-E",
            "-T",
            " logs ",
            "-w",
            &w.to_string(),
            "-h",
            &h.to_string(),
            "cpd tui view logs"
        ]
    );
    assert!(
        popup[..at].starts_with(&[
            "-u".to_owned(),
            "-L".to_owned(),
            fx.env.tmux_socket().to_owned()
        ]),
        "{popup:?}"
    );

    // the legend action opens the legend, not the log
    fx.cpd_silent(&["tui", "action", "legend", "--client", "/dev/pts/9"]);
    assert_eq!(popups(&fx).len(), 2);
    assert!(popups(&fx)[1].contains(&" legend ".to_owned()));
}

// ---------- opening at an Event (`--event <id>`, the Inbox click of an engine-error or engine-down item) ----------

/// The row indexes (under the title) of `buf` whose first and last cell are the blue block (background 4, text 0).
fn selected_rows(buf: &Buffer) -> Vec<u16> {
    let width = buf.area.width;
    let block = palette::selected();
    (HEAD..buf.area.height)
        .filter(|&y| {
            [0, width - 1]
                .iter()
                .all(|&x| (Some(buf[(x, y)].bg), Some(buf[(x, y)].fg)) == (block.bg, block.fg))
        })
        .map(|y| y - HEAD)
        .collect()
}

#[test]
fn opened_at_an_event_its_line_is_the_blue_block_in_the_middle_of_the_view_and_the_view_does_not_follow()
 {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    append_numbered(&mut store, 0, 30);
    let events = read::logs(&mut store, 500).unwrap().events;
    let id = events.iter().find(|e| e.text == "event 10").unwrap().id;
    let mut log = Log::at(events, id, zone());
    let buf = render(20, 5, &mut log);
    assert_eq!(
        rows(&buf),
        [
            " 19:00:08 event 8",
            " 19:00:09 event 9",
            " 19:00:10 event 10",
            " 19:00:11 event 11",
            " 19:00:12 event 12"
        ]
    );
    assert_eq!(selected_rows(&buf), [2], "its whole line, and only it");
    assert!(!log.follows());
    // new Events do not move what is shown; scrolling to the bottom follows again
    append_numbered(&mut store, 30, 2);
    log.push(read::logs_after(&mut store, 30).unwrap().events);
    assert_eq!(rows(&render(20, 5, &mut log))[2], " 19:00:10 event 10");
    log.scroll_down(1000);
    assert!(log.follows());
    assert_eq!(rows(&render(20, 5, &mut log))[4], " 19:00:31 event 31");
}

#[test]
fn an_event_at_either_end_fills_the_view_and_one_that_does_not_exist_opens_at_the_bottom() {
    let fx = Fx::new();
    let mut store = open(&fx.env);
    append_numbered(&mut store, 0, 30);
    let events = read::logs(&mut store, 500).unwrap().events;
    let (first, last) = (events[0].id, events[29].id);
    let mut log = Log::at(events.clone(), first, zone());
    let buf = render(20, 5, &mut log);
    assert_eq!(
        (rows(&buf)[0].as_str(), rows(&buf)[4].as_str()),
        (" 19:00:00 event 0", " 19:00:04 event 4")
    );
    assert_eq!(selected_rows(&buf), [0]);
    let mut log = Log::at(events.clone(), last, zone());
    let buf = render(20, 5, &mut log);
    assert_eq!(rows(&buf)[4], " 19:00:29 event 29");
    assert_eq!(selected_rows(&buf), [4]);
    assert!(log.follows(), "its line is the bottom one");
    let mut log = Log::at(events, 9999, zone());
    let buf = render(20, 5, &mut log);
    assert_eq!(rows(&buf)[4], " 19:00:29 event 29");
    assert!(selected_rows(&buf).is_empty() && log.follows());
}

#[test]
fn the_view_opens_at_the_event_even_when_it_is_older_than_the_latest_500_and_does_not_follow() {
    let fx = Fx::new();
    let home = fx.env.cpd_home();
    append_numbered(&mut open(&fx.env), 0, 600);
    // `event 2` is Event 3: older than the 500 the view starts with, so it is read on its own and shown first
    let screen = SharedBackend::new(30, 5 + HEAD);
    let steps = vec![
        Step::look(|s| {
            let rows = log_rows(s);
            assert_eq!(rows[0], " 19:00:02 event 2", "the Event");
            assert_eq!(rows[1], " 19:01:40 event 100", "then the latest 500");
        }),
        commit_later(home, 600, 1),
        // The view reads the store after each key it handles, so once `j` has scrolled it one row, it has read Event 600:
        // it did not follow it.
        Step::ch('j'),
        Step::look(|s| {
            let rows = log_rows(s);
            assert_ne!(rows[0], " 19:00:02 event 2", "scrolled down once");
            assert_eq!(
                rows[0], " 19:01:40 event 100",
                "it does not follow: {rows:?}"
            );
        }),
        Step::ch('q'),
    ];
    run_log(&fx, &screen, Some(3), steps).unwrap();

    // an Event among the latest 500 is in the middle of the view
    let screen = SharedBackend::new(30, 5 + HEAD);
    let steps = vec![
        Step::look(|s| {
            let rows = log_rows(s);
            assert_eq!(rows[2], " 19:05:00 event 300", "the Event");
            assert_eq!(
                (rows[0].as_str(), rows[4].as_str()),
                (" 19:04:58 event 298", " 19:05:02 event 302")
            );
        }),
        Step::key(KeyCode::Esc),
    ];
    run_log(&fx, &screen, Some(301), steps).unwrap();

    // an Event that does not exist: the bottom, as without the option
    let screen = SharedBackend::new(30, 5 + HEAD);
    let steps = vec![
        look_log("the bottom", |r| {
            r.get(4).is_some_and(|l| l == " 19:10:00 event 600")
        }),
        Step::ch('q'),
    ];
    run_log(&fx, &screen, Some(99999), steps).unwrap();
}
