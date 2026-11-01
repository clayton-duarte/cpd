//! `worker_runner` (contract §8.2): which runner starts a Worker. The closed parse makes any other value unrepresentable.

use serde::Serialize;

use super::Config;
use super::migrate::top_level_line;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;

/// `pi` (pi in a tmux pane, the default) or `durable` (the durable runner, `pi/durable-worker/`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum WorkerRunner {
    Pi,
    Durable,
}

impl WorkerRunner {
    pub fn parse(text: &str) -> Result<WorkerRunner, String> {
        match text {
            "pi" => Ok(WorkerRunner::Pi),
            "durable" => Ok(WorkerRunner::Durable),
            other => Err(format!("{other:?} is not \"pi\" or \"durable\"")),
        }
    }

    /// The text stored in `attempts.runner` and shown by `cpd config`.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkerRunner::Pi => "pi",
            WorkerRunner::Durable => "durable",
        }
    }

    /// The other runner: `Pi` becomes `Durable` and back.
    pub fn flipped(self) -> WorkerRunner {
        match self {
            WorkerRunner::Pi => WorkerRunner::Durable,
            WorkerRunner::Durable => WorkerRunner::Pi,
        }
    }
}

/// The comment of the `worker_runner` line a click on the runner pill writes.
pub const TOGGLE_COMMENT: &str = "set by a click on the status line's runner pill (pro-d199); a started Attempt keeps its runner";

/// The text of a `config.toml` whose top-level `worker_runner` line is `worker_runner = "<to>" # <TOGGLE_COMMENT>`.
/// The old line is replaced whole; when absent, the line goes after the last top-level `key = value` line before the
/// first table (at the end when there is no table). Every other byte is unchanged.
pub fn set_in_text(text: &str, to: WorkerRunner) -> String {
    let line = format!("worker_runner = \"{}\" # {TOGGLE_COMMENT}", to.as_str());
    if let Some((start, body_end, _, _)) = top_level_line(text, "worker_runner") {
        return format!("{}{line}{}", &text[..start], &text[body_end..]);
    }
    // The end of the last top-level `key = value` line (after its newline), before the first table header.
    let mut at = 0;
    let mut start = 0;
    for l in text.split_inclusive('\n') {
        let trimmed = l.trim_start();
        if trimmed.starts_with('[') {
            break;
        }
        start += l.len();
        if !trimmed.starts_with('#') && l.contains('=') {
            at = start;
        }
    }
    let (head, tail) = text.split_at(at);
    let sep = if head.is_empty() || head.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("{head}{sep}{line}\n{tail}")
}

/// Flips `worker_runner` in `$CPD_HOME/config.toml` atomically and returns the new value.
pub fn toggle(home: &CpdHome) -> Result<WorkerRunner, CpdError> {
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
    let (config, _) = Config::parse(&text, &path)?;
    let to = config.worker_runner.flipped();
    let new = set_in_text(&text, to);
    Config::parse(&new, &path)?;
    crate::atomic::write_atomic(&path, &new).map_err(|e| {
        CpdError::new(
            ErrorCode::ConfigInvalid,
            format!("cannot write config.toml: {e}."),
        )
        .with("path", path.display().to_string())
    })?;
    Ok(to)
}
