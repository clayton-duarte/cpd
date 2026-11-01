//! The causes an Attempt may end with, as the live `attempts` table admits them (its `cause` CHECK).

use super::TestEnv;

/// The strings of the `cause IN (...)` list of the `attempts` table's CHECK, read from `sqlite_master` of the env's
/// (already migrated) store. Panics when the list is not found or holds nothing.
pub fn attempt_causes(env: &TestEnv) -> Vec<String> {
    let sql: String =
        env.query("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'attempts'");
    let marker = "(cause IN (";
    let from = sql
        .find(marker)
        .unwrap_or_else(|| panic!("no `cause IN (...)` list in the attempts table: {sql}"))
        + marker.len();
    let list = &sql[from..];
    let list = &list[..list
        .find(')')
        .unwrap_or_else(|| panic!("the cause list of the attempts table is not closed: {sql}"))];
    let causes: Vec<String> = list
        .split(',')
        .map(|c| c.trim().trim_matches('\'').to_owned())
        .collect();
    assert!(
        causes.iter().all(|c| !c.is_empty() && !c.contains('\'')),
        "unreadable cause list {list:?} in {sql}"
    );
    causes
}

/// The `outcome` that goes with a stored `cause`: `merged`/`merged`, `cancelled`/`cancel_requested`, else `failed`.
pub fn outcome_of(cause: &str) -> &'static str {
    match cause {
        "merged" => "merged",
        "cancel_requested" => "cancelled",
        _ => "failed",
    }
}
