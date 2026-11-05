use super::jira_title::conform;

#[test]
fn conform_composes_the_key_after_the_type() {
    assert_eq!(
        conform("feat: Show skeletons", "ABC-12").as_deref(),
        Some("feat: ABC-12/Show skeletons")
    );
    assert_eq!(
        conform("chore: x", "A1-2").as_deref(),
        Some("chore: A1-2/x")
    );
}

#[test]
fn conform_composes_after_a_breaking_type() {
    assert_eq!(
        conform("refactor!: Drop the old API", "ABC-12").as_deref(),
        Some("refactor!: ABC-12/Drop the old API")
    );
}

#[test]
fn conform_keeps_a_title_that_already_holds_the_key() {
    assert_eq!(
        conform("fix: ABC-12/Mend it", "ABC-12").as_deref(),
        Some("fix: ABC-12/Mend it")
    );
    assert_eq!(
        conform("fix!: ABC-12/Mend it", "ABC-12").as_deref(),
        Some("fix!: ABC-12/Mend it")
    );
}

#[test]
fn conform_refuses_a_title_with_another_key() {
    assert_eq!(conform("fix: XYZ-9/Mend it", "ABC-12"), None);
    // The key match is exact: a lowercase key is not a key, so it is part of the summary.
    assert_eq!(
        conform("fix: abc-12/Mend it", "ABC-12").as_deref(),
        Some("fix: ABC-12/abc-12/Mend it")
    );
    assert_eq!(conform("fix: ABC-123/Mend it", "ABC-12"), None);
}

#[test]
fn conform_refuses_an_unknown_type_and_a_missing_type() {
    // The same title with a known type conforms: the refusals below are about the type alone.
    for ty in "feat fix docs style refactor perf test build ci chore revert".split(' ') {
        assert!(conform(&format!("{ty}: x"), "ABC-12").is_some(), "{ty}");
    }
    for title in [
        "Show skeletons",
        "wip: Show skeletons",
        "Feat: Show skeletons",
        "feat Show skeletons",
        "feat:Show skeletons",
        "feat!!: x",
        ": x",
        "",
    ] {
        assert_eq!(conform(title, "ABC-12"), None, "{title:?}");
    }
}

#[test]
fn conform_refuses_an_empty_summary() {
    assert!(conform("feat: x", "ABC-12").is_some());
    for title in ["feat: ", "feat:", "feat: ABC-12/", "feat!: "] {
        assert_eq!(conform(title, "ABC-12"), None, "{title:?}");
    }
}

#[test]
fn conform_refuses_a_composed_title_over_72_characters() {
    // "feat: " (6) + "ABC-12/" (7) + summary: 72 fits, 73 does not.
    let fits = "a".repeat(72 - 13);
    let over = "a".repeat(72 - 13 + 1);
    assert_eq!(
        conform(&format!("feat: {fits}"), "ABC-12"),
        Some(format!("feat: ABC-12/{fits}"))
    );
    assert_eq!(conform(&format!("feat: {over}"), "ABC-12"), None);
}
