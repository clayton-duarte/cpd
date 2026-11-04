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

#[cfg(test)]
mod distill_runner_tests;

#[cfg(test)]
mod arch_citations_tests;
mod archive;
mod contexts;
pub mod curate;
pub mod curate_backoff;
#[cfg(test)]
mod curate_backoff_tests;
pub mod curate_judge;
#[cfg(test)]
mod curate_judge_tests;
pub mod curate_neighbours;
#[cfg(test)]
mod curate_neighbours_tests;
pub mod curate_run;
#[cfg(test)]
mod curate_run_tests;
pub mod curate_vectors;
#[cfg(test)]
mod curate_vectors_hash_tests;
#[cfg(test)]
mod curate_vectors_tests;
pub mod curate_verdict;
#[cfg(test)]
mod curate_verdict_tests;
pub mod curated;
pub mod curation;
mod delete;
pub mod distill;
#[cfg(test)]
mod distill_git_tests;
mod distill_known;
#[cfg(test)]
mod distill_known_tests;
#[cfg(test)]
mod doc_flag_gone_tests;
mod embed;
#[cfg(test)]
mod embed_tests;
mod fts;
#[cfg(test)]
mod fts_tests;
mod hybrid;
#[cfg(test)]
mod hybrid_tests;
pub mod inforce;
pub(crate) mod legacy;
mod list;
pub(crate) mod manager;
mod owner;
mod pending;
pub mod permanent;
#[cfg(test)]
mod permanent_tests;
pub mod prose_supersedes;
#[cfg(test)]
mod prose_supersedes_tests;
mod query;
#[cfg(test)]
mod query_tests;
mod reads;
#[cfg(test)]
mod reads_best_effort_tests;
#[cfg(test)]
mod reads_tests;
mod record;
mod rename;
pub(crate) mod render;
mod search;
mod show;
#[cfg(test)]
mod supersedes_tests;
mod topics;
mod write;

use std::fmt;
use std::str::FromStr;

use clap::Args;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};

pub use contexts::{ContextRow, Contexts, Written};
pub use record::{By, Draft, PlanRef, Record};

use crate::args::{PlanArg, Text, Title};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, OwnerLogin, RecordId};
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
    /// List the curated topics in force with their record counts (§3.3a)
    Topics(TopicsArgs),
    /// Search the Context's records, ranked by in-process full-text search (§3.4)
    Search(search::SearchArgs),
    /// Record a decision (§3.4)
    Decide(DecideArgs),
    /// Record a note (§3.4)
    Note(NoteArgs),
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
    /// Build the Context's digest with one-shot pi (§3.12)
    Distill(distill::DistillArgs),
    /// Curate the next record with its neighbours (§3.13a)
    Curate(curate_run::CurateArgs),
    /// Embed the Context's in-force records into the vector cache, skipping unchanged ones
    Embed(embed::EmbedArgs),
    /// Set or change the owner of a Context (§3.13)
    Owner(owner::OwnerArgs),
    /// Delete an archived or asleep Context: its Plans, records, Manager, files, worktrees and local branches (§3.14)
    Delete(DeleteArgs),
    /// List the pending retirements (§3.15)
    #[command(hide = true)]
    Curation(curation::CurationArgs),
    /// Decide the pending retirements (§3.16)
    #[command(hide = true)]
    Retire(curation::RetireArgs),
    /// List the Context records Sessions have read or cited (§3.18)
    Reads(reads::ReadsArgs),
}

/// A Jira key (a Context's Epic, a Workflow's ticket): `^[A-Z][A-Z0-9]+-[0-9]+$`, else `usage` (commands.md §3.1).
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
            Err(CpdError::new(
                ErrorCode::Usage,
                format!("{s:?} is not a Jira key: ^[A-Z][A-Z0-9]+-[0-9]+$."),
            )
            .with("value", s))
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
                .query_row(
                    "SELECT context FROM plans WHERE id = ?1",
                    [plan.as_str()],
                    |r| r.get::<_, String>(0),
                )
                .optional()?)
        })?;
        let context = context.ok_or_else(|| CpdError::plan_not_found(plan))?;
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
    /// The GitHub owner of every repo the Context's Plans work on, its policy key
    #[arg(long, value_name = "login")]
    owner: OwnerLogin,
    /// Link an unknown owner to clayton-duarte's policy (the User confirmed)
    #[arg(long)]
    confirm_fallback: bool,
}

#[derive(Args, Debug)]
pub struct ListArgs {}

#[derive(Args, Debug)]
pub struct TopicsArgs {
    #[command(flatten)]
    select: Select,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    #[command(flatten)]
    select: Select,
    /// Every record, the superseded, answered and done ones too (bare `show` lists only what is active)
    #[arg(long, conflicts_with_all = ["since", "record", "topic"])]
    all: bool,
    /// Only the records whose index position is greater
    #[arg(long, value_name = "seq")]
    since: Option<u64>,
    /// Print this one record in full
    #[arg(long, value_name = "id", conflicts_with = "since")]
    record: Option<RecordId>,
    /// Print every curated record in force of this topic, oldest first
    #[arg(long, value_name = "topic", conflicts_with_all = ["since", "record"])]
    topic: Option<String>,
    /// Log the shown records as pushed reads (the pi turn start)
    #[arg(long, requires = "since")]
    pushed: bool,
    /// A record id, as --record
    #[arg(value_name = "record", conflicts_with_all = ["record", "since", "topic"])]
    record_positional: Option<RecordId>,
}

#[derive(Args, Debug)]
pub struct DecideArgs {
    /// The decision
    text: Text,
    /// The decision or note this one replaces
    #[arg(long, value_name = "record", action = clap::ArgAction::Append, value_delimiter = ',')]
    supersedes: Vec<RecordId>,
    /// Confirm the text is not a supersession: skip the prose check
    #[arg(long)]
    no_supersedes: bool,
    #[command(flatten)]
    select: Select,
}

#[derive(Args, Debug)]
pub struct NoteArgs {
    /// The note
    text: Text,
    /// The note this one replaces
    #[arg(long, value_name = "record", action = clap::ArgAction::Append, value_delimiter = ',')]
    supersedes: Vec<RecordId>,
    /// Confirm the text is not a supersession: skip the prose check
    #[arg(long)]
    no_supersedes: bool,
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

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// The Context
    id: ContextId,
    /// Delete: without it the command deletes nothing and prints what would go
    #[arg(long)]
    yes: bool,
}

/// `--no-supersedes` with `--supersedes` is `args_invalid`.
fn no_supersedes_alone(supersedes: &[RecordId], no_supersedes: bool) -> Result<(), CpdError> {
    if no_supersedes && !supersedes.is_empty() {
        return Err(CpdError::new(
            ErrorCode::ArgsInvalid,
            "--no-supersedes conflicts with --supersedes.",
        ));
    }
    Ok(())
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::New(args) => new(args),
        Command::List(_) => list::list(),
        Command::Show(args) => show::show(args),
        Command::Topics(args) => topics::topics(args),
        Command::Search(args) => search::search(args),
        Command::Decide(args) => {
            no_supersedes_alone(&args.supersedes, args.no_supersedes)?;
            let draft = Draft::Decision {
                supersedes: args.supersedes,
            };
            write::write(
                "context decide",
                &args.select,
                &args.text,
                draft,
                args.no_supersedes,
            )
        }
        Command::Note(args) => {
            no_supersedes_alone(&args.supersedes, args.no_supersedes)?;
            let draft = if args.supersedes.is_empty() {
                Draft::Note
            } else {
                Draft::NoteSupersedes {
                    supersedes: args.supersedes,
                }
            };
            write::write(
                "context note",
                &args.select,
                &args.text,
                draft,
                args.no_supersedes,
            )
        }
        Command::Rename(args) => rename::rename(args),
        Command::Archive(args) => archive::archive(args),
        Command::Unarchive(args) => archive::unarchive(args),
        Command::Manager(args) => manager::manager(args),
        Command::Pending(args) => pending::pending(args),
        Command::Distill(args) => distill::distill(args),
        Command::Curate(args) => curate_run::curate(args),
        Command::Embed(args) => embed::embed(args),
        Command::Owner(args) => owner::owner(args),
        Command::Delete(args) => delete::delete(args),
        Command::Curation(args) => curation::curation(args),
        Command::Retire(args) => curation::retire(args),
        Command::Reads(args) => reads::reads(args),
    }
}

fn context_json(row: &ContextRow) -> Value {
    json!({"id": row.id, "title": row.title, "jira": row.jira, "owner": row.owner, "created_at": row.created_at,
           "created_on": row.created_on})
}

/// `context new` (§3.1).
fn new(args: NewArgs) -> Result<Output, CpdError> {
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let title = Text::from(args.title.clone());
    let (row, already) = Contexts::new(&config, &mut store).create(
        &home,
        &args.id,
        &title,
        args.jira.as_ref(),
        &args.owner,
        args.confirm_fallback,
    )?;
    let mut out = json!({"context": context_json(&row)});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}
