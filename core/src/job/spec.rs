//! The Spec file of `job add --spec` and `job edit --spec` (commands.md §5):
//! JSON, every field required, unknown fields refused with `spec_invalid`
//! (exit 2) and the JSON path of the fault in `details.path`.

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::{CpdError, ErrorCode};

const FIELDS: [&str; 5] = ["goal", "context", "touches", "acceptance", "out_of_scope"];

/// A Spec as the file gives it, valid.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub goal: String,
    pub context: String,
    pub touches: Vec<String>,
    pub acceptance: Vec<String>,
    pub out_of_scope: Vec<String>,
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
    if let Some(unknown) = fields.keys().find(|k| !FIELDS.contains(&k.as_str())) {
        return Err(invalid(&format!("$.{unknown}"), "unknown field"));
    }
    Ok(Spec {
        goal: text_field(&mut fields, "goal", true)?,
        context: text_field(&mut fields, "context", false)?,
        touches: list_field(&mut fields, "touches", true)?,
        acceptance: list_field(&mut fields, "acceptance", true)?,
        out_of_scope: list_field(&mut fields, "out_of_scope", false)?,
    })
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
}
