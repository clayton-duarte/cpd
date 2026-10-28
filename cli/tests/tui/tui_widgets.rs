//! The `widgets` view (commands.md §13.2, §17): snapshot tests of the right column's machine header and the view's loop
//! with ratatui's `TestBackend`. The Inbox under the header is `tui_widgets_column.rs`'s.

use crate::common;

use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::time::Duration;

use common::fx::Fx;
use common::screen::{NoInput, Screen, SharedBackend, Steps};
use cpd_core::error::CpdError;
use cpd_core::host::runner::Reply;
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use cpd_tui::ViewError;
use cpd_tui::pump::{Feed, Signal};
use cpd_tui::view_ports::Ctx;
use cpd_tui::palette::{self, Meaning};
use cpd_tui::read;
use cpd_tui::widgets::{self, Machine, Usage};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

fn open(fx: &Fx) -> Store {
    Store::open(&CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn machine() -> Machine {
    Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } }
}

fn render(width: u16, height: u16, w: &read::Widgets) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| drop(widgets::draw(f, f.area(), &machine(), 0, &w.inbox, 0, time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
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

/// The machine header, then the Inbox with no item directly under it (commands.md §13.2): no PRs section.
#[test]
fn the_dispatch_sections_are_titled_with_a_separator_between_each_pair() {
    let fx = Fx::new();
    let _engine = common::tree::engine(&fx);
    let w = read::widgets(&mut open(&fx), 3, 0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(30, 20)).unwrap();
    let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: None, gpus: vec![], mem: Some(7) } };
    terminal.draw(|f| drop(widgets::draw(f, f.area(), &machine, 0, &w.inbox, 0, time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
    let lines = rows(terminal.backend().buffer());
    assert_eq!(lines[..3], ["", " homeserver", " \u{F4BC} –%  \u{EFC5} 7%"], "no CPU % until a second sample");
}

#[test]
fn a_snapshot_error_is_the_last_row_in_red() {
    let fx = Fx::new();
    let _engine = common::tree::engine(&fx);
    let w = read::widgets(&mut open(&fx), 3, 0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(46, 25)).unwrap();
    terminal
        .draw(|f| {
            drop(widgets::draw(f, f.area(), &machine(), 0, &w.inbox, 0, time::OffsetDateTime::UNIX_EPOCH, &Default::default()));
            widgets::draw_error(f, f.area(), "no cockpit");
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    assert_eq!(row(&buf, 24).trim_end(), "no cockpit");
    assert_eq!(buf[(0, 24)].fg, palette::color(Meaning::Failed), "red, colour 1");
    assert_eq!(row(&buf, 23), row(&render(46, 25, &w), 23), "the rest is as without it");
    assert!(row(&buf, 23).contains("up to date"), "{}", row(&buf, 23));
}

// ---------- the view's loop, in this process ----------

/// A second of virtual time, on the `ManualClock` the view runs on.
const ONE_SECOND: Duration = Duration::from_secs(1);

/// The root command tree, in this process, on the ports the view runs on (`show` scopes them).
fn run_command(argv: Vec<OsString>) -> Result<Output, CpdError> {
    cpd::cli::run_with(&Ports::current(), argv)
}

/// A press of the character `c`, as the scripted pump hands it to the loop.
fn press(c: char) -> Signal {
    Signal::Input(common::screen::ch(c))
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

/// `text` on the database `db` through a connection of its own (not through `Store`, which refuses a changed schema and keeps the Events append-only).
fn raw(db: &Path, text: &str) {
    rusqlite::Connection::open(db).unwrap().execute_batch(text).unwrap();
}

/// What the Inbox draws in a home with no engine: its one item.
const ENGINE_DOWN: &str = "engine is down";

/// `q` does nothing in the column: with the cockpit's `pane-died` hook a view that exits is started again, so a `q` that
/// ended the program would be a restart (tui-W1).
#[test]
fn q_does_not_end_the_view() {
    let fx = view_fx();
    let clock = fx.fakes.clock.clone();
    let steps = Steps::new()
        .at("the Inbox", |s| s.contains(ENGINE_DOWN), |_| {})
        // the frame the `q` made: two seconds go by with the view still running
        .next("the frame after the q", move |_| clock.advance(2 * ONE_SECOND));
    // the script ends the view after the `q` and the time: a `q` that ended it would have returned before the script's end
    let shown = show(&fx, vec![press('q'), Signal::Due, gone()], &steps).screen();
    assert!(shown.contains(ENGINE_DOWN));
}

/// A failed read keeps the column on screen and shows the error in its last row; the next refresh reads again, and the
/// error goes when a read succeeds (tui-W4). The read is made to fail by renaming the table it reads.
#[test]
fn a_failed_read_keeps_the_column_and_shows_the_error_until_a_read_succeeds() {
    let fx = view_fx();
    let (db, clock) = (fx.env.cpd_home().join("cpd.db"), fx.fakes.clock.clone());
    let failed = |s: &Screen| s.contains("no such table");
    let steps = Steps::new()
        .at("the Inbox", |s| s.contains(ENGINE_DOWN), {
            let (db, clock) = (db.clone(), clock.clone());
            move |_| {
                // not through `Store`, which refuses a changed schema
                raw(&db, "ALTER TABLE pull_requests RENAME TO pull_requests_gone;");
                clock.advance(ONE_SECOND);
            }
        })
        .at("the error in the last row", failed, move |screen| {
            assert!(screen.contains(ENGINE_DOWN), "the column stays: {}", screen.text());
            raw(&db, "ALTER TABLE pull_requests_gone RENAME TO pull_requests;");
            clock.advance(ONE_SECOND);
        })
        .at("the error to go", move |s| !failed(s) && s.contains(ENGINE_DOWN), |_| {});
    // a commit makes the view read: it fails; the next commit makes it read again: it succeeds
    show(&fx, vec![Signal::Commit, Signal::Commit, gone()], &steps);
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
