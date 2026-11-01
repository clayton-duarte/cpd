//! The pure Tier advice (contract §3.4b): per Job kind, the Tier with the lowest expected cost over first-Attempt history.

use crate::config::{Letter, Tier};
use crate::error::CpdError;

/// The smallest sample a cell needs (pro-d278).
pub const FLOOR: u32 = 30;
/// The least Wilson lower bound of the pass rate a cell needs (pro-d278).
pub const MIN_LOWER: f64 = 0.70;
/// The z of the Wilson 95% interval (pro-d278).
const Z: f64 = 1.96;

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

/// The Wilson 95% lower bound of `passed / n`, clamped to [0, 1]; 0 when `n` is 0.
pub fn wilson_lower(passed: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let n = f64::from(n);
    let p = f64::from(passed) / n;
    let z2 = Z * Z;
    let lower =
        (p + z2 / (2.0 * n) - Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt()) / (1.0 + z2 / n);
    lower.clamp(0.0, 1.0)
}

/// Ladder order: letter H before S (then O, G), then the effort.
fn rank(tier: Tier) -> (u8, u8) {
    let letter = match tier.letter() {
        Letter::H => 0,
        Letter::S => 1,
        Letter::O => 2,
        Letter::G => 3,
    };
    (letter, tier.effort())
}

/// Every cell that passes the floor, the Wilson bound, the Worker rule and has an expected cost, in ladder order per kind.
pub fn qualifying(rows: &[Row], floor: u32, next: impl Fn(Tier) -> Option<Tier>) -> Vec<Advice> {
    let mut sorted: Vec<&Row> = rows.iter().collect();
    sorted.sort_by(|a, b| a.kind.cmp(&b.kind).then(rank(a.tier).cmp(&rank(b.tier))));
    sorted
        .into_iter()
        .filter_map(|r| {
            let lower = wilson_lower(r.passed, r.n);
            if r.n < floor || lower < MIN_LOWER || !r.tier.is_worker_allowed() {
                return None;
            }
            let own = r.avg_cost?;
            let rate = f64::from(r.passed) / f64::from(r.n);
            let failure = match next(r.tier) {
                None => 0.0,
                Some(up) => {
                    let cost = rows
                        .iter()
                        .find(|o| o.kind == r.kind && o.tier == up)?
                        .avg_cost?;
                    (1.0 - rate) * cost
                }
            };
            Some(Advice {
                kind: r.kind.clone(),
                tier: r.tier,
                n: r.n,
                passed: r.passed,
                rate,
                lower,
                expected_cost: own + failure,
            })
        })
        .collect()
}

/// The qualifying cell with the lowest expected cost per kind (a tie goes to the lower Tier), sorted by kind.
pub fn recommend(rows: &[Row], floor: u32, next: impl Fn(Tier) -> Option<Tier>) -> Vec<Advice> {
    let mut best: Vec<Advice> = Vec::new();
    for a in qualifying(rows, floor, next) {
        match best.last_mut() {
            Some(b) if b.kind == a.kind => {
                if a.expected_cost < b.expected_cost {
                    *b = a;
                }
            }
            _ => best.push(a),
        }
    }
    best
}

/// The first-Attempt history per (Job kind, Tier) from the store.
pub fn first_attempts(conn: &rusqlite::Connection) -> Result<Vec<Row>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT j.kind, a.tier, count(*) AS n, sum(a.outcome = 'merged') AS passed, avg(json_extract(a.metrics, '$.cost_usd')) AS avg_cost
           FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
          WHERE a.number = 1 AND a.outcome IN ('merged', 'failed') AND a.cause IS NOT 'provider_error'
          GROUP BY j.kind, a.tier
          ORDER BY j.kind, a.tier",
    )?;
    let raw = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, Option<f64>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rows = raw
        .into_iter()
        .map(|(kind, tier, n, passed, avg_cost)| {
            let tier = Tier::parse(&tier).map_err(|_| {
                CpdError::bad_row(format!("Job kind {kind} has an Attempt with tier {tier:?}"))
            })?;
            Ok(Row {
                kind,
                tier,
                n,
                passed,
                avg_cost,
            })
        })
        .collect::<Result<Vec<_>, CpdError>>()?;
    rows.sort_by(|a, b| a.kind.cmp(&b.kind).then(rank(a.tier).cmp(&rank(b.tier))));
    Ok(rows)
}
