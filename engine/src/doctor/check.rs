//! One line of the `doctor` report (commands.md §14.1).

use serde::Serialize;
use serde_json::Value;

use cpd_core::error::CpdError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A failing `error` check makes the result `doctor_failed`.
    Error,
    /// A failing `warn` check is reported and nothing more.
    Warn,
}

/// A check's result: `found`, `expected` and `message` appear only when set.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Check {
    pub fn new(name: &'static str, severity: Severity, ok: bool) -> Check {
        Check {
            name,
            ok,
            severity,
            found: None,
            expected: None,
            message: None,
        }
    }

    pub fn pass(name: &'static str, severity: Severity) -> Check {
        Check::new(name, severity, true)
    }

    pub fn fail(name: &'static str, severity: Severity, message: impl Into<String>) -> Check {
        Check::new(name, severity, false).message(message)
    }

    /// A check that could not run because what it needs is broken: it fails.
    pub fn not_checked(name: &'static str, severity: Severity, needs: &str) -> Check {
        Check::fail(name, severity, format!("not checked: {needs}"))
    }

    /// A check that failed with `err`: its code and message, and its details as `found`.
    pub fn from_error(name: &'static str, severity: Severity, err: &CpdError) -> Check {
        let mut check = Check::fail(
            name,
            severity,
            format!("{}: {}", err.code().as_str(), err.message()),
        );
        if !err.details().is_empty() {
            check.found = Some(Value::Object(err.details().clone()));
        }
        check
    }

    pub fn found(mut self, found: impl Into<Value>) -> Check {
        self.found = Some(found.into());
        self
    }

    pub fn expected(mut self, expected: impl Into<Value>) -> Check {
        self.expected = Some(expected.into());
        self
    }

    pub fn message(mut self, message: impl Into<String>) -> Check {
        self.message = Some(message.into());
        self
    }

    pub fn is_error_failure(&self) -> bool {
        !self.ok && self.severity == Severity::Error
    }
}
