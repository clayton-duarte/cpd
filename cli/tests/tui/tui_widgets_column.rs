//! The right column of the `widgets` view (commands.md §13.2): the machine header, then the Inbox directly under it, and no
//! PRs section. The view runs in this process on a script (`common/inbox.rs`).

use crate::common;
use crate::tui_titles;

use common::inbox::{
    W, World, click, collapse, condition, cue_row, frame_with, gone, linked_item_row, many, open_at, pull_requests, rows, seed,
    show, up_to_date, view_fx,
};
use common::fx::Fx;
use common::sql;
use common::tree::engine;
use cpd_core::inbox::model::{Condition, Kind, Urgency};
use cpd_tui::pump::Signal;
use ratatui::crossterm::event::Event;

/// The glyph of an authored pull request's row.
const PR_GLYPH: &str = "\u{F12F4}";

/// The usage line when the machine reads fail: the cpu icon, the memory icon, one space each side.
const USAGE: &str = " \u{F4BC} –%  \u{EFC5} –%";

fn pill(count: usize) -> String {
    tui_titles::collapsible_title(W, "Inbox", Some(count))
}

fn pr_item(urgency: Urgency, label: &str, number: i64) -> Condition {
    let mut c = condition(Kind::PrReady, urgency, &format!("pr:acme/repo#{number}"), label);
    c.url = Some(format!("https://github.com/acme/repo/pull/{number}"));
    c
}

fn prepared(items: &[Condition]) -> Fx {
    let fx = view_fx();
    seed(&fx, items, 330);
    fx
}

fn item(glyph: &str, label: &str) -> String {
    common::inbox::item_row(glyph, label, "5m")
}

#[test]
fn the_column_is_the_machine_header_then_the_inbox_and_has_no_prs_section() {
    let items = [pr_item(Urgency::Yellow, "api#3 ready to merge", 3), condition(Kind::Dialog, Urgency::Red, "session:d", "dialog")];
    let fx = prepared(&items);
    let _engine = engine(&fx);
    pull_requests(&fx.env, 3);
    let frames = show(&fx, 20, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the header and the Inbox's title at row 4", |r| r.get(4) == Some(&pill(2)));
    let screen = rows(&frames, at);
    assert_eq!(screen[0], "", "{screen:?}");
    assert_eq!(screen[1], " homeserver", "{screen:?}");
    assert_eq!(screen[2], USAGE, "{screen:?}");
    assert_eq!(screen[3], "", "{screen:?}");
    assert_eq!(screen[4], pill(2), "{screen:?}");
    for row in &screen {
        assert!(!row.contains("PRs") && !row.contains("no open PRs") && !row.contains("no review") && !row.starts_with(" acme/repo#"), "{screen:?}");
    }
}

#[test]
fn the_inbox_sits_right_under_the_machine_header_not_at_the_bottom() {
    let fx = prepared(&[condition(Kind::Dialog, Urgency::Red, "session:a", "first"), condition(Kind::Dialog, Urgency::Red, "session:b", "second")]);
    let _engine = engine(&fx);
    let frames = show(&fx, 30, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the Inbox's title at row 4", |r| r.get(4) == Some(&pill(2)));
    let screen = rows(&frames, at);
    let red = cpd_tui::palette::TIER_RED;
    assert_eq!(screen[5], item(red, "first"), "{screen:?}");
    assert_eq!(screen[6], item(red, "second"), "{screen:?}");
    assert_eq!(screen[7], " ╰\u{F0B2A}", "{screen:?}");
    assert_eq!(screen[8], "", "{screen:?}");
    assert_eq!(screen[9..], vec![String::new(); 21], "{screen:?}");
}

#[test]
fn an_expanded_inbox_takes_every_row_under_the_header_with_no_third_cap() {
    let red = cpd_tui::palette::TIER_RED;
    let list = |n: usize| -> Vec<String> { (0..n).map(|i| item(red, &format!("item {i:02}"))).collect() };
    // the folded Inbox: its toggle is row 8 (rows 5 to 7 are three items)
    let fx = prepared(&many(14));
    let _engine = engine(&fx);
    let frames = show(&fx, 20, vec![Signal::Commit, click(10, 8), gone()], &World::new());
    let expanded = [vec![String::new(), pill(14)], list(12), vec![cue_row('▼', 2), collapse(), String::new()]].concat();
    assert_eq!(expanded.len(), 17);
    let at = frame_with(&frames, 0, "the expanded Inbox in rows 3 to 19", |r| r.len() == 20 && r[3..] == expanded[..]);
    assert_eq!(rows(&frames, at)[3..], expanded[..]);

    let fx = prepared(&many(12));
    let _engine = engine(&fx);
    let frames = show(&fx, 20, vec![Signal::Commit, click(10, 8), gone()], &World::new());
    let all = [vec![String::new(), pill(12)], list(12), vec![collapse(), String::new()]].concat();
    let at = frame_with(&frames, 0, "all 12 items listed", |r| r.len() == 20 && r[3..3 + all.len()] == all[..]);
    let screen = rows(&frames, at);
    assert!(screen.iter().all(|r| !r.contains(" more")), "no cue is drawn: {screen:?}");
}

#[test]
fn an_authored_pr_is_one_inbox_row_and_open_prs_draw_nothing_else() {
    let items = [
        pr_item(Urgency::Yellow, "repo#1 ready to merge", 1),
        pr_item(Urgency::Cyan, "repo#2 checks running", 2),
        pr_item(Urgency::White, "repo#3 draft", 3),
    ];
    let fx = prepared(&items);
    let _engine = engine(&fx);
    pull_requests(&fx.env, 3);
    let frames = show(&fx, 30, vec![Signal::Commit, gone()], &World::new());
    let wanted = [
        linked_item_row(PR_GLYPH, "repo#1 ready to merge", "5m"),
        linked_item_row(PR_GLYPH, "repo#2 checks running", "5m"),
        linked_item_row(PR_GLYPH, "repo#3 draft", "5m"),
    ];
    let at = frame_with(&frames, 0, "the three rows", |r| wanted.iter().all(|w| r.contains(w)));
    let screen = rows(&frames, at);
    let named: Vec<&String> = screen.iter().filter(|r| r.contains("repo#")).collect();
    assert_eq!(named, wanted.iter().collect::<Vec<_>>(), "{screen:?}");
}

#[test]
fn an_inbox_label_changed_in_the_store_with_no_event_is_drawn_at_the_next_read() {
    let fx = prepared(&[pr_item(Urgency::Cyan, "repo#2 checks running", 2)]);
    let _engine = engine(&fx);
    let home = fx.env.cpd_home();
    // The view's first read always reads the Inbox (the pull request rows are new to it): the label changes after it.
    let shown = |r: &[String]| r.iter().any(|row| row.contains("repo#2 checks running"));
    let world = World::new().at(shown, || {}).at(shown, move || sql(&mut open_at(&home), "UPDATE inbox_items SET label = 'repo#2 in review';"));
    let frames = show(&fx, 20, vec![Signal::Commit, common::inbox::input(Event::Resize(W as u16, 20)), Signal::Commit, gone()], &world);
    let old = frame_with(&frames, 0, "the old label", |r| r.iter().any(|row| row.contains("repo#2 checks running")));
    let new = frame_with(&frames, old, "the new label", |r| r.iter().any(|row| row.contains("repo#2 in review")));
    for (i, f) in frames.iter().enumerate().skip(new) {
        assert!(f.shown_rows().iter().all(|r| !r.contains("checks running")), "frame #{i} draws the old label");
    }
}

#[test]
fn an_empty_inbox_is_under_the_machine_header() {
    let fx = prepared(&[]);
    let _engine = engine(&fx);
    let frames = show(&fx, 20, vec![Signal::Commit, gone()], &World::new());
    let want = [vec![String::new(), pill(0), up_to_date(), String::new()]].concat();
    let at = frame_with(&frames, 0, "the empty Inbox under the header", |r| r.len() == 20 && r[3..7] == want[..]);
    let screen = rows(&frames, at);
    assert_eq!((screen[1].as_str(), screen[2].as_str()), (" homeserver", USAGE), "{screen:?}");
}
