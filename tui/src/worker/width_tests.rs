//! Red tests for audit-plan F1 in the Worker view: the pinned header wraps by display columns, not `char`s.

use crate::cells;

use super::wrap_header;

#[test]
fn the_worker_header_in_wide_characters_wraps_by_columns() {
    // Ten `計` (U+8A08): ten `char`s, twenty display columns. In an 11-column header every row is at most nine
    // columns of text (one is the leading blank), so the word takes more than one row.
    let rows = wrap_header(&"計".repeat(10), 11);
    assert!(rows.len() > 1, "twenty columns of header in eleven columns are more than one row: {rows:?}");
    for (i, row) in rows.iter().enumerate() {
        assert!(cells::width(row) <= 11, "row {i} is {} columns wide in an 11-column header: {row:?}", cells::width(row));
    }
}
