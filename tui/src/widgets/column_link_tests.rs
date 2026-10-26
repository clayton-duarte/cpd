//! The Deploy column's PR rows and the `Link` cue (decision pro-d60): each row's first line ends in the cue, a plain left
//! click on it opens that row's PR through the `Runner` (`open <url>`), and nothing else does. The view is driven as the
//! scripted loop of `pump_scripted_tests.rs` drives it: a store, a scripted pump, the `ManualClock`, a `TestBackend`, and a
//! `FakeRunner` that records what a click ran. No real `open`, tmux or process.

use std::ffi::OsString;
use std::io;
use std::sync::Arc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{Prepared, SessionId, show_with};
use crate::link::CUE;
use crate::palette;
use crate::pump::{Feed, Signal};
use crate::test_text::row;
use crate::view_ports::{Ctx, Input};
use cpd_core::clock::ManualClock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, FakeRunner, Reply};
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;

const WIDTH: u16 = 30;
/// The pane's last column: where a PR row's first line ends in the cue.
const CUE_X: u16 = WIDTH - 1;
/// The first line of the first PR row (the blank row, the machine's two, the PRs title's three).
const FIRST: u16 = 6;

/// A first line as the User reads it: the name after a blank column, the cue in the last column.
fn line(name: &str) -> String {
    format!(" {name:<w$}{CUE}", w = usize::from(CUE_X) - 1)
}

/// A row without the escapes of any OSC 8 a cell may still carry (they take no column), to read the layout alone.
fn plain(buf: &Buffer, y: u16) -> String {
    let text = row(buf, y);
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(|c| *c == '\x07');
        } else {
            out.push(c);
        }
    }
    out
}

fn url(n: i64) -> String {
    format!("https://github.com/acme/repo/pull/{n}")
}

struct NoInput;

impl Input for NoInput {
    fn poll(&mut self, _: std::time::Duration) -> io::Result<bool> {
        panic!("the loop polls ctx.input: its events come from the pump");
    }
    fn read(&mut self) -> io::Result<Event> {
        panic!("the loop reads ctx.input: its events come from the pump");
    }
}

fn no_command(_: Vec<OsString>) -> Result<Output, CpdError> {
    Err(CpdError::new(ErrorCode::StoreBusy, "no command runs in this test"))
}

/// A home with the schema and these open, unowned pull requests (`acme/repo#n`, the nth created on day n).
fn home_with(numbers: &[i64]) -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let sql: String = numbers
        .iter()
        .map(|n| {
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/repo', {n}, '{}', 'Title {n}', 0, 'open', 'none', 'passed', '2026-01-0{n}T00:00:00.000Z', NULL, '2026-01-09T00:00:00.000Z');",
                url(*n)
            )
        })
        .collect();
    exec(&mut store, &sql);
    (dir, home)
}

fn exec(store: &mut Store, sql: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

fn mouse(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> Signal {
    Signal::Input(Event::Mouse(MouseEvent { kind, column, row, modifiers }))
}

fn click(column: u16, row: u16) -> Signal {
    mouse(MouseEventKind::Down(MouseButton::Left), column, row, KeyModifiers::NONE)
}

fn gone() -> Signal {
    Signal::InputFailed(io::Error::other("gone"))
}

/// What a scripted run left behind.
struct Seen {
    /// The last frame.
    buf: Buffer,
    /// Every call of the `open` program, as its arguments.
    opened: Vec<Vec<String>>,
    /// Every call that is not `open`'s.
    others: Vec<Call>,
    /// The view's log, empty when nothing was logged.
    log: String,
}

impl Seen {
    fn opened_urls(&self) -> Vec<String> {
        self.opened.iter().map(|args| args.join(" ")).collect()
    }
}

/// Runs the Deploy column on `home` at `WIDTH` x `height`: `tweak` changes what `prepare` read, `between` changes the store
/// after it (the next read sees it), `script` is the pump's signals up to the end of the run. `open` answers every `open`.
fn run(home: &CpdHome, height: u16, open: Reply, tweak: impl FnOnce(&mut Prepared), between: impl FnOnce(&mut Store), mut script: Vec<Signal>) -> Seen {
    let clock = ManualClock::new();
    let runner = Arc::new(FakeRunner::new(clock.clone()).on("open", |_| true, open));
    let fakes = Fakes::new();
    let ports = Ports { clock: Arc::new(clock), runner: runner.clone(), spawn: fakes.spawn.clone() };
    let mut prepared = super::prepare(&ports, home.clone()).unwrap();
    tweak(&mut prepared);
    between(&mut Store::open(home, 5000).unwrap());
    script.push(gone());
    let feed = Feed::scripted(ports.clock.clone(), script);
    let mut input = NoInput;
    let mut ctx = Ctx::new(&ports, Terminal::new(TestBackend::new(WIDTH, height)).unwrap(), &mut input);
    match show_with(&mut ctx, feed, prepared, no_command) {
        Err(crate::ViewError::Terminal(e)) => assert_eq!(e.to_string(), "gone"),
        Err(crate::ViewError::Cpd(e)) => panic!("the view failed: {}", e.message()),
        Ok(()) => panic!("the script ends the loop"),
    }
    let calls = runner.calls();
    let is_open = |c: &Call| std::path::Path::new(&c.program).file_name().is_some_and(|n| n == "open");
    Seen {
        buf: ctx.terminal.backend().buffer().clone(),
        opened: calls.iter().filter(|c| is_open(c)).map(|c| c.args.clone()).collect(),
        others: calls.iter().filter(|c| !is_open(c)).cloned().collect(),
        log: std::fs::read_to_string(home.run_dir().join("tui/widgets.log")).unwrap_or_default(),
    }
}

fn exit0() -> Reply {
    Reply::ok("")
}

fn owned(prepared: &mut Prepared, at: usize) {
    prepared.prs[at].lead = Some(SessionId::parse("l-a").unwrap());
}

fn no_tweak(_: &mut Prepared) {}
fn no_change(_: &mut Store) {}

/// The cells of the frame no escape character may be in: no OSC 8 anywhere.
fn assert_no_escape(buf: &Buffer) {
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            assert!(!buf[(x, y)].symbol().contains('\x1b'), "cell ({x}, {y}) holds an escape: {:?}", buf[(x, y)].symbol());
        }
    }
}

#[test]
fn a_pr_row_ends_its_first_line_in_the_cue_and_no_cell_holds_an_escape() {
    let (_dir, home) = home_with(&[7, 8]);
    let seen = run(&home, 30, exit0(), |p| owned(p, 0), no_change, vec![]);
    assert_eq!(row(&seen.buf, FIRST), line("acme/repo#7"), "an owned row");
    assert_eq!(row(&seen.buf, FIRST + 2), line("acme/repo#8"), "an unowned row");
    assert_no_escape(&seen.buf);
    for y in [FIRST, FIRST + 2] {
        assert_eq!(seen.buf[(CUE_X, y)].fg, palette::clickable_text().fg.unwrap(), "the cue is clickable-coloured, row {y}");
    }
    // the name is cut before the cue's column and its gap
    let narrow = {
        let clock = ManualClock::new();
        let fakes = Fakes::new();
        let ports = Ports { clock: Arc::new(clock.clone()), runner: Arc::new(FakeRunner::new(clock)), spawn: fakes.spawn.clone() };
        let prepared = super::prepare(&ports, home.clone()).unwrap();
        let mut input = NoInput;
        let feed = Feed::scripted(ports.clock.clone(), vec![gone()]);
        let mut ctx = Ctx::new(&ports, Terminal::new(TestBackend::new(9, 30)).unwrap(), &mut input);
        let _ = show_with(&mut ctx, feed, prepared, no_command);
        ctx.terminal.backend().buffer().clone()
    };
    assert_eq!(row(&narrow, FIRST), format!(" acme/… {CUE}"));
    assert_no_escape(&narrow);
}

#[test]
fn a_left_click_on_a_pr_rows_cue_opens_that_rows_url_and_nothing_else() {
    let (_dir, home) = home_with(&[7, 8]);
    let ctrl_shift = mouse(MouseEventKind::Down(MouseButton::Left), CUE_X, FIRST + 2, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
    let seen = run(&home, 30, exit0(), |p| owned(p, 0), no_change, vec![click(CUE_X, FIRST), ctrl_shift]);
    assert_eq!(seen.opened, [vec![url(7)], vec![url(8)]], "one `open <url>` for each click, any modifiers");
    assert!(seen.others.iter().all(|c| c.program != "tmux"), "no Go to Lead, no popup: {:?}", seen.others);
    assert_eq!(seen.log, "", "no failure");
}

#[test]
fn a_left_click_on_a_pr_rows_text_opens_no_page() {
    let (_dir, home) = home_with(&[7, 8]);
    let script = vec![click(3, FIRST), click(3, FIRST + 1), click(3, FIRST + 2), click(CUE_X - 1, FIRST + 2), click(CUE_X, FIRST + 3), click(CUE_X, FIRST + 2)];
    let seen = run(&home, 30, exit0(), |p| owned(p, 0), no_change, script);
    assert_eq!(seen.opened_urls(), [url(8)], "only the click on the cue opened a page");
}

#[test]
fn the_cue_of_an_unowned_pr_opens_its_url_too() {
    let (_dir, home) = home_with(&[7]);
    let seen = run(&home, 30, exit0(), no_tweak, no_change, vec![click(CUE_X, FIRST)]);
    assert_eq!(seen.opened_urls(), [url(7)]);
    assert!(seen.others.iter().all(|c| c.program != "tmux"), "{:?}", seen.others);
}

#[test]
fn after_the_rows_are_reordered_the_cue_opens_the_url_of_the_row_now_under_it() {
    let (_dir, home) = home_with(&[7, 8]);
    // PR 8 becomes the older one: the next read lists it first. A click anywhere asks for a read.
    let reorder = |store: &mut Store| exec(store, "UPDATE pull_requests SET created_at = '2025-12-01T00:00:00.000Z' WHERE number = 8;");
    let seen = run(&home, 30, exit0(), no_tweak, reorder, vec![click(0, 0), Signal::Read, click(CUE_X, FIRST), click(CUE_X, FIRST + 2)]);
    assert_eq!(plain(&seen.buf, FIRST), line("acme/repo#8"), "the frame shows the new order");
    assert_eq!(seen.opened_urls(), [url(8), url(7)]);
}

#[test]
fn a_cue_whose_row_went_away_or_scrolled_off_opens_nothing() {
    // a row went away: its cue's cell is the blank under the last row
    let (_dir, home) = home_with(&[7, 8]);
    let close = |store: &mut Store| exec(store, "UPDATE pull_requests SET state = 'closed' WHERE number = 8;");
    let seen = run(&home, 30, exit0(), no_tweak, close, vec![click(0, 0), Signal::Read, click(CUE_X, FIRST + 2), click(CUE_X, FIRST)]);
    assert_eq!(plain(&seen.buf, FIRST + 2), "", "the frame shows no second row");
    assert_eq!(seen.opened_urls(), [url(7)], "only the row still there opens its page");
    // a row scrolled off: the `▲` row now holds the cell where its cue was
    let (_dir, home) = home_with(&[7, 8, 9]);
    let wheel = mouse(MouseEventKind::ScrollDown, 3, FIRST + 1, KeyModifiers::NONE);
    let seen = run(&home, 15, exit0(), no_tweak, no_change, vec![wheel, click(CUE_X, FIRST), click(CUE_X, FIRST + 1)]);
    assert!(plain(&seen.buf, FIRST).contains('▲'), "the first row scrolled off: {:?}", plain(&seen.buf, FIRST));
    assert_eq!(seen.opened_urls(), [url(8)], "the cell of the cue row is no link, the row now in view is");
}

#[test]
fn a_refused_open_is_reported_as_a_failure_dialog_of_the_view() {
    let (_dir, home) = home_with(&[7]);
    let refused = Reply::exit(1, "", "The file does not exist.");
    let seen = run(&home, 30, refused, no_tweak, no_change, vec![click(CUE_X, FIRST)]);
    assert_eq!(seen.opened_urls(), [url(7)], "`open` ran once");
    assert!(seen.log.contains("failed:"), "the view's log: {:?}", seen.log);
    assert!(seen.log.contains(&url(7)) && seen.log.contains("The file does not exist."), "the log names the url and what `open` said: {:?}", seen.log);
}
