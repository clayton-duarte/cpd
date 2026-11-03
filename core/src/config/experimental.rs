//! The `[experimental]` table of `config.toml` (contract §8.1, §8.2).

use serde::Serialize;

use super::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

/// The four experimental features' switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Experimental {
    pub durable_workers: bool,
    pub write_curation: bool,
    pub failure_triage: bool,
    pub turn_question_guard: bool,
}

/// One experimental feature, named by its `[experimental]` key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    DurableWorkers,
    WriteCuration,
    FailureTriage,
    TurnQuestionGuard,
}

impl Default for Experimental {
    /// The defaults (Plan experimental-config, A3): every feature on but `durable_workers`.
    fn default() -> Self {
        Experimental {
            durable_workers: false,
            write_curation: true,
            failure_triage: true,
            turn_question_guard: true,
        }
    }
}

impl Feature {
    /// The key of the feature in `[experimental]`.
    pub fn key(self) -> &'static str {
        match self {
            Feature::DurableWorkers => "durable_workers",
            Feature::WriteCuration => "write_curation",
            Feature::FailureTriage => "failure_triage",
            Feature::TurnQuestionGuard => "turn_question_guard",
        }
    }
}

/// `text` with `key = on` in `[experimental]`: the value of an existing line is replaced (its comment stays), else the
/// line goes right after the table header, else the table is appended. Every other byte is unchanged.
pub(super) fn set_in_text(text: &str, key: &str, on: bool) -> String {
    let value = if on { "true" } else { "false" };
    let mut in_table = false;
    let mut header_end = None;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim();
        let end = start + line.len();
        if body.starts_with('[') {
            in_table = body.split('#').next().unwrap_or("").trim() == "[experimental]";
            if in_table && header_end.is_none() {
                header_end = Some(end);
            }
        } else if in_table
            && let Some((name, _)) = body.split_once('=')
            && name.trim().trim_matches(['"', '\'']) == key
        {
            let eq = start + line.find('=').expect("the line has `=`");
            let after = &text[eq + 1..start + line.trim_end().len()];
            let comment = after.find('#').map_or("", |at| &after[at..]);
            let sep = if comment.is_empty() { "" } else { " " };
            return format!(
                "{}= {value}{sep}{comment}{}",
                &text[..eq],
                &text[start + line.trim_end().len()..]
            );
        }
        start = end;
    }
    match header_end {
        Some(at) => {
            let (head, tail) = text.split_at(at);
            let nl = if head.ends_with('\n') { "" } else { "\n" };
            format!("{head}{nl}{key} = {value}\n{tail}")
        }
        None => {
            let nl = if text.is_empty() || text.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            format!("{text}{nl}\n[experimental]\n{key} = {value}\n")
        }
    }
}

/// Sets one key of `[experimental]` in `$CPD_HOME/config.toml` atomically; comments and other keys stay. A config that
/// does not parse, before or after, is refused and left as it was.
pub fn set(home: &CpdHome, feature: Feature, on: bool) -> Result<(), CpdError> {
    let path = home.config();
    let text = std::fs::read_to_string(&path).map_err(|e| {
        let code = if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::ConfigMissing
        } else {
            ErrorCode::ConfigInvalid
        };
        CpdError::new(code, format!("cannot read config.toml: {e}."))
            .with("path", path.display().to_string())
    })?;
    Config::parse(&text, &path)?;
    let new = set_in_text(&text, feature.key(), on);
    Config::parse(&new, &path)?;
    crate::atomic::write_atomic(&path, &new).map_err(|e| {
        CpdError::new(
            ErrorCode::ConfigInvalid,
            format!("cannot write config.toml: {e}."),
        )
        .with("path", path.display().to_string())
    })
}
