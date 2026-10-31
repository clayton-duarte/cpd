//! Red test of the callers metadata of `context show` and `context search` (Plan `explore-roles`, R10; commands.md §2):
//! an explore Worker may run them read-only, so the interface table lists `Worker` besides `Lead` and `Manager`.
//! No other row changes.

use crate::interface::{Caller, TABLE};

fn callers(path: &str) -> Vec<Caller> {
    TABLE.iter().find(|(p, _, _)| *p == path).unwrap_or_else(|| panic!("{path} is in the table")).1.to_vec()
}

#[test]
fn context_show_and_search_list_worker_among_their_callers() {
    for path in ["context show", "context search"] {
        let mut got = callers(path);
        got.sort_by_key(|c| format!("{c:?}"));
        assert_eq!(got, [Caller::Lead, Caller::Manager, Caller::Worker], "{path}");
    }
    // The neighbouring Context rows keep their callers.
    for path in ["context list", "context reindex", "context decide", "context note", "context ask", "context answer", "context pending", "context owner"] {
        assert_eq!(callers(path), [Caller::Lead, Caller::Manager], "{path} is unchanged");
    }
    assert_eq!(callers("context distill"), [Caller::Lead], "context distill is unchanged");
}

#[test]
fn preset_commands_list_their_callers() {
    // The tier-preset switch (Plan tier-presets; commands.md §14.6): the read-only rows are
    // Manager and Lead, `apply` writes and is the Manager's (like `session retier`).
    for path in ["preset list", "preset show"] {
        assert_eq!(callers(path), [Caller::Manager, Caller::Lead], "{path} is read-only");
    }
    assert_eq!(callers("preset apply"), [Caller::Manager], "preset apply writes");
    // The neighbouring rows keep their callers.
    assert_eq!(callers("models refresh"), [Caller::Manager, Caller::Lead], "models refresh is unchanged");
    assert_eq!(callers("models show"), [Caller::Manager, Caller::Lead], "models show is unchanged");
}
