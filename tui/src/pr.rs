//! PR status of a Plan, for the Dispatch view.
//!
//! Interim source: cpd records only `workflow.pr = {number, url, openedAt}`, so the live state comes from
//! `gh pr view` until cpd reports it. This module holds the pure parts: parsing gh's JSON, the cache record with its
//! refresh rule, and the row rendering. The `gh` call itself lives in `main.rs`.

use ratatui::{
    style::Stylize,
    text::{Line, Span},
};
use serde_json::{Value, json};

use crate::palette;

/// A PR is re-read at most this often (seconds), whether the last read worked or not.
pub const REFRESH_SECS: u64 = 60;

/// The `--json` fields asked of `gh pr view`.
pub const GH_FIELDS: &str = "state,statusCheckRollup,reviewDecision,mergeable,url,title";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checks {
    Passed,
    Failed,
    Pending,
    None,
}

/// What the row shows of a PR, parsed from gh's JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    pub state: String,
    pub checks: Checks,
    pub review: String,
    pub title: String,
}

/// Parse `gh pr view --json …` output. An error names what is missing: nothing is defaulted.
pub fn parse_live(v: &Value) -> Result<Live, String> {
    let state = v["state"].as_str().ok_or("gh output has no state")?;
    let rollup = v["statusCheckRollup"].as_array().ok_or("gh output has no statusCheckRollup")?;
    Ok(Live {
        state: state.to_lowercase(),
        checks: checks_of(rollup),
        review: match v["reviewDecision"].as_str().unwrap_or("") {
            "APPROVED" => "approved",
            "CHANGES_REQUESTED" => "changes requested",
            _ => "no review",
        }
        .into(),
        title: v["title"].as_str().unwrap_or("").into(),
    })
}

/// Any failure wins over pending, pending over passed. CheckRuns carry `status`/`conclusion`, StatusContexts `state`.
fn checks_of(rollup: &[Value]) -> Checks {
    if rollup.is_empty() {
        return Checks::None;
    }
    let (mut failed, mut pending) = (false, false);
    for c in rollup {
        let verdict = if c["__typename"] == "StatusContext" { c["state"].as_str() } else if c["status"] == "COMPLETED" { c["conclusion"].as_str() } else { Some("PENDING") };
        match verdict.unwrap_or("PENDING") {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => {}
            "PENDING" | "EXPECTED" | "QUEUED" | "IN_PROGRESS" => pending = true,
            _ => failed = true,
        }
    }
    if failed { Checks::Failed } else if pending { Checks::Pending } else { Checks::Passed }
}

/// The cache record of one Plan's PR (`cache/pr-<plan>.json`): when gh was last tried, when it last worked, and what it said.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub number: u64,
    pub checked_at: u64,
    pub fetched_at: Option<u64>,
    pub data: Option<Value>,
}

impl Record {
    pub fn from_json(v: &Value) -> Option<Record> {
        Some(Record { number: v["number"].as_u64()?, checked_at: v["checkedAt"].as_u64()?, fetched_at: v["fetchedAt"].as_u64(), data: v.get("data").filter(|d| !d.is_null()).cloned() })
    }

    pub fn to_json(&self) -> Value {
        json!({"number": self.number, "checkedAt": self.checked_at, "fetchedAt": self.fetched_at, "data": self.data})
    }

    /// The record after a gh attempt at `now`: a failure keeps the last known data and its `fetched_at`.
    pub fn after(prev: Option<&Record>, number: u64, now: u64, result: Option<Value>) -> Record {
        match (result, prev.filter(|p| p.number == number)) {
            (Some(data), _) => Record { number, checked_at: now, fetched_at: Some(now), data: Some(data) },
            (None, Some(p)) => Record { checked_at: now, ..p.clone() },
            (None, None) => Record { number, checked_at: now, fetched_at: None, data: None },
        }
    }
}

/// The refresh rule: read gh when nothing is cached for this PR number, or the last attempt is a minute old.
pub fn due(rec: Option<&Record>, number: u64, now: u64) -> bool {
    match rec {
        Some(r) if r.number == number => now.saturating_sub(r.checked_at) >= REFRESH_SECS,
        _ => true,
    }
}

fn age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}

/// Cut `s` to `max` characters, ending in `…` when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.into();
    }
    let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
    if max > 0 {
        t.push('…');
    }
    t
}

/// The lines of one Plan: its label, then `#n state checks review title` (title cut first to fit `width`).
/// `pr` is the Plan's recorded PR number (`None`: no PR); `rec` its cache record. A record whose last gh attempt failed
/// is stale: dimmed, with the age of its data.
pub fn render(label: &str, pr: Option<u64>, rec: Option<&Record>, now: u64, width: usize) -> Vec<Line<'static>> {
    let head = Line::from(vec![" ".into(), truncate(label, width.saturating_sub(1)).bold()]);
    let Some(number) = pr else {
        return vec![Line::from(vec![" ".into(), truncate(label, width.saturating_sub(8)).bold(), "  no PR".dim()])];
    };
    let rec = rec.filter(|r| r.number == number);
    let live = rec.and_then(|r| r.data.as_ref()).and_then(|d| parse_live(d).ok());
    let num = format!("  #{number}");
    let Some(live) = live else {
        let note = if rec.is_some_and(|r| r.checked_at > 0) { " gh failed, no data yet" } else { " loading…" };
        return vec![head, Line::from(vec![num.dim(), note.dim()])];
    };
    let stale = rec.is_some_and(|r| r.fetched_at != Some(r.checked_at));
    let (sym, checks_meaning) = palette::checks(live.checks);
    let mut spans: Vec<Span> = vec![num.clone().into(), " ".into(), live.state.clone().fg(palette::color(palette::pr_state(&live.state))), " ".into(), sym.fg(palette::color(checks_meaning)), " ".into(), live.review.clone().into()];
    let mut used = num.chars().count() + 1 + live.state.chars().count() + 3 + live.review.chars().count();
    if stale {
        let ago = rec.and_then(|r| r.fetched_at).map(|t| format!(" {} ago", age(now.saturating_sub(t)))).unwrap_or_default();
        used += ago.chars().count();
        spans.push(ago.into());
    }
    let room = width.saturating_sub(used + 2);
    if room >= 4 {
        spans.push(format!("  {}", truncate(&live.title, room)).dim());
    }
    let line = Line::from(spans);
    vec![head, if stale { line.dim() } else { line }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::Meaning;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn gh(state: &str, rollup: Value, review: &str) -> Value {
        json!({"state": state, "statusCheckRollup": rollup, "reviewDecision": review, "title": "A long pull request title here", "url": "u", "mergeable": "UNKNOWN"})
    }

    fn done(c: &str) -> Value {
        json!({"__typename": "CheckRun", "status": "COMPLETED", "conclusion": c})
    }

    fn rec(data: Option<Value>, checked: u64, fetched: Option<u64>) -> Record {
        Record { number: 2, checked_at: checked, fetched_at: fetched, data }
    }

    #[test]
    fn checks_summarise_the_rollup() {
        let c = |r: Value| parse_live(&gh("OPEN", r, "")).unwrap().checks;
        assert_eq!(c(json!([])), Checks::None);
        assert_eq!(c(json!([done("SUCCESS"), done("SKIPPED")])), Checks::Passed);
        assert_eq!(c(json!([done("SUCCESS"), json!({"__typename": "CheckRun", "status": "IN_PROGRESS", "conclusion": ""})])), Checks::Pending);
        assert_eq!(c(json!([done("FAILURE"), json!({"__typename": "CheckRun", "status": "QUEUED", "conclusion": ""})])), Checks::Failed);
        assert_eq!(c(json!([json!({"__typename": "StatusContext", "state": "SUCCESS"})])), Checks::Passed);
        assert_eq!(c(json!([json!({"__typename": "StatusContext", "state": "ERROR"})])), Checks::Failed);
        assert_eq!(c(json!([json!({"__typename": "StatusContext", "state": "PENDING"})])), Checks::Pending);
    }

    #[test]
    fn state_and_review_are_read() {
        let l = parse_live(&gh("MERGED", json!([]), "APPROVED")).unwrap();
        assert_eq!((l.state.as_str(), l.review.as_str()), ("merged", "approved"));
        assert_eq!(parse_live(&gh("OPEN", json!([]), "CHANGES_REQUESTED")).unwrap().review, "changes requested");
        assert_eq!(parse_live(&gh("OPEN", json!([]), "")).unwrap().review, "no review");
        assert_eq!(parse_live(&gh("OPEN", json!([]), "REVIEW_REQUIRED")).unwrap().review, "no review");
    }

    #[test]
    fn output_missing_its_fields_is_an_error() {
        assert!(parse_live(&json!({"state": "OPEN"})).is_err());
        assert!(parse_live(&json!({"statusCheckRollup": []})).is_err());
    }

    #[test]
    fn refresh_is_due_when_unknown_or_a_minute_old() {
        assert!(due(None, 2, 1000));
        let r = rec(None, 1000, None);
        assert!(!due(Some(&r), 2, 1059));
        assert!(due(Some(&r), 2, 1060));
        assert!(due(Some(&r), 3, 1001), "another PR number is a new PR");
    }

    #[test]
    fn a_failed_attempt_counts_toward_the_minute_and_keeps_the_data() {
        let ok = Record::after(None, 2, 100, Some(json!({"state": "OPEN"})));
        assert_eq!((ok.checked_at, ok.fetched_at), (100, Some(100)));
        let failed = Record::after(Some(&ok), 2, 200, None);
        assert_eq!((failed.checked_at, failed.fetched_at, failed.data.clone()), (200, Some(100), Some(json!({"state": "OPEN"}))));
        assert!(!due(Some(&failed), 2, 230));
        let other = Record::after(Some(&ok), 3, 200, None);
        assert_eq!((other.fetched_at, other.data), (None, None));
    }

    #[test]
    fn a_record_survives_its_json() {
        let r = rec(Some(json!({"a": 1})), 5, Some(4));
        assert_eq!(Record::from_json(&r.to_json()), Some(r));
        let none = rec(None, 5, None);
        assert_eq!(Record::from_json(&none.to_json()), Some(none));
    }

    #[test]
    fn a_row_shows_number_state_checks_review_and_title() {
        let r = rec(Some(gh("MERGED", json!([done("SUCCESS")]), "APPROVED")), 100, Some(100));
        let lines = render("lead-fork", Some(2), Some(&r), 110, 60);
        assert_eq!(text(&lines[0]), " lead-fork");
        assert_eq!(text(&lines[1]), "  #2 merged ✓ approved  A long pull request title here");
    }

    #[test]
    fn a_row_takes_its_colours_from_the_palette() {
        let colours = |state: &str, rollup: Value| {
            let r = rec(Some(gh(state, rollup, "")), 100, Some(100));
            let spans = render("p", Some(2), Some(&r), 100, 60)[1].spans.clone();
            let fg = |t: &str| spans.iter().find(|s| s.content == t).and_then(|s| s.style.fg).unwrap();
            (fg(state.to_lowercase().as_str()), fg(palette::checks(parse_live(&r.data.unwrap()).unwrap().checks).0))
        };
        let c = |m| palette::color(m);
        assert_eq!(colours("OPEN", json!([done("SUCCESS")])), (c(Meaning::Review), c(Meaning::Done)));
        assert_eq!(colours("MERGED", json!([done("FAILURE")])), (c(Meaning::Done), c(Meaning::Failed)));
        assert_eq!(colours("CLOSED", json!([])), (c(Meaning::Failed), c(Meaning::Nothing)));
        assert_eq!(colours("OPEN", json!([json!({"__typename": "CheckRun", "status": "IN_PROGRESS", "conclusion": ""})])), (c(Meaning::Review), c(Meaning::InProgress)));
    }

    #[test]
    fn the_title_is_cut_first() {
        let r = rec(Some(gh("OPEN", json!([done("FAILURE")]), "CHANGES_REQUESTED")), 100, Some(100));
        let l = text(&render("x", Some(12), Some(&Record { number: 12, ..r.clone() }), 100, 43)[1]);
        assert!(l.starts_with("  #12 open ✗ changes requested  A long"), "{l}");
        assert!(l.ends_with('…') && l.chars().count() <= 43, "{l}");
        let tight = text(&render("x", Some(12), Some(&Record { number: 12, ..r }), 100, 33)[1]);
        assert_eq!(tight, "  #12 open ✗ changes requested");
    }

    #[test]
    fn no_pr_and_pending_states() {
        assert_eq!(text(&render("plan", None, None, 0, 43)[0]), " plan  no PR");
        assert_eq!(text(&render("plan", Some(2), None, 0, 43)[1]), "  #2 loading…");
        let failed = rec(None, 50, None);
        assert_eq!(text(&render("plan", Some(2), Some(&failed), 60, 43)[1]), "  #2 gh failed, no data yet");
    }

    #[test]
    fn a_failed_refresh_shows_the_last_state_dimmed_with_its_age() {
        let r = rec(Some(gh("MERGED", json!([done("SUCCESS")]), "APPROVED")), 400, Some(100));
        let lines = render("p", Some(2), Some(&r), 400, 60);
        assert!(text(&lines[1]).starts_with("  #2 merged ✓ approved 5m ago"), "{}", text(&lines[1]));
        assert!(lines[1].style.add_modifier.contains(ratatui::style::Modifier::DIM));
        let fresh = rec(Some(gh("MERGED", json!([done("SUCCESS")]), "APPROVED")), 400, Some(400));
        assert!(!render("p", Some(2), Some(&fresh), 400, 60)[1].style.add_modifier.contains(ratatui::style::Modifier::DIM));
    }

    #[test]
    fn a_long_label_is_cut() {
        assert_eq!(text(&render("abcdefghij", None, None, 0, 14)[0]), " abcde…  no PR");
    }
}
