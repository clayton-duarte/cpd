//! The two-row Inbox item in the view (pro-d171): Row 1 and Row 2 drawn under the header and bottom-aligned, and a left or right
//! click on Row 2 acting like one on Row 1. The view runs in this process on a script (`common/inbox.rs`).

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{World, click, condition, frame_with, gone, item_row, open, plan_row, rows, seed, session_row, show, show_with, view_fx};
use common::screen::right_click;
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

const CLIENT: &str = "/dev/pts/9";
const HEIGHT: u16 = 20;
const URL: &str = "https://github.com/acme/shop/pull/4";

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

/// The Contexts, the Plan `ship` with its Lead `l-ship` and fork `f-blocked`; then `items`.
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

fn dialog(subject: &str, label: &str, session: &str) -> Condition {
    Condition { session: Some(SessionId::parse(session).unwrap()), plan: Some(PlanId::parse("ship").unwrap()), ..condition(Kind::Dialog, Urgency::Red, subject, label) }
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
fn the_inbox_draws_a_two_row_label_on_two_rows_at_the_bottom() {
    let fx = rig(vec![dialog("session:f-blocked", "f-blocked label\nWhich repo?", "f-blocked"), dialog("session:l-rel", "l-rel label", "l-rel")]);
    let _engine = engine(&fx);
    let frames = show(&fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let want = vec![
        blank(),
        header(2),
        blank(),
        item_row(palette::TIER_RED, "f-blocked label", "5m"),
        "   Which repo?".to_owned(),
        item_row(palette::TIER_RED, "l-rel label", "5m"),
        blank(),
        blank(),
    ];
    let what = format!("the two-row Inbox, rows 12.. being\n{}\n", want.join("\n"));
    let at = frame_with(&frames, 0, &what, |r| r.len() == usize::from(HEIGHT) && r[12..] == *want);
    assert_eq!(rows(&frames, at).len(), 20);
}

/// The commands a left click at the offset `down` rows under Row 1 of the only item runs, on a fresh rig.
fn click_ran(down: u16) -> Vec<Vec<String>> {
    let fx = rig(vec![dialog("session:f-blocked", "f-blocked label\nWhich repo?", "f-blocked")]);
    let _engine = engine(&fx);
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    let one = row_of(&first_rows(&fx), "f-blocked label");
    RAN.with(|r| r.borrow_mut().clear());
    show_with(&fx, HEIGHT, vec![Signal::Commit, click(10, one + down), gone()], &World::new(), recording);
    cpd_calls(&fx)
}

#[test]
fn a_left_click_on_row_two_jumps_like_a_click_on_row_one() {
    let (one, two) = (click_ran(0), click_ran(1));
    assert_eq!(one.len(), 1, "a click on Row 1 jumps: {one:?}");
    assert_eq!(two, one, "Row 2 runs what Row 1 runs");
}

fn menu_labels(screen: &[String]) -> Vec<String> {
    let Some(top) = screen.iter().position(|r| r.contains('╮')) else { return vec![] };
    screen[top + 1..].iter().take_while(|r| !r.contains('╯')).filter_map(|r| r.rsplit('│').nth(1)).map(|l| l.trim().to_owned()).collect()
}

/// The labels of the menu a right click at `y` opens (none when no menu opens).
fn menu_at(fx: &Fx, y: u16) -> Vec<String> {
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    let signals = vec![Signal::Commit, Signal::Input(right_click(10, y)), gone()];
    let frames = show_with(fx, HEIGHT, signals, &World::new(), recording);
    frames.iter().enumerate().map(|(i, _)| menu_labels(&rows(&frames, i))).find(|m| !m.is_empty()).unwrap_or_default()
}

#[test]
fn a_right_click_on_row_two_opens_the_items_menu() {
    let pr = Condition {
        session: Some(SessionId::parse("l-ship").unwrap()),
        plan: Some(PlanId::parse("ship").unwrap()),
        url: Some(URL.to_owned()),
        ..condition(Kind::PrReady, Urgency::Yellow, "pr:acme/shop#4", "shop#4 ready to merge\nA title")
    };
    let fx = rig(vec![pr]);
    let _engine = engine(&fx);
    fx.tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "120", "-y", "40", "cpd tui view widgets"]);
    fx.tmux_route.fake().add_client(CLIENT, 200, 60);
    let screen = first_rows(&fx);
    let one = row_of(&screen, "shop#4 ready to merge");
    let first = menu_at(&fx, one);
    assert!(first.iter().any(|l| l == "Dismiss"), "a right click on Row 1 opens the menu: {first:?}");
    assert_eq!(menu_at(&fx, one + 1), first, "Row 2 opens the same menu");
}
