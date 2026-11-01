//! The shared scroll window of a list with a ▲ line above and a ▼ line below (commands.md §13.2): the two cue lines are not in `rows`.

use super::column::Window;

fn w(first: usize, shown: usize, cue_above: bool, below: usize) -> Window {
    Window {
        first,
        shown,
        cue_above,
        below,
    }
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

#[test]
fn the_largest_weighted_offset_is_the_first_from_which_every_remaining_item_fits() {
    assert_eq!(
        Window::max_scroll_weighted(&[2, 1, 2, 1, 1], 5),
        1,
        "from 1: 1+2+1+1 = 5 fits; from 0: 7 does not"
    );
    assert_eq!(Window::max_scroll_weighted(&[1, 1, 1], 5), 0, "all fit");
    assert_eq!(
        Window::max_scroll_weighted(&[2, 2, 2], 5),
        1,
        "from 1: 4 fits, from 0: 6 does not"
    );
    assert_eq!(
        Window::max_scroll_weighted(&[1, 2], 0),
        2,
        "no list row: past the last item"
    );
    assert_eq!(Window::max_scroll_weighted(&[], 5), 0);
}

#[test]
fn a_weighted_window_shows_the_run_that_fits_and_never_splits_an_item() {
    assert_eq!(Window::weighted(&[1, 1, 1], 5, 0), w(0, 3, false, 0));
    assert_eq!(
        Window::weighted(&[2, 2, 2], 5, 0),
        w(0, 2, false, 1),
        "a spare row stays blank"
    );
    assert_eq!(
        Window::weighted(&[1, 2, 1, 1], 2, 0),
        w(0, 1, false, 3),
        "stops at the first that does not fit, though a later one would"
    );
    assert_eq!(
        Window::weighted(&[2, 2, 2], 5, 100),
        w(1, 2, true, 0),
        "the scroll clamps to the largest offset"
    );
    assert_eq!(Window::weighted(&[2, 1, 1, 1], 1, 1), w(1, 1, true, 2));
    assert_eq!(
        Window::weighted(&[2], 1, 0),
        w(0, 0, false, 1),
        "one list row never shows a two-row item"
    );
    assert_eq!(Window::weighted(&[1, 1], 0, 0), w(0, 0, false, 2));
    assert_eq!(Window::weighted(&[], 5, 3), w(0, 0, false, 0));
}
