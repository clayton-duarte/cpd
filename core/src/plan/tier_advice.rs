//! The pure Tier advice (contract §3.4b): per Job kind, the Tier with the lowest expected cost over first-Attempt history.

use crate::config::Tier;
use crate::error::CpdError;

pub const FLOOR: u32 = 30;
pub const MIN_LOWER: f64 = 0.70;

/// One `(kind, tier)` cell as read from the store.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: String,
    pub tier: Tier,
    pub n: u32,
    pub passed: u32,
    pub avg_cost: Option<f64>,
}

/// The chosen cell of one kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Advice {
    pub kind: String,
    pub tier: Tier,
    pub n: u32,
    pub passed: u32,
    pub rate: f64,
    pub lower: f64,
    pub expected_cost: f64,
}

pub fn wilson_lower(_passed: u32, _n: u32) -> f64 {
    todo!()
}

pub fn qualifying(_rows: &[Row], _floor: u32, _next: impl Fn(Tier) -> Option<Tier>) -> Vec<Advice> {
    todo!()
}

pub fn recommend(_rows: &[Row], _floor: u32, _next: impl Fn(Tier) -> Option<Tier>) -> Vec<Advice> {
    todo!()
}

pub fn first_attempts(_conn: &rusqlite::Connection) -> Result<Vec<Row>, CpdError> {
    todo!()
}
