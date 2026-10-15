//! Session metrics (contract §2.2 `metrics`, A9; cpd 1 §14) from fixture pi session files.
//!
//! The expected numbers are what cpd 1 computes for the same files:
//! `sessionMetrics(path)` of `src/metrics/session.ts` run under node, on
//! `PLAIN` itself and, for the fork, on `FORK`'s header and the entries after it.

use std::path::Path;

use cpd_core::error::ErrorCode;
use cpd_core::metrics::{self, Metrics};
use serde_json::json;

/// A Worker's session: a model, a thinking level, an entry of a kind metrics skip, a model
/// change, two assistant messages.
const PLAIN: &str = r#"{"type":"session","version":3,"id":"aaa","timestamp":"2026-01-01T00:00:00.000Z","cwd":"/w"}
{"type":"model_change","id":"1","parentId":null,"timestamp":"2026-01-01T00:00:00.100Z","provider":"p","modelId":"claude-sonnet-x"}
{"type":"thinking_level_change","id":"2","parentId":"1","timestamp":"2026-01-01T00:00:00.200Z","thinkingLevel":"high"}
{"type":"message","id":"3","parentId":"2","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"user","content":"go"}}
{"type":"message","id":"4","parentId":"3","timestamp":"2026-01-01T00:00:05.500Z","message":{"role":"assistant","content":[],"usage":{"input":100,"output":50,"cacheRead":1000,"cacheWrite":200,"totalTokens":1350,"cost":{"input":0.01,"output":0.002,"cacheRead":0.0003,"cacheWrite":0,"total":0.0123}},"stopReason":"toolUse"}}
{"type":"custom","id":"5","parentId":"4","timestamp":"2026-01-01T00:00:06.000Z","customType":"x"}
{"type":"message","id":"6","parentId":"5","timestamp":"2026-01-01T00:00:06.250Z","message":{"role":"toolResult","content":"ok"}}
{"type":"model_change","id":"7","parentId":"6","timestamp":"2026-01-01T00:00:07.000Z","provider":"p","modelId":"claude-opus-y"}
{"type":"message","id":"8","parentId":"7","timestamp":"2026-01-01T00:00:20.250Z","message":{"role":"assistant","content":[],"usage":{"input":10,"output":5,"cacheRead":0,"cacheWrite":7,"totalTokens":22,"cost":{"total":0.0045}},"stopReason":"stop"}}
"#;

/// A fork of `PLAIN` an hour later: its header, then `PLAIN`'s entries as pi copies them (their
/// own, earlier timestamps), then what the fork added.
fn fork(added: &[&str]) -> String {
    let header = r#"{"type":"session","version":3,"id":"bbb","timestamp":"2026-01-01T01:00:00.000Z","cwd":"/w","parentSession":"/s/plain.jsonl"}"#;
    let inherited: String = PLAIN.lines().skip(1).map(|l| format!("{l}\n")).collect();
    let added: String = added.iter().map(|l| format!("{l}\n")).collect();
    format!("{header}\n{inherited}{added}")
}

const USER: &str = r#"{"type":"message","id":"a1","parentId":"8","timestamp":"2026-01-01T01:00:02.000Z","message":{"role":"user","content":"continue"}}"#;
const LENGTH: &str = r#"{"type":"message","id":"a2","parentId":"a1","timestamp":"2026-01-01T01:00:12.500Z","message":{"role":"assistant","content":[],"usage":{"input":3,"output":4,"cacheRead":5,"cacheWrite":6,"cost":{"total":0.5}},"stopReason":"length"}}"#;
const STOP: &str = r#"{"type":"message","id":"a3","parentId":"a2","timestamp":"2026-01-01T01:00:30.000Z","message":{"role":"assistant","content":[],"usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.25}},"stopReason":"stop"}}"#;

fn parse(text: &str) -> Result<Metrics, cpd_core::error::CpdError> {
    metrics::parse(text, Path::new("/s/session.jsonl"))
}

#[test]
fn a_plain_session_matches_cpd_1() {
    // cpd 1: {"turns":2,"tokensIn":110,"tokensOut":55,"cacheRead":1000,"cacheWrite":207,
    //         "costUsd":0.0168,"durationS":20.25,"model":"claude-opus-y","thinkingLevel":"high",
    //         "finalStopReason":"stop"}
    let m = parse(PLAIN).unwrap();
    assert_eq!(
        m,
        Metrics {
            turns: 2,
            tokens_in: 110,
            tokens_out: 55,
            cache_read: 1000,
            cache_write: 207,
            cost_usd: 0.0168,
            duration_s: 20.25,
            model: Some("claude-opus-y".to_owned()),
            thinking_level: Some("high".to_owned()),
            stop_reason: Some("stop".to_owned()),
        }
    );
}

#[test]
fn the_json_has_the_contract_s_keys() {
    let value = parse(PLAIN).unwrap().to_json();
    assert_eq!(
        value,
        json!({
            "turns": 2, "tokens_in": 110, "tokens_out": 55, "cache_read": 1000, "cache_write": 207,
            "cost_usd": 0.0168, "duration_s": 20.25, "model": "claude-opus-y", "thinking_level": "high",
            "stop_reason": "stop",
        })
    );
}

#[test]
fn a_fork_counts_only_what_it_added() {
    // cpd 1 on the fork's header and its own entries: {"turns":2,"tokensIn":4,"tokensOut":5,
    // "cacheRead":5,"cacheWrite":6,"costUsd":0.75,"durationS":30,"finalStopReason":"stop"}.
    // cpd 1 on the whole fork file would count PLAIN's two turns again.
    let m = parse(&fork(&[USER, LENGTH, STOP])).unwrap();
    assert_eq!((m.turns, m.tokens_in, m.tokens_out, m.cache_read, m.cache_write), (2, 4, 5, 5, 6));
    assert_eq!((m.cost_usd, m.duration_s), (0.75, 30.0));
    assert_eq!(m.stop_reason.as_deref(), Some("stop"));
}

#[test]
fn a_fork_starts_from_the_model_and_thinking_level_it_inherited() {
    let m = parse(&fork(&[USER, LENGTH])).unwrap();
    assert_eq!((m.model.as_deref(), m.thinking_level.as_deref()), (Some("claude-opus-y"), Some("high")));
    let changed = r#"{"type":"model_change","id":"a0","parentId":"8","timestamp":"2026-01-01T01:00:01.000Z","provider":"p","modelId":"claude-haiku-z"}"#;
    assert_eq!(parse(&fork(&[changed, LENGTH])).unwrap().model.as_deref(), Some("claude-haiku-z"));
}

#[test]
fn a_fork_that_has_done_nothing_yet_has_no_turns_no_duration_and_no_stop_reason() {
    let m = parse(&fork(&[])).unwrap();
    assert_eq!((m.turns, m.tokens_in, m.cost_usd, m.duration_s), (0, 0, 0.0, 0.0));
    assert_eq!(m.stop_reason, None);
}

#[test]
fn a_session_without_assistant_messages_is_zeros_and_nulls() {
    let m = parse("{\"type\":\"session\",\"timestamp\":\"2026-01-01T00:00:00.000Z\"}\n").unwrap();
    assert_eq!((m.turns, m.duration_s, m.model, m.thinking_level, m.stop_reason), (0, 0.0, None, None, None));
}

#[test]
fn parse_file_reads_the_path_it_is_given() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.jsonl");
    std::fs::write(&path, PLAIN).unwrap();
    assert_eq!(metrics::parse_file(&path).unwrap(), parse(PLAIN).unwrap());
    let err = metrics::parse_file(&dir.path().join("missing.jsonl")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::FileUnreadable);
}

/// The line a malformed file is refused at.
fn refused_at(text: &str) -> u64 {
    let err = parse(text).unwrap_err();
    assert_eq!(err.code(), ErrorCode::SessionMalformed);
    assert_eq!(err.details()["path"], "/s/session.jsonl");
    err.details()["line"].as_u64().unwrap()
}

#[test]
fn every_complete_line_must_be_a_session_entry() {
    let header = PLAIN.lines().next().unwrap();
    assert_eq!(refused_at(""), 1);
    assert_eq!(refused_at(&format!("{header}\nnot json\n")), 2);
    assert_eq!(refused_at(&format!("{header}\n\n")), 2, "a blank line is not an entry");
    assert_eq!(refused_at(r#"{"type":"message","timestamp":"2026-01-01T00:00:00.000Z"}"#), 1, "the first entry is the header");
    assert_eq!(refused_at(&format!("{header}\n{{\"type\":\"custom\"}}\n")), 2, "no timestamp");
    assert_eq!(refused_at(&format!("{header}\n{{\"type\":\"custom\",\"timestamp\":\"yesterday\"}}\n")), 2);
    let no_usage = r#"{"type":"message","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","stopReason":"stop"}}"#;
    assert_eq!(refused_at(&format!("{header}\n{no_usage}\n")), 2, "an assistant message needs usage");
    let no_model = r#"{"type":"model_change","timestamp":"2026-01-01T00:00:01.000Z"}"#;
    assert_eq!(refused_at(&format!("{header}\n{USER}\n{no_model}\n")), 3);
}
