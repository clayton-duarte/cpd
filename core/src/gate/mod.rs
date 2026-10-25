//! `cpd gate finish`: the Gate's leak check, run by `scripts/gate-run.sh` after its command (docs/implementing.md,
//! "The split: `scripts/gate-run.sh` and `cpd gate finish`"; hygiene.md, "The Gate leak check").
//!
//! The run's directory is `$TMPDIR` (the wrapper exported `TMPDIR=$RUN`); the tmux sockets are searched in
//! `/tmp/tmux-<uid>` and `$TMUX_TMPDIR/tmux-<uid>`; the run's processes are `Spawn::with_env("GATE_RUN", <id>)`.

use clap::{Args, Subcommand};

use crate::error::CpdError;
use crate::ports::Ports;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Check what the run left behind, stop and remove it, report it (the Gate's leak check)
    Finish(FinishArgs),
}

#[derive(Args, Debug)]
pub struct FinishArgs {
    /// The run id (`GATE_RUN`, 6 characters)
    #[arg(long)]
    pub run: String,
    /// The exit status of the run's command
    #[arg(long)]
    pub status: i32,
}

/// What `finish` decided: the exit status of the whole run and the text for stderr (empty for a clean run).
#[derive(Debug, PartialEq, Eq)]
pub struct Finished {
    pub status: i32,
    pub report: String,
}

/// The exit status of a run that left something behind (or called the real tmux) while its command succeeded.
pub const LEAK_STATUS: i32 = 64;

pub fn run(ports: &Ports, command: Command) -> Result<Finished, CpdError> {
    match command {
        Command::Finish(a) => Ok(finish(ports, &a.run, a.status)),
    }
}

/// The leak check of run `run_id` whose command exited with `status`.
pub fn finish(_ports: &Ports, _run_id: &str, _status: i32) -> Finished {
    todo!("gate::finish: list, report, kill and remove what carries the run id")
}

#[cfg(test)]
mod tests;
