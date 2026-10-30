//! The Inbox rows of the link kinds and the `Link` widget (decision pro-d60, assumption A7): the label of an item of kind
//! `plan-review`, `pr-ready` or `review-requested` that has a url is the `Link`'s text and the cue follows it
//! (`<label> 󱆭`, the label underlined, the whole output the click target); a plain left click on it opens the url through the
//! `Runner` (`open <url>`) and runs no jump; a click elsewhere on the row jumps as before. The Dispatch column is driven as
//! `widgets/column_link_tests.rs` drives it: a store, a scripted pump, the `ManualClock`, a `TestBackend` and a `FakeRunner`
//! that records what a click ran. No real `open`, tmux or process.

use std::ffi::OsString;
use std::io;
use std::sync::Arc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Modifier;

use crate::link::CUE;
use crate::palette;
use crate::pump::{Feed, Signal};
use crate::test_text::row;
use crate::view_ports::{Ctx, Input};
use crate::widgets::{prepare, show_with};
use cpd_core::clock::ManualClock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, FakeRunner, Reply};
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;

const WIDTH: u16 = 41;
const HEIGHT: u16 = 30;
/// An item's age ends at column W-2: the flat Inbox's rows have one blank column on each side.
const AGE_END: u16 = WIDTH - 2;
const URL: &str = "https://github.com/acme/api/pull/12";

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

/// An open Inbox item to seed: its kind, subject, label and url.
struct Seed {
    kind: &'static str,
    subject: &'static str,
    label: String,
    url: Option<&'static str>,
}

fn seed(kind: &'static str, subject: &'static str, label: &str, url: Option<&'static str>) -> Seed {
    Seed { kind, subject, label: label.to_owned(), url }
}

/// A home with the schema and these open items (and no engine, so the view also draws its `engine-down` item).
fn home_with(items: &[Seed]) -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let sql: String = items
        .iter()
        .map(|s| {
            let url = s.url.map_or("NULL".to_owned(), |u| format!("'{u}'"));
            format!(
                "INSERT INTO inbox_items (kind, urgency, subject, label, url, opened_at) VALUES ('{}', 'magenta', '{}', '{}', {url}, '2026-01-01T00:00:00.000Z');",
                s.kind, s.subject, s.label
            )
        })
        .collect();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    (dir, home)
}

fn click(column: u16, row: u16) -> Signal {
    Signal::Input(Event::Mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }))
}

fn click_with(column: u16, row: u16, modifiers: KeyModifiers) -> Signal {
    Signal::Input(Event::Mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers }))
}

/// What a scripted run left behind.
struct Seen {
    buf: Buffer,
    /// The arguments of every `open` call.
    opened: Vec<Vec<String>>,
    /// Every call of `tmux`: what a jump runs (a url copy, a popup).
    tmux: Vec<Vec<String>>,
}

fn named(call: &Call, program: &str) -> bool {
    std::path::Path::new(&call.program).file_name().is_some_and(|n| n == program)
}

/// Runs the Dispatch column on `home` at `WIDTH` x `HEIGHT` through `script`; `open` and `tmux` answer exit 0.
fn run(home: &CpdHome, mut script: Vec<Signal>) -> Seen {
    let clock = ManualClock::new();
    let runner = Arc::new(FakeRunner::new(clock.clone()).on("open", |_| true, Reply::ok("")).on("tmux", |_| true, Reply::ok("")));
    let fakes = Fakes::new();
    let ports = Ports { clock: Arc::new(clock), runner: runner.clone(), spawn: fakes.spawn.clone() };
    let prepared = prepare(&ports, home.clone()).unwrap();
    script.push(Signal::InputFailed(io::Error::other("gone")));
    let feed = Feed::scripted(ports.clock.clone(), script);
    let mut input = NoInput;
    let mut ctx = Ctx::new(&ports, Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap(), &mut input);
    match show_with(&mut ctx, feed, prepared, no_command) {
        Err(crate::ViewError::Terminal(e)) => assert_eq!(e.to_string(), "gone"),
        Err(crate::ViewError::Cpd(e)) => panic!("the view failed: {}", e.message()),
        Ok(()) => panic!("the script ends the loop"),
    }
    let calls = runner.calls();
    Seen {
        buf: ctx.terminal.backend().buffer().clone(),
        opened: calls.iter().filter(|c| named(c, "open")).map(|c| c.args.clone()).collect(),
        tmux: calls.iter().filter(|c| named(c, "tmux")).map(|c| c.args.clone()).collect(),
    }
}

fn frame(home: &CpdHome) -> Buffer {
    run(home, vec![]).buf
}

/// Where `text` starts in the frame: its column and row (every cell of the Inbox holds one `char`).
fn locate(buf: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buf.area.height {
        let line: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        let joined = line.concat();
        if let Some(at) = joined.find(text) {
            return (u16::try_from(joined[..at].chars().count()).unwrap(), y);
        }
    }
    panic!("{text:?} is not in the frame: {:?}", (0..buf.area.height).map(|y| row(buf, y)).collect::<Vec<_>>());
}

fn underlined(buf: &Buffer, x: u16, y: u16) -> bool {
    buf[(x, y)].modifier.contains(Modifier::UNDERLINED)
}

fn assert_no_escape(buf: &Buffer) {
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            assert!(!buf[(x, y)].symbol().contains('\x1b'), "cell ({x}, {y}) holds an escape: {:?}", buf[(x, y)].symbol());
        }
    }
}

fn cues(buf: &Buffer) -> usize {
    (0..buf.area.height).flat_map(|y| (0..buf.area.width).map(move |x| (x, y))).filter(|&(x, y)| buf[(x, y)].symbol() == CUE).count()
}

/// The three link kinds: (kind, subject, label), subjects that need no Plan or Session to exist.
const KINDS: [(&str, &str, &str); 3] = [
    ("plan-review", "plan-review:ship", "ship plan review"),
    ("pr-ready", "pr:acme/api#12", "api#12 ready"),
    ("review-requested", "review-requested:acme/api#12", "api#12 failing"),
];

/// Whether a `set-buffer` call (a url copy) is among `tmux`.
fn copied(tmux: &[Vec<String>]) -> Vec<String> {
    tmux.iter().filter(|a| a.iter().any(|w| w == "set-buffer")).filter_map(|a| a.last().cloned()).collect()
}

#[test]
fn a_link_kind_row_draws_its_label_underlined_then_the_cue_and_no_cell_holds_an_escape() {
    for (kind, subject, label) in KINDS {
        let (_dir, home) = home_with(&[seed(kind, subject, label, Some(URL))]);
        let buf = frame(&home);
        let (x, y) = locate(&buf, label);
        let width = u16::try_from(label.chars().count()).unwrap();
        for dx in 0..width {
            assert!(underlined(&buf, x + dx, y), "{kind}: label cell {dx} is underlined");
        }
        assert!(!underlined(&buf, x - 1, y), "{kind}: the lead's space is not underlined");
        assert_eq!(buf[(x + width, y)].symbol(), " ", "{kind}: one plain gap");
        assert!(!underlined(&buf, x + width, y), "{kind}: the gap is not underlined");
        assert_eq!(buf[(x + width + 1, y)].symbol(), CUE, "{kind}: the cue follows the label");
        assert!(!underlined(&buf, x + width + 1, y), "{kind}: the cue is not underlined");
        assert_eq!(buf[(x + width + 1, y)].fg, palette::clickable_text().fg.unwrap(), "{kind}: the cue is clickable-coloured");
        assert_eq!(cues(&buf), 1, "{kind}: one cue in the frame");
        assert_no_escape(&buf);
    }
}

#[test]
fn a_click_on_the_label_or_the_cue_opens_the_items_url_and_runs_no_jump() {
    for (kind, subject, label) in KINDS {
        let (_dir, home) = home_with(&[seed(kind, subject, label, Some(URL))]);
        let (x, y) = locate(&frame(&home), label);
        let width = u16::try_from(label.chars().count()).unwrap();
        let cue = x + width + 1;
        let script = vec![click(x, y), click(x + width / 2, y), click(x + width - 1, y), click(cue, y), click_with(x, y, KeyModifiers::CONTROL | KeyModifiers::SHIFT)];
        let seen = run(&home, script);
        assert_eq!(seen.opened, vec![vec![URL.to_owned()]; 5], "{kind}: one `open <url>` per click, any modifiers");
        assert!(seen.tmux.is_empty(), "{kind}: no jump, no url copy, no popup: {:?}", seen.tmux);
    }
}

#[test]
fn a_click_elsewhere_on_a_link_kind_row_jumps_as_before() {
    // `review-requested` jumps by copying the url: the glyph, the gap before the age and the age do; the label does not
    let (kind, subject, label) = KINDS[2];
    let (_dir, home) = home_with(&[seed(kind, subject, label, Some(URL))]);
    let (x, y) = locate(&frame(&home), label);
    let width = u16::try_from(label.chars().count()).unwrap();
    let on_label = run(&home, vec![click(x, y)]);
    assert_eq!(on_label.opened, [vec![URL.to_owned()]], "a click on the label opens the page");
    assert_eq!(copied(&on_label.tmux), Vec::<String>::new(), "and copies nothing");
    for (what, column) in [("the glyph", x - 2), ("the gap after the cue", x + width + 2), ("the age", AGE_END)] {
        let seen = run(&home, vec![click(column, y)]);
        assert_eq!(copied(&seen.tmux), [URL], "a click on {what} copies the url, as before");
        assert!(seen.opened.is_empty(), "a click on {what} opens no page: {:?}", seen.opened);
    }
}

#[test]
fn a_long_label_is_cut_before_the_cue_and_the_age_keeps_its_column() {
    let long = "abcdefghij-klmnopqrst-uvwxyz-0123456789-ABCDEFGHIJ";
    let (_dir, home) = home_with(&[seed("review-requested", "review-requested:acme/api#12", long, Some(URL))]);
    let buf = frame(&home);
    let (x, y) = locate(&buf, "abcdefghij");
    let cue = (x..WIDTH).find(|&c| buf[(c, y)].symbol() == CUE).expect("the cue is drawn");
    assert_eq!(buf[(cue - 1, y)].symbol(), " ", "the gap");
    assert_eq!(buf[(cue - 2, y)].symbol(), "…", "the text is cut with an ellipsis, right before the gap");
    for c in x..cue - 1 {
        assert!(underlined(&buf, c, y), "the cut text is underlined, cell {c}");
    }
    assert!(!underlined(&buf, cue, y));
    let age_end = u16::try_from(row(&buf, y).chars().count()).unwrap() - 1;
    assert_eq!(age_end, AGE_END, "the age keeps its column");
    assert!(cue + 1 < AGE_END, "the cue never reaches the age: cue at {cue}");
    assert_eq!(buf[(cue + 1, y)].symbol(), " ", "a blank between the cue and the age");
    assert_no_escape(&buf);
    let seen = run(&home, vec![click(cue, y), click(AGE_END, y)]);
    assert_eq!(seen.opened, [vec![URL.to_owned()]], "the cue opens the page, the age does not");
}

#[test]
fn a_kind_that_is_no_link_draws_no_cue_and_opens_nothing() {
    // a dialog with a url is no link kind; the review request beside it is the control: the frame holds its cue only
    let (_dir, home) = home_with(&[
        seed("dialog", "dialog:one", "dialog label", Some(URL)),
        seed("review-requested", "review-requested:acme/api#12", "api#12 failing", Some(URL)),
    ]);
    let buf = frame(&home);
    assert_eq!(cues(&buf), 1, "only the review request's row draws a cue");
    let (x, y) = locate(&buf, "dialog label");
    assert!(!(0..WIDTH).any(|c| underlined(&buf, c, y)), "no underlined cell on the dialog's row");
    let (rx, ry) = locate(&buf, "api#12 failing");
    let seen = run(&home, vec![click(x, y), click(x + 5, y), click(rx, ry)]);
    assert_eq!(seen.opened, [vec![URL.to_owned()]], "only the click on the link kind's label opens a page");
}

#[test]
fn a_link_kind_item_with_no_url_draws_no_link_and_its_click_does_nothing() {
    // a `pr-ready` item with no url: a plain click does nothing (pro-d111); the one beside it, with a url, is the control
    let (_dir, home) = home_with(&[
        seed("pr-ready", "pr:acme/api#3", "api#3 ready", None),
        seed("review-requested", "review-requested:acme/api#12", "api#12 failing", Some(URL)),
    ]);
    let buf = frame(&home);
    assert_eq!(cues(&buf), 1, "only the item with a url draws a cue");
    let (x, y) = locate(&buf, "api#3 ready");
    assert!(!(0..WIDTH).any(|c| underlined(&buf, c, y)), "no underlined cell on the item with no url");
    let before = run(&home, vec![]).tmux.len();
    let seen = run(&home, vec![click(x, y)]);
    assert!(seen.opened.is_empty(), "no page opens: {:?}", seen.opened);
    assert_eq!(seen.tmux.len(), before, "the click on its label runs nothing: {:?}", seen.tmux);
}
