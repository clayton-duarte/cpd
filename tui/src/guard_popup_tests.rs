//! `the_popup_command_is_built_in_one_place` (audit-plan F2, audit-shell F6): the `display-popup` command is built
//! three times (`widgets/legend.rs`, `merge/mod.rs`, `dialog/mod.rs`), `sh_quote` is written twice and the
//! newest-client parser twice. This guard is a source scan: it pins that the repo has exactly one popup builder,
//! one quoting helper and one newest-client parser, so a refactor that moves them never breaks it.

use crate::guard_tests::product_sources;

/// Every product line that contains `needle`, as `path:line: text`, so a failure lists each occurrence.
fn occurrences(needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (file, text) in product_sources() {
        for (n, line) in text.lines().enumerate() {
            for _ in 0..line.matches(needle).count() {
                found.push(format!("{file}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    found
}

/// The popup command is built in exactly one place: the literal `"display-popup"` occurs once. `fn sh_quote` and
/// the newest-client parser (`fn newest_client` / `fn newest_named_client` together) are each defined once. The
/// message lists every copy.
#[test]
fn the_popup_command_is_built_in_one_place() {
    let popup = occurrences("\"display-popup\"");
    let sh_quote = occurrences("fn sh_quote");
    let mut newest = occurrences("fn newest_client");
    newest.extend(occurrences("fn newest_named_client"));

    let mut report = Vec::new();
    if popup.len() != 1 {
        report.push(format!(
            "the popup command is built in {} places:\n{}",
            popup.len(),
            popup.join("\n")
        ));
    }
    if sh_quote.len() != 1 {
        report.push(format!(
            "sh_quote is defined {} times:\n{}",
            sh_quote.len(),
            sh_quote.join("\n")
        ));
    }
    if newest.len() != 1 {
        report.push(format!(
            "the newest-client parser is defined {} times:\n{}",
            newest.len(),
            newest.join("\n")
        ));
    }
    assert!(report.is_empty(), "{}", report.join("\n\n"));
}
