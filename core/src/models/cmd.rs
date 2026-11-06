//! `cpd models refresh` and `cpd models show` (commands.md §14.5).

use serde_json::{Value, json};
use time::OffsetDateTime;

use super::Outcome;
use super::helper::Helper;
use super::safe_name;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::swr::source::models as rows;

/// The `models` commands (commands.md §14.5).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Read the model lists pi can offer now (§14.5)
    Refresh,
    /// Print the cached model lists (§14.5)
    Show(ShowArgs),
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// Only this provider's file
    #[arg(long, value_name = "provider")]
    provider: Option<String>,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Refresh => refresh(),
        Command::Show(args) => show(args),
    }
}

fn refresh() -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let answer = Helper::new(&env.config, &env.home)?.run()?;
    let now = env.ports.clock.utc();
    let outcomes = rows::apply_answer(&mut env.store, &answer, now)?;
    let lists: Vec<Value> = outcomes
        .iter()
        .map(|o| match o {
            Outcome::Written(w) => json!({"provider": w.provider, "polled_at": w.polled_at, "count": w.count, "changes": w.changes}),
            Outcome::Failed(f) => f.entry(),
        })
        .collect();
    // The good ones are written; the command then fails with the first failure's code.
    let first = outcomes.iter().find_map(|o| match o {
        Outcome::Failed(f) => Some(f),
        Outcome::Written(_) => None,
    });
    match first {
        Some(failure) => Err(failure.error().with("providers", lists)),
        None => Ok(Output::success(
            json!({"pi_version": answer.pi_version, "providers": lists}),
        )),
    }
}

fn show(args: ShowArgs) -> Result<Output, CpdError> {
    if let Some(provider) = args.provider.as_deref().filter(|p| !safe_name(p)) {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!(
                "--provider {provider:?} is not a file-name-safe name (letters, digits, `.`, `_`, `-`)."
            ),
        ));
    }
    let mut env = Env::open()?;
    let now = OffsetDateTime::now_utc();
    Ok(Output::success(rows::show(
        &mut env.store,
        args.provider.as_deref(),
        now,
    )?))
}
