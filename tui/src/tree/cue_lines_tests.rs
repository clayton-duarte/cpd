//! The tree's reserved cue lines (commands.md §13.2, §17): row 0 is the ▲ line, the list starts at row 1, the ▼ line is
//! the pane's last row (above the error footer); both are blank when nothing is hidden, and neither is a click target.

use ratatui::crossterm::event::KeyCode;

use super::actions::Action;
use super::scroll_tests::{click, draw_now, long_tree, press, text_rows, wheel};
use super::view::{Effect, View};

/// A cue as drawn in a 43-column pane: right-aligned, ending at column 41, column 42 blank.
fn cue_row(arrow: char, n: usize) -> String {
    format!(" {:>41}", format!("{arrow} {n} more"))
}

fn assert_no(rows: &[String], glyph: char) {
    assert!(
        !rows.iter().any(|r| r.contains(glyph)),
        "no {glyph}: {rows:?}"
    );
}

/// The `n` of a `<arrow> <n> more` cue row ending at column 41.
fn cue_count(row: &str, arrow: char) -> usize {
    (1..1000)
        .find(|n| row == cue_row(arrow, *n))
        .unwrap_or_else(|| panic!("a {arrow} cue ending at column 41: {row:?}"))
}

#[test]
fn a_tree_one_row_taller_than_its_rows_hides_one_row_under_a_below_cue() {
    let mut view = View::new(long_tree(10), true);
    let rows = text_rows(&draw_now(&mut view, 43, 12));
    assert_eq!(rows[0], "", "{rows:?}");
    assert!(rows[1].ends_with(" alpha"), "{rows:?}");
    assert!(rows[10].contains("alpha08"), "{rows:?}");
    assert!(
        !rows.iter().any(|r| r.contains("alpha09")),
        "the hidden row: {rows:?}"
    );
    assert_eq!(rows[11], cue_row('▼', 1), "{rows:?}");
    assert_no(&rows, '▲');
}

#[test]
fn scrolled_down_the_above_cue_is_drawn_in_the_blank_top_row() {
    let mut view = View::new(long_tree(60), true);
    draw_now(&mut view, 43, 31);
    wheel(&mut view, true, 10, 5);
    let rows = text_rows(&draw_now(&mut view, 43, 31));
    assert_eq!(rows[0], cue_row('▲', 3), "{rows:?}");
    assert!(rows[1].contains("alpha02"), "{rows:?}");
    assert!(rows[29].contains("alpha30"), "{rows:?}");
    assert_eq!(rows[30], cue_row('▼', 29), "{rows:?}");
}

#[test]
fn scrolled_to_the_end_the_last_row_is_above_a_blank_below_line() {
    let mut view = View::new(long_tree(60), true);
    draw_now(&mut view, 43, 31);
    for _ in 0..40 {
        wheel(&mut view, true, 10, 5);
    }
    let rows = text_rows(&draw_now(&mut view, 43, 31));
    assert_eq!(rows[0], cue_row('▲', 32), "{rows:?}");
    assert!(rows[1].contains("alpha31"), "{rows:?}");
    assert!(rows[29].contains("alpha59"), "{rows:?}");
    assert_eq!(rows[30], "", "{rows:?}");
    assert_no(&rows, '▼');
}

#[test]
fn the_keyboard_cursor_stays_between_the_cue_lines() {
    let mut view = View::new(long_tree(60), true);
    for _ in 0..40 {
        press(&mut view, KeyCode::Down);
    }
    let buf = draw_now(&mut view, 43, 31);
    let rows = text_rows(&buf);
    let selected: Vec<u16> = (0..31)
        .filter(|y| crate::palette::check::is_selected(&buf[(5, *y)]))
        .collect();
    assert!(
        !selected.is_empty() && selected.iter().all(|y| (1..=29).contains(y)),
        "the cursor row is one of rows 1-29: {selected:?} {rows:?}"
    );
    let (above, below) = (cue_count(&rows[0], '▲'), cue_count(&rows[30], '▼'));
    assert_eq!(above + 29 + below, 61, "{rows:?}");
}

#[test]
fn the_error_footer_is_below_the_below_cue_line() {
    let mut view = View::new(long_tree(40), true);
    view.error = Some("snapshot failed".into());
    let rows = text_rows(&draw_now(&mut view, 43, 12));
    assert_eq!(rows[0], "", "{rows:?}");
    assert!(
        rows[1].ends_with(" alpha") && rows[9].contains("alpha07"),
        "nine tree rows: {rows:?}"
    );
    assert_eq!(rows[10], cue_row('▼', 32), "{rows:?}");
    assert!(rows[11].contains("snapshot failed"), "{rows:?}");

    // One row over what fits above the ▼ line: the line is reserved (a ▼ cue), though the footer's row alone would fit it.
    let mut view = View::new(long_tree(9), true);
    view.error = Some("snapshot failed".into());
    let rows = text_rows(&draw_now(&mut view, 43, 12));
    assert!(
        !rows.iter().any(|r| r.contains("alpha08")),
        "the tenth row is hidden: {rows:?}"
    );
    assert_eq!(rows[10], cue_row('▼', 1), "{rows:?}");
    assert!(rows[11].contains("snapshot failed"), "{rows:?}");
}

#[test]
fn a_click_on_a_cue_line_selects_nothing() {
    let mut view = View::new(long_tree(10), true);
    draw_now(&mut view, 43, 12);
    let before = view.state.selected().to_vec();
    assert_eq!(
        click(&mut view, 5, 11),
        Effect::None,
        "the ▼ line is no row"
    );
    assert_eq!(view.state.selected(), before, "the selection moved nowhere");
    assert!(
        matches!(click(&mut view, 5, 1), Effect::Run(Action::OpenManager(_))),
        "the alpha row is hit as today"
    );
}
