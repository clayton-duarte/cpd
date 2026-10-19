//! Contexts and their records (contract §2.2, §4.6; commands.md §3).
//!
//! The database tables `contexts` and `context_records` are the only store of
//! a Context's records; Contexts are machine-local (contract §4.6). The
//! clap types and `new` are in this file, each other command in a file of its
//! name, the writes in `contexts`.
//!
//! Other areas use [`Contexts`]: `plan new` writes its `plan` record with
//! [`Contexts::write`]. [`legacy`] reads the git stores of earlier versions,
//! for the store's migration 0007 only.

mod archive;
mod contexts;
mod list;
pub(crate) mod legacy;
mod manager;
mod pending;
mod record;
pub(crate) mod render;
mod rename;
mod show;
mod write;

use std::fmt;
use std::str::FromStr;

use clap::Args;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

pub use contexts::{ContextRow, Contexts, Written};
pub use record::{By, Draft, PlanRef, Record};
pub(crate) use show::plan_repos;

use crate::args::{PlanArg, Text, Title};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, RecordId};
use crate::output::Output;
use crate::store::Store;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Create a Context (§3.1)
    New(NewArgs),
    /// List the Contexts (§3.2)
    List(ListArgs),
    /// Show a Context and its records (§3.3)
    Show(ShowArgs),
    /// Record a decision (§3.4)
    Decide(DecideArgs),
    /// Record a note (§3.4)
    Note(NoteArgs),
    /// Record a question (§3.4)
    Ask(AskArgs),
    /// Record the answer to a question (§3.5)
    Answer(AnswerArgs),
    /// Change a Context's title (§3.7)
    Rename(RenameArgs),
    /// Archive the Context with its Plans and their Sessions (§3.8)
    Archive(ArchiveArgs),
    /// Unarchive the Context with its Plans and their Sessions (§3.9)
    Unarchive(UnarchiveArgs),
    /// Start or resume the Context's Manager (§3.10)
    Manager(manager::ManagerArgs),
    /// List the in-flight work of the Context's active Plans (§3.11)
    Pending(pending::PendingArgs),
}

/// A Jira Epic key: `^[A-Z][A-Z0-9]+-[0-9]+$`, else `usage` (commands.md §3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Jira(String);

impl Jira {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for Jira {
    type Err = CpdError;
    fn from_str(s: &str) -> Result<Jira, CpdError> {
        let valid = s.split_once('-').is_some_and(|(project, number)| {
            let mut chars = project.chars();
            chars.next().is_some_and(|c| c.is_ascii_uppercase())
                && project.len() >= 2
                && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && !number.is_empty()
                && number.chars().all(|c| c.is_ascii_digit())
        });
        if valid {
            Ok(Jira(s.to_owned()))
        } else {
            Err(CpdError::new(ErrorCode::Usage, format!("{s:?} is not a Jira key: ^[A-Z][A-Z0-9]+-[0-9]+$.")).with("value", s))
        }
    }
}

impl fmt::Display for Jira {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `--context <id>`, else the Context of the selected Plan (commands.md §1.3).
#[derive(Args, Clone, Debug, PartialEq)]
pub struct Select {
    /// The Context [default: the selected Plan's]
    #[arg(long, value_name = "id")]
    context: Option<ContextId>,
    #[command(flatten)]
    plan: PlanArg,
}

impl Select {
    /// The named Context, else the selected Plan's; `context_required` with
    /// neither, `plan_not_found` when the Plan does not exist.
    fn resolve(&self, store: &mut Store) -> Result<ContextId, CpdError> {
        if let Some(context) = &self.context {
            return Ok(context.clone());
        }
        let Some(plan) = &self.plan.plan else {
            return Err(CpdError::new(
                ErrorCode::ContextRequired,
                "No Context selected: pass --context <id>, or select a Plan with --plan or CPD_PLAN.",
            ));
        };
        let context = store.read(|conn| {
            Ok(conn
                .query_row("SELECT context FROM plans WHERE id = ?1", [plan.as_str()], |r| r.get::<_, String>(0))
                .optional()?)
        })?;
        let context = context.ok_or_else(|| {
            CpdError::plan_not_found(plan)
        })?;
        ContextId::parse(&context)
    }
}

#[derive(Args, Debug)]
pub struct NewArgs {
    /// The Context id
    id: ContextId,
    /// The title
    #[arg(long, value_name = "text")]
    title: Title,
    /// The Jira Epic key, stored only
    #[arg(long, value_name = "key")]
    jira: Option<Jira>,
}

#[derive(Args, Debug)]
pub struct ListArgs {}

#[derive(Args, Debug)]
pub struct ShowArgs {
    #[command(flatten)]
    select: Select,
    /// Only the records whose index position is greater
    #[arg(long, value_name = "seq")]
    since: Option<u64>,
    /// Print this one record in full
    #[arg(long, value_name = "id", conflicts_with = "since")]
    record: Option<RecordId>,
}

#[derive(Args, Debug)]
pub struct DecideArgs {
    /// The decision
    text: Text,
    /// The decision this one replaces
    #[arg(long, value_name = "record")]
    supersedes: Option<RecordId>,
    #[command(flatten)]
    select: Select,
}

#[derive(Args, Debug)]
pub struct NoteArgs {
    /// The note
    text: Text,
    #[command(flatten)]
    select: Select,
}

#[derive(Args, Debug)]
pub struct AskArgs {
    /// The question
    question: Text,
    #[command(flatten)]
    select: Select,
}

#[derive(Args, Debug)]
pub struct AnswerArgs {
    /// The record id of the question
    question: RecordId,
    /// The answer
    text: Text,
    #[command(flatten)]
    select: Select,
}

#[derive(Args, Debug)]
pub struct RenameArgs {
    /// The Context
    id: ContextId,
    /// The new title
    #[arg(long, value_name = "text")]
    title: Title,
}

#[derive(Args, Debug)]
pub struct ArchiveArgs {
    /// The Context
    id: ContextId,
}

#[derive(Args, Debug)]
pub struct UnarchiveArgs {
    /// The Context
    id: ContextId,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::New(args) => new(args),
        Command::List(_) => list::list(),
        Command::Show(args) => show::show(args),
        Command::Decide(args) => {
            let draft = Draft::Decision { supersedes: args.supersedes };
            write::write("context decide", &args.select, &args.text, draft)
        }
        Command::Note(args) => write::write("context note", &args.select, &args.text, Draft::Note),
        Command::Ask(args) => write::write("context ask", &args.select, &args.question, Draft::Question),
        Command::Answer(args) => {
            write::write("context answer", &args.select, &args.text, Draft::Answer { question: args.question })
        }
        Command::Rename(args) => rename::rename(args),
        Command::Archive(args) => archive::archive(args),
        Command::Unarchive(args) => archive::unarchive(args),
        Command::Manager(args) => manager::manager(args),
        Command::Pending(args) => pending::pending(args),
    }
}

/// Now, as contract §1.3 writes times: RFC 3339 UTC with milliseconds.
fn now() -> String {
    OffsetDateTime::now_utc()
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .expect("a UTC time always formats")
}


fn context_json(row: &ContextRow) -> Value {
    json!({"id": row.id, "title": row.title, "jira": row.jira, "created_at": row.created_at,
           "created_on": row.created_on})
}

/// `context new` (§3.1).
fn new(args: NewArgs) -> Result<Output, CpdError> {
    let Env { config, mut store, .. } = Env::open()?;
    let (row, already) = Contexts::new(&config, &mut store).create(&args.id, &Text::from(args.title.clone()), args.jira.as_ref())?;
    let mut out = json!({"context": context_json(&row)});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}
