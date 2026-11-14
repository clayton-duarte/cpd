//! Red tests for the b70 guard: a test Job's submit that removes a test case, or lowers its assertion count,
//! is refused `tests_removed`; the Lead may excuse a test with a reasoned, recorded approval.

use super::touches_tests::{Rig, Spec, rig};
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

// ---- single-line tests: assertions on the declaration line count ----

#[test]
fn single_line_js_assertions_are_counted() {
    let base = "test(\"a\", () => { expect(1).toBe(1); });\n";
    assert_eq!(test_cases(base)[0].assertions, 1);
    let head = "test(\"a\", () => { });\n";
    assert!(refused(base, head, &[]));
}

#[test]
fn single_line_rust_fn_assertions_are_counted() {
    let base = "#[test]\nfn a() { assert!(true); assert_eq!(1, 1); }\n";
    assert_eq!(test_cases(base)[0].assertions, 2);
    let head = "#[test]\nfn a() { assert!(true); }\n";
    assert!(refused(base, head, &[]));
    let attr_inline = "#[test] fn a() { assert!(true); }\n";
    assert_eq!(test_cases(attr_inline)[0].assertions, 1);
}

#[test]
fn a_single_line_case_does_not_swallow_the_next_lines() {
    let src = "test(\"a\", () => { expect(1).toBe(1); });\nexpect(2).toBe(2);\n";
    assert_eq!(test_cases(src)[0].assertions, 1);
}

// ---- identity: scoped, and distinct cases stay distinct ----

#[test]
fn same_named_tests_in_different_scopes_are_distinct_and_accepted_unchanged() {
    let rust = "mod x {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\nmod y {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\n";
    let cases = test_cases(rust);
    assert_eq!(cases.len(), 2);
    assert_ne!(cases[0].name, cases[1].name);
    assert!(check_tests_kept(rust, rust, &[]).is_ok());
    let js = "describe(\"x\", () => {\n  test(\"a\", () => {\n    expect(1).toBe(1);\n  });\n});\ndescribe(\"y\", () => {\n  test(\"a\", () => {\n    expect(1).toBe(1);\n  });\n});\n";
    let cases = test_cases(js);
    assert_eq!(cases.len(), 2);
    assert_ne!(cases[0].name, cases[1].name);
    assert!(check_tests_kept(js, js, &[]).is_ok());
}

#[test]
fn deleting_one_of_two_same_named_scoped_tests_is_refused() {
    let base = "mod x {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\nmod y {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\n";
    let head = "mod x {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\n";
    assert!(refused(base, head, &[]));
}

#[test]
fn deleting_one_of_two_same_named_tests_in_one_scope_is_refused() {
    let base = "test(\"a\", () => {\n  expect(1).toBe(1);\n});\ntest(\"a\", () => {\n  expect(2).toBe(2);\n});\n";
    let head = "test(\"a\", () => {\n  expect(1).toBe(1);\n});\n";
    assert_eq!(test_cases(base).len(), 2);
    assert!(refused(base, head, &[]));
    assert!(check_tests_kept(base, base, &[]).is_ok());
}

#[test]
fn moving_a_test_to_another_scope_is_refused() {
    let base = "mod x {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\n";
    let head = "mod y {\n    #[test]\n    fn a() {\n        assert!(true);\n    }\n}\n";
    assert!(refused(base, head, &[]));
}

// ---- the production submit path refuses a removed test and takes it after the Lead's approval ----

const WEAKER: &str = "#[test]\nfn a() {\n    assert!(true);\n}\n";

fn test_rig(changed: &'static [(&'static str, &'static str)]) -> Rig {
    rig(Spec {
        // The guard runs for every Job whose submit checks a worktree; `work` has no `[test-commands]` check to stub.
        kind: "work",
        touches: "[\"tests/t.rs\"]",
        changed,
        ..Spec::default()
    })
}

fn approve(rig: &mut Rig, names: &[&str]) {
    let stored = serde_json::to_string(names).unwrap();
    rig.env
        .store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE jobs SET test_removal_approved = ?1 WHERE plan = 'pay' AND key = 'a'",
                rusqlite::params![stored],
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn submit_refuses_a_test_job_that_deletes_an_existing_test() {
    let mut rig = test_rig(&[("tests/t.rs", WEAKER)]);
    let err = rig.submit().expect_err("the removal is refused");
    assert_eq!(err.code(), ErrorCode::TestsRemoved, "{err:?}");
    assert_eq!(err.exit().code(), 3);
    assert_eq!(err.details()["tests"], serde_json::json!(["b"]));
    assert_eq!(err.details()["path"], "tests/t.rs");
    assert_eq!(rig.attempt("status").as_deref(), Some("running"));
}

#[test]
fn submit_refuses_a_test_job_that_deletes_a_test_file() {
    // The head tree holds an empty test file: every case of the base is gone.
    let mut rig = test_rig(&[("tests/t.rs", "")]);
    let err = rig.submit().expect_err("refused");
    assert_eq!(err.code(), ErrorCode::TestsRemoved, "{err:?}");
    assert_eq!(err.details()["tests"], serde_json::json!(["a", "b"]));
}

#[test]
fn submit_accepts_the_removal_after_the_leads_approval() {
    let mut rig = test_rig(&[("tests/t.rs", WEAKER)]);
    assert_eq!(
        rig.submit().expect_err("refused").code(),
        ErrorCode::TestsRemoved
    );
    // The Lead approves while the Attempt runs (`job approve-test-removal`): the check reads it fresh.
    approve(&mut rig, &["b"]);
    assert!(rig.submit().is_ok(), "an approved removal passes");
    assert_eq!(rig.attempt("status").as_deref(), Some("merging"));
}

#[test]
fn submit_accepts_a_test_job_that_keeps_and_adds_tests() {
    const MORE: &str = "#[test]\nfn a() {\n    assert!(true);\n}\n\n#[test]\nfn b() {\n    assert!(true);\n}\n\n#[test]\nfn c() {\n    assert!(true);\n}\n";
    let mut rig = test_rig(&[("tests/t.rs", MORE)]);
    assert!(rig.submit().is_ok());
    assert_eq!(rig.attempt("status").as_deref(), Some("merging"));
}

#[test]
fn unicode_before_an_assertion_does_not_panic_or_hide_it() {
    let src = "#[test]\nfn a() { let café = 1; assert_eq!(café, 1); }\n";
    assert_eq!(test_cases(src)[0].assertions, 1);
}

#[test]
fn comments_and_literals_cannot_replace_assertions() {
    let base = "#[test]\nfn a() { assert!(true); }\n";
    for body in [
        "// assert!(true);\n",
        "/* assert!(true); */",
        "let s = \"assert!(true)\";",
        "let s = r#\"assert!(true)\"#;",
    ] {
        let head = format!("#[test]\nfn a() {{\n{body}\n}}\n");
        assert!(refused(base, &head, &[]), "{body}");
    }
    let src =
        "/*\n#[test]\nfn fake() { assert!(true); }\n*/\n#[test]\nfn real() { assert!(true); }";
    assert_eq!(test_cases(src).len(), 1);
    assert_eq!(test_cases(src)[0].name, "real");
}

#[test]
fn quoted_and_commented_braces_do_not_close_cases() {
    for body in ["let ch = '}';", "// }", "/* } */", "let s = r#\"\"}\"#;"] {
        let src = format!("#[test]\nfn a() {{\n{body}\nassert!(true);\n}}\n");
        assert_eq!(test_cases(&src)[0].assertions, 1, "{body}");
    }
    let js =
        "test('a', () => {\n const s = '}';\n const t = `expect(1) }`;\n expect(1).toBe(1);\n});\n";
    assert_eq!(test_cases(js)[0].assertions, 1);
}

#[test]
fn regex_quotes_and_braces_do_not_change_assertion_counts() {
    for regex in ["/}/", "/\"/", "/[}/\"']/g", "/expect\\(x\\)/", "/a\\/b/"] {
        let src = format!("test('a', () => {{\n const re = {regex};\n expect(1).toBe(1);\n}});\n");
        assert_eq!(test_cases(&src)[0].assertions, 1, "{regex}");
    }
    let division = "test('a', () => { const n = 6 / 2; expect(n).toBe(3); });";
    assert_eq!(test_cases(division)[0].assertions, 1);
}

#[test]
fn deleting_a_test_added_by_an_earlier_lead_job_is_refused() {
    let mut r = rig(Spec {
        kind: "work",
        touches: "[\"tests/lead.rs\"]",
        lead_only: &[(
            "tests/lead.rs",
            "#[test]\nfn lead_test() { assert!(true); }\n",
        )],
        deleted: &["tests/lead.rs"],
        ..Spec::default()
    });
    let err = r
        .submit()
        .expect_err("Lead tests cannot disappear through the footprint filter");
    assert_eq!(err.code(), ErrorCode::TestsRemoved);
    assert_eq!(err.details()["path"], "tests/lead.rs");
}

#[test]
fn bare_approval_preserves_punctuation_in_the_complete_title() {
    let base = "describe('suite', () => {\nit('handles: value > threshold', () => { expect(true).toBe(true); });\n});";
    assert!(check_tests_kept(base, "", &["threshold".into()]).is_err());
    assert!(check_tests_kept(base, "", &["handles: value > threshold".into()]).is_ok());
    assert!(check_tests_kept(base, "", &["suite > handles: value > threshold".into()]).is_ok());
}

#[test]
fn js_options_object_does_not_end_the_test_before_its_callback() {
    let base =
        "test('keeps assertions', {\n timeout: 1000,\n}, () => {\n expect(true).toBe(true);\n});";
    assert_eq!(test_cases(base)[0].assertions, 1);
    let weakened = base.replace("expect(true).toBe(true);", "");
    assert!(check_tests_kept(base, &weakened, &[]).is_err());
    let compact =
        "test('keeps assertions', { timeout: 1000 }, () => { expect(true).toBe(true); });";
    assert!(check_tests_kept(base, compact, &[]).is_ok());
}

#[test]
fn template_interpolations_keep_executable_assertions() {
    let base = r#"test("a", () => { const msg = `literal expect(fake) ${expect(1).toBe(1)}`; });"#;
    assert_eq!(test_cases(base)[0].assertions, 1);
    assert!(check_tests_kept(base, &base.replace("expect(1).toBe(1)", "1"), &[]).is_err());
    let plain = r#"test("a", () => { expect(1).toBe(1); });"#;
    assert!(check_tests_kept(plain, base, &[]).is_ok());
    let nested = r#"test("a", () => { const msg = `outer ${`inner ${(() => { expect(1).toBe(1); return 1; })()}`}`; });"#;
    assert_eq!(test_cases(nested)[0].assertions, 1);
}

#[test]
fn upstream_test_changes_brought_by_a_merge_need_no_removal_approval() {
    let mut r = rig(Spec {
        kind: "update",
        touches: "[\"tests/lead.rs\"]",
        lead_only: &[(
            "tests/lead.rs",
            "#[test]\nfn a() { assert!(true); assert!(true); }\n",
        )],
        base_ahead: &[("tests/lead.rs", "#[test]\nfn a() { assert!(true); }\n")],
        brings_base: true,
        ..Spec::default()
    });
    r.submit().expect("upstream test changes are exempt");
}

#[test]
fn multiline_javascript_titles_preserve_case_identity_and_assertions() {
    let compact = "describe('suite', () => {\ntest('a', () => { expect(1).toBe(1); });\n});";
    let multiline =
        "describe(\n 'suite', () => {\ntest(\n 'a',\n () => { expect(1).toBe(1); }\n);\n});";
    assert_eq!(test_cases(multiline), test_cases(compact));
    assert!(check_tests_kept(compact, multiline, &[]).is_ok());
    assert!(
        check_tests_kept(multiline, &multiline.replace("expect(1).toBe(1);", ""), &[]).is_err()
    );
}
