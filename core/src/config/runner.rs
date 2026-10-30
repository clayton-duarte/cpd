//! `worker_runner` (contract §8.2): which runner starts a Worker. The closed parse makes any other value unrepresentable.

use serde::Serialize;

use crate::error::CpdError;
use crate::home::CpdHome;

/// `pi` (pi in a tmux pane, the default) or `durable` (the durable runner, `pi/durable-worker/`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
        todo!("not implemented")
    }
}

/// The comment of the `worker_runner` line a click on the runner pill writes.
pub const TOGGLE_COMMENT: &str = "set by a click on the status line's runner pill (pro-d199); a started Attempt keeps its runner";

/// The text of a `config.toml` whose top-level `worker_runner` line is `worker_runner = "<to>" # <TOGGLE_COMMENT>`.
pub fn set_in_text(text: &str, to: WorkerRunner) -> String {
    let _ = (text, to);
    todo!("not implemented")
}

/// Flips `worker_runner` in `$CPD_HOME/config.toml` atomically and returns the new value.
pub fn toggle(home: &CpdHome) -> Result<WorkerRunner, CpdError> {
    let _ = home;
    todo!("not implemented")
}
