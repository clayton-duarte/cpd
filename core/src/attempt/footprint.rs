//! The footprint of an Attempt: what it wrote against what its Job declared (contract 7.3 "The footprint").

use serde::Serialize;
use serde_json::Value;

use crate::error::CpdError;
use crate::git::tamper::touches_matches;
use crate::plan::jobs::Job;
use crate::plan::shared::is_shared;

/// How the written files stand against the Job's `touches`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FootprintClass {
    Match,
    Subset,
    UndeclaredWrite,
}

/// The class, the written paths no entry covers and the entries nothing written matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Footprint {
    pub class: FootprintClass,
    pub undeclared: Vec<String>,
    pub untouched: Vec<String>,
}

/// Pure: F1-F4 of contract 7.3.
pub fn classify(touches: &[String], actual: &[String], shared: &dyn Fn(&str) -> bool) -> Footprint {
    let covered = |path: &str| touches.iter().any(|e| touches_matches(e, path));
    let undeclared: Vec<String> = actual
        .iter()
        .filter(|p| !covered(p) && !shared(p))
        .cloned()
        .collect();
    let mut untouched: Vec<String> = Vec::new();
    for entry in touches {
        if !untouched.contains(entry) && !actual.iter().any(|p| touches_matches(entry, p)) {
            untouched.push(entry.clone());
        }
    }
    let class = if !undeclared.is_empty() {
        FootprintClass::UndeclaredWrite
    } else if untouched.is_empty() {
        FootprintClass::Match
    } else {
        FootprintClass::Subset
    };
    Footprint {
        class,
        undeclared,
        untouched,
    }
}

/// The footprint of an Attempt as JSON (F6): null for a kind with no touches check, no Spec or no captured files.
pub fn derive(job: &Job, actual_files: &Value) -> Result<Value, CpdError> {
    if !matches!(
        job.kind.as_str(),
        "docs" | "test" | "impl" | "work" | "verify"
    ) || job.spec.is_none()
    {
        return Ok(Value::Null);
    }
    let entries = match actual_files {
        Value::Null => return Ok(Value::Null),
        Value::Array(entries) => entries,
        other => return Err(CpdError::bad_row(format!("actual_files is {other}"))),
    };
    let actual = entries
        .iter()
        .map(|e| {
            e.as_str()
                .map(str::to_owned)
                .ok_or_else(|| CpdError::bad_row(format!("actual_files holds {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let touches = crate::job::guard::touches(job);
    serde_json::to_value(classify(&touches, &actual, &is_shared))
        .map_err(|e| CpdError::bad_row(format!("footprint: {e}")))
}
