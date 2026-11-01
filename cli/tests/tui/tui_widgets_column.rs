//! The right column of the `widgets` view (commands.md §13.2): the machine header, then the Inbox pinned to the bottom, and no
//! PRs section. The view runs in this process on a script (`common/inbox.rs`).

use crate::common;
use crate::tui_titles;

use common::inbox::{W, World, condition, frame_with, gone, linked_item_row, open_at, pull_requests, rows, seed, show, view_fx};
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
    tui_titles::flush_title(W, "Inbox", count)
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

#[test]
fn the_column_is_the_machine_header_then_the_inbox_and_has_no_prs_section() {
    let items = [pr_item(Urgency::Yellow, "api#3 ready to merge", 3), condition(Kind::Dialog, Urgency::Red, "session:d", "dialog")];
    let fx = prepared(&items);
    let _engine = engine(&fx);
    pull_requests(&fx.env, 3);
    let frames = show(&fx, 20, vec![Signal::Commit, gone()], &World::new());
    // 20 rows: the machine lines, then the section (2 one-row items + 4 rows) at the bottom, so its header is row 15
    let at = frame_with(&frames, 0, "the header and the Inbox's title at row 15", |r| r.len() == 20 && r.get(15) == Some(&pill(2)));
    let screen = rows(&frames, at);
    assert_eq!(screen[0], "", "{screen:?}");
    assert_eq!(screen[1], " homeserver", "{screen:?}");
    assert_eq!(screen[2], USAGE, "{screen:?}");
    assert!(screen[3..15].iter().all(String::is_empty), "{screen:?}");
    assert_eq!(screen[15], pill(2), "{screen:?}");
    assert_eq!(screen[16], "", "the ▲ line: {screen:?}");
    assert_eq!(screen[19], "", "the ▼ line: {screen:?}");
    for row in &screen {
        assert!(!row.contains("PRs") && !row.contains("no open PRs") && !row.contains("no review") && !row.starts_with(" acme/repo#"), "{screen:?}");
    }
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

/// Linux reads `/proc/meminfo` through the view's reader, which the test scripts; macOS reads other tools, so these are Linux's.
#[cfg(target_os = "linux")]
#[test]
fn a_scripted_proc_meminfo_is_the_memory_percent_of_the_machine_row() {
    let fx = prepared(&[]);
    let meminfo = "MemTotal:       16000000 kB\nMemFree:         1000000 kB\nMemAvailable:    4000000 kB\n";
    let frames = show_proc(&fx, 10, vec![Signal::Commit, gone()], &World::new(), &[("/proc/meminfo", meminfo)]);
    let at = frame_with(&frames, 0, "the machine row", |r| r.get(1).is_some_and(|h| h == " homeserver"));
    assert_eq!(rows(&frames, at)[2], " \u{F4BC} –%  \u{EFC5} 75%");
}

#[cfg(target_os = "linux")]
#[test]
fn an_unscripted_proc_meminfo_is_an_unreadable_memory_on_the_machine_row() {
    let fx = prepared(&[]);
    let frames = show_proc(&fx, 10, vec![Signal::Commit, gone()], &World::new(), &[]);
    let at = frame_with(&frames, 0, "the machine row", |r| r.get(1).is_some_and(|h| h == " homeserver"));
    assert_eq!(rows(&frames, at)[2], USAGE);
}
