//! The command tree (commands.md §2). The root only delegates: each area
//! module defines its own subcommand type and its `run` functions, and adds
//! its commands' arguments there.

use std::ffi::OsString;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser, Subcommand};

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::{ask, attempt, config, context, doctor, events, inbox, job, plan, pr, session, watch};


/// cpd 2: every command prints exactly one JSON value on stdout.
#[derive(Parser, Debug)]
#[command(name = "cpd", version, color = clap::ColorChoice::Never)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Contexts and their records (commands.md §3)
    #[command(subcommand)]
    Context(context::Command),
    /// Plans and their phases (commands.md §4)
    #[command(subcommand)]
    Plan(plan::Command),
    /// Jobs, Specs and Dependencies (commands.md §5)
    #[command(subcommand)]
    Job(job::Command),
    /// Attempts (commands.md §6)
    #[command(subcommand)]
    Attempt(attempt::Command),
    /// Ask the Lead a question and wait for the answer (Worker) (commands.md §8.1)
    Ask(ask::AskArgs),
    /// Answer a Worker's Escalation (Lead) (commands.md §8.2)
    Answer(ask::AnswerArgs),
    /// Pull requests (commands.md §9)
    #[command(subcommand)]
    Pr(pr::Command),
    /// Sessions and their tmux panes (commands.md §10)
    #[command(subcommand)]
    Session(session::Command),
    /// Events (commands.md §11)
    #[command(subcommand)]
    Events(events::Command),
    /// What needs you (commands.md §21)
    #[command(subcommand)]
    Inbox(inbox::cmd::Command),
    /// The engine, one per machine (commands.md §12)
    Watch(watch::Args),
    /// Build the cockpit and start the engine (commands.md §13)
    Tui(cpd_tui::Args),
    /// Check the machine (commands.md §14.1)
    Doctor(doctor::Args),
    /// Print the effective configuration (commands.md §14.2)
    Config(config::ConfigArgs),
    /// Print the Tiers and their valid efforts (commands.md §14.3)
    Tiers(config::TiersArgs),
}

/// Parses `argv` and runs the command.
pub fn run(argv: impl IntoIterator<Item = OsString>) -> Result<Output, CpdError> {
    let argv: Vec<OsString> = argv.into_iter().collect();
    let root_help = matches!(argv.get(1..), Some([word]) if ["--help", "-h", "help"].iter().any(|h| word == h));
    match Cli::try_parse_from(&argv) {
        Ok(cli) => dispatch(cli.command),
        Err(err) if root_help && err.kind() == ErrorKind::DisplayHelp => {
            let text = err.render().to_string();
            Ok(Output::success(serde_json::json!({ "help": text, "tree": crate::interface::tree(&Cli::command()) })))
        }
        Err(err) => clap_result(err),
    }
}

fn dispatch(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Context(c) => context::run(c),
        Command::Plan(c) => plan::run(c),
        Command::Job(c) => job::run(c),
        Command::Attempt(c) => attempt::run(c),
        Command::Ask(a) => ask::ask(a),
        Command::Answer(a) => ask::answer(a),
        Command::Pr(c) => pr::run(c),
        Command::Session(c) => session::run(c),
        Command::Events(c) => events::run(c),
        Command::Inbox(c) => inbox::cmd::run(c),
        Command::Watch(a) => watch::run(a),
        Command::Tui(a) => cpd_tui::run(a, run_command),
        Command::Doctor(a) => doctor::run(a),
        Command::Config(a) => config::config(a),
        Command::Tiers(a) => config::tiers(a),
    }
}

/// The runner `cpd tui` hands the cockpit, which runs commands in this process.
fn run_command(argv: Vec<OsString>) -> Result<Output, CpdError> {
    run(argv)
}

/// What a clap parse failure prints. `--help` and `--version` are successes
/// whose text is a field, so even they print one JSON value (contract §1.2).
/// A value that failed one of our own parsers (an id, `Text`, `Name`) keeps
/// its code, for example `invalid_plan_id`; every other failure is `usage`.
pub fn clap_result(err: clap::Error) -> Result<Output, CpdError> {
    let text = err.render().to_string();
    match err.kind() {
        ErrorKind::DisplayHelp => Ok(Output::success(serde_json::json!({ "help": text }))),
        ErrorKind::DisplayVersion => {
            let store = store_verdict(&CpdHome::from_env()?)?;
            Ok(Output::success(serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "protocol": cpd_core::events::PROTOCOL,
                "store": store,
            })))
        }
        _ => Err(usage_error(&err, &text)),
    }
}

/// The `store` object of `--version` (commands.md §1.2): the store verdict of `home`'s database,
/// opened read-only (nothing is created, migrated or checked), or `None` when there is no database.
fn store_verdict(home: &CpdHome) -> Result<Option<serde_json::Value>, CpdError> {
    let path = home.db();
    let exists = path.try_exists().map_err(|e| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot read {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    })?;
    if !exists {
        return Ok(None);
    }
    let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let verdict = cpd_core::store::verdict(&conn)?;
    let refusal = verdict.refusal().map(|e| serde_json::json!({"code": e.code().as_str(), "message": e.message()}));
    Ok(Some(serde_json::json!({
        "opens": verdict.opens(),
        "refusal": refusal,
        "unknown": verdict.unknown,
        "pending": verdict.pending_names(),
    })))
}

fn usage_error(err: &clap::Error, text: &str) -> CpdError {
    let ours = std::error::Error::source(err).and_then(|source| source.downcast_ref::<CpdError>());
    if let Some(ours) = ours {
        return ours.clone();
    }
    let message = match err.kind() {
        // clap renders the help text itself here, not an error line.
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => "A command is required: see details.help.".to_owned(),
        _ => {
            let first = text.lines().next().unwrap_or_default();
            first.strip_prefix("error: ").unwrap_or(first).to_owned()
        }
    };
    CpdError::new(ErrorCode::Usage, message).with("help", text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpd_core::args::{Name, PlanArg, Text};
    use cpd_core::output::Output;

    fn argv(words: &[&str]) -> Vec<OsString> {
        std::iter::once("cpd").chain(words.iter().copied()).map(OsString::from).collect()
    }

    fn fails(words: &[&str]) -> CpdError {
        run(argv(words)).expect_err("expected a failure")
    }

    #[test]
    fn unknown_or_missing_commands_are_usage() {
        for words in [&[][..], &["nope"], &["plan"], &["plan", "nope"], &["--bogus"]] {
            let err = fails(words);
            assert_eq!(err.code(), ErrorCode::Usage, "{words:?}");
            assert!(!err.message().contains('\n'), "{:?}", err.message());
            assert!(err.details()["help"].is_string());
        }
    }

    #[test]
    fn usage_messages_say_what_is_wrong() {
        assert_eq!(fails(&[]).message(), "A command is required: see details.help.");
        assert_eq!(fails(&["plan"]).message(), "A command is required: see details.help.");
        assert!(fails(&["nope"]).message().contains("'nope'"));
    }

    #[test]
    fn help_and_version_are_one_json_value() {
        let Ok(Output::Success(fields)) = run(argv(&["--help"])) else { panic!("help is a success") };
        assert!(fields["help"].as_str().unwrap().contains("context"));
        assert!(fields["tree"].is_array());
        for words in [&["plan", "--help"][..], &["help", "plan"]] {
            let Ok(Output::Success(fields)) = run(argv(words)) else { panic!("help is a success") };
            assert!(fields.contains_key("help") && !fields.contains_key("tree"));
        }
        let Ok(Output::Success(fields)) = run(argv(&["--version"])) else { panic!("version is a success") };
        assert_eq!(fields["version"], env!("CARGO_PKG_VERSION"));
        let mut keys: Vec<_> = fields.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["protocol", "store", "version"]);
        assert!(fields.contains_key("store"));
        assert_eq!(fields["protocol"], 5);
    }

    /// A stand-in for an area's arguments, to test how our parsers' errors
    /// come out of clap.
    #[derive(Parser, Debug)]
    #[command(name = "t")]
    struct Probe {
        #[command(flatten)]
        plan: PlanArg,
        #[arg(long)]
        title: Option<Text>,
        #[arg(long)]
        name: Option<Name>,
    }

    fn probe(words: &[&str]) -> Result<Probe, CpdError> {
        let argv = std::iter::once("t").chain(words.iter().copied());
        Probe::try_parse_from(argv).map_err(|err| {
            let text = err.render().to_string();
            usage_error(&err, &text)
        })
    }

    #[test]
    fn our_parsers_keep_their_codes_through_clap() {
        assert_eq!(probe(&["--plan", "Bad"]).unwrap_err().code(), ErrorCode::InvalidPlanId);
        // --plan is always given: plan_falls_back_to_cpd_plan changes CPD_PLAN meanwhile.
        assert_eq!(probe(&["--plan", "cpd2", "--name", ""]).unwrap_err().code(), ErrorCode::InvalidName);
        assert_eq!(probe(&["--plan", "cpd2", "--title", ""]).unwrap_err().code(), ErrorCode::Usage);
        let ok = probe(&["--plan", "cpd2", "--title", "T", "--name", "N"]).unwrap();
        assert_eq!(ok.plan.require().unwrap().as_str(), "cpd2");
    }

    #[test]
    fn plan_falls_back_to_cpd_plan() {
        // SAFETY: this is the only test that touches CPD_PLAN.
        unsafe { std::env::set_var("CPD_PLAN", "from-env") };
        let from_env = probe(&[]).map(|p| p.plan);
        let flag_wins = probe(&["--plan", "from-flag"]).map(|p| p.plan);
        unsafe { std::env::set_var("CPD_PLAN", "Bad") };
        let bad_env = probe(&[]).map(|p| p.plan);
        unsafe { std::env::remove_var("CPD_PLAN") };
        let none = probe(&[]).map(|p| p.plan);

        assert_eq!(from_env.unwrap().require().unwrap().as_str(), "from-env");
        assert_eq!(flag_wins.unwrap().require().unwrap().as_str(), "from-flag");
        assert_eq!(bad_env.unwrap_err().code(), ErrorCode::InvalidPlanId);
        assert_eq!(none.unwrap().require().unwrap_err().code(), ErrorCode::PlanRequired);
    }

    #[test]
    fn clap_definition_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
