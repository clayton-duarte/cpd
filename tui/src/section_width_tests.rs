//! Red tests for audit-tree F1 in `section::title`: the title is cut and laid out in display columns, not `char`s.

use crate::section::title;

#[test]
fn a_section_title_with_wide_text_is_as_wide_as_it_is_told() {
    // Fourteen `計` (U+8A08): fourteen `char`s, twenty-eight display columns. `take(..)` keeps all fourteen in a 20-column
    // title, so the line is 34 columns wide; columns cut the pill to the room.
    let line = title(&"計".repeat(14), None, 20);
    assert_eq!(
        line.width(),
        20,
        "a 20-column title draws a 20-column row: {line:?}"
    );
}
