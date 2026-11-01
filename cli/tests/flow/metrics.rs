//! Session metrics (contract §2.2 `metrics`, A9; cpd 1 §14) from fixture pi session files.
//!
//! The expected numbers are what cpd 1 computes for the same files:
//! `sessionMetrics(path)` of `src/metrics/session.ts` run under node, on
//! `PLAIN` itself and, for the fork, on `FORK`'s header and the entries after it.
//!
//! Also: the `cpd metrics` command (hs-d798, commands.md §14.7), a read-only aggregate
//! of the machine's Ledger.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::common::fx::Fx;

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
    assert_eq!(
        (
            m.turns,
            m.tokens_in,
            m.tokens_out,
            m.cache_read,
            m.cache_write
        ),
        (2, 4, 5, 5, 6)
    );
    assert_eq!((m.cost_usd, m.duration_s), (0.75, 30.0));
    assert_eq!(m.stop_reason.as_deref(), Some("stop"));
}

#[test]
fn a_fork_starts_from_the_model_and_thinking_level_it_inherited() {
    let m = parse(&fork(&[USER, LENGTH])).unwrap();
    assert_eq!(
        (m.model.as_deref(), m.thinking_level.as_deref()),
        (Some("claude-opus-y"), Some("high"))
    );
    let changed = r#"{"type":"model_change","id":"a0","parentId":"8","timestamp":"2026-01-01T01:00:01.000Z","provider":"p","modelId":"claude-haiku-z"}"#;
    assert_eq!(
        parse(&fork(&[changed, LENGTH])).unwrap().model.as_deref(),
        Some("claude-haiku-z")
    );
}

#[test]
fn a_fork_that_has_done_nothing_yet_has_no_turns_no_duration_and_no_stop_reason() {
    let m = parse(&fork(&[])).unwrap();
    assert_eq!(
        (m.turns, m.tokens_in, m.cost_usd, m.duration_s),
        (0, 0, 0.0, 0.0)
    );
    assert_eq!(m.stop_reason, None);
}

#[test]
fn a_session_without_assistant_messages_is_zeros_and_nulls() {
    let m = parse("{\"type\":\"session\",\"timestamp\":\"2026-01-01T00:00:00.000Z\"}\n").unwrap();
    assert_eq!(
        (
            m.turns,
            m.duration_s,
            m.model,
            m.thinking_level,
            m.stop_reason
        ),
        (0, 0.0, None, None, None)
    );
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
    assert_eq!(
        refused_at(&format!("{header}\n\n")),
        2,
        "a blank line is not an entry"
    );
    assert_eq!(
        refused_at(r#"{"type":"message","timestamp":"2026-01-01T00:00:00.000Z"}"#),
        1,
        "the first entry is the header"
    );
    assert_eq!(
        refused_at(&format!("{header}\n{{\"type\":\"custom\"}}\n")),
        2,
        "no timestamp"
    );
    assert_eq!(
        refused_at(&format!(
            "{header}\n{{\"type\":\"custom\",\"timestamp\":\"yesterday\"}}\n"
        )),
        2
    );
    let no_usage = r#"{"type":"message","timestamp":"2026-01-01T00:00:01.000Z","message":{"role":"assistant","stopReason":"stop"}}"#;
    assert_eq!(
        refused_at(&format!("{header}\n{no_usage}\n")),
        2,
        "an assistant message needs usage"
    );
    let no_model = r#"{"type":"model_change","timestamp":"2026-01-01T00:00:01.000Z"}"#;
    assert_eq!(refused_at(&format!("{header}\n{USER}\n{no_model}\n")), 3);
}

// ---------- cpd metrics (commands.md §14.7, hs-d798) ----------

/// A fixture store: Context `ctx`, Plan `pl`, PR `repo` merged, Job `api` merged, Attempt
/// `api-a1` ended `merged` with its `attempts.metrics` blob and its `attempt-ended` Event.
fn fixture_metrics() -> Fx {
    let fx = Fx::with_ports();
    let env = &fx.env;
    env.write_config("version = 1\nmachine = \"m1\"\n");
    env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pl', 'ctx', 'Pl', 'done', 'lead1', 't', 't', 't');
         INSERT INTO pull_requests (id, repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES (1, 'acme/pl', 1, 'https://github.com/acme/pl/1', 'Pl #1', 0, 'merged', 'approved', 'passed', 't', 't', 't');
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, merged_at,
                          pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pl', 'repo', '/repos/pl', 'pl', 'main', 'merged', 'pl/repo', '/repos/pl.pl', cpd_txn(), 't', 't', 't',
                   1, 'https://github.com/acme/pl/1', 't', 1);
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, merged_sha, created_txn, created_at, kind)
           VALUES ('pl', 'repo', 'api', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'merged-sha-test', cpd_txn(), 't', 'work');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree,
                               base_sha, merged_sha, started_at, ended_at, metrics)
           VALUES ('pl', 'api-a1', 'api', 1, 'S2', 'ladder', 'ended', 'merged', 'merged', 'cpd/pl--api-a1', '/repos/pl--api-a1',
                   '0123456789abcdef0123456789abcdef01234567', '0123456789abcdef0123456789abcdef01234567', 't', 't',
                   '{\"turns\": 3, \"tokens_in\": 900, \"tokens_out\": 400, \"cache_read\": 2000, \"cache_write\": 500, \"cost_usd\": 1.234, \"duration_s\": 62.5, \"model\": \"claude-sonnet-x\", \"stop_reason\": \"stop\", \"thinking_level\": \"high\"}');
        ",
    );
    fx
}

#[test]
fn cpd_metrics_aggregates_a_fixture_store() {
    let fx = fixture_metrics();
    let store = fx.env.cpd_home().join("cpd.db").display().to_string();
    let run = fx.cpd(&["metrics"]);
    let out = run.ok();
    assert_eq!(
        out,
        json!({
            "ok": true,
            "store": store,
            "per_tier": {
                "S2": {
                    "attempts": 1,
                    "merged": 1,
                    "failed_by_cause": {},
                    "duration_s": { "avg": 62.5, "median": 62.5 },
                    "cost_usd": { "avg": 1.234, "total": 1.234 }
                }
            },
            "per_model": {
                "claude-sonnet-x": {
                    "attempts": 1,
                    "merged": 1,
                    "failed_by_cause": {},
                    "duration_s": { "avg": 62.5, "median": 62.5 },
                    "cost_usd": { "avg": 1.234, "total": 1.234 }
                }
            },
            "retries": {},
            "plans": { "pl": 1.234 },
            "queue": { "job_added_to_attempt_started": { "avg": 0.0, "max": 0.0 }, "job_blocked_episodes": 0 }
        })
    );
}

/// The parsed Prometheus text exposition format: the families' `# HELP`/`# TYPE` and their series.
#[derive(Default)]
struct Prometheus {
    help: BTreeMap<String, String>,
    types: BTreeMap<String, String>,
    /// family name -> the label set as written -> its value.
    series: BTreeMap<String, BTreeMap<String, f64>>,
}

/// Parses `body` enforcing the text format: `# HELP` and `# TYPE` before each family's first series, a known gauge
/// type, and no duplicate series. Panics on a violation.
fn parse_prometheus(body: &str) -> Prometheus {
    let mut prom = Prometheus::default();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for line in body.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("# HELP ") {
            let (name, text) = rest
                .split_once(' ')
                .expect("a HELP line needs a name and text");
            assert!(
                prom.help.insert(name.to_owned(), text.to_owned()).is_none(),
                "duplicate HELP for {name}"
            );
            assert!(
                prom.series.get(name).is_none_or(BTreeMap::is_empty),
                "HELP for {name} comes after its series"
            );
            continue;
        }
        if let Some(rest) = line.strip_prefix("# TYPE ") {
            let (name, kind) = rest
                .split_once(' ')
                .expect("a TYPE line needs a name and kind");
            assert_eq!(kind, "gauge", "{name} is not a gauge");
            assert!(
                prom.types
                    .insert(name.to_owned(), kind.to_owned())
                    .is_none(),
                "duplicate TYPE for {name}"
            );
            continue;
        }
        assert!(!line.starts_with('#'), "an unknown comment line: {line}");
        let (name, labels, value) = if let Some(open) = line.find('{') {
            let close = line.rfind('}').expect("a series needs a closing brace");
            (
                &line[..open],
                line[open + 1..close].to_owned(),
                line[close + 1..].trim(),
            )
        } else {
            let (name, value) = line
                .split_once(' ')
                .expect("a series needs a name and a value");
            (name, String::new(), value.trim())
        };
        assert!(
            prom.help.contains_key(name),
            "series {name} before its HELP"
        );
        assert!(
            prom.types.contains_key(name),
            "series {name} before its TYPE"
        );
        assert!(
            seen.insert((name.to_owned(), labels.clone())),
            "duplicate series {name}{{{labels}}}"
        );
        let value: f64 = value
            .parse()
            .unwrap_or_else(|e| panic!("series {name} has value {value:?}: {e}"));
        prom.series
            .entry(name.to_owned())
            .or_default()
            .insert(labels, value);
    }
    prom
}

#[test]
fn prometheus_output_parses() {
    let fx = fixture_metrics();
    let run = fx.cpd(&["metrics", "--format", "prometheus"]);
    let out = run.ok();
    assert_eq!(out["ok"], true);
    let body = out["data"].as_str().unwrap();
    let prom = parse_prometheus(body);

    // Every family of the Spec is a gauge with its # HELP and # TYPE before its series.
    for family in [
        "cpd_attempts",
        "cpd_attempts_merged",
        "cpd_attempts_failed",
        "cpd_attempt_duration_seconds",
        "cpd_attempt_cost_usd_total",
        "cpd_attempt_cost_usd_avg",
        "cpd_model_attempts",
        "cpd_model_cost_usd_total",
        "cpd_attempt_retries",
        "cpd_plan_cost_usd_per_merged_pr",
        "cpd_queue_wait_seconds",
        "cpd_job_blocked_episodes",
    ] {
        assert!(prom.help.contains_key(family), "no HELP for {family}");
        assert_eq!(
            prom.types.get(family).map(String::as_str),
            Some("gauge"),
            "no TYPE gauge for {family}"
        );
    }

    let series = |name: &str, labels: &str| {
        *prom
            .series
            .get(name)
            .and_then(|s| s.get(labels))
            .unwrap_or_else(|| panic!("no series {name}{{{labels}}}"))
    };
    assert_eq!(
        series("cpd_attempts", "tier=\"S2\",model=\"claude-sonnet-x\""),
        1.0
    );
    assert_eq!(series("cpd_attempts_merged", "tier=\"S2\""), 1.0);
    assert_eq!(
        series("cpd_attempt_duration_seconds", "tier=\"S2\",stat=\"avg\""),
        62.5
    );
    assert_eq!(
        series(
            "cpd_attempt_duration_seconds",
            "tier=\"S2\",stat=\"median\""
        ),
        62.5
    );
    assert_eq!(series("cpd_attempt_cost_usd_total", "tier=\"S2\""), 1.234);
    assert_eq!(series("cpd_attempt_cost_usd_avg", "tier=\"S2\""), 1.234);
    assert_eq!(
        series("cpd_model_attempts", "model=\"claude-sonnet-x\""),
        1.0
    );
    assert_eq!(
        series("cpd_model_cost_usd_total", "model=\"claude-sonnet-x\""),
        1.234
    );
    assert_eq!(
        series("cpd_plan_cost_usd_per_merged_pr", "plan=\"pl\""),
        1.234
    );
    assert_eq!(
        series(
            "cpd_queue_wait_seconds",
            "phase=\"added_to_started\",stat=\"avg\""
        ),
        0.0
    );
    assert_eq!(
        series(
            "cpd_queue_wait_seconds",
            "phase=\"added_to_started\",stat=\"max\""
        ),
        0.0
    );
    assert_eq!(series("cpd_job_blocked_episodes", ""), 0.0);
}
