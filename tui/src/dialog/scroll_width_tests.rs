//! Red tests for audit-tree F1 in the shared scroll cue: it is cut and padded in display columns, not `char`s.

use super::scroll::{Cue, cue};

#[test]
fn the_scroll_cue_is_padded_by_columns() {
    for width in [0, 1, 5, 80] {
        let line = cue(Cue::Below, 33, width);
        assert_eq!(
            line.width(),
            width,
            "a cue in a {width}-column row is {width} columns wide, padding included: {line:?}"
        );
    }
}
