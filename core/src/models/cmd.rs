//! `cpd models refresh` and `cpd models show` (commands.md §14.5).

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::cache::{self, Outcome, Read};
use super::helper::Helper;
use super::safe_name;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;

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
    let env = Env::open()?;
    let answer = Helper::new(&env.config, &env.home)?.run()?;
    let applied = cache::apply(&env.home, &env.config.timeouts, &answer)?;
    let lists: Vec<Value> = applied
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
    let home = CpdHome::from_env()?;
    let now = OffsetDateTime::now_utc();
    let (mut lists, mut warnings) = (Vec::new(), Vec::new());
    for read in cache::read_all(&home)? {
        match read {
            Read::Good(file) if args.provider.as_ref().is_none_or(|p| *p == file.provider) => {
                let polled = OffsetDateTime::parse(&file.polled_at, &Rfc3339)
                    .expect("a good file's polled_at parses");
                lists.push(json!({
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
            Read::Unparseable { provider, path }
                if args.provider.as_ref().is_none_or(|p| *p == provider) =>
            {
                warnings.push(json!({"provider": provider, "file": path.display().to_string(), "code": "models_unparseable"}));
            }
            _ => {}
        }
    }
    Ok(Output::success(
        json!({"providers": lists, "warnings": warnings}),
    ))
}
