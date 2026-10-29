//! `worker_runner` (contract §8.2): which runner starts a Worker. The closed parse makes any other value unrepresentable.

use serde::Serialize;

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
}
