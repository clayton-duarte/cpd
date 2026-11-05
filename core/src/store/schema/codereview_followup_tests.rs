//! The additive migration `20261105T1400-codereview-followup` (pro-d416): the columns a re-review needs.

use super::*;

#[test]
fn codereview_followup_is_additive_and_adds_the_four_columns() {
    let entry = NAMED
        .iter()
        .find(|n| n.name == "20261105T1400-codereview-followup")
        .expect("codereview-followup is a built-in migration");
    assert!(entry.compat == Compat::Additive);
    assert_eq!(entry.sql.lines().next(), Some("-- cpd-migration: additive"));
    for (table, column) in [
        ("plans", "pr_subject"),
        ("plans", "changes_requested_at"),
        ("inbox_items", "head_oid"),
        ("inbox_items", "reviewed_oid"),
    ] {
        assert!(
            entry
                .sql
                .contains(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT;")),
            "{table}.{column}"
        );
    }
}
