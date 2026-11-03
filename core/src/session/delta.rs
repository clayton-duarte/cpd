//! The Rust op applier of the ops of a watch call (contract §6.20, "the op applier"): Chord's seven
//! ops over plain JSON, pinned by the vectors of `delta_tests.rs`.

use std::fmt;

use serde_json::Value;

/// An op the applier refused: the message names why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaError(pub String);

impl fmt::Display for DeltaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "refused op: {}", self.0)
    }
}

impl std::error::Error for DeltaError {}

/// Applies the ops in order. Pure: returns the new value, or the first refused op's error; never a
/// partial result.
pub fn apply(value: Value, ops: &[Value]) -> Result<Value, DeltaError> {
    let _ = (value, ops);
    unimplemented!("delta::apply")
}
