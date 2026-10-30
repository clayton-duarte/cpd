//! Escalations (contract §3.3, §3.5; commands.md §8): `ask` and `answer`.
//!
//! - `question`: `ask`, the Worker's blocking command;
//! - `reply`: `answer`, the Lead's.
//!
//! Expiring an Escalation whose asker died is the engine's (E1), not here.

pub(crate) mod question;
mod reply;

pub use question::resume_attempt;

use crate::args::{PlanArg, Text};
use crate::env::Env;
use crate::error::CpdError;
use crate::ids::{AttemptId, EscalationId};
use crate::output::Output;

#[derive(clap::Args, Debug)]
pub struct AskArgs {
    /// The Attempt asking
    #[arg(value_name = "attempt")]
    attempt: AttemptId,
    /// The question
    #[arg(value_name = "question")]
    question: Text,
    /// Minutes to wait for the answer; 0 expires it at the first poll
    /// [default: timeouts.ask_minutes]
    #[arg(long = "timeout-minutes", value_name = "n")]
    timeout_minutes: Option<u32>,
    #[command(flatten)]
    plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct AnswerArgs {
    /// The Escalation to answer
    #[arg(value_name = "escalation")]
    escalation: EscalationId,
    /// The answer
    #[arg(value_name = "answer")]
    answer: Text,
    #[command(flatten)]
    plan: PlanArg,
}

/// `cpd ask` (§8.1).
pub fn ask(args: AskArgs) -> Result<Output, CpdError> {
    question::ask(args)
}

/// `cpd answer` (§8.2).
pub fn answer(args: AnswerArgs) -> Result<Output, CpdError> {
    reply::answer(args)
}
