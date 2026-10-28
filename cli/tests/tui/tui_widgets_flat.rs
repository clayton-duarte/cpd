//! The flat, bottom-aligned Inbox of the Dispatch column (commands.md §13.2, pro-d147): no fold, no rail, the section pinned to
//! the bottom of the column, a ▲ line under the header and a ▼ line under the last row. The view runs in this process on a script
//! (`common/inbox.rs`); the rows are spelled out here.

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{FOUR, W, World, click, four, frame_with, gone, many, notch, open_at, rows, seed, show, show_with, view_fx};
use common::screen::Screen;
use common::tree::{engine, stamp};
use cpd_core::error::CpdError;
use cpd_core::events::{Kind as EventKind, NewEvent, append};
use cpd_core::host::runner::Reply;
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_tui::palette;
use cpd_tui::pump::Signal;
use ratatui::crossterm::event::MouseEventKind;
use serde_json::json;

/// The usage line when nothing is scripted.
const USAGE: &str = " \u{F4BC} –%  \u{EFC5} –%";

fn pill(text: &str) -> String {
    format!("\u{E0B6}{text}\u{E0B4}")
}

/// The header at `W` columns with `count`: `───`, the title pill, the fill, the count pill, `─`.
fn header(count: usize) -> String {
    let count = pill(&count.to_string());
    let fill = W - 3 - 7 - count.chars().count() - 1;
    format!("───{}{}{count}─", pill("Inbox"), "─".repeat(fill))
}

/// An item row aged `5m`: ` <glyph> <label>`, spaces, the age ending at column `W - 2`.
fn item(glyph: &str, label: &str) -> String {
    format!(" {glyph} {label}{}5m", " ".repeat(W - 4 - label.chars().count() - 2))
}

/// A cue line: the text right-aligned to end at column `W - 2`.
fn cue(text: &str) -> String {
    format!(" {text:>w$}", w = W - 2)
}

fn blank() -> String {
    String::new()
}

fn up_to_date() -> String {
    " up to date, drink some water".to_owned()
}

fn prepared(items: &[Condition]) -> Fx {
    let fx = view_fx();
    seed(&fx, items, 330);
    fx
}

fn item_n(i: usize) -> String {
    item(palette::TIER_RED, &format!("item {i:02}"))
}

fn list(range: std::ops::Range<usize>) -> Vec<String> {
    range.map(item_n).collect()
}

/// The first frame whose rows from `from_row` on are `want` (and whose height is `height`).
fn frame_at(frames: &[Screen], from: usize, what: &str, height: usize, from_row: usize, want: &[String]) -> usize {
    let what = format!("{what}, rows {from_row}.. being\n{}\n", want.join("\n"));
    frame_with(frames, from, &what, |r| r.len() == height && r[from_row..] == *want)
}

#[test]
fn the_inbox_is_flat_and_pinned_to_the_bottom_of_the_column() {
    let fx = prepared(&four());
    let _engine = engine(&fx);
    let frames = show(&fx, 20, vec![Signal::Commit, gone()], &World::new());
    let mut want = vec![blank(), header(4), blank()];
    want.extend(FOUR.iter().map(|(g, l)| item(g, l)));
    want.push(blank());
    let at = frame_at(&frames, 0, "the flat Inbox", 20, 12, &want);
    let screen = rows(&frames, at);
    assert_eq!(screen[..3], [blank(), " homeserver".to_owned(), USAGE.to_owned()], "{screen:?}");
    assert_eq!(screen[3..12], vec![blank(); 9], "{screen:?}");
    for row in &screen {
        for word in ["│", "╰", "expand", "collapse"] {
            assert!(!row.contains(word), "{word} in {row:?}");
        }
    }
}

#[test]
fn an_empty_inbox_is_the_header_and_the_up_to_date_row_at_the_bottom() {
    let fx = prepared(&[]);
    let _engine = engine(&fx);
    let frames = show(&fx, 12, vec![Signal::Commit, gone()], &World::new());
    let want = vec![blank(), header(0), blank(), up_to_date(), blank()];
    let at = frame_at(&frames, 0, "the empty Inbox", 12, 7, &want);
    let screen = rows(&frames, at);
    assert_eq!(screen[3..7], vec![blank(); 4], "{screen:?}");
}

#[test]
fn an_inbox_that_overflows_fills_the_column_and_the_wheel_scrolls_it_between_cue_lines() {
    let fx = prepared(&many(40));
    let _engine = engine(&fx);
    let height: usize = 34;
    let section = |up: Option<usize>, range: std::ops::Range<usize>, down: Option<usize>| -> Vec<String> {
        let mut v = vec![blank(), header(40), up.map_or_else(blank, |n| cue(&format!("▲ {n} more")))];
        v.extend(list(range));
        v.push(down.map_or_else(blank, |n| cue(&format!("▼ {n} more"))));
        v
    };
    let top = section(None, 0..27, Some(13));
    let scrolled = section(Some(3), 3..30, Some(10));
    let (down, up) = (MouseEventKind::ScrollDown, MouseEventKind::ScrollUp);
    let signals = vec![
        Signal::Commit,
        notch(down, 10, 1),
        notch(up, 10, 1),
        notch(down, 10, 10),
        notch(up, 10, 10),
        gone(),
    ];
    let frames = show(&fx, 34, signals, &World::new());
    let first = frame_at(&frames, 0, "the Inbox at its top", height, 3, &top);
    // the wheel over row 1 changes nothing: the next frames are at the top until the wheel over the section
    let moved = frame_at(&frames, first, "the Inbox three down", height, 3, &scrolled);
    assert!(frames[first..moved].iter().all(|f| f.shown_rows()[3..] == top[..]), "the wheel outside the section moved the list");
    frame_at(&frames, moved, "the Inbox back at its top", height, 3, &top);
}

thread_local! {
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
}

/// `run_command` of the view under test: records the words of every command a click runs.
fn recording(argv: Vec<OsString>) -> Result<Output, CpdError> {
    let mut words: Vec<String> = argv.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    if words.first().is_some_and(|w| w == "cpd") {
        words.remove(0);
    }
    RAN.with(|ran| ran.borrow_mut().push(words));
    Ok(Output::success(json!({})))
}

/// The commands the view ran, through `run_command` and through the runner's `cpd`.
fn cpd_calls(fx: &Fx) -> Vec<Vec<String>> {
    let mut all = RAN.with(|ran| ran.borrow().clone());
    all.extend(fx.calls("cpd").into_iter().map(|call| call.args));
    all
}

/// Runs the view on `many(40)` scrolled by one notch and then `clicks`; returns the frames and what ran.
fn scrolled_and_clicked(clicks: Vec<Signal>) -> (Vec<Screen>, Vec<Vec<String>>) {
    let items: Vec<Condition> = many(40)
        .into_iter()
        .enumerate()
        .map(|(i, c)| Condition { session: Some(cpd_core::ids::SessionId::parse(&format!("item-{i:02}")).unwrap()), ..c })
        .collect();
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = common::inbox::open(&fx.env);
    let sessions: String = (0..40).map(|i| common::inbox::session_row(&format!("item-{i:02}"), "lead", "alpha", "", "2026-01-02T00:00:10.000Z")).collect();
    common::sql(&mut store, &sessions);
    reconcile(&mut store, &items, &stamp(&fx, -330)).unwrap();
    drop(store);
    let _engine = engine(&fx);
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    RAN.with(|ran| ran.borrow_mut().clear());
    let mut signals = vec![Signal::Commit, notch(MouseEventKind::ScrollDown, 10, 10)];
    signals.extend(clicks);
    signals.push(gone());
    let frames = show_with(&fx, 34, signals, &World::new(), recording);
    let ran = cpd_calls(&fx);
    (frames, ran)
}

#[test]
fn a_click_on_the_header_a_blank_row_or_a_cue_line_runs_nothing_and_a_click_on_an_item_jumps() {
    let mut scrolled = vec![blank(), header(40), cue("▲ 3 more")];
    scrolled.extend(list(3..30));
    scrolled.push(cue("▼ 10 more"));

    let (frames, ran) = scrolled_and_clicked(vec![click(10, 3), click(10, 4), click(10, 5), click(10, 33)]);
    let at = frame_at(&frames, 0, "the Inbox three down", 34, 3, &scrolled);
    for (i, f) in frames.iter().enumerate().skip(at) {
        assert_eq!(f.shown_rows()[3..], scrolled[..], "frame {i} changed after a click");
    }
    assert_eq!(ran, Vec::<Vec<String>>::new(), "those clicks run nothing");

    // item 05 is the third list row of the window (items 03-29 from row 6): row 8
    let (_, ran) = scrolled_and_clicked(vec![click(10, 8)]);
    assert_eq!(ran, [vec!["session".to_owned(), "open".to_owned(), "item-05".to_owned()]]);
}

#[test]
fn the_inbox_grows_upward_when_an_item_arrives() {
    let fx = view_fx();
    let one = || vec![common::inbox::condition(Kind::Dialog, Urgency::Red, "session:a-first", "first")];
    seed(&fx, &one(), 330);
    let _engine = engine(&fx);
    let home = fx.env.cpd_home();
    let at = stamp(&fx, -330);
    let before = vec![blank(), header(1), blank(), item(palette::TIER_RED, "first"), blank()];
    let world = World::new().at(
        {
            let before = before.clone();
            move |r| r.len() == 12 && r[7..] == before[..]
        },
        move || {
            let mut items = one();
            items.push(common::inbox::condition(Kind::Dialog, Urgency::Red, "session:b-second", "second"));
            let mut store = open_at(&home);
            reconcile(&mut store, &items, &at).unwrap();
            drop(store);
            open_at(&home)
                .write("test", |tx| {
                    append(tx, NewEvent::new(EventKind::InboxOpened, "Inbox item opened.", json!({"item": 2, "kind": "dialog", "urgency": "red", "label": "second"})))?;
                    Ok(())
                })
                .unwrap();
        },
    );
    let frames = show(&fx, 12, vec![Signal::Commit, Signal::Commit, gone()], &world);
    let first = frame_at(&frames, 0, "one item", 12, 7, &before);
    let two = vec![blank(), header(2), blank(), item(palette::TIER_RED, "first"), item(palette::TIER_RED, "second"), blank()];
    frame_at(&frames, first, "two items, grown upward", 12, 6, &two);
}
