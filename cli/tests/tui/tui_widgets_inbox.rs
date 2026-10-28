//! The Inbox section of the `widgets` view (commands.md §13.2, pro-d130): the right column draws it directly under the machine
//! header. The view runs in this process on a script (`common/inbox.rs`): the `ManualClock`, a scripted pump, no sleep,
//! no tmux; a test prepares the store, runs the loop, and reads the frames it drew. The rendering of single rows, colours
//! and keys is unit-tested in `tui/src/`.

use crate::common;
use crate::tui_titles;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use common::fx::Fx;
use common::inbox::{
    FOUR, W, World, engine_at, four, frame_with, gone, manager, open, open_at, press, rows, seed, show,
    typed, view_fx,
};
use common::tree::{engine, stamp};
use cpd_core::events::{Kind as EventKind, NewEvent, append};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_tui::palette;
use cpd_tui::pump::Signal;
use ratatui::crossterm::event::KeyCode;

const ONE_SECOND: Duration = Duration::from_secs(1);

/// The Inbox's title with the count pill, at `W` columns.
fn pill(count: usize) -> String {
    tui_titles::flush_title(W, "Inbox", count)
}

/// The section as it is drawn, pinned to the bottom: its blank row, the header with `count`, the ▲ line (blank), the item rows
/// and the ▼ line (blank), which is the pane's last row.
fn section(count: usize, items: &[String]) -> Vec<String> {
    [vec![String::new(), pill(count), String::new()], items.to_vec(), vec![String::new()]].concat()
}

/// The item rows of `items` (glyph, label), all aged `age`.
fn item_rows(items: &[(&str, &str)], age: &str) -> Vec<String> {
    items.iter().flat_map(|(glyph, label)| common::inbox::item_rows(glyph, label, "", age)).collect()
}

/// [`four`]'s section, aged `age`.
fn folded4_at(age: &str) -> Vec<String> {
    section(4, &item_rows(&FOUR, age))
}

fn folded4() -> Vec<String> {
    folded4_at("5m")
}

/// Whether `section` is the last rows of `screen`.
fn under(screen: &[String], section: &[String]) -> bool {
    screen.len() >= 3 + section.len() && screen[screen.len() - section.len()..] == *section
}

/// [`frame_with`] for a section right under the header: the first frame from `from` on that has `section` there.
fn frame_under(frames: &[common::screen::Screen], from: usize, what: &str, section: &[String]) -> usize {
    frame_with(frames, from, &format!("{what}, at the bottom, being\n{}\n", section.join("\n")), |r| under(r, section))
}

/// An `Fx` with the engine's lock held by the caller's guard, the store seeded with `items` opened 330 s ago (`5m`).
fn prepared(items: &[Condition]) -> Fx {
    let fx = view_fx();
    seed(&fx, items, 330);
    fx
}

/// The `cpd` and `tmux` commands the view ran that name the Inbox or a Session: none of the tests of this file runs one.
fn commands_of_the_inbox(fx: &Fx) -> Vec<Vec<String>> {
    let calls = fx.fakes.runner.calls();
    calls.into_iter().map(|c| c.args).filter(|a| a.iter().any(|w| w == "inbox" || w == "session")).collect()
}

#[test]
fn the_age_of_an_item_is_redrawn_every_second() {
    let fx = view_fx();
    seed(&fx, &four(), 58);
    let _engine = engine(&fx);
    let (first, second) = (fx.fakes.clock.clone(), fx.fakes.clock.clone());
    let world = World::new()
        .at(|screen| under(screen, &folded4_at("58s")), move || first.advance(ONE_SECOND))
        .at(|screen| under(screen, &folded4_at("59s")), move || second.advance(ONE_SECOND));
    let frames = show(&fx, 30, vec![Signal::Commit, Signal::Due, Signal::Due, gone()], &world);
    // the ages move with the clock: 58 s, 59 s, then the first minute
    let a = frame_under(&frames, 0, "the items aged 58s", &folded4_at("58s"));
    let b = frame_under(&frames, a, "the items aged 59s", &folded4_at("59s"));
    frame_under(&frames, b, "the items aged 1m", &folded4_at("1m"));
}

/// The glyph cell of the row that shows `label`: the second cell of ` <glyph> <label>`.
fn glyph_of(screen: &[String], label: &str) -> Option<String> {
    screen.iter().find(|r| r.contains(label)).and_then(|r| r.chars().nth(1)).map(String::from)
}

#[test]
fn a_pr_ready_item_whose_merge_runs_shows_the_spinner() {
    let fx = view_fx();
    let ready = "acme/api#3 ready";
    let mut items = vec![common::inbox::condition(Kind::Dialog, Urgency::Red, "session:f-blocked", "f-blocked label")];
    items.push(Condition {
        url: Some("https://x.io/ready".into()),
        ..common::inbox::condition(Kind::PrReady, Urgency::White, "pr:acme/api#3", ready)
    });
    seed(&fx, &items, 330);
    let _engine = engine(&fx);
    // a merge of acme/api#3 runs: its marker is there before the view starts, as a restarted view finds it
    let (home, clock) = (fx.env.cpd_home(), fx.fakes.clock.clone());
    crate::tui_merge::put_marker_at(&home, "acme__api__3.json", fx.live_pid());
    let dead = fx.ended_pid();
    let spinning = |screen: &[String]| glyph_of(screen, ready).is_some_and(|g| palette::SPINNER.contains(&g.as_str()));
    // the same marker, stale (a dead pid), is ignored as no marker is
    let world = World::new().at(spinning, {
        let home = home.clone();
        move || {
            crate::tui_merge::put_marker_at(&home, "acme__api__3.json", dead);
            clock.advance(3 * ONE_SECOND);
        }
    });
    let frames = show(&fx, 30, vec![Signal::Commit, Signal::Due, Signal::Due, gone()], &world);
    let at = frame_with(&frames, 0, "the spinner in the glyph cell of the pr-ready item", spinning);
    let screen = rows(&frames, at);
    let glyph = glyph_of(&screen, ready).unwrap();
    let red = common::inbox::item_row(palette::TIER_RED, "f-blocked label", "5m");
    let busy = common::inbox::linked_item_row(&glyph, ready, "5m");
    // each item is two rows: Row 1, then a blank Row 2
    let head = screen.iter().position(|r| *r == pill(2)).unwrap();
    assert_eq!([screen[head + 2].clone(), screen[head + 4].clone()], [red.clone(), busy], "the other items keep their tier glyph: {screen:?}");
    // stale: the tier glyph is back
    let white = common::inbox::linked_item_row(palette::ITEM_PR_READY, ready, "5m");
    frame_with(&frames, at, "the tier glyph back", |screen| screen.contains(&white));
}

/// Changes the item `subject` as `inbox dismiss` (`dismissed_at`) or `inbox snooze` (`snoozed_until`) does: the row, and its Event
/// unless `event` is false (a change with none), in one transaction.
fn change(home: &Path, subject: &str, column: &str, value: &str, event: bool) {
    open_at(home)
        .write("test", |tx| {
            tx.execute(&format!("UPDATE inbox_items SET {column} = ?2 WHERE subject = ?1"), [subject, value])?;
            if event {
                let (kind, data) = match column {
                    "dismissed_at" => (EventKind::InboxDismissed, serde_json::json!({"item": 1})),
                    _ => (EventKind::InboxSnoozed, serde_json::json!({"item": 1, "until": value})),
                };
                append(tx, NewEvent::new(kind, "Inbox item changed.", data))?;
            }
            Ok(())
        })
        .unwrap();
}

/// The Inbox follows what changes it, one run of the view per change (the view starts on what the store holds, the
/// change is made at its first frame that shows the section, and a commit tells the view): `phase` runs the view once.
fn phase(fx: &Fx, before: Vec<String>, change: impl FnMut() + 'static, signals: Vec<Signal>) -> Vec<common::screen::Screen> {
    show(fx, 30, signals, &World::new().at(move |screen| under(screen, &before), change))
}

/// The start of the `engine-down` item's row: red, first of the rows, `engine is down`.
fn engine_down() -> String {
    format!(" {} engine is down", palette::TIER_RED)
}

/// Whether the Inbox counts 5 items and its first row is `engine-down`'s.
fn shows_down(screen: &[String]) -> bool {
    screen.iter().position(|r| *r == pill(5)).is_some_and(|t| screen[t + 2].starts_with(&engine_down()))
}

#[test]
fn the_inbox_follows_its_events_a_started_engine_and_a_snooze_that_ends() {
    let fx = view_fx();
    seed(&fx, &four(), 330);
    let home = fx.env.cpd_home();
    let commits = || vec![Signal::Commit, Signal::Commit, gone()];

    // an engine that starts takes `engine-down` away; it has no Event, the view reads when the clock moves on
    let held = Rc::new(RefCell::new(None));
    let world = {
        let (held, clock, home) = (Rc::clone(&held), fx.fakes.clock.clone(), home.clone());
        World::new().at(shows_down, move || {
            *held.borrow_mut() = Some(engine_at(&home));
            clock.advance(3 * ONE_SECOND);
        })
    };
    let frames = show(&fx, 30, vec![Signal::Commit, Signal::Due, Signal::Due, gone()], &world);
    let down = frame_with(&frames, 0, "the engine-down item first, 5 items counted", shows_down);
    frame_under(&frames, down, "the four items once an engine holds the lock", &folded4());
    assert!(held.borrow().is_some(), "the engine's lock was taken");

    // inbox-opened: a fifth item, opened 300 s ago, comes after the two red ones that are older
    let five = || {
        let mut items = four();
        items.push(common::inbox::condition(Kind::Dialog, Urgency::Red, "session:new", "new label"));
        items
    };
    let with_new = section(5, &item_rows(&[FOUR[0], FOUR[1], (palette::TIER_RED, "new label"), FOUR[2], FOUR[3]], "5m"));
    let (opened, at) = (home.clone(), stamp(&fx, -300));
    let frames = phase(&fx, folded4(), move || drop(reconcile(&mut open_at(&opened), &five(), &at).unwrap()), commits());
    frame_under(&frames, 0, "the fifth item after inbox-opened", &with_new);

    // inbox-closed: its cause ends
    let (closed, at) = (home.clone(), stamp(&fx, -300));
    let frames = phase(&fx, with_new, move || drop(reconcile(&mut open_at(&closed), &four(), &at).unwrap()), commits());
    frame_under(&frames, 0, "the four items after inbox-closed", &folded4());

    // inbox-dismissed: the first leaves
    let three = section(3, &item_rows(&[FOUR[1], FOUR[2], FOUR[3]], "5m"));
    let dismissed = home.clone();
    let frames = phase(&fx, folded4(), move || change(&dismissed, "session:f-blocked", "dismissed_at", "2026-01-04T00:00:00.000Z", true), commits());
    frame_under(&frames, 0, "three items after inbox-dismissed", &three);

    // inbox-snoozed: `m-beta` hides for 8 s
    let two = section(2, &item_rows(&[FOUR[1], FOUR[3]], "5m"));
    let (snoozed, until) = (home, stamp(&fx, 8));
    let frames = phase(&fx, three.clone(), move || change(&snoozed, "session:m-beta", "snoozed_until", &until, true), commits());
    frame_under(&frames, 0, "two items after inbox-snoozed", &two);

    // the snooze ends by the clock, with no Event: the item is back, and 9 s older still `5m`
    let clock = fx.fakes.clock.clone();
    let frames = phase(&fx, two, move || clock.advance(9 * ONE_SECOND), vec![Signal::Commit, Signal::Due, Signal::Due, gone()]);
    frame_under(&frames, 0, "the snoozed item back", &three);
}

#[test]
fn the_engine_down_item_shows_first_when_no_engine_and_no_manager() {
    let fx = prepared(&four());
    // no engine holds the lock and no Manager lives: red, first of the rows, counted
    let frames = show(&fx, 30, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the engine-down item first and 5 items counted", |screen| {
        let want = section(5, &item_rows(&FOUR, "5m"));
        let start = screen.len() - want.len() - 2;
        screen[start..start + 3] == want[..3]
            && screen[start + 3].starts_with(&engine_down())
            && screen[start + 4].is_empty()
            && screen[start + 5..] == want[3..]
    });
    assert!(at < frames.len());
    // with a live Manager the engine's absence is no item
    manager(&fx.env, "alpha");
    let frames = show(&fx, 30, vec![Signal::Commit, gone()], &World::new());
    frame_under(&frames, 0, "the four items, no engine-down", &folded4());
}

#[test]
fn the_deploy_column_has_no_keys_for_the_inbox() {
    let fx = prepared(&four());
    let _engine = engine(&fx);
    let signals = vec![
        Signal::Commit,
        press(KeyCode::Up),
        press(KeyCode::Down),
        press(KeyCode::Enter),
        typed('s'),
        press(KeyCode::Down),
        press(KeyCode::Enter),
        typed('d'),
        typed('j'),
        typed('k'),
        press(KeyCode::Esc),
        gone(),
    ];
    let frames = show(&fx, 30, signals, &World::new());
    // the section is on screen, and every frame from then on is the same: no cursor moves a row, no menu opens
    let first = frame_under(&frames, 0, "the folded Inbox", &folded4());
    for (i, frame) in frames.iter().enumerate().skip(first) {
        let screen = frame.shown_rows();
        assert!(under(&screen, &folded4()), "frame {i}");
    }
    assert_eq!(commands_of_the_inbox(&fx), Vec::<Vec<String>>::new(), "no command ran");
    let touched = open(&fx.env)
        .read(|c| Ok(c.query_row("SELECT count(*) FROM inbox_items WHERE dismissed_at IS NOT NULL OR snoozed_until IS NOT NULL", [], |r| r.get::<_, i64>(0))?))
        .unwrap();
    assert_eq!(touched, 0, "`s` and `d` snoozed and dismissed nothing");
}
