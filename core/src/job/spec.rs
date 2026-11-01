//! The Spec file of `job add --spec` and `job edit --spec` (commands.md §5):
//! JSON, every field required, unknown fields refused with `spec_invalid`
//! (exit 2) and the JSON path of the fault in `details.path`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::error::{CpdError, ErrorCode};
use crate::plan::jobs::JobSpec;

const FIELDS: [&str; 5] = ["goal", "context", "touches", "acceptance", "out_of_scope"];
/// The Spec fields any Job may add (contract §2.2).
const OPTIONAL_FIELDS: [&str; 1] = ["resources"];
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
    /// `resources` of a Job: the names of the repo's declared exclusive resources it takes.
    pub resources: Option<Vec<String>>,
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

    /// The Spec names no primary checkout of any Workflow of its Plan (commands.md §5.1 "A Spec names no primary
    /// checkout", §5.2): every string is searched for each `roots` entry's root (a Workflow key and its repo path),
    /// or, when that root is inside `home`, its `~`, `$HOME` and `${HOME}` forms. A form matches where it ends
    /// at the end of the string or before a character outside `[A-Za-z0-9._-]`; the first match in field order
    /// (goal, context, touches, acceptance, out_of_scope, tests, test_files) then Workflow order is `spec_invalid`
    /// with `details {path, primary, pr}`. Pure: no I/O, so unit tests reach it.
    pub fn check_primary_roots(&self, roots: &[(String, PathBuf)], home: &Path) -> Result<(), CpdError> {
        let mut fields: Vec<(String, &str)> =
            vec![("$.goal".to_owned(), self.goal.as_str()), ("$.context".to_owned(), self.context.as_str())];
        let lists: [(&str, Option<&Vec<String>>); 5] = [
            ("touches", Some(&self.touches)),
            ("acceptance", Some(&self.acceptance)),
            ("out_of_scope", Some(&self.out_of_scope)),
            ("tests", self.tests.as_ref()),
            ("test_files", self.test_files.as_ref()),
        ];
        for (name, values) in lists {
            if let Some(values) = values {
                for (i, value) in values.iter().enumerate() {
                    fields.push((format!("$.{name}[{i}]"), value.as_str()));
                }
            }
        }
        for (path, value) in &fields {
            for (key, root) in roots {
                if names_root(value, root, home) {
                    let primary = root.display().to_string();
                    return Err(invalid(
                        path,
                        format!(
                            "the Spec names {primary}, the primary checkout of the repo of Workflow {key}; a Worker works in its own worktree; write paths relative to the repo, and to read the latest main say `git fetch origin` then `git show origin/main:<path>`"
                        ),
                    )
                    .with("primary", primary)
                    .with("pr", key.clone()));
                }
            }
        }
        Ok(())
    }

    /// The Spec with every citation of an explore artefact directory (`run/<plan>/explore/` under `cpd_home`, in the
    /// forms of [`Spec::check_primary_roots`]) taken out, so that a path under `$CPD_HOME` is no primary checkout.
    pub fn without_artefacts(&self, cpd_home: &Path, plan: &str, user_home: &Path) -> Spec {
        let tail = format!("run/{plan}/explore/");
        let mut forms = vec![format!("{}/{tail}", cpd_home.display())];
        if let Ok(rest) = cpd_home.strip_prefix(user_home).map(|r| r.to_string_lossy().into_owned()) {
            if !user_home.as_os_str().is_empty() {
                let rest = if rest.is_empty() { String::new() } else { format!("/{rest}") };
                forms.extend(["~", "$HOME", "${HOME}"].map(|p| format!("{p}{rest}/{tail}")));
            }
        }
        let strip = |s: &str| forms.iter().fold(s.to_owned(), |acc, f| acc.replace(f.as_str(), ""));
        let strips = |v: &[String]| v.iter().map(|s| strip(s)).collect::<Vec<_>>();
        Spec {
            goal: strip(&self.goal),
            context: strip(&self.context),
            touches: strips(&self.touches),
            acceptance: strips(&self.acceptance),
            out_of_scope: strips(&self.out_of_scope),
            tests: self.tests.as_deref().map(strips),
            test_files: self.test_files.as_deref().map(strips),
            resources: self.resources.clone(),
        }
    }

    /// `test_files` are repo-relative: no leading `/` (commands.md §5, the rule of the `protected` list,
    /// contract §7.5). Checked after [`Spec::check_primary_roots`], which reports a primary checkout first.
    pub fn check_files_relative(&self) -> Result<(), CpdError> {
        let Some(files) = &self.test_files else { return Ok(()) };
        for (i, file) in files.iter().enumerate() {
            if file.starts_with('/') {
                return Err(invalid(&format!("$.test_files[{i}]"), "must be a path relative to the repo, not absolute"));
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

/// Reads and validates the file: `file_unreadable` when it is not UTF-8 text (commands.md §1.5), else the parse's `spec_invalid`.
/// For a Job of `kind`: an explore Job's Spec has exactly four fields ([`parse_explore`]).
pub fn read_for(path: &Path, kind: &str) -> Result<Spec, CpdError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CpdError::file_unreadable(path, e)
    })?;
    let spec = if crate::plan::jobs::is_explore(kind) { parse_explore(&text) } else { parse(&text) };
    spec.map_err(|e| e.with("file", path.display().to_string()))
}

/// The artefact of explore Job `key` of `plan`, relative to `$CPD_HOME`: its derived `touches`.
pub fn explore_artefact(plan: &str, key: &str) -> String {
    format!("run/{plan}/explore/{key}.md")
}

/// The Spec of an explore Job: `goal`, `context`, `acceptance` and `out_of_scope` only; `touches`, `tests`, `test_files`
/// and `resources` are `spec_invalid`. `touches` is empty: the caller derives it ([`explore_artefact`]).
pub fn parse_explore(text: &str) -> Result<Spec, CpdError> {
    let value: Value = serde_json::from_str(text).map_err(|e| invalid("$", format!("not JSON: {e}")))?;
    let Value::Object(mut fields) = value else {
        return Err(invalid("$", "expected an object"));
    };
    if let Some(bad) = fields.keys().find(|k| !["goal", "context", "acceptance", "out_of_scope"].contains(&k.as_str())) {
        let why = if FIELDS.contains(&bad.as_str()) || TEST_FIELDS.contains(&bad.as_str()) || OPTIONAL_FIELDS.contains(&bad.as_str()) {
            "not a field of an explore Job's Spec: cpd derives touches, and it has no tests, test_files or resources"
        } else {
            "unknown field"
        };
        return Err(invalid(&format!("$.{bad}"), why));
    }
    Ok(Spec {
        resources: None,
        tests: None,
        test_files: None,
        goal: text_field(&mut fields, "goal", true)?,
        context: text_field(&mut fields, "context", false)?,
        touches: Vec::new(),
        acceptance: list_field(&mut fields, "acceptance", true)?,
        out_of_scope: list_field(&mut fields, "out_of_scope", false)?,
    })
}

fn invalid(path: &str, why: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::SpecInvalid, format!("The Spec is invalid at {path}: {why}.")).with("path", path)
}

/// Whether `value` names `root`, directly or by a home form of it: the forms are `<root>` and, when `root` is
/// inside `home`, `~/<rest>`, `$HOME/<rest>` and `${HOME}/<rest>`.
fn names_root(value: &str, root: &Path, home: &Path) -> bool {
    if boundaried(value, &root.to_string_lossy()) {
        return true;
    }
    if home.as_os_str().is_empty() {
        return false;
    }
    let Ok(rest) = root.strip_prefix(home) else {
        return false;
    };
    let rest = rest.to_string_lossy();
    ["~", "$HOME", "${HOME}"].into_iter().any(|prefix| {
        let form = if rest.is_empty() { prefix.to_owned() } else { format!("{prefix}/{rest}") };
        boundaried(value, &form)
    })
}

/// Whether `form` occurs in `value` ending at the end of `value` or before a character outside
/// `[A-Za-z0-9._-]` (commands.md §5.1 "A Spec names no primary checkout").
fn boundaried(value: &str, form: &str) -> bool {
    if form.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(pos) = value[from..].find(form) {
        let at = from + pos;
        let end = at + form.len();
        let boundary = match value[end..].chars().next() {
            None => true,
            Some(c) => !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-')),
        };
        if boundary {
            return true;
        }
        from = at + value[at..].chars().next().unwrap().len_utf8();
    }
    false
}

pub fn parse(text: &str) -> Result<Spec, CpdError> {
    let value: Value = serde_json::from_str(text).map_err(|e| invalid("$", format!("not JSON: {e}")))?;
    let Value::Object(mut fields) = value else {
        return Err(invalid("$", "expected an object"));
    };
    if let Some(unknown) = fields.keys().find(|k| !FIELDS.contains(&k.as_str()) && !TEST_FIELDS.contains(&k.as_str()) && !OPTIONAL_FIELDS.contains(&k.as_str())) {
        return Err(invalid(&format!("$.{unknown}"), "unknown field"));
    }
    let tests = fields.contains_key("tests").then(|| tests_field(&mut fields)).transpose()?;
    let test_files = fields.contains_key("test_files").then(|| test_files_field(&mut fields)).transpose()?;
    let resources = fields.contains_key("resources").then(|| resources_field(&mut fields)).transpose()?;
    Ok(Spec {
        resources,
        tests,
        test_files,
        goal: text_field(&mut fields, "goal", true)?,
        context: text_field(&mut fields, "context", false)?,
        touches: list_field(&mut fields, "touches", true)?,
        acceptance: list_field(&mut fields, "acceptance", true)?,
        out_of_scope: list_field(&mut fields, "out_of_scope", false)?,
    })
}

/// `resources`: one or more distinct names, each `^[a-z][a-z0-9-]{0,31}$` (contract §2.2).
fn resources_field(fields: &mut Map<String, Value>) -> Result<Vec<String>, CpdError> {
    let names = list_field(fields, "resources", true)?;
    for (i, name) in names.iter().enumerate() {
        let mut chars = name.chars();
        let ok = chars.next().is_some_and(|c| c.is_ascii_lowercase()) && name.len() <= 32 && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !ok {
            return Err(invalid(&format!("$.resources[{i}]"), "must be a resource name, ^[a-z][a-z0-9-]{0,31}$"));
        }
        if name == crate::resource::WORKERS {
            return Err(invalid(&format!("$.resources[{i}]"), "`workers` is the built-in resource: the starter takes it, a Spec never lists it"));
        }
        if names[..i].contains(name) {
            return Err(invalid(&format!("$.resources[{i}]"), "repeats an earlier resource"));
        }
    }
    Ok(names)
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

/// `test_files`: one or more repo-relative paths or, ending in `/`, directory prefixes: no `..`
/// (the rule of the `protected` list, contract §7.5). The leading-`/` rule runs after
/// [`Spec::check_primary_roots`] ([`Spec::check_files_relative`]).
fn test_files_field(fields: &mut Map<String, Value>) -> Result<Vec<String>, CpdError> {
    let files = list_field(fields, "test_files", true)?;
    for (i, file) in files.iter().enumerate() {
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
        let absolute = with(r#"["a"]"#, r#"["f", "/abs"]"#);
        let err = parse(&absolute).unwrap().check_files_relative().unwrap_err();
        assert_eq!(err.details()["path"], "$.test_files[1]");
        assert_eq!(path_of(&with(r#"["a"]"#, r#"["../f"]"#)), "$.test_files[0]");
        assert_eq!(path_of(&with(r#"["a"]"#, r#"["d/../f"]"#)), "$.test_files[0]");
        parse(&with(r#"["a"]"#, r#"["d/", "f..g"]"#)).unwrap();
    }
}
