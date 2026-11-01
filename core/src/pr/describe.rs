//! `pr describe` (commands.md §9.5a): stores a PR's description, the body of its pull request. In any phase and any PR status
//! but `merged` and `cancelled`; on an adopted PR it is refused; with an open pull request GitHub is updated first.

use std::io::Read;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use super::{DescribeArgs, Env, find_workflow, gh};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::ids::{PlanId, WorkflowKey};
use crate::output::Output;
use crate::plan::fingerprint::load_workflows;

const COMMAND: &str = "pr describe";

/// The most characters a description holds.
const MAX_CHARS: usize = 65536;

enum Decision {
    Already(String),
    /// `open` is the `(repo, number)` of the PR's open pull request, if any.
    Update { status: String, open: Option<(String, i64)> },
}

pub(super) fn describe(args: DescribeArgs) -> Result<Output, CpdError> {
    let plan = args.plan.require()?.clone();
    let key = args.key;
    let mut env = Env::open()?;
    // The PR first (`pr_adopted`, `pr_ended`), then the text.
    env.store.read(|conn| decide(conn, &plan, &key, None).map(drop))?;
    let text = read_text(&args.file)?;
    if text.trim().is_empty() || text.chars().count() > MAX_CHARS {
        return Err(CpdError::new(
            ErrorCode::DescriptionInvalid,
            format!("The description must have text and at most {MAX_CHARS} characters, it has {} (trimmed: {}).", text.chars().count(), text.trim().chars().count()),
        )
        .with("plan", plan.as_str())
        .with("pr", key.as_str())
        .with("max", MAX_CHARS));
    }
    let (status, already) = match env.store.read(|conn| decide(conn, &plan, &key, Some(&text)))? {
        Decision::Already(status) => (status, true),
        Decision::Update { open, .. } => {
            if let Some((repo, number)) = open {
                gh::describe_pr(env.home.root(), &repo, number, &text)?;
            }
            let status = env.store.write(COMMAND, |tx| match decide(tx, &plan, &key, Some(&text))? {
                Decision::Already(status) => Ok(status),
                Decision::Update { status, .. } => {
                    tx.execute("UPDATE workflows SET description = ?1 WHERE plan = ?2 AND key = ?3", [text.as_str(), plan.as_str(), key.as_str()])?;
                    let message = format!("PR {key} of Plan {plan} described. Next: cpd plan render --plan {plan}");
                    append(tx, NewEvent::workflow_described(message, &key).plan(plan.clone()))?;
                    crate::session::working_pr::name(tx, &plan, &key)?;
                    Ok(status)
                }
            })?;
            (status, false)
        }
    };
    let mut out = json!({"workflow": {"key": key, "status": status}});
    if already {
        out["already"] = json!(true);
    }
    Ok(Output::success(out))
}

/// The text of `--file`, `-` being stdin: UTF-8, else `file_unreadable`.
fn read_text(file: &Path) -> Result<String, CpdError> {
    if file == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes).map_err(|e| CpdError::file_unreadable(file, e))?;
        return String::from_utf8(bytes).map_err(|e| CpdError::file_unreadable(file, e));
    }
    std::fs::read_to_string(file).map_err(|e| CpdError::file_unreadable(file, e))
}

/// The PR's status when it may be described, and with `text` whether that changes it; the open pull request's repo and number, if any.
fn decide(conn: &Connection, plan: &PlanId, key: &WorkflowKey, text: Option<&str>) -> Result<Decision, CpdError> {
    let workflows = load_workflows(conn, plan)?;
    let workflow = find_workflow(&workflows, plan, key)?;
    if workflow.head_ref.is_some() {
        return Err(CpdError::new(ErrorCode::WorkflowAdopted, format!("PR {key} is an adopted pull request: its own body is kept, so it is not described."))
            .with("plan", plan.as_str())
            .with("pr", key.as_str()));
    }
    if matches!(workflow.status.as_str(), "merged" | "cancelled") {
        return Err(CpdError::new(ErrorCode::WorkflowEnded, format!("PR {key} of Plan {plan} is {}: its description is history.", workflow.status))
            .with("plan", plan.as_str())
            .with("pr", key.as_str())
            .with("status", workflow.status.as_str()));
    }
    let Some(text) = text else { return Ok(Decision::Already(workflow.status.clone())) };
    let stored: Option<String> = conn.query_row("SELECT description FROM workflows WHERE plan = ?1 AND key = ?2", [plan.as_str(), key.as_str()], |r| r.get(0))?;
    if stored.as_deref() == Some(text) {
        return Ok(Decision::Already(workflow.status.clone()));
    }
    let open = conn
        .prepare(
            "SELECT p.repo, p.number FROM workflows r JOIN pull_requests p ON p.id = r.pull_request
              WHERE r.plan = ?1 AND r.key = ?2 AND r.status = 'open' AND p.state = 'open'",
        )?
        .query_row([plan.as_str(), key.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(Decision::Update { status: workflow.status.clone(), open })
}
