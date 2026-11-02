//! The footprint of an Attempt: what it wrote against what its Job declared (contract 7.3 "The footprint").

#![allow(dead_code)] // the stub is unused until `derive-impl` wires it; it removes this line.

use serde::Serialize;

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
pub fn classify(
    _touches: &[String],
    _actual: &[String],
    _shared: &dyn Fn(&str) -> bool,
) -> Footprint {
    unimplemented!("classify")
}
