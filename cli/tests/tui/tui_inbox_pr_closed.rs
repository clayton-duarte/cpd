//! The `pr-closed` Inbox item in the cockpit (PLAN closed-unmerged): a red row naming the pull request, a click that goes to the
//! Plan's Lead, and a menu with no Merge and no Post. The view runs in this process on a script (`common/inbox.rs`).
//!
//! The row and the click tests may already pass at the branch's base (`Kind::PrClosed` has the `jump` arm of `PrReady`, and a
//! red item gets the tier glyph by the palette's fallback); they are kept as the contract. The menu and link tests are red.

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{
    World, condition, frame_with, gone, item_row, linked_item_row, open, plan_row, rows, seed,
    session_row, show, show_with, view_fx,
};
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
const HEIGHT: u16 = 30;
const LABEL: &str = "shop#4 closed without a merge";
const URL: &str = "https://github.com/acme/shop/pull/4";

thread_local! {
    /// The words (without the program) of every command the view ran through `run_command`, in order.
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
}

fn recording(argv: Vec<OsString>) -> Result<Output, CpdError> {
    let mut words: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if words.first().is_some_and(|w| w == "cpd") {
        words.remove(0);
    }
    RAN.with(|ran| ran.borrow_mut().push(words));
    Ok(Output::success(json!({})))
}

/// Every command of `cpd` the view ran, without the program.
fn cpd_calls(fx: &Fx) -> Vec<Vec<String>> {
    let mut all = RAN.with(|ran| ran.borrow().clone());
    all.extend(fx.calls("cpd").into_iter().map(|call| call.args));
    all
}

/// The red `pr-closed` item of Plan `ship`, with `url`.
fn closed(url: Option<&str>) -> Condition {
    Condition {
        url: url.map(str::to_owned),
        plan: Some(PlanId::parse("ship").unwrap()),
        session: Some(SessionId::parse("l-ship").unwrap()),
        ..condition(Kind::PrClosed, Urgency::Red, "pr:acme/shop#4", LABEL)
    }
}

/// The Plan `ship` (Lead `l-ship`), the item open for 330 s, and the cockpit with a client.
fn rig(url: Option<&str>) -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = open(&fx.env);
    sql(
        &mut store,
        &(plan_row(
            "ship",
            "alpha",
            "dispatch",
            "l-ship",
            "2026-01-02T00:00:10.000Z",
        ) + &session_row(
            "l-ship",
            "lead",
            "alpha",
            "ship",
            "2026-01-02T00:00:10.000Z",
        )),
    );
    reconcile(&mut store, &[closed(url)], &stamp(&fx, -330)).unwrap();
    let ok = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(
            out.status.success(),
            "tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let view = ok(&[
        "new-session",
        "-d",
        "-s",
        "cpd",
        "-n",
        "main",
        "-x",
        "120",
        "-y",
        "40",
        "-P",
        "-F",
        "#{pane_id}",
        "cpd tui view widgets",
    ]);
    ok(&["set", "-p", "-t", &view, "@cpd_view", "widgets"]);
    let chat = ok(&[
        "split-window",
        "-h",
        "-d",
        "-t",
        &view,
        "-P",
        "-F",
        "#{pane_id}",
        "cpd tui view chat",
    ]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    fx.tmux_route.fake().add_client(CLIENT, 200, 60);
    fx
}

fn first_rows(fx: &Fx) -> Vec<String> {
    let frames = show(fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the item's row", |r| {
        r.iter().any(|row| row.contains(LABEL))
    });
    rows(&frames, at)
}

#[test]
fn a_pr_closed_item_row_is_red_and_names_the_pr() {
    let fx = rig(None);
    let _engine = engine(&fx);
    let screen = first_rows(&fx);
    let expected = item_row(palette::TIER_RED, LABEL, "5m");
    assert!(
        screen.contains(&expected),
        "the exact red row {expected:?} on\n{}",
        screen.join("\n")
    );
}

#[test]
fn clicking_a_pr_closed_item_goes_to_its_lead() {
    let fx = rig(None);
    let _engine = engine(&fx);
    let screen = first_rows(&fx);
    let row = screen
        .iter()
        .position(|r| r.contains(LABEL))
        .expect("the row");
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    RAN.with(|ran| ran.borrow_mut().clear());
    let signals = vec![
        Signal::Commit,
        common::inbox::click(1, u16::try_from(row).unwrap()),
        gone(),
    ];
    show_with(&fx, HEIGHT, signals, &World::new(), recording);
    assert_eq!(
        cpd_calls(&fx),
        [vec![
            "session".to_owned(),
            "open".to_owned(),
            "l-ship".to_owned()
        ]],
        "the Lead of the Plan opens"
    );
}

#[test]
fn the_menu_of_a_pr_closed_item_has_no_merge_and_no_post() {
    let fx = rig(Some(URL));
    let _engine = engine(&fx);
    let screen = first_rows(&fx);
    let row = screen
        .iter()
        .position(|r| r.contains(LABEL))
        .expect("the row");
    let frames = show(
        &fx,
        HEIGHT,
        vec![
            Signal::Commit,
            Signal::Input(right_click(10, u16::try_from(row).unwrap())),
            gone(),
        ],
        &World::new(),
    );
    let at = frame_with(&frames, 0, "a menu", |r| {
        r.iter().any(|row| row.contains('╮'))
    });
    let screen = rows(&frames, at);
    let top = screen.iter().position(|r| r.contains('╮')).unwrap();
    let labels: Vec<String> = screen[top + 1..]
        .iter()
        .take_while(|r| !r.contains('╯'))
        .filter_map(|r| r.rsplit('│').nth(1))
        .map(|l| l.trim().to_owned())
        .collect();
    assert_eq!(
        labels,
        [
            "Go to Lead",
            "Copy Link",
            "Snooze 15 min",
            "Snooze 1 h",
            "Snooze until tomorrow 09:00",
            "Dismiss"
        ]
    );
}

#[test]
fn a_pr_closed_item_is_a_link_kind() {
    let fx = rig(Some(URL));
    let _engine = engine(&fx);
    let screen = first_rows(&fx);
    let expected = linked_item_row(palette::TIER_RED, LABEL, "5m");
    assert!(
        screen.contains(&expected),
        "the label is a link with its cue: {expected:?} on\n{}",
        screen.join("\n")
    );
}
