//! The active time of a Workflow (Context record pro-d175): the union of its Attempts' intervals minus the union of
//! the Plan Lead's `blocked` intervals, in Unix milliseconds. Timestamps are parsed once, here.

use cpd_core::error::CpdError;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::shared::bad_row;

type Span = (i64, i64);

/// A stored RFC 3339 timestamp as Unix milliseconds.
pub(super) fn parse_ms(what: &str, value: &str) -> Result<i64, CpdError> {
    let t = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| bad_row(format!("{what} has time {value:?}")))?;
    Ok((t.unix_timestamp_nanos() / 1_000_000) as i64)
}

/// The Lead's blocked spans from its `session-state` transitions `(at_ms, to)` in order; one still open ends at `now_ms`.
pub(super) fn blocked_spans(events: &[(i64, String)], now_ms: i64) -> Vec<Span> {
    let mut out = Vec::new();
    let mut open: Option<i64> = None;
    for (at, to) in events {
        match (open, to == "blocked") {
            (None, true) => open = Some(*at),
            (Some(start), false) => {
                out.push((start, (*at).max(start)));
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        out.push((start, now_ms.max(start)));
    }
    out
}

/// Sorted, disjoint, non-empty spans covering the same time as `spans`.
fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.retain(|(s, e)| e > s);
    spans.sort();
    let mut out: Vec<Span> = Vec::new();
    for (s, e) in spans {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Whole seconds of `attempts` minus `blocked`; `None` when there is no Attempt. An Attempt that ends before it starts is empty.
pub(super) fn elapsed_seconds(attempts: &[Span], blocked: &[Span]) -> Option<u64> {
    if attempts.is_empty() {
        return None;
    }
    let active = merge(attempts.to_vec());
    let blocked = merge(blocked.to_vec());
    let mut total: i64 = 0;
    for (s, e) in active {
        let mut cursor = s;
        for &(bs, be) in &blocked {
            if be <= cursor || bs >= e {
                continue;
            }
            total += (bs.max(cursor) - cursor).max(0);
            cursor = cursor.max(be);
        }
        total += (e - cursor).max(0);
    }
    Some((total.max(0) / 1000) as u64)
}
