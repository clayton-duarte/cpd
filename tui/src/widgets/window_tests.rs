//! The shared scroll window of a list with a ▲ line above and a ▼ line below (commands.md §13.2): the two cue lines are not in `rows`.

use super::column::Window;

fn w(first: usize, shown: usize, cue_above: bool, below: usize) -> Window {
    Window { first, shown, cue_above, below }
}

#[test]
fn a_list_that_fits_shows_every_row_and_hides_none() {
    assert_eq!(Window::new(3, 5, 0), w(0, 3, false, 0));
    assert_eq!(Window::new(5, 5, 9), w(0, 5, false, 0));
    assert_eq!(Window::max_scroll(3, 5), 0);
    assert_eq!(Window::max_scroll(5, 5), 0);
}

#[test]
fn a_scrolled_list_shows_its_rows_between_the_above_and_below_counts() {
    assert_eq!(Window::new(12, 5, 3), w(3, 5, true, 4));
}

#[test]
fn the_scroll_clamps_so_the_last_row_reaches_the_bottom_of_the_rows() {
    assert_eq!(Window::max_scroll(12, 5), 7);
    assert_eq!(Window::new(12, 5, 100), w(7, 5, true, 0));
}

#[test]
fn no_list_rows_show_nothing_and_count_every_item_below() {
    assert_eq!(Window::new(4, 0, 0), w(0, 0, false, 4));
}

#[test]
fn an_empty_list_shows_no_row_and_no_cue() {
    assert_eq!(Window::new(0, 5, 3), w(0, 0, false, 0));
}
