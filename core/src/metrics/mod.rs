//! Session metrics (contract §2.2 Attempt `metrics`, A9; cpd 1 `docs/archive/cpd1/cli-contract.md` §14):
//! parsed from the Worker's pi session file, whose path comes from the Session's
//! `file`, never from the cwd.
//!
//! - `turns` = the assistant messages; `tokens_in`, `tokens_out`, `cache_read`,
//!   `cache_write`, `cost_usd` = the sums of their `usage`;
//! - `duration_s` = the last entry's timestamp minus the header's;
//! - `model`, `thinking_level` = the last change; `stop_reason` = the last assistant message's.
//!
//! A fork's file keeps its parent's entries, copied under their own earlier
//! timestamps, after a header stamped at the fork (pi `SessionManager.forkFrom`).
//! Those entries are not the Attempt's work: nothing counts them, they do not
//! move `duration_s`, and they do not give `stop_reason`. They do give `model`
//! and `thinking_level` their starting values, because a fork that never changes
//! them still runs with them (assumption: the contract does not say).

use std::path::Path;

use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{CpdError, ErrorCode};

/// The Attempt's `metrics` (contract §2.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Metrics {
    pub turns: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost_usd: f64,
    pub duration_s: f64,
    pub model: Option<String>,
    pub thinking_level: Option<String>,
    pub stop_reason: Option<String>,
}

impl Metrics {
    /// The JSON stored in `attempts.metrics`.
    pub fn to_json(&self) -> Value {
        json!({
            "turns": self.turns,
            "tokens_in": self.tokens_in,
            "tokens_out": self.tokens_out,
            "cache_read": self.cache_read,
            "cache_write": self.cache_write,
            "cost_usd": self.cost_usd,
            "duration_s": self.duration_s,
            "model": self.model,
            "thinking_level": self.thinking_level,
            "stop_reason": self.stop_reason,
        })
    }
}

#[derive(Deserialize)]
struct Base {
    #[serde(rename = "type")]
    kind: String,
    timestamp: String,
}

#[derive(Deserialize)]
struct ModelChange {
    #[serde(rename = "modelId")]
    model_id: String,
}

#[derive(Deserialize)]
struct ThinkingChange {
    #[serde(rename = "thinkingLevel")]
    thinking_level: String,
}

#[derive(Deserialize)]
struct Assistant {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    usage: Usage,
    #[serde(rename = "stopReason")]
    stop_reason: String,
}

#[derive(Deserialize)]
struct Usage {
    input: u64,
    output: u64,
    #[serde(rename = "cacheRead")]
    cache_read: u64,
    #[serde(rename = "cacheWrite")]
    cache_write: u64,
    cost: Cost,
}

#[derive(Deserialize)]
struct Cost {
    total: f64,
}

/// The `errorMessage` of the LAST assistant message of the session file `path`; None when that message has none, when
/// there is no assistant message, or while the file cannot be read or a line cannot be parsed (a line being written).
pub fn last_error_message(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut last = None;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let value: Value = serde_json::from_str(line).ok()?;
        if value.get("type").and_then(Value::as_str) == Some("message")
            && value.pointer("/message/role").and_then(Value::as_str) == Some("assistant")
        {
            last = value.pointer("/message/errorMessage").and_then(Value::as_str).map(str::to_owned);
        }
    }
    last
}

/// Parses the session file at `path`. `file_unreadable` when it cannot be read as
/// UTF-8 text, `session_malformed` (with `path` and `line`) when a line is not a
/// session entry.
pub fn parse_file(path: &Path) -> Result<Metrics, CpdError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CpdError::file_unreadable(path, e)
    })?;
    parse(&text, path)
}

/// Parses the text of a session file; `path` only names it in errors.
pub fn parse(text: &str, path: &Path) -> Result<Metrics, CpdError> {
    let malformed = |line: usize, why: String| {
        CpdError::new(ErrorCode::SessionMalformed, format!("{}:{line}: {why}", path.display()))
            .with("path", path.display().to_string())
            .with("line", line)
    };
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    if lines.is_empty() {
        return Err(malformed(1, "empty file, missing session header".to_owned()));
    }

    let mut m = Metrics {
        turns: 0,
        tokens_in: 0,
        tokens_out: 0,
        cache_read: 0,
        cache_write: 0,
        cost_usd: 0.0,
        duration_s: 0.0,
        model: None,
        thinking_level: None,
        stop_reason: None,
    };
    // Milliseconds since the epoch, as JavaScript's Date.parse gives them.
    let (mut first, mut last, mut forked) = (0_i128, 0_i128, false);

    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        let value: Value = serde_json::from_str(line).map_err(|e| malformed(n, format!("invalid JSON: {e}")))?;
        let base = Base::deserialize(&value).map_err(|e| malformed(n, e.to_string()))?;
        let at = OffsetDateTime::parse(&base.timestamp, &Rfc3339)
            .map_err(|e| malformed(n, format!("invalid timestamp {:?}: {e}", base.timestamp)))?
            .unix_timestamp_nanos()
            .div_euclid(1_000_000);
        if i == 0 {
            if base.kind != "session" {
                return Err(malformed(n, format!("expected the session header, found type {:?}", base.kind)));
            }
            first = at;
            last = at;
            forked = value.get("parentSession").is_some_and(|p| p.is_string());
            continue;
        }
        let inherited = forked && at < first;
        if !inherited {
            last = at;
        }
        let bad = |e: serde_json::Error| malformed(n, e.to_string());
        match base.kind.as_str() {
            "model_change" => m.model = Some(ModelChange::deserialize(&value).map_err(bad)?.model_id),
            "thinking_level_change" => {
                m.thinking_level = Some(ThinkingChange::deserialize(&value).map_err(bad)?.thinking_level);
            }
            "message" if !inherited && value.pointer("/message/role").and_then(Value::as_str) == Some("assistant") => {
                let u = Assistant::deserialize(&value).map_err(bad)?.message;
                m.turns += 1;
                m.tokens_in += u.usage.input;
                m.tokens_out += u.usage.output;
                m.cache_read += u.usage.cache_read;
                m.cache_write += u.usage.cache_write;
                m.cost_usd += u.usage.cost.total;
                m.stop_reason = Some(u.stop_reason);
            }
            _ => {}
        }
    }

    m.duration_s = (last - first) as f64 / 1000.0;
    Ok(m)
}
