//! `cpd models refresh` and `cpd models show` (commands.md §14.4).

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::cache::{self, Outcome, Read};
use super::helper::Helper;
use super::safe_name;
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;

/// The `models` commands (commands.md §14.4).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Read the model lists of the providers pi can use now (§14.4)
    Refresh,
    /// Print the cached model lists (§14.4)
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
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let answer = Helper::new(&config, &home)?.run()?;
    let applied = cache::apply(&home, config.timeouts.store_busy_ms, &answer)?;
    let providers: Vec<Value> = applied
        .outcomes
        .iter()
        .map(|o| match o {
            Outcome::Written(w) => json!({"provider": w.provider, "polled_at": w.polled_at, "count": w.count, "changes": w.changes}),
            Outcome::Failed(f) => f.entry(),
        })
        .collect();
    // The good ones are written; the command then fails with the first failure's code.
    let first = applied.outcomes.iter().find_map(|o| match o {
        Outcome::Failed(f) => Some(f),
        Outcome::Written(_) => None,
    });
    match first {
        Some(failure) => Err(failure.error().with("providers", providers)),
        None => Ok(Output::success(json!({"pi_version": answer.pi_version, "providers": providers}))),
    }
}

fn show(args: ShowArgs) -> Result<Output, CpdError> {
    if let Some(provider) = args.provider.as_deref().filter(|p| !safe_name(p)) {
        return Err(CpdError::new(ErrorCode::Usage, format!("--provider {provider:?} is not a file-name-safe name (letters, digits, `.`, `_`, `-`).")));
    }
    let home = CpdHome::from_env()?;
    let now = OffsetDateTime::now_utc();
    let (mut providers, mut warnings) = (Vec::new(), Vec::new());
    for read in cache::read_all(&home)? {
        match read {
            Read::Good(file) if args.provider.as_ref().is_none_or(|p| *p == file.provider) => {
                let polled = OffsetDateTime::parse(&file.polled_at, &Rfc3339).expect("a good file's polled_at parses");
                providers.push(json!({
                    "provider": file.provider,
                    "polled_at": file.polled_at,
                    "age_seconds": (now - polled).whole_seconds().max(0),
                    "pi_version": file.pi_version,
                    "auth": file.source.auth,
                    "count": file.ids.len(),
                    "ids": file.ids,
                    "changes": file.changes,
                }));
            }
            Read::Unparseable { provider, path } if args.provider.as_ref().is_none_or(|p| *p == provider) => {
                warnings.push(json!({"provider": provider, "file": path.display().to_string(), "code": "models_unparseable"}));
            }
            _ => {}
        }
    }
    Ok(Output::success(json!({"providers": providers, "warnings": warnings})))
}
