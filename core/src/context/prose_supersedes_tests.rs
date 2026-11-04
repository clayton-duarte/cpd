use super::prose_supersedes::prose_supersession_targets;

fn ids(text: &str) -> Vec<String> {
    prose_supersession_targets(text)
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn the_scanner_finds_the_id_after_each_of_the_three_verbs() {
    assert_eq!(ids("This supersedes pro-d344."), ["pro-d344"]);
    assert_eq!(ids("This overrules pro-d12."), ["pro-d12"]);
    assert_eq!(ids("This replaces pro-n7."), ["pro-n7"]);
}

#[test]
fn the_scanner_is_case_insensitive() {
    assert_eq!(ids("Supersedes pro-d1"), ["pro-d1"]);
    assert_eq!(ids("OVERRULES pro-d2"), ["pro-d2"]);
    assert_eq!(ids("ReplAces pro-d3"), ["pro-d3"]);
}

#[test]
fn the_scanner_splits_a_list_on_comma_and_and_and_ampersand() {
    assert_eq!(
        ids("supersedes pro-d344 and pro-d341"),
        ["pro-d344", "pro-d341"]
    );
    assert_eq!(ids("supersedes pro-d1, pro-d2"), ["pro-d1", "pro-d2"]);
    assert_eq!(ids("supersedes pro-d1 & pro-d2"), ["pro-d1", "pro-d2"]);
    assert_eq!(
        ids("supersedes pro-d1, pro-d2 and pro-n3"),
        ["pro-d1", "pro-d2", "pro-n3"]
    );
}

#[test]
fn the_scanner_ignores_a_clause_reference_such_as_a1_of_an_id() {
    assert!(ids("supersedes A1 of pro-d342").is_empty());
    assert_eq!(
        ids("supersedes A1 of pro-d342, and also supersedes pro-d5"),
        ["pro-d5"]
    );
    assert!(ids("Nothing here names a record: pro-d5 stays.").is_empty());
}

#[test]
fn the_scanner_returns_each_id_once_in_text_order() {
    assert_eq!(
        ids("supersedes pro-d2 and pro-d1; also replaces pro-d2 and overrules pro-d3"),
        ["pro-d2", "pro-d1", "pro-d3"]
    );
}
