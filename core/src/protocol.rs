//! `cpd protocol bug report | mitigate | close | list | show` (commands.md §33, contract §2.2 "Bug record";
//! air-d103, air-d104, air-d107): the bug report protocol as records, not prose.
//!
//! One row per bug in `bug_records`; no Event is appended and the protocol number does not change. A mitigation
//! (air-d103) is an agent-applied stopgap, recorded with who applied it and when; until then the bug reads
//! **mitigation applied: no** (`mitigation_at` null). A close is refused `root_fix_required` until a root-fix Work
//! and a regression test are recorded (air-d107).

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::args::{Text, Title};
use crate::clock;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{PlanId, SessionId};
use crate::output::Output;
use crate::store::Store;

/// The `protocol` commands (commands.md §33).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// The bug record: report, mitigate, close, list, show (commands.md §33)
    #[command(subcommand)]
    Bug(BugCommand),
}

/// The `protocol bug` commands (commands.md §33.1–§33.5).
#[derive(clap::Subcommand, Debug)]
pub enum BugCommand {
    /// Record a bug and open its protocol state (commands.md §33.1)
    Report(ReportArgs),
    /// Record the mitigation applied to an open bug (commands.md §33.2)
    Mitigate(MitigateArgs),
    /// Close a bug whose root fix and regression test are in (commands.md §33.3)
    Close(CloseArgs),
    /// List bug records, id ascending (commands.md §33.4)
    List(ListArgs),
    /// Show one bug record (commands.md §33.5)
    Show(ShowArgs),
}

#[derive(clap::Args, Debug)]
pub struct ReportArgs {
    /// The one-line name of the bug
    #[arg(long, value_name = "text")]
    pub title: Title,
    /// What was seen: the failure, its surface and when it showed
    #[arg(long, value_name = "text")]
    pub symptom: Text,
    /// The diagnosed cause, when it is known at report time
    #[arg(long, value_name = "text")]
    pub cause: Option<Text>,
    /// The Plan the bug belongs to [default: a cross-Plan bug]
    #[arg(long, value_name = "id")]
    pub plan: Option<PlanId>,
}

#[derive(clap::Args, Debug)]
pub struct MitigateArgs {
    /// The bug id, `b<k>`
    #[arg(value_name = "id")]
    pub id: String,
    /// The stopgap that removes or contains the User's symptom now
    #[arg(long, value_name = "text")]
    pub mitigation: Text,
    /// The diagnosed cause, when it is first known here
    #[arg(long, value_name = "text")]
    pub cause: Option<Text>,
}

#[derive(clap::Args, Debug)]
pub struct CloseArgs {
    /// The bug id, `b<k>`
    #[arg(value_name = "id")]
    pub id: String,
    /// The root-fix Work: a Plan id, `<plan>/<key>` or `<plan>/<workflow>/<key>`
    #[arg(long, value_name = "ref")]
    pub root_fix: Option<String>,
    /// The test that pins the fix, as the repo names its tests
    #[arg(long, value_name = "text")]
    pub regression_test: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// The bugs of one Plan
    #[arg(long, value_name = "id")]
    pub plan: Option<PlanId>,
    /// The bugs with this status: `open` or `closed`
    #[arg(long, value_name = "open|closed")]
    pub status: Option<String>,
    /// The bugs with a mitigation (`yes`) or with mitigation applied: no (`no`)
    #[arg(long, value_name = "yes|no")]
    pub mitigated: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    /// The bug id, `b<k>`
    #[arg(value_name = "id")]
    pub id: String,
}

/// Every column of a `bug_records` row, in the order [`raw`] reads them.
const COLUMNS: &str = "id, plan, title, symptom, cause, mitigation, mitigated_by, mitigated_at, root_fix, \
                       regression_test, status, reported_by, reported_at, closed_at, closed_by";

/// Runs one `protocol` command.
pub fn run(command: Command) -> Result<Output, CpdError> {
    let Command::Bug(command) = command;
    let mut env = Env::open()?;
    match command {
        BugCommand::Report(args) => report(&mut env.store, args),
        BugCommand::Mitigate(args) => mitigate(&mut env.store, args),
        BugCommand::Close(args) => close(&mut env.store, args),
        BugCommand::List(args) => list(&mut env.store, args),
        BugCommand::Show(args) => show(&mut env.store, args),
    }
}

/// One row of `bug_records` as the `bug` object of commands.md §33.1.
#[derive(Debug, Clone, PartialEq)]
struct Bug {
    id: String,
    plan: Option<String>,
    title: String,
    symptom: String,
    cause: Option<String>,
    mitigation: Option<String>,
    mitigated_by: Option<String>,
    mitigated_at: Option<String>,
    root_fix: Option<String>,
    regression_test: Option<String>,
    status: String,
    reported_by: Option<String>,
    reported_at: String,
    closed_at: Option<String>,
    closed_by: Option<String>,
}

impl Bug {
    /// The `bug` object: every field of contract §2.2 "Bug record", so **mitigation applied: no** is visible.
    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "plan": self.plan,
            "title": self.title,
            "symptom": self.symptom,
            "cause": self.cause,
            "mitigation": self.mitigation,
            "mitigated_by": self.mitigated_by,
            "mitigated_at": self.mitigated_at,
            "root_fix": self.root_fix,
            "regression_test": self.regression_test,
            "status": self.status,
            "reported_by": self.reported_by,
            "reported_at": self.reported_at,
            "closed_at": self.closed_at,
            "closed_by": self.closed_by,
        })
    }
}

fn raw(r: &rusqlite::Row<'_>) -> rusqlite::Result<Bug> {
    Ok(Bug {
        id: r.get(0)?,
        plan: r.get(1)?,
        title: r.get(2)?,
        symptom: r.get(3)?,
        cause: r.get(4)?,
        mitigation: r.get(5)?,
        mitigated_by: r.get(6)?,
        mitigated_at: r.get(7)?,
        root_fix: r.get(8)?,
        regression_test: r.get(9)?,
        status: r.get(10)?,
        reported_by: r.get(11)?,
        reported_at: r.get(12)?,
        closed_at: r.get(13)?,
        closed_by: r.get(14)?,
    })
}

/// The bug `id`, if the table holds it.
fn fetch(conn: &Connection, id: &str) -> Result<Option<Bug>, CpdError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM bug_records WHERE id = ?1"),
            [id],
            raw,
        )
        .optional()?)
}

/// `bug_not_found` (exit 4): no `bug_records` row `id`.
fn bug_not_found(id: &str) -> CpdError {
    CpdError::new(ErrorCode::BugNotFound, format!("No bug {id}.")).with("id", id)
}

/// The next bug id, `b<k>`: the highest `<k>` in the table plus one, starting at 1 (never reused).
fn next_id(conn: &Connection) -> Result<String, CpdError> {
    let highest: i64 = conn.query_row(
        "SELECT coalesce(max(CAST(substr(id, 2) AS INTEGER)), 0) FROM bug_records WHERE id GLOB 'b[0-9]*'",
        [],
        |r| r.get(0),
    )?;
    Ok(format!("b{}", highest + 1))
}

/// The caller's Session id (`CPD_SESSION`), or null outside a Session.
fn caller() -> Result<Option<String>, CpdError> {
    match std::env::var_os("CPD_SESSION").filter(|v| !v.is_empty()) {
        Some(value) => Ok(Some(
            SessionId::parse(&value.to_string_lossy())?.as_str().to_owned(),
        )),
        None => Ok(None),
    }
}

/// A Worker does not record a bug: it reports it to its Lead (commands.md §33; `job addendum` refuses the same way).
fn require_lead() -> Result<(), CpdError> {
    if std::env::var("CPD_ATTEMPT").is_ok_and(|v| !v.is_empty()) {
        return Err(CpdError::new(
            ErrorCode::NotALead,
            "Only the Lead or the Manager records a bug: a Worker reports it to its Lead with cpd ask.",
        ));
    }
    Ok(())
}

/// `cpd protocol bug report` (commands.md §33.1): one transaction, one insert, no Event.
fn report(store: &mut Store, args: ReportArgs) -> Result<Output, CpdError> {
    require_lead()?;
    let caller = caller()?;
    store.write("protocol bug report", |tx| {
        let plan = match &args.plan {
            Some(plan) => {
                if crate::plan::row::find(tx, plan)?.is_none() {
                    return Err(CpdError::plan_not_found(plan));
                }
                Some(plan.as_str().to_owned())
            }
            None => None,
        };
        let id = next_id(tx)?;
        tx.execute(
            "INSERT INTO bug_records
               (id, plan, title, symptom, cause, mitigation, mitigated_by, mitigated_at,
                root_fix, regression_test, status, reported_by, reported_at, closed_at, closed_by)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, NULL, NULL, 'open', ?6, ?7, NULL, NULL)",
            params![
                id,
                plan,
                args.title.as_str(),
                args.symptom.as_str(),
                args.cause.as_ref().map(Text::as_str),
                caller.as_deref(),
                clock::now(),
            ],
        )?;
        let bug = fetch(tx, &id)?.expect("the row just inserted");
        Ok(Output::success(json!({"bug": bug.json()})))
    })
}

/// `cpd protocol bug mitigate` (commands.md §33.2): one transaction, the stopgap, who and when; the bug stays open.
fn mitigate(store: &mut Store, args: MitigateArgs) -> Result<Output, CpdError> {
    require_lead()?;
    let caller = caller()?;
    store.write("protocol bug mitigate", |tx| {
        let bug = fetch(tx, &args.id)?.ok_or_else(|| bug_not_found(&args.id))?;
        if bug.status == "closed" {
            return Err(CpdError::new(
                ErrorCode::IllegalTransition,
                format!("Bug {} is closed: a closed bug takes no mitigation.", args.id),
            )
            .with("id", args.id.as_str())
            .with("status", "closed"));
        }
        // `--cause` when given, else a recorded cause stands.
        let cause = args
            .cause
            .as_ref()
            .map(|cause| cause.as_str().to_owned())
            .or_else(|| bug.cause.clone());
        let mitigation = args.mitigation.as_str();
        if bug.mitigation.as_deref() == Some(mitigation) && bug.cause == cause {
            return Ok(Output::success(json!({"already": true, "bug": bug.json()})));
        }
        tx.execute(
            "UPDATE bug_records SET mitigation = ?1, mitigated_by = ?2, mitigated_at = ?3, cause = ?4 WHERE id = ?5",
            params![
                mitigation,
                caller.as_deref(),
                clock::now(),
                cause.as_deref(),
                args.id,
            ],
        )?;
        let bug = fetch(tx, &args.id)?.expect("the row just updated");
        Ok(Output::success(json!({"bug": bug.json()})))
    })
}

/// `cpd protocol bug close` (commands.md §33.3, air-d107): one transaction; refused `root_fix_required` until a
/// root-fix link and a regression test are recorded.
fn close(store: &mut Store, args: CloseArgs) -> Result<Output, CpdError> {
    require_lead()?;
    let caller = caller()?;
    store.write("protocol bug close", |tx| {
        let bug = fetch(tx, &args.id)?.ok_or_else(|| bug_not_found(&args.id))?;
        if bug.status == "closed" {
            return Ok(Output::success(json!({"already": true, "bug": bug.json()})));
        }
        // Malformed text is `usage` (over 200 characters or a NUL); a missing or empty flag is the refusal below.
        for value in [args.root_fix.as_deref(), args.regression_test.as_deref()]
            .into_iter()
            .flatten()
        {
            if value.chars().count() > 200 || value.contains('\0') {
                return Err(CpdError::new(
                    ErrorCode::Usage,
                    "A root fix and a regression test are 1 to 200 characters with no NUL.",
                )
                .with("chars", value.chars().count()));
            }
        }
        let missing: Vec<&str> = [
            ("root-fix", args.root_fix.as_deref()),
            ("regression-test", args.regression_test.as_deref()),
        ]
        .into_iter()
        .filter(|(_, value)| value.unwrap_or_default().is_empty())
        .map(|(name, _)| name)
        .collect();
        if !missing.is_empty() {
            return Err(CpdError::new(
                ErrorCode::RootFixRequired,
                format!(
                    "Bug {} cannot close without {}: record the root-fix Work and the regression test.",
                    args.id,
                    missing.join(" and ")
                ),
            )
            .with("id", args.id.as_str())
            .with("missing", missing));
        }
        tx.execute(
            "UPDATE bug_records
               SET root_fix = ?1, regression_test = ?2, status = 'closed', closed_at = ?3, closed_by = ?4
             WHERE id = ?5",
            params![
                args.root_fix,
                args.regression_test,
                clock::now(),
                caller.as_deref(),
                args.id,
            ],
        )?;
        let bug = fetch(tx, &args.id)?.expect("the row just closed");
        Ok(Output::success(json!({"bug": bug.json()})))
    })
}

/// `cpd protocol bug list` (commands.md §33.4): every bug, id ascending, with its mitigation state; the filters
/// combine. Read-only.
fn list(store: &mut Store, args: ListArgs) -> Result<Output, CpdError> {
    if let Some(status) = &args.status {
        if status != "open" && status != "closed" {
            return Err(CpdError::new(
                ErrorCode::Usage,
                "A --status is `open` or `closed`.",
            )
            .with("status", status.as_str()));
        }
    }
    if let Some(mitigated) = &args.mitigated {
        if mitigated != "yes" && mitigated != "no" {
            return Err(CpdError::new(ErrorCode::Usage, "A --mitigated is `yes` or `no`.")
                .with("mitigated", mitigated.as_str()));
        }
    }
    let bugs = store.read(|conn| {
        if let Some(plan) = &args.plan {
            if crate::plan::row::find(conn, plan)?.is_none() {
                return Err(CpdError::plan_not_found(plan));
            }
        }
        let mut sql = format!("SELECT {COLUMNS} FROM bug_records");
        let mut conditions: Vec<&str> = Vec::new();
        let mut binds: Vec<&str> = Vec::new();
        if let Some(plan) = &args.plan {
            conditions.push("plan = ?");
            binds.push(plan.as_str());
        }
        if let Some(status) = &args.status {
            conditions.push("status = ?");
            binds.push(status);
        }
        match args.mitigated.as_deref() {
            Some("yes") => conditions.push("mitigation IS NOT NULL"),
            Some("no") => conditions.push("mitigation IS NULL"),
            _ => {}
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY CAST(substr(id, 2) AS INTEGER)");
        let mut stmt = conn.prepare(&sql)?;
        let bugs = stmt
            .query_map(rusqlite::params_from_iter(binds.iter().copied()), raw)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(bugs)
    })?;
    Ok(Output::success(
        json!({"bugs": bugs.iter().map(Bug::json).collect::<Vec<_>>()}),
    ))
}

/// `cpd protocol bug show` (commands.md §33.5): one bug record, every field. Read-only.
fn show(store: &mut Store, args: ShowArgs) -> Result<Output, CpdError> {
    let bug = store.read(|conn| fetch(conn, &args.id))?;
    let bug = bug.ok_or_else(|| bug_not_found(&args.id))?;
    Ok(Output::success(json!({"bug": bug.json()})))
}
