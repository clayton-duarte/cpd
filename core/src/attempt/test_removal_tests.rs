//! Red tests for the b70 guard: a test Job's submit that removes a test case, or lowers its assertion count,
//! is refused `tests_removed`; the Lead may excuse a test with a reasoned, recorded approval.

use crate::error::ErrorCode;
use crate::git::tamper::{approve_test_removal, check_tests_kept, test_cases};

const BASE: &str = "#[test]\nfn a() {\n    assert_eq!(1, 1);\n    assert!(true);\n}\n\n#[test]\nfn b() {\n    assert!(true);\n}\n";

fn refused(base: &str, head: &str, approved: &[String]) -> bool {
    check_tests_kept(base, head, approved).is_err_and(|e| e.code() == ErrorCode::TestsRemoved)
}

#[test]
fn removed_test_case_is_refused() {
    let head = "#[test]\nfn a() {\n    assert_eq!(1, 1);\n    assert!(true);\n}\n";
    assert_eq!(test_cases(BASE).len(), 2);
    assert!(refused(BASE, head, &[]));
}

#[test]
fn weakened_assertion_count_is_refused() {
    let head =
        "#[test]\nfn a() {\n    assert_eq!(1, 1);\n}\n\n#[test]\nfn b() {\n    assert!(true);\n}\n";
    assert_eq!(test_cases(BASE)[0].assertions, 2);
    assert!(refused(BASE, head, &[]));
}

#[test]
fn added_test_is_accepted() {
    let head = format!("{BASE}\n#[test]\nfn c() {{\n    assert!(true);\n}}\n");
    assert_eq!(test_cases(&head).len(), 3);
    assert!(check_tests_kept(BASE, &head, &[]).is_ok());
}

#[test]
fn unchanged_tests_are_accepted() {
    assert_eq!(test_cases(BASE).len(), 2);
    assert!(check_tests_kept(BASE, BASE, &[]).is_ok());
}

#[test]
fn approved_removal_with_reason_is_accepted() {
    let head = "#[test]\nfn a() {\n    assert_eq!(1, 1);\n    assert!(true);\n}\n";
    let mut approved = Vec::new();
    approve_test_removal(&mut approved, "b", "obsolete").expect("approved");
    assert_eq!(approved, ["b"]);
    assert!(check_tests_kept(BASE, head, &approved).is_ok());
}

#[test]
fn approval_requires_a_reason() {
    let mut approved = Vec::new();
    let err = approve_test_removal(&mut approved, "b", "  ").expect_err("blank reason");
    assert_eq!(err.code(), ErrorCode::Usage);
    assert!(approved.is_empty());
}

#[test]
fn reason_is_recorded_visible_to_user() {
    let mut approved = Vec::new();
    let text = approve_test_removal(&mut approved, "b", "obsolete after merge").expect("approved");
    assert!(text.contains("obsolete after merge") && text.contains('b'));
}
