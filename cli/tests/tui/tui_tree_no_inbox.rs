//! The tree view is the Context tree only (commands.md §13.2, pro-d55): the Inbox section moved to the `widgets` view, so with
//! open Inbox items in the store the tree draws only Contexts, Plans and Sessions, its cursor walks Sessions only (`s` and `d`
//! are no keys any more), a right click where the Inbox was opens no menu, and the view no longer redraws every second for
//! the age of an item. The view runs in this process on a `Tree` (`common/tree.rs`): the `ManualClock`, the fake `cpd`.
//!
//! The store is the shape of `tui_tree.rs`'s `populate` in little: two Contexts with their Managers (so `engine-down` is no
//! item; the engine's lock is held too), the Leads `l-ship` (and its fork `f-idle`) and `l-rel`, and the four open items of
//! `items`, whose rows the tree drew at the bottom of its column: three rows and `expand`.

use crate::common;

use std::time::Duration;

use common::inbox::{plan_row, session_row};
use common::tree::{self as drive, Tree, engine, fx, stamp};
use common::{TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

fn condition(kind: Kind, urgency: Urgency, session: &str, context: &str) -> Condition {
    Condition {
        kind,
        urgency,
        subject: format!("session:{session}"),
        label: format!("{session} label"),
        url: None,
        context: Some(ContextId::parse(context).unwrap()),
        plan: None,
        job: None,
        session: Some(SessionId::parse(session).unwrap()),
        record: None,
        event: None,
        opened_at: None,
    }
}

fn items() -> Vec<Condition> {
    vec![
        condition(Kind::Dialog, Urgency::Red, "f-idle", "alpha"),
        condition(Kind::Dialog, Urgency::Red, "l-rel", "beta"),
        condition(Kind::Unread, Urgency::White, "m-alpha", "alpha"),
        condition(Kind::Dialog, Urgency::Magenta, "m-beta", "beta"),
    ]
}

fn populate(fx: &common::fx::Fx, forks: usize) {
    let mut store = open(&fx.env);
    let mut s = context("alpha") + &context("beta");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z");
    s += &plan_row("rel", "beta", "plan", "l-rel", "2026-01-02T00:00:02.000Z");
    s += &session_row("l-ship", "lead", "alpha", "ship", "2026-01-02T00:00:10.000Z");
    s += &session_row("f-idle", "lead", "alpha", "ship", "2026-01-02T00:00:02.000Z");
    s += &session_row("l-rel", "lead", "beta", "rel", "2026-01-02T00:00:02.000Z");
    for n in 0..forks {
        s += &session_row(&format!("f-{n:02}"), "lead", "alpha", "ship", &format!("2026-01-01T00:00:{n:02}.000Z"));
    }
    s += &session_row("m-alpha", "manager", "alpha", "", "2026-01-02T00:00:07.000Z");
    s += &session_row("m-beta", "manager", "beta", "", "2026-01-02T00:00:07.000Z");
    sql(&mut store, &s);
    reconcile(&mut store, &items(), &stamp(fx, -330)).unwrap();
}


/// The tree's dots: the filled one (a Session or Context with an open item) and the hollow one.
const FILLED: &str = "\u{e3e3}";
const HOLLOW: &str = "\u{e3d5}";

/// A pane 46 columns wide: the Inbox had the rows `[tree][empty][title][3 items][expand]` below the tree in 21 rows.
const WIDTH: u16 = 46;

/// The Contexts, Plans and Sessions of `populate(fx, 0)`, as the tree draws them: all of it, nothing below.
fn the_tree() -> Vec<String> {
    vec![
        String::new(),
        format!(" ╭{FILLED} beta"),
        format!(" ╰─{FILLED} rel"),
        String::new(),
        format!(" ╭{FILLED} alpha"),
        format!(" ├─{FILLED} ship·f-id"),
        format!(" ╰─{HOLLOW} ship"),
    ]
}

/// The count of open items in the store: the Inbox is not empty, whatever the tree draws.
fn open_items(fx: &common::fx::Fx) -> i64 {
    fx.env.query("SELECT count(*) FROM inbox_items WHERE closed_at IS NULL")
}

/// The first screen of `tree`: it has drawn the tree.
fn ready(tree: &Tree) -> Vec<String> {
    let screen = tree.screen();
    assert!(screen.iter().any(|r| r.contains("alpha")), "the tree: {screen:?}");
    screen
}

/// The screen has a menu box whose first inner row (after the top border) holds `first`.
fn menu_open(screen: &[String], first: &str) -> bool {
    screen.windows(2).any(|w| w[0].contains('╮') && w[1].contains(&format!("│ {first}")))
}

fn the_menu_is_closed(screen: &[String]) -> bool {
    screen.iter().all(|r| !r.contains('╮') && !r.contains('╯'))
}

#[test]
fn the_tree_draws_no_inbox_section_while_items_are_open() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, 0);
    assert_eq!(open_items(&fx), 4, "the Inbox has items");
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, WIDTH, 21);
    ready(&tree);
    // the tree from its blank top row, and nothing below it: no title with its count, no item row, no `expand`
    tree.idle();
    assert_eq!(tree.screen(), the_tree());
    // a click on what used to be the `expand` row folds nothing and runs nothing
    tree.click(10, 20);
    tree.idle();
    assert_eq!(tree.screen(), the_tree());
    assert!(drive::calls(&fx).is_empty(), "{:?}", drive::calls(&fx));
}

#[test]
fn a_long_tree_takes_the_whole_column_below_its_blank_top_row() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, 20);
    assert_eq!(open_items(&fx), 4, "the Inbox has items");
    // 27 rows in a column of 30: the tree used to be capped at two thirds of it (20 rows, then `▼ 5 more`) for the Inbox below
    let tree = Tree::start(&fx, WIDTH, 30);
    ready(&tree);
    let forks: Vec<String> = (0..20).rev().map(|n| format!(" {}─{HOLLOW} ship·f-{n:02}", if n == 0 { "╰" } else { "├" })).collect();
    let expected = [the_tree()[..5].to_vec(), vec![format!(" ├─{FILLED} ship·f-id"), format!(" ├─{HOLLOW} ship")], forks].concat();
    assert_eq!(expected.len(), 27);
    assert_eq!(tree.screen(), expected);
}

#[test]
fn the_cursor_stops_at_the_last_session_and_never_reaches_an_item() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, 0);
    assert_eq!(open_items(&fx), 4, "the Inbox has items");
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, WIDTH, 21);
    ready(&tree);
    // three Sessions, in the order they are drawn: `rel`, `ship·f-id`, `ship`; more presses than rows stop on the last
    tree.down(8);
    tree.keys(&["Enter"]);
    assert_eq!(drive::calls(&fx), [drive::words(&["session", "open", "l-ship"])]);
    tree.keys(&["k", "Enter"]);
    assert_eq!(drive::calls(&fx)[1], drive::words(&["session", "open", "f-idle"]));
    tree.keys(&["Up", "Enter"]);
    assert_eq!(drive::calls(&fx)[2], drive::words(&["session", "open", "l-rel"]));
    tree.keys(&["Up", "Enter"]);
    assert_eq!(drive::calls(&fx)[3], drive::words(&["session", "open", "l-rel"]), "Up stops at the first Session");
    assert_eq!(drive::calls(&fx).len(), 4, "{:?}", drive::calls(&fx));
}

#[test]
fn down_past_the_last_session_then_s_and_d_run_nothing() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, 0);
    assert_eq!(open_items(&fx), 4, "the Inbox has items");
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, WIDTH, 21);
    ready(&tree);
    tree.down(8);
    tree.keys(&["s", "d"]);
    tree.idle();
    let screen = tree.screen();
    assert!(the_menu_is_closed(&screen), "`s` opens no snooze choices:\n{}", screen.join("\n"));
    assert!(!menu_open(&screen, "Snooze 15 min"), "{screen:?}");
    assert_eq!(drive::calls(&fx), Vec::<Vec<String>>::new(), "`d` dismisses nothing, `s` snoozes nothing");
    assert_eq!(screen, the_tree());
    // on a Session row they do nothing either
    tree.keys(&["k", "s", "d"]);
    tree.idle();
    assert!(the_menu_is_closed(&tree.screen()));
    assert_eq!(drive::calls(&fx), Vec::<Vec<String>>::new());
}

#[test]
fn a_right_click_where_the_inbox_was_opens_no_inbox_menu() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, 0);
    assert_eq!(open_items(&fx), 4, "the Inbox has items");
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, WIDTH, 21);
    ready(&tree);
    tree.idle();
    // the rows of the three items, the title above them and the `expand` row below: 15 to 19
    for row in 15..20 {
        tree.right_click(10, row);
        tree.idle();
        let screen = tree.screen();
        // the empty area keeps the tree's own `New Context…` menu (Lead's answer deploy-act-impl-a1-q1); no Inbox entry in it
        assert!(!menu_open(&screen, "Snooze 15 min") && !screen.iter().any(|r| r.contains("Dismiss")), "row {row}: {screen:?}");
        tree.keys(&["Escape"]);
        tree.idle();
        assert!(the_menu_is_closed(&tree.screen()), "row {row}");
    }
    assert_eq!(tree.screen(), the_tree());
    assert!(drive::calls(&fx).is_empty(), "{:?}", drive::calls(&fx));
}

#[test]
fn the_tree_no_longer_redraws_every_second_for_the_age_of_an_item() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, 0);
    assert_eq!(open_items(&fx), 4, "the Inbox has items");
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, WIDTH, 21);
    ready(&tree);
    // nothing in the tree animates: no working Session, no clock on any row; the items' ages (`5m`) were the only one
    tree.advance(Duration::from_secs(1));
    let before = tree.flushes();
    for _ in 0..5 {
        tree.advance(Duration::from_secs(1));
    }
    assert_eq!(tree.flushes(), before, "the tree drew {} times in 5 s while nothing on its rows changed", tree.flushes() - before);
    assert_eq!(tree.screen(), the_tree());
}
