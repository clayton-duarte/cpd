//! Plannotator removed (hs-d738): `cpd review open|close|list` are gone; clap refuses them as unknown subcommands.

use crate::common::{self, TestEnv};

/// Arguments the removed commands accepted, so only the missing subcommand can make them a usage error.
#[test]
fn drop_plannotator_review_open_close_list_are_usage_errors() {
    let env = TestEnv::new();
    let words: [&[&str]; 3] = [
        &["review", "open", "--session", "s1", "--pn", "pn-abcdef", "--kind", "plan", "--subject", "x", "--port", "20000"],
        &["review", "close", "--session", "s1", "--all"],
        &["review", "list", "--session", "s1"],
    ];
    for args in words {
        common::refused(&env.cpd(args), "usage", 2);
    }
}
