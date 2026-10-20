//! The Spec file of `job add --spec` and `job edit --spec` (commands.md §5):
//! JSON, every field required, unknown fields refused with `spec_invalid`
//! (exit 2) and the JSON path of the fault in `details.path`.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::error::{CpdError, ErrorCode};
use crate::plan::jobs::JobSpec;

const FIELDS: [&str; 5] = ["goal", "context", "touches", "acceptance", "out_of_scope"];
/// The two more of a `test` Job's Spec (contract §2.2 "Job kinds"), refused for every other kind.
const TEST_FIELDS: [&str; 2] = ["tests", "test_files"];

/// A Spec as the file gives it, valid.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub goal: String,
    pub context: String,
    pub touches: Vec<String>,
    pub acceptance: Vec<String>,
    pub out_of_scope: Vec<String>,
    /// `tests` of a `test` Job: the named tests (one or more distinct one-line strings).
    pub tests: Option<Vec<String>>,
    /// `test_files` of a `test` Job: the files that hold them (repo-relative paths or `dir/` prefixes).
    pub test_files: Option<Vec<String>>,
}

impl Spec {
    /// The Spec fits `kind` (commands.md §5): a `test` Job's Spec has `tests` and `test_files`, every other kind's
    /// neither, else `spec_invalid` with the JSON path of the field.
    pub fn check_kind(&self, kind: &str) -> Result<(), CpdError> {
        for (name, given) in [("tests", self.tests.is_some()), ("test_files", self.test_files.is_some())] {
            match (kind == "test", given) {
                (true, false) => return Err(invalid(&format!("$.{name}"), format!("missing field: a `test` Job's Spec names its {name}"))),
                (false, true) => return Err(invalid(&format!("$.{name}"), format!("unknown field: only a `test` Job's Spec has {name}, and this Job is `{kind}`"))),
                _ => {}
            }
        }
        Ok(())
    }

    /// The Spec as a Job holds it.
    pub fn stored(&self) -> JobSpec {
        JobSpec {
            goal: self.goal.clone(),
            context: self.context.clone(),
            touches: json!(self.touches),
            acceptance: json!(self.acceptance),
            out_of_scope: json!(self.out_of_scope),
        }
    }
}

/// Reads and validates the file: `file_unreadable` when it is not UTF-8 text
/// (commands.md §1.5), else [`parse`]'s `spec_invalid`.
pub fn read(path: &Path) -> Result<Spec, CpdError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CpdError::file_unreadable(path, e)
    })?;
    parse(&text).map_err(|e| e.with("file", path.display().to_string()))
}

fn invalid(path: &str, why: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::SpecInvalid, format!("The Spec is invalid at {path}: {why}.")).with("path", path)
}

pub fn parse(text: &str) -> Result<Spec, CpdError> {
    let value: Value = serde_json::from_str(text).map_err(|e| invalid("$", format!("not JSON: {e}")))?;
    let Value::Object(mut fields) = value else {
        return Err(invalid("$", "expected an object"));
    };
    if let Some(unknown) = fields.keys().find(|k| !FIELDS.contains(&k.as_str()) && !TEST_FIELDS.contains(&k.as_str())) {
        return Err(invalid(&format!("$.{unknown}"), "unknown field"));
    }
    let tests = fields.contains_key("tests").then(|| tests_field(&mut fields)).transpose()?;
    let test_files = fields.contains_key("test_files").then(|| test_files_field(&mut fields)).transpose()?;
    Ok(Spec {
        tests,
        test_files,
        goal: text_field(&mut fields, "goal", true)?,
        context: text_field(&mut fields, "context", false)?,
        touches: list_field(&mut fields, "touches", true)?,
        acceptance: list_field(&mut fields, "acceptance", true)?,
        out_of_scope: list_field(&mut fields, "out_of_scope", false)?,
    })
}

/// `tests`: one or more distinct one-line strings.
fn tests_field(fields: &mut Map<String, Value>) -> Result<Vec<String>, CpdError> {
    let tests = list_field(fields, "tests", true)?;
    for (i, test) in tests.iter().enumerate() {
        if test.contains(['\n', '\r']) {
            return Err(invalid(&format!("$.tests[{i}]"), "must be one line"));
        }
        if tests[..i].contains(test) {
            return Err(invalid(&format!("$.tests[{i}]"), "repeats an earlier test"));
        }
    }
    Ok(tests)
}

/// `test_files`: one or more repo-relative paths or, ending in `/`, directory prefixes: no `..`, no leading `/`
/// (the rule of the `protected` list, contract §7.5).
fn test_files_field(fields: &mut Map<String, Value>) -> Result<Vec<String>, CpdError> {
    let files = list_field(fields, "test_files", true)?;
    for (i, file) in files.iter().enumerate() {
        if file.starts_with('/') {
            return Err(invalid(&format!("$.test_files[{i}]"), "must be a path relative to the repo, not absolute"));
        }
        if file.split('/').any(|part| part == "..") {
            return Err(invalid(&format!("$.test_files[{i}]"), "must not contain .."));
        }
    }
    Ok(files)
}

fn take(fields: &mut Map<String, Value>, name: &str) -> Result<Value, CpdError> {
    fields.remove(name).ok_or_else(|| invalid(&format!("$.{name}"), "missing field"))
}

/// `goal` must say something (the `jobs` table refuses an empty one); `context` may be empty.
fn text_field(fields: &mut Map<String, Value>, name: &str, non_empty: bool) -> Result<String, CpdError> {
    match take(fields, name)? {
        Value::String(s) if !non_empty || !s.trim().is_empty() => Ok(s),
        Value::String(_) => Err(invalid(&format!("$.{name}"), "must not be empty")),
        _ => Err(invalid(&format!("$.{name}"), "expected a string")),
    }
}

/// An array of non-empty strings; `at_least_one` for `touches` and `acceptance`.
fn list_field(fields: &mut Map<String, Value>, name: &str, at_least_one: bool) -> Result<Vec<String>, CpdError> {
    let Value::Array(items) = take(fields, name)? else {
        return Err(invalid(&format!("$.{name}"), "expected an array of strings"));
    };
    if at_least_one && items.is_empty() {
        return Err(invalid(&format!("$.{name}"), "needs at least one string"));
    }
    items
        .into_iter()
        .enumerate()
        .map(|(i, item)| match item {
            Value::String(s) if !s.trim().is_empty() => Ok(s),
            Value::String(_) => Err(invalid(&format!("$.{name}[{i}]"), "must not be empty")),
            _ => Err(invalid(&format!("$.{name}[{i}]"), "expected a string")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": []}"#;

    fn path_of(text: &str) -> String {
        let err = parse(text).unwrap_err();
        assert_eq!(err.code(), ErrorCode::SpecInvalid);
        err.details()["path"].as_str().unwrap().to_owned()
    }

    #[test]
    fn a_complete_spec_parses() {
        let spec = parse(GOOD).unwrap();
        assert_eq!((spec.goal.as_str(), spec.context.as_str()), ("g", "c"));
        assert_eq!(spec.touches, ["a"]);
        assert!(spec.out_of_scope.is_empty());
    }

    #[test]
    fn each_fault_names_its_json_path() {
        assert_eq!(path_of("nope"), "$");
        assert_eq!(path_of("[]"), "$");
        assert_eq!(path_of(r#"{"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": [], "extra": 1}"#), "$.extra");
        assert_eq!(path_of(r#"{"context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": []}"#), "$.goal");
        assert_eq!(path_of(r#"{"goal": 1, "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": []}"#), "$.goal");
        assert_eq!(path_of(r#"{"goal": " ", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": []}"#), "$.goal");
        assert_eq!(path_of(r#"{"goal": "g", "context": "c", "touches": [], "acceptance": ["b"], "out_of_scope": []}"#), "$.touches");
        assert_eq!(path_of(r#"{"goal": "g", "context": "c", "touches": ["a", ""], "acceptance": ["b"], "out_of_scope": []}"#), "$.touches[1]");
        assert_eq!(path_of(r#"{"goal": "g", "context": "c", "touches": ["a"], "acceptance": [2], "out_of_scope": []}"#), "$.acceptance[0]");
        assert_eq!(path_of(r#"{"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": "x"}"#), "$.out_of_scope");
        assert_eq!(path_of(r#"{"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"]}"#), "$.out_of_scope");
    }

    const TEST_SPEC: &str = r#"{"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": [],
                                "tests": ["pr_open_job::merges_the_base", "pr_open_job::opens"], "test_files": ["cli/tests/pr_open_job.rs", "core/src/pr/"]}"#;

    #[test]
    fn a_test_spec_has_named_tests_and_the_files_that_hold_them() {
        let spec = parse(TEST_SPEC).unwrap();
        assert_eq!(spec.tests.as_deref(), Some(&["pr_open_job::merges_the_base".to_owned(), "pr_open_job::opens".to_owned()][..]));
        assert_eq!(spec.test_files.as_deref(), Some(&["cli/tests/pr_open_job.rs".to_owned(), "core/src/pr/".to_owned()][..]));
        spec.check_kind("test").unwrap();
        assert_eq!(parse(GOOD).unwrap().tests, None);
    }

    #[test]
    fn the_spec_fits_its_kind_and_the_fault_names_its_json_path() {
        let plain = parse(GOOD).unwrap();
        for kind in ["work", "docs", "impl"] {
            plain.check_kind(kind).unwrap();
        }
        let err = plain.check_kind("test").unwrap_err();
        assert_eq!((err.code(), err.details()["path"].as_str()), (ErrorCode::SpecInvalid, Some("$.tests")));
        let test = parse(TEST_SPEC).unwrap();
        for kind in ["work", "docs", "impl"] {
            let err = test.check_kind(kind).unwrap_err();
            assert_eq!((err.code(), err.details()["path"].as_str()), (ErrorCode::SpecInvalid, Some("$.tests")), "{kind}");
        }
        let only_tests = parse(r#"{"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": [], "tests": ["t"]}"#).unwrap();
        assert_eq!(only_tests.check_kind("test").unwrap_err().details()["path"], "$.test_files");
    }

    #[test]
    fn the_named_tests_and_files_are_checked_where_they_are_read() {
        let with = |tests: &str, files: &str| {
            format!(r#"{{"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": [], "tests": {tests}, "test_files": {files}}}"#)
        };
        assert_eq!(path_of(&with("[]", r#"["f"]"#)), "$.tests");
        assert_eq!(path_of(&with(r#"["a", "a"]"#, r#"["f"]"#)), "$.tests[1]");
        assert_eq!(path_of(&with(r#"["a\nb"]"#, r#"["f"]"#)), "$.tests[0]");
        assert_eq!(path_of(&with(r#"[1]"#, r#"["f"]"#)), "$.tests[0]");
        assert_eq!(path_of(&with(r#"["a"]"#, "[]")), "$.test_files");
        assert_eq!(path_of(&with(r#"["a"]"#, r#"["f", "/abs"]"#)), "$.test_files[1]");
        assert_eq!(path_of(&with(r#"["a"]"#, r#"["../f"]"#)), "$.test_files[0]");
        assert_eq!(path_of(&with(r#"["a"]"#, r#"["d/../f"]"#)), "$.test_files[0]");
        parse(&with(r#"["a"]"#, r#"["d/", "f..g"]"#)).unwrap();
    }
}
