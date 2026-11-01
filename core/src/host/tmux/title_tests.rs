//! The terminal window's title leads with cpd's mark φ (contract §6.1 `set-titles-string`, hs-d708): the window reads
//! `φ · <machine>` (`φ · hs`, `φ · pro`), never `cpd · <machine>`.

use super::{SERVER_OPTIONS, SET_TITLES_STRING, strings};

/// The value of the server option `name` (set with `set -g` or `set -gq`).
fn option(name: &str) -> Vec<String> {
    let found: Vec<&Vec<String>> = SERVER_OPTIONS
        .iter()
        .filter(|o| o[0] == "set" && o[2] == name)
        .collect();
    assert_eq!(found.len(), 1, "{name} is set once");
    found[0].clone()
}

#[test]
fn the_terminal_title_leads_with_phi() {
    assert!(
        SET_TITLES_STRING.starts_with("φ · "),
        "the title leads with `φ · `: {SET_TITLES_STRING}"
    );
    assert!(
        !SET_TITLES_STRING.starts_with("cpd · "),
        "the title no longer leads with `cpd · `: {SET_TITLES_STRING}"
    );
    assert_eq!(
        option("set-titles-string"),
        strings(&["set", "-g", "set-titles-string", SET_TITLES_STRING]),
        "the server option carries the constant"
    );
}
