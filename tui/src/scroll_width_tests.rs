//! Red tests for audit-tree F1 in `scroll::wrap`: rows are split at display columns, not `char`s.

use ratatui::text::Line;

use crate::scroll::wrap;

#[test]
fn wrap_splits_wide_text_by_columns_not_characters() {
    // Ten `計` (U+8A08): ten `char`s, twenty display columns. At width 10 `chars()` fits them in one row that is twice as
    // wide as its pane; columns split them into two rows of five.
    let lines = vec![Line::from("計".repeat(10))];
    let rows = wrap(&lines, 10);
    for (i, row) in rows.iter().enumerate() {
        assert!(
            row.width() <= 10,
            "row {i} is {} columns wide in a 10-column body: {row:?}",
            row.width()
        );
    }
    assert_eq!(
        rows.len(),
        2,
        "twenty columns at width ten are two rows: {rows:?}"
    );
    assert_eq!(
        rows.iter().map(Line::width).sum::<usize>(),
        20,
        "the two rows keep all twenty columns"
    );
}
