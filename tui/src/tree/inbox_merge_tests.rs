//! Merge from the Inbox item menu (decision pro-d111, assumption A3, commands.md §13.2): the right-click menu of a `pr-ready`
//! item whose pull request is clean (`merge_state = 'clean'`) starts with Merge, above the Snooze and Dismiss entries; picking
//! it runs the merge at once, with no popup; while that merge runs the entry is the spinner and cannot be picked. The rig is
//! `merge_at_once_tests.rs`'s.

use crate::merge_at_once_tests::{enter, has, home_with, locate, merging, right_click, rows, run, run_with_setup};
use crate::palette;

/// The text of the menu rows, from the first one: the row of `first` and the ones below it, trimmed of the box's walls.
fn entries(frame: &ratatui::buffer::Buffer, first: &str, count: usize) -> Vec<String> {
    let (_, y) = locate(frame, first);
    let all = rows(frame);
    (y..y + count as u16).map(|r| all[usize::from(r)].clone()).collect()
}

#[test]
fn inbox_menu_of_a_clean_pr_ready_item_starts_with_merge() {
    let (_dir, home) = home_with("clean");
    let (x, y) = locate(&run(&home, vec![]).buf, "api#3 ");
    let seen = run(&home, vec![right_click(x, y)]);
    let shown = entries(&seen.buf, "Merge", 5);
    for (row, label) in shown.iter().zip(["Merge", "Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"]) {
        assert!(row.contains(label), "{label:?} in order, got {shown:?}");
    }
}

#[test]
fn inbox_menu_merge_runs_the_merge_action_at_once_with_no_popup() {
    let (_dir, home) = home_with("clean");
    let (x, y) = locate(&run(&home, vec![]).buf, "api#3 ");
    let opened = run(&home, vec![right_click(x, y)]);
    assert!(has(&opened.buf, "Merge"), "the menu offers Merge");
    let seen = run(&home, vec![right_click(x, y), enter()]);
    seen.assert_merged_at_once();
}

#[test]
fn inbox_menu_of_a_pr_ready_item_that_is_not_clean_has_no_merge() {
    let (_clean_dir, clean) = home_with("clean");
    let (x, y) = locate(&run(&clean, vec![]).buf, "api#3 ");
    assert!(has(&run(&clean, vec![right_click(x, y)]).buf, "Merge"), "control: the same item with a clean pull request has Merge");
    for state in ["blocked", "unknown"] {
        let (_dir, home) = home_with(state);
        let (x, y) = locate(&run(&home, vec![]).buf, "api#3 ");
        let seen = run(&home, vec![right_click(x, y)]);
        assert!(!has(&seen.buf, "Merge"), "{state}: no Merge entry: {:?}", rows(&seen.buf));
        let shown = entries(&seen.buf, "Snooze 15 min", 4);
        for (row, label) in shown.iter().zip(["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"]) {
            assert!(row.contains(label), "{state}: {label:?} in order, got {shown:?}");
        }
    }
}

#[test]
fn inbox_menu_merge_is_the_spinner_and_unselectable_while_it_merges() {
    let (_dir, home) = home_with("clean");
    let (x, y) = locate(&run(&home, vec![]).buf, "api#3 ");
    let seen = run_with_setup(&home, merging, vec![right_click(x, y)]);
    let (mx, my) = locate(&seen.buf, "Merge ");
    let line = rows(&seen.buf)[usize::from(my)].clone();
    let after: String = line.chars().skip(usize::from(mx) + "Merge ".len()).collect();
    assert!(palette::SPINNER.iter().any(|s| after.starts_with(s)), "the spinner follows Merge: {line:?}");
    let picked = run_with_setup(&home, merging, vec![right_click(x, y), enter()]);
    assert!(picked.run_shell().is_empty() && picked.popups().is_empty(), "picking it does nothing: {:?}", picked.tmux);
    let clicked = run_with_setup(&home, merging, vec![right_click(x, y), crate::merge_at_once_tests::click(mx, my)]);
    assert!(clicked.run_shell().is_empty() && clicked.popups().is_empty(), "clicking it does nothing: {:?}", clicked.tmux);
}
