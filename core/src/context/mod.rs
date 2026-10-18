//! Contexts and their records (contract §2.2, §4.6; commands.md §3).
//!
//! The database tables `contexts` and `context_records` are the only store of
//! a Context's records; Contexts are machine-local (contract §4.6). The
//! commands are in this file, the writes in `contexts`.
//!
//! Other areas use [`Contexts`]: `plan new` writes its `plan` record with
//! [`Contexts::write`]. [`legacy`] reads the git stores of earlier versions,
//! for the store's migration 0007 only.

mod archive;
mod contexts;
pub mod digest;
pub(crate) mod distill;
pub mod inforce;
pub(crate) mod legacy;
mod manager;
mod pending;
mod record;
pub(crate) mod render;
mod rename;

use std::fmt;
use std::str::FromStr;

use clap::Args;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::format_description;

pub use contexts::{ContextRow, Contexts, Written};
pub use record::{By, Draft, PlanRef, Record};

use crate::args::{PlanArg, Text, Title};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId, RecordKind, SessionId};
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
    /// Build the Context's digest with one-shot pi (§3.12)
    Distill(distill::DistillArgs),
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
            CpdError::new(ErrorCode::PlanNotFound, format!("No Plan {plan}.")).with("plan", plan.as_str())
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
    /// Print the Context's latest digest, and no record
    #[arg(long, conflicts_with_all = ["since", "record"])]
    digest: bool,
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
        Command::List(_) => list(),
        Command::Show(args) => show(args),
        Command::Decide(args) => {
            let draft = Draft::Decision { supersedes: args.supersedes };
            write("context decide", &args.select, &args.text, draft)
        }
        Command::Note(args) => write("context note", &args.select, &args.text, Draft::Note),
        Command::Ask(args) => write("context ask", &args.select, &args.question, Draft::Question),
        Command::Answer(args) => {
            write("context answer", &args.select, &args.text, Draft::Answer { question: args.question })
        }
        Command::Rename(args) => rename::rename(args),
        Command::Archive(args) => archive::archive(args),
        Command::Unarchive(args) => archive::unarchive(args),
        Command::Manager(args) => manager::manager(args),
        Command::Pending(args) => pending::pending(args),
        Command::Distill(args) => distill::distill(args),
    }
}

/// Now, as contract §1.3 writes times: RFC 3339 UTC with milliseconds.
fn now() -> String {
    OffsetDateTime::now_utc()
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .expect("a UTC time always formats")
}

fn open() -> Result<(CpdHome, Config, Store), CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    Ok((home, config, store))
}

/// `CPD_SESSION`, else `PI_SESSION_ID`, else none (commands.md §3.4).
fn session_from_env() -> Result<Option<SessionId>, CpdError> {
    ["CPD_SESSION", "PI_SESSION_ID"]
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
        .map(|v| SessionId::parse(&v.to_string_lossy()))
        .transpose()
}

fn context_json(row: &ContextRow) -> Value {
    json!({"id": row.id, "title": row.title, "jira": row.jira, "created_at": row.created_at,
           "created_on": row.created_on})
}

/// `context new` (§3.1).
fn new(args: NewArgs) -> Result<Output, CpdError> {
    let (_, config, mut store) = open()?;
    let (row, already) = Contexts::new(&config, &mut store).create(&args.id, &Text::from(args.title.clone()), args.jira.as_ref())?;
    let mut out = json!({"context": context_json(&row)});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// `context list` (§3.2).
fn list() -> Result<Output, CpdError> {
    let (_, _, mut store) = open()?;
    let (contexts, cursor) = store.read(|conn| {
        let mut contexts = Vec::new();
        let mut stmt = conn.prepare(&format!("SELECT {} FROM contexts ORDER BY id", ContextRow::COLUMNS))?;
        let rows = stmt.query_map([], ContextRow::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        for row in rows {
            let records: i64 =
                conn.query_row("SELECT count(*) FROM context_records WHERE context = ?1", [&row.id], |r| r.get(0))?;
            let open_questions: i64 = conn.query_row(
                "SELECT count(*) FROM context_records q WHERE q.context = ?1 AND q.kind = 'question'
                   AND NOT EXISTS (SELECT 1 FROM context_records a
                                   WHERE a.context = q.context AND a.kind = 'answer' AND a.question = q.id)",
                [&row.id],
                |r| r.get(0),
            )?;
            let mut stmt =
                conn.prepare("SELECT id, title, phase, lead_session, archived_at FROM plans WHERE context = ?1 ORDER BY created_at, id")?;
            let plans = stmt
                .query_map([&row.id], |r| {
                    Ok(json!({"id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?,
                              "phase": r.get::<_, String>(2)?, "lead_session": r.get::<_, String>(3)?,
                              "archived_at": r.get::<_, Option<String>>(4)?}))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            contexts.push(json!({
                "id": row.id, "title": row.title, "jira": row.jira, "archived_at": row.archived_at,
                "records": records, "open_questions": open_questions, "plans": plans,
            }));
        }
        let cursor: i64 = conn.query_row("SELECT coalesce(max(id), 0) FROM events", [], |r| r.get(0))?;
        Ok((contexts, cursor))
    })?;
    Ok(Output::success(json!({"contexts": contexts, "events_cursor": cursor})))
}

/// `context show` (§3.3).
fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let (home, config, mut store) = open()?;
    let id = args.select.resolve(&mut store)?;
    if args.digest {
        return show_digest(&home, &mut store, &id);
    }
    if let Some(record) = &args.record {
        return show_record(&mut store, &id, record);
    }
    let since = args.since.map_or(0, |s| i64::try_from(s).unwrap_or(i64::MAX));
    let (row, records, repos) = store.read(|conn| {
        let row = ContextRow::require(conn, &id)?;
        let mut stmt =
            conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
        let records = stmt.query_map([id.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let repos = plan_repos(conn, &config.machine, &records)?;
        Ok((row, records, repos))
    })?;
    let markdown = render::markdown(&row.title, row.jira.as_deref(), &records, since, &repos);
    let records: Vec<Value> = records
        .iter()
        .filter(|(seq, _)| *seq > since)
        .map(|(seq, record)| {
            let mut v = record.to_json_listed();
            v["seq"] = json!(seq);
            if let Some(repos) = repos.get(&record.id.to_string()) {
                v["repos"] = json!(repos);
            }
            v
        })
        .collect();
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "records": records,
        "markdown": markdown,
    })))
}

/// `context show --digest` (§3.3): the Context's latest digest file, and no record.
fn show_digest(home: &CpdHome, store: &mut Store, context: &ContextId) -> Result<Output, CpdError> {
    let row = store.read(|conn| ContextRow::require(conn, context))?;
    let digest = digest::latest(home, context)?;
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "digest": digest.as_ref().map(|d| json!({"upto_seq": d.upto_seq, "file": d.file, "bytes": d.text.len()})),
        "markdown": digest.map(|d| d.text),
    })))
}

/// `context show --record <id>` (§3.3): that one record in full.
fn show_record(store: &mut Store, context: &ContextId, id: &RecordId) -> Result<Output, CpdError> {
    let (row, found) = store.read(|conn| {
        let row = ContextRow::require(conn, context)?;
        let found = conn
            .query_row(
                &format!("SELECT {} FROM context_records WHERE context = ?1 AND id = ?2", Record::COLUMNS),
                [context.as_str(), id.to_string().as_str()],
                Record::from_row,
            )
            .optional()?;
        Ok((row, found))
    })?;
    let (seq, record) = found.ok_or_else(|| {
        CpdError::new(ErrorCode::RecordNotFound, format!("Context {context} has no record {id}."))
            .with("context", context.as_str())
            .with("record", id.to_string())
    })?;
    let markdown = render::single(&row.title, row.jira.as_deref(), &record);
    let mut v = record.to_json();
    v["seq"] = json!(seq);
    Ok(Output::success(json!({
        "context": {"id": row.id, "title": row.title, "jira": row.jira},
        "records": [v],
        "markdown": markdown,
    })))
}

/// The repos of the PRs that are not `cancelled` of each `plan` record's Plan
/// (commands.md §3.3), by record id: each repo once, in PR key order, from
/// this machine's database. A Plan of another machine has none.
pub(crate) fn plan_repos(
    conn: &rusqlite::Connection,
    machine: &crate::ids::MachineName,
    records: &[render::Indexed],
) -> Result<render::Repos, CpdError> {
    let mut stmt = conn.prepare("SELECT repo FROM prs WHERE plan = ?1 AND status <> 'cancelled' ORDER BY key")?;
    let mut out = render::Repos::new();
    for (_, record) in records {
        let Some(plan) = record.plan().filter(|_| record.kind() == RecordKind::Plan) else { continue };
        let mut repos: Vec<String> = Vec::new();
        if plan.machine == *machine {
            for repo in stmt.query_map([plan.id.as_str()], |r| r.get::<_, String>(0))? {
                let repo = repo?;
                if !repos.contains(&repo) {
                    repos.push(repo);
                }
            }
        }
        out.insert(record.id.to_string(), repos);
    }
    Ok(out)
}

/// `context decide`, `note`, `ask` and `answer` (§3.4, §3.5).
fn write(command: &str, select: &Select, text: &Text, draft: Draft) -> Result<Output, CpdError> {
    let (_, config, mut store) = open()?;
    let context = select.resolve(&mut store)?;
    let by = By { session: session_from_env()?, plan: select.plan.plan.clone() };
    let written = Contexts::new(&config, &mut store).write(&context, command, by, text, draft)?;
    let mut out = json!({"record": written.record.to_json(), "seq": written.seq});
    if written.already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}
