//! Red tests: the parser sums standalone `usage`, `compaction` and `branch_summary` usage, and `format_cost`.

use std::path::Path;

use super::{Metrics, format_cost, parse as parse_at};
use crate::error::{CpdError, ErrorCode};

const HEADER: &str = r#"{"type":"session","version":3,"id":"a","timestamp":"2026-01-01T00:00:00.000Z","cwd":"/w"}"#;
const FORK_HEADER: &str = r#"{"type":"session","version":3,"id":"b","timestamp":"2026-01-01T01:00:00.000Z","cwd":"/w","parentSession":"/s/plain.jsonl"}"#;

fn parse(text: &str) -> Result<Metrics, CpdError> {
    parse_at(text, Path::new("/s/session.jsonl"))
}

fn lines(items: &[String]) -> String {
    items.iter().map(|l| format!("{l}\n")).collect()
}

fn usage_obj(input: u64, output: u64, cache_read: u64, cache_write: u64, cost: f64) -> String {
    format!(
        r#"{{"input":{input},"output":{output},"cacheRead":{cache_read},"cacheWrite":{cache_write},"totalTokens":{},"reasoning":0,"cost":{{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":{cost}}}}}"#,
        input + output + cache_read + cache_write
    )
}

fn message(id: &str, ts: &str, usage: &str, stop: &str) -> String {
    format!(
        r#"{{"type":"message","id":"{id}","parentId":null,"timestamp":"{ts}","message":{{"role":"assistant","content":[],"usage":{usage},"stopReason":"{stop}"}}}}"#
    )
}

fn usage_entry(id: &str, ts: &str, kind: &str, usage: &str) -> String {
    format!(
        r#"{{"type":"usage","id":"{id}","parentId":null,"timestamp":"{ts}","kind":"{kind}","provider":"p","model":"m","usage":{usage}}}"#
    )
}

fn compaction(id: &str, ts: &str, usage: Option<&str>) -> String {
    let usage = usage.map(|u| format!(r#","usage":{u}"#)).unwrap_or_default();
    format!(
        r#"{{"type":"compaction","id":"{id}","parentId":null,"timestamp":"{ts}","summary":"s","firstKeptEntryId":"x","tokensBefore":1000{usage}}}"#
    )
}

fn branch_summary(id: &str, ts: &str, usage: Option<&str>) -> String {
    let usage = usage.map(|u| format!(r#","usage":{u}"#)).unwrap_or_default();
    format!(
        r#"{{"type":"branch_summary","id":"{id}","parentId":null,"timestamp":"{ts}","fromId":"x","summary":"s"{usage}}}"#
    )
}

fn session(entries: &[String]) -> String {
    format!("{HEADER}\n{}", lines(entries))
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

const T1: &str = "2026-01-01T00:00:01.000Z";
const T2: &str = "2026-01-01T00:00:02.000Z";
const T3: &str = "2026-01-01T00:00:03.000Z";
const T4: &str = "2026-01-01T00:00:04.000Z";

fn base_message() -> String {
    message("m1", T1, &usage_obj(100, 50, 1000, 200, 0.0123), "stop")
}

#[test]
fn a_standalone_usage_entry_adds_its_tokens_and_cost() {
    let text = session(&[base_message(), usage_entry("u1", T2, "cache_warm", &usage_obj(0, 0, 50000, 0, 0.015))]);
    let m = parse(&text).unwrap();
    assert_eq!(m.turns, 1);
    assert_eq!(m.tokens_in, 100);
    assert_eq!(m.tokens_out, 50);
    assert_eq!(m.cache_read, 51000);
    assert_eq!(m.cache_write, 200);
    close(m.cost_usd, 0.0273);
}

#[test]
fn an_unknown_usage_kind_counts_as_usage() {
    let text = session(&[usage_entry("u1", T1, "anything-else", &usage_obj(7, 3, 0, 0, 0.25))]);
    let m = parse(&text).unwrap();
    assert_eq!((m.tokens_in, m.tokens_out), (7, 3));
    close(m.cost_usd, 0.25);
}

#[test]
fn a_compaction_usage_adds_its_tokens_and_cost() {
    let text = session(&[base_message(), compaction("c1", T2, Some(&usage_obj(11, 12, 13, 14, 0.5)))]);
    let m = parse(&text).unwrap();
    assert_eq!(m.turns, 1);
    assert_eq!((m.tokens_in, m.tokens_out, m.cache_read, m.cache_write), (111, 62, 1013, 214));
    close(m.cost_usd, 0.5123);
}

#[test]
fn a_branch_summary_usage_adds_its_tokens_and_cost() {
    let text = session(&[base_message(), branch_summary("b1", T2, Some(&usage_obj(21, 22, 23, 24, 0.75)))]);
    let m = parse(&text).unwrap();
    assert_eq!(m.turns, 1);
    assert_eq!((m.tokens_in, m.tokens_out, m.cache_read, m.cache_write), (121, 72, 1023, 224));
    close(m.cost_usd, 0.7623);
}

#[test]
fn a_compaction_or_branch_summary_without_usage_adds_nothing() {
    let without = parse(&session(&[base_message()])).unwrap();
    let with = parse(&session(&[base_message(), compaction("c1", T2, None), branch_summary("b1", T3, None)])).unwrap();
    // A usage entry beside them still counts: the entries without usage add nothing to it.
    let beside = parse(&session(&[
        base_message(),
        compaction("c1", T2, None),
        usage_entry("u1", T3, "cache_warm", &usage_obj(1, 0, 0, 0, 0.5)),
        branch_summary("b1", T4, None),
    ]))
    .unwrap();
    assert_eq!(beside.tokens_in, without.tokens_in + 1);
    close(beside.cost_usd, without.cost_usd + 0.5);
    assert_eq!(with.turns, without.turns);
    assert_eq!(with.tokens_in, without.tokens_in);
    assert_eq!(with.tokens_out, without.tokens_out);
    assert_eq!(with.cache_read, without.cache_read);
    assert_eq!(with.cache_write, without.cache_write);
    close(with.cost_usd, without.cost_usd);
    close(with.cost_usd, 0.0123);
}

#[test]
fn the_four_sources_sum_into_the_session_total() {
    let text = session(&[
        message("m1", T1, &usage_obj(1, 2, 3, 4, 0.1), "stop"),
        usage_entry("u1", T2, "cache_warm", &usage_obj(10, 20, 30, 40, 0.02)),
        compaction("c1", T3, Some(&usage_obj(100, 200, 300, 400, 0.003))),
        branch_summary("b1", T4, Some(&usage_obj(1000, 2000, 3000, 4000, 0.0004))),
    ]);
    let m = parse(&text).unwrap();
    assert_eq!(m.turns, 1);
    assert_eq!(m.tokens_in, 1111);
    assert_eq!(m.tokens_out, 2222);
    assert_eq!(m.cache_read, 3333);
    assert_eq!(m.cache_write, 4444);
    close(m.cost_usd, 0.1234);
}

#[test]
fn a_standalone_usage_is_no_turn_and_gives_no_stop_reason() {
    let only = parse(&session(&[usage_entry("u1", T1, "cache_warm", &usage_obj(1, 1, 1, 1, 0.1))])).unwrap();
    assert_eq!(only.turns, 0);
    assert_eq!(only.stop_reason, None);
    close(only.cost_usd, 0.1);

    let then = parse(&session(&[
        usage_entry("u1", T1, "cache_warm", &usage_obj(1, 1, 1, 1, 0.1)),
        message("m1", T2, &usage_obj(1, 1, 1, 1, 0.2), "length"),
    ]))
    .unwrap();
    assert_eq!(then.turns, 1);
    assert_eq!(then.stop_reason.as_deref(), Some("length"));
    close(then.cost_usd, 0.3);
}

fn refused_at(text: &str) -> u64 {
    let err = parse(text).unwrap_err();
    assert_eq!(err.code(), ErrorCode::SessionMalformed);
    assert_eq!(err.details()["path"], "/s/session.jsonl");
    err.details()["line"].as_u64().unwrap()
}

#[test]
fn a_usage_that_is_not_the_usage_shape_is_session_malformed() {
    let no_cost = r#"{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2}"#;
    let text = session(&[base_message(), usage_entry("u1", T2, "cache_warm", no_cost)]);
    assert_eq!(refused_at(&text), 3);

    let string_input = r#"{"input":"1","output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.1}}"#;
    let text = session(&[base_message(), base_message(), compaction("c1", T3, Some(string_input))]);
    assert_eq!(refused_at(&text), 4);
}

fn inherited_entries() -> Vec<String> {
    let early = |s: &str| format!("2025-12-31T23:00:{s}.000Z");
    vec![
        message("i1", &early("01"), &usage_obj(1, 1, 1, 1, 1.0), "stop"),
        usage_entry("i2", &early("02"), "cache_warm", &usage_obj(1, 1, 1, 1, 2.0)),
        compaction("i3", &early("03"), Some(&usage_obj(1, 1, 1, 1, 4.0))),
        branch_summary("i4", &early("04"), Some(&usage_obj(1, 1, 1, 1, 8.0))),
    ]
}

#[test]
fn an_inherited_usage_compaction_or_branch_summary_of_a_fork_never_counts() {
    let mut entries = inherited_entries();
    // One entry the fork added, so that only the inherited ones are left out of the sums.
    entries.push(usage_entry("a1", "2026-01-01T01:00:05.000Z", "cache_warm", &usage_obj(5, 0, 0, 0, 0.5)));
    let text = format!("{FORK_HEADER}\n{}", lines(&entries));
    let m = parse(&text).unwrap();
    assert_eq!(m.turns, 0);
    assert_eq!((m.tokens_in, m.tokens_out, m.cache_read, m.cache_write), (5, 0, 0, 0));
    close(m.cost_usd, 0.5);
    close(m.duration_s, 5.0);
    assert_eq!(m.stop_reason, None);
}

#[test]
fn a_fork_counts_the_usage_entries_it_added() {
    let mut entries = inherited_entries();
    entries.push(usage_entry("a1", "2026-01-01T01:00:10.000Z", "cache_warm", &usage_obj(10, 20, 30, 40, 0.01)));
    entries.push(compaction("a2", "2026-01-01T01:00:20.000Z", Some(&usage_obj(100, 200, 300, 400, 0.02))));
    entries.push(branch_summary("a3", "2026-01-01T01:00:30.000Z", Some(&usage_obj(1000, 2000, 3000, 4000, 0.04))));
    let text = format!("{FORK_HEADER}\n{}", lines(&entries));
    let m = parse(&text).unwrap();
    assert_eq!(m.turns, 0);
    assert_eq!((m.tokens_in, m.tokens_out, m.cache_read, m.cache_write), (1110, 2220, 3330, 4440));
    close(m.cost_usd, 0.07);
    close(m.duration_s, 30.0);
}

fn pin(table: &[(f64, &str)]) {
    for (usd, want) in table {
        assert_eq!(format_cost(*usd), *want, "format_cost({usd})");
    }
}

#[test]
fn format_cost_of_zero_and_under_half_a_mill_is_dollar_zero_point_zeros() {
    pin(&[(0.0, "$0.000"), (0.0004, "$0.000")]);
}

#[test]
fn format_cost_below_a_cent_has_no_special_form() {
    pin(&[(0.004, "$0.004"), (0.0123, "$0.012")]);
}

#[test]
fn format_cost_fills_five_cells_after_the_dollar_under_a_thousand() {
    pin(&[(0.12, "$0.120"), (1.2344, "$1.234"), (12.344, "$12.34"), (123.44, "$123.4")]);
}

#[test]
fn format_cost_in_thousands_is_two_one_or_no_decimals_and_k() {
    pin(&[(999.96, "$1.00k"), (1234.0, "$1.23k"), (12345.0, "$12.3k"), (123456.0, "$123k")]);
}

#[test]
fn format_cost_switches_form_at_the_rounded_boundary() {
    pin(&[
        (9.9994, "$9.999"),
        (9.9996, "$10.00"),
        (99.994, "$99.99"),
        (99.996, "$100.0"),
        (999.94, "$999.9"),
        (999.96, "$1.00k"),
        (9994.0, "$9.99k"),
        (9996.0, "$10.0k"),
        (99949.0, "$99.9k"),
        (99951.0, "$100k"),
        (999499.0, "$999k"),
        (999501.0, "$1000k"),
    ]);
}
