//! The variable-height Inbox item in the view (pro-d177): a one-row item takes one row with no blank row under it, a two-row
//! item takes two, and a click on the row under a one-row item is the next item's hit. The view runs in this process on a
//! script (`common/inbox.rs`).

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{World, click, condition, frame_with, gone, item_row, open, plan_row, rows, seed, session_row, show, show_with, view_fx};
use common::sql;
use common::tree::{engine, stamp};
use cpd_core::error::CpdError;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_tui::palette;
use cpd_tui::pump::Signal;
use serde_json::json;

const HEIGHT: u16 = 20;

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

fn blank() -> String {
    String::new()
}

fn header(count: usize) -> String {
    let count = format!("\u{E0B6}{count}\u{E0B4}");
    let fill = common::inbox::W - 3 - 7 - count.chars().count() - 1;
    format!("───\u{E0B6}Inbox\u{E0B4}{}{count}─", "─".repeat(fill))
}

/// The Contexts, the Plan `ship` with its Lead `l-ship` and forks `f-blocked`, `l-rel`; then `items`.
fn rig(items: Vec<Condition>) -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = open(&fx.env);
    let sessions = [
        session_row("l-ship", "lead", "alpha", "ship", "2026-01-02T00:00:10.000Z"),
        session_row("f-blocked", "lead", "alpha", "ship", "2026-01-02T00:00:02.000Z"),
        session_row("l-rel", "lead", "alpha", "ship", "2026-01-02T00:00:03.000Z"),
    ];
    sql(&mut store, &(plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z") + &sessions.concat()));
    reconcile(&mut store, &items, &stamp(&fx, -330)).unwrap();
    fx
}

fn on(kind: Kind, urgency: Urgency, subject: &str, label: &str, session: &str) -> Condition {
    Condition { session: Some(SessionId::parse(session).unwrap()), plan: Some(PlanId::parse("ship").unwrap()), ..condition(kind, urgency, subject, label) }
}

fn dialog(subject: &str, label: &str, session: &str) -> Condition {
    on(Kind::Dialog, Urgency::Red, subject, label, session)
}

/// The first frame's rows with the Inbox on them.
fn first_rows(fx: &Fx) -> Vec<String> {
    let frames = show(fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the Inbox section", |r| r.iter().any(|row| row.contains("Inbox")));
    rows(&frames, at)
}

/// The screen row of the first row containing `text`.
fn row_of(screen: &[String], text: &str) -> u16 {
    let at = screen.iter().position(|r| r.contains(text)).unwrap_or_else(|| panic!("{text} on\n{}", screen.join("\n")));
    u16::try_from(at).unwrap()
}

#[test]
fn the_view_draws_one_row_and_two_row_items_with_no_blank_row_between() {
    let fx = rig(vec![
        dialog("session:f-blocked", "f-blocked label\nWhich repo?", "f-blocked"),
        dialog("session:l-rel", "l-rel label", "l-rel"),
        on(Kind::Unread, Urgency::White, "session:l-ship", "ship replied\nShip it", "l-ship"),
    ]);
    let _engine = engine(&fx);
    let frames = show(&fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let want = vec![
        blank(),
        header(3),
        blank(),
        item_row(palette::TIER_RED, "f-blocked label", "5m"),
        "   Which repo?".to_owned(),
        item_row(palette::TIER_RED, "l-rel label", "5m"),
        item_row(palette::TIER_WHITE, "ship replied", "5m"),
        "   Ship it".to_owned(),
        blank(),
    ];
    let what = format!("the Inbox, rows 11.. being\n{}\n", want.join("\n"));
    let at = frame_with(&frames, 0, &what, |r| r.len() == usize::from(HEIGHT) && r[11..] == *want);
    assert_eq!(rows(&frames, at).len(), 20);
}

/// The commands a left click `down` rows under the one-row item `l-rel` runs, on a fresh rig.
fn click_ran(down: u16) -> Vec<Vec<String>> {
    let fx = rig(vec![dialog("session:l-rel", "l-rel label", "l-rel"), dialog("session:f-blocked", "f-blocked label\nWhich repo?", "f-blocked")]);
    let _engine = engine(&fx);
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    let one = row_of(&first_rows(&fx), "l-rel label");
    RAN.with(|r| r.borrow_mut().clear());
    show_with(&fx, HEIGHT, vec![Signal::Commit, click(10, one + down), gone()], &World::new(), recording);
    cpd_calls(&fx)
}

/// The commands a left click `down` rows under Row 1 of the two-row item `f-blocked` runs, on a fresh rig.
fn click_ran_second(down: u16) -> Vec<Vec<String>> {
    let fx = rig(vec![dialog("session:l-rel", "l-rel label", "l-rel"), dialog("session:f-blocked", "f-blocked label\nWhich repo?", "f-blocked")]);
    let _engine = engine(&fx);
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    let two = row_of(&first_rows(&fx), "f-blocked label");
    RAN.with(|r| r.borrow_mut().clear());
    show_with(&fx, HEIGHT, vec![Signal::Commit, click(10, two + down), gone()], &World::new(), recording);
    cpd_calls(&fx)
}

#[test]
fn a_click_on_the_row_under_a_one_row_item_is_the_next_items_hit() {
    // the Fx lives only in this block: `click_ran` makes its own, and the process holds one at a time
    let screen = {
        let fx = rig(vec![dialog("session:l-rel", "l-rel label", "l-rel"), dialog("session:f-blocked", "f-blocked label\nWhich repo?", "f-blocked")]);
        let _engine = engine(&fx);
        first_rows(&fx)
    };
    assert_eq!(row_of(&screen, "f-blocked label"), row_of(&screen, "l-rel label") + 1, "no blank row between the items");
    let (first, under, second_row_two) = (click_ran(0), click_ran(1), click_ran_second(1));
    assert_eq!(first.len(), 1, "a click on the first item runs one command: {first:?}");
    assert_eq!(under.len(), 1, "a click under the one-row item runs one command: {under:?}");
    assert_eq!(second_row_two.len(), 1, "a click on the second item's Row 2 runs one command: {second_row_two:?}");
    assert_eq!(under, second_row_two, "the row under a one-row item is the next item's");
    assert_ne!(under, first, "and not the first item's");
}
