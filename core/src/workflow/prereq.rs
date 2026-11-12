//! Workflow external prerequisites (commands.md §9.5b, §9.8).
//!
//! External prerequisites gate merge, post, and deploy until met.

use clap::Subcommand;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::args::PlanArg;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::host::deadline::Failed;
use crate::host::runner::{Call, Runner};
use crate::ids::WorkflowKey;
use crate::output::Output;
use crate::plan::fingerprint::load_workflows;
use crate::pr::find_workflow;

#[derive(Subcommand, Debug)]
pub enum PrereqCommand {
    /// Add an external prerequisite to a workflow
    Add(AddPrereqArgs),
    /// Confirm a manual prerequisite
    Confirm(ConfirmPrereqArgs),
}

#[derive(clap::Args, Debug)]
pub struct AddPrereqArgs {
    /// The Workflow key
    #[arg(long, value_name = "key")]
    pub workflow: WorkflowKey,
    /// Kind of prerequisite: ld-flag or manual
    #[arg(long, value_name = "kind")]
    pub kind: String,
    /// LaunchDarkly project key (for kind ld-flag)
    #[arg(long, value_name = "project")]
    pub project: Option<String>,
    /// LaunchDarkly flag key (for kind ld-flag)
    #[arg(long, value_name = "key")]
    pub key: Option<String>,
    /// LaunchDarkly environment (for kind ld-flag, optional)
    #[arg(long, value_name = "env")]
    pub env: Option<String>,
    /// Manual prerequisite description text (for kind manual)
    #[arg(long, value_name = "text")]
    pub text: Option<String>,
    #[command(flatten)]
    pub plan: PlanArg,
}

#[derive(clap::Args, Debug)]
pub struct ConfirmPrereqArgs {
    /// The Workflow key
    #[arg(long, value_name = "key")]
    pub workflow: WorkflowKey,
    /// The prerequisite id
    #[arg(long, value_name = "id")]
    pub id: String,
    #[command(flatten)]
    pub plan: PlanArg,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrereqRow {
    pub plan: String,
    pub workflow: String,
    pub id: i64,
    pub kind: String,
    pub details_json: String,
    pub met_at: Option<String>,
    pub met_by: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnmetPrereq {
    pub id: i64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// Run a prerequisite command.
pub fn run(command: PrereqCommand) -> Result<Output, CpdError> {
    match command {
        PrereqCommand::Add(args) => add(args),
        PrereqCommand::Confirm(args) => confirm(args),
    }
}

pub fn add(args: AddPrereqArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let plan = args.plan.require()?.clone();
    let key = args.workflow;

    // Precondition 1: the Workflow is a Workflow of the Plan
    // Precondition 2: the Workflow is not merged or cancelled
    let workflows = env.store.read(|conn| load_workflows(conn, &plan))?;
    let workflow = find_workflow(&workflows, &plan, &key)?;
    if matches!(workflow.status.as_str(), "merged" | "cancelled") {
        return Err(CpdError::new(
            ErrorCode::WorkflowEnded,
            format!(
                "Workflow {key} of Plan {plan} is {}: its prerequisites are history.",
                workflow.status
            ),
        )
        .with("plan", plan.as_str())
        .with("workflow", key.as_str())
        .with("status", workflow.status.as_str()));
    }

    // Precondition 3: --kind is ld-flag or manual; flags of that kind given; no flags of other kind given
    let (details, prereq_json) = match args.kind.as_str() {
        "ld-flag" => {
            if args.text.is_some() || args.project.is_none() || args.key.is_none() {
                return Err(CpdError::new(
                    ErrorCode::Usage,
                    "kind ld-flag requires --project and --key and forbids --text",
                ));
            }
            let project = args.project.unwrap();
            let flag_key = args.key.unwrap();
            let env_val = args.env;
            let det = json!({
                "project": project,
                "key": flag_key,
                "env": env_val,
            });
            let out_fields = json!({
                "kind": "ld-flag",
                "project": project,
                "key": flag_key,
                "env": env_val,
            });
            (det, out_fields)
        }
        "manual" => {
            if args.project.is_some()
                || args.key.is_some()
                || args.env.is_some()
                || args.text.is_none()
            {
                return Err(CpdError::new(
                    ErrorCode::Usage,
                    "kind manual requires --text and forbids --project, --key, --env",
                ));
            }
            let text = args.text.unwrap();
            let det = json!({
                "text": text,
            });
            let out_fields = json!({
                "kind": "manual",
                "text": text,
            });
            (det, out_fields)
        }
        _ => {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("Unknown kind: {}", args.kind),
            ));
        }
    };

    let details_str =
        serde_json::to_string(&details).map_err(|e| CpdError::bad_row(e.to_string()))?;

    // Check if already declared (idempotence)
    let existing: Option<(i64, String, Value)> = env.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, kind, details_json FROM workflow_prereqs WHERE plan = ?1 AND workflow = ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![plan.as_str(), key.as_str()], |r| {
            let id: i64 = r.get(0)?;
            let kind: String = r.get(1)?;
            let dj: String = r.get(2)?;
            let parsed: Value = serde_json::from_str(&dj).unwrap_or(Value::Null);
            Ok((id, kind, parsed))
        })?;
        for row in rows {
            let (id, k, p) = row?;
            if k == args.kind && p == details {
                return Ok(Some((id, k, p)));
            }
        }
        Ok(None)
    })?;

    if let Some((id, _, _)) = existing {
        let mut obj = prereq_json;
        if let Some(map) = obj.as_object_mut() {
            map.insert("id".to_string(), json!(id));
        }
        return Ok(Output::success(json!({
            "already": true,
            "workflow": key.as_str(),
            "prereq": obj,
        })));
    }

    let inserted_id = env.store.write("workflow prereq add", |tx| {
        let next_id: i64 = tx.query_row(
            "SELECT COALESCE(MAX(id), 0) + 1 FROM workflow_prereqs WHERE plan = ?1 AND workflow = ?2",
            rusqlite::params![plan.as_str(), key.as_str()],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO workflow_prereqs (plan, workflow, id, kind, details_json, met_at, met_by)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL)",
            rusqlite::params![plan.as_str(), key.as_str(), next_id, args.kind, details_str],
        )?;
        Ok(next_id)
    })?;

    let mut obj = prereq_json;
    if let Some(map) = obj.as_object_mut() {
        map.insert("id".to_string(), json!(inserted_id));
    }

    Ok(Output::success(json!({
        "workflow": key.as_str(),
        "prereq": obj,
    })))
}

pub fn confirm(args: ConfirmPrereqArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let plan = args.plan.require()?.clone();
    let key = args.workflow;
    let req_id: i64 = args
        .id
        .parse()
        .map_err(|_| CpdError::new(ErrorCode::Usage, format!("Invalid prereq id: {}", args.id)))?;

    let workflows = env.store.read(|conn| load_workflows(conn, &plan))?;
    let workflow = find_workflow(&workflows, &plan, &key)?;
    if matches!(workflow.status.as_str(), "merged" | "cancelled") {
        return Err(CpdError::new(
            ErrorCode::WorkflowEnded,
            format!(
                "Workflow {key} of Plan {plan} is {}: its prerequisites are history.",
                workflow.status
            ),
        )
        .with("plan", plan.as_str())
        .with("workflow", key.as_str())
        .with("status", workflow.status.as_str()));
    }

    // Precondition 4: --id names a manual prerequisite of the Workflow; ld-flag is checked, never confirmed, and an unknown --id is refused with usage
    let row: Option<(String, Option<String>, String)> = env.store.read(|conn| {
        Ok(conn.query_row(
            "SELECT kind, met_at, details_json FROM workflow_prereqs WHERE plan = ?1 AND workflow = ?2 AND id = ?3",
            rusqlite::params![plan.as_str(), key.as_str(), req_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
    })?;

    let (kind, met_at, details_json) = row.ok_or_else(|| {
        CpdError::new(
            ErrorCode::Usage,
            format!("Prerequisite {} not found for workflow {}", req_id, key),
        )
    })?;

    if kind != "manual" {
        return Err(CpdError::new(
            ErrorCode::Usage,
            format!(
                "Prerequisite {} is of kind {}, only manual prerequisites can be confirmed",
                req_id, kind
            ),
        ));
    }

    let det: Value = serde_json::from_str(&details_json).unwrap_or(Value::Null);
    let prereq_obj = json!({
        "id": req_id,
        "kind": "manual",
        "text": det.get("text").cloned().unwrap_or(Value::Null),
    });

    if met_at.is_some() {
        return Ok(Output::success(json!({
            "already": true,
            "workflow": key.as_str(),
            "prereq": prereq_obj,
        })));
    }

    let now_str = crate::clock::timestamp(env.ports.clock.utc());
    let caller = "user"; // confirmed by user

    env.store.write("workflow prereq confirm", |tx| {
        tx.execute(
            "UPDATE workflow_prereqs SET met_at = ?1, met_by = ?2 WHERE plan = ?3 AND workflow = ?4 AND id = ?5",
            rusqlite::params![now_str, caller, plan.as_str(), key.as_str(), req_id],
        )?;
        Ok(())
    })?;

    Ok(Output::success(json!({
        "workflow": key.as_str(),
        "prereq": prereq_obj,
    })))
}

/// Load prerequisites for a workflow from the database.
pub fn load_prereqs(
    conn: &Connection,
    plan: &str,
    workflow: &str,
) -> Result<Vec<PrereqRow>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT plan, workflow, id, kind, details_json, met_at, met_by
         FROM workflow_prereqs
         WHERE plan = ?1 AND workflow = ?2
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![plan, workflow], |r| {
        Ok(PrereqRow {
            plan: r.get(0)?,
            workflow: r.get(1)?,
            id: r.get(2)?,
            kind: r.get(3)?,
            details_json: r.get(4)?,
            met_at: r.get(5)?,
            met_by: r.get(6)?,
        })
    })?;
    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

/// Retrieve the LaunchDarkly API token from vault `cpd`, item `launchdarkly/api-token` via runner.
fn get_ld_token(runner: &dyn Runner) -> Result<String, CpdError> {
    crate::secret::op::get_with(runner, "cpd", "launchdarkly/api-token", "token")
        .or_else(|_| {
            crate::secret::op::get_with(runner, "cpd", "launchdarkly/api-token", "password")
        })
        .or_else(|_| {
            crate::secret::op::get_with(runner, "cpd", "launchdarkly/api-token", "credential")
        })
        .map(|fields| {
            fields
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        })
}

/// Check live if a LaunchDarkly flag exists in a project using REST API via Runner curl.
fn check_ld_flag(
    runner: &dyn Runner,
    token: &str,
    project: &str,
    key: &str,
) -> Result<bool, String> {
    let url = format!("https://app.launchdarkly.com/api/v2/flags/{project}/{key}");
    let auth_header = format!("Authorization: {token}");
    let call = Call {
        tool: "curl".to_string(),
        program: "curl".to_string(),
        args: vec![
            "-s".to_string(),
            "-o".to_string(),
            "/dev/null".to_string(),
            "-w".to_string(),
            "%{http_code}".to_string(),
            "-H".to_string(),
            auth_header,
            "-H".to_string(),
            "LD-API-Version: 20240415".to_string(),
            url,
        ],
        envs: Vec::new(),
        cwd: None,
        deadline: std::time::Duration::from_secs(15),
        stdin: None,
        stderr_to: None,
    };

    let res = runner.run(&call).map_err(|e| match e {
        Failed::Timeout(_) => "unreachable".to_string(),
        Failed::Io(_) => "unreachable".to_string(),
    })?;

    if !res.status.success() {
        return Err("unreachable".to_string());
    }

    let code_str = String::from_utf8_lossy(&res.stdout).trim().to_string();
    if code_str == "200" {
        Ok(true)
    } else if code_str == "404" {
        Ok(false)
    } else {
        Err("unreachable".to_string())
    }
}

/// Evaluate all prerequisites of a workflow and return the unmet list.
pub fn evaluate_unmet(
    conn: &Connection,
    runner: &dyn Runner,
    plan: &str,
    workflow: &str,
) -> Result<Vec<UnmetPrereq>, CpdError> {
    let rows = load_prereqs(conn, plan, workflow)?;
    let mut unmet = Vec::new();

    // Lazy load token if needed
    let mut ld_token: Option<Result<String, CpdError>> = None;

    for row in rows {
        let details: Value = serde_json::from_str(&row.details_json).unwrap_or(Value::Null);
        match row.kind.as_str() {
            "manual" => {
                if row.met_at.is_none() {
                    unmet.push(UnmetPrereq {
                        id: row.id,
                        kind: "manual".to_string(),
                        project: None,
                        key: None,
                        env: None,
                        text: details
                            .get("text")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        cause: None,
                    });
                }
            }
            "ld-flag" => {
                let project = details
                    .get("project")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let flag_key = details
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let env_val = details.get("env").cloned().unwrap_or(Value::Null);

                let token_res = ld_token.get_or_insert_with(|| get_ld_token(runner));
                match token_res {
                    Err(_) => {
                        unmet.push(UnmetPrereq {
                            id: row.id,
                            kind: "ld-flag".to_string(),
                            project: Some(project.to_string()),
                            key: Some(flag_key.to_string()),
                            env: if env_val.is_null() {
                                None
                            } else {
                                Some(env_val)
                            },
                            text: None,
                            cause: Some("no_token".to_string()),
                        });
                    }
                    Ok(tok) => {
                        match check_ld_flag(runner, tok, project, flag_key) {
                            Ok(true) => {
                                // flag exists, prerequisite is met!
                            }
                            Ok(false) => {
                                // flag does not exist
                                unmet.push(UnmetPrereq {
                                    id: row.id,
                                    kind: "ld-flag".to_string(),
                                    project: Some(project.to_string()),
                                    key: Some(flag_key.to_string()),
                                    env: if env_val.is_null() {
                                        None
                                    } else {
                                        Some(env_val)
                                    },
                                    text: None,
                                    cause: None,
                                });
                            }
                            Err(cause) => {
                                unmet.push(UnmetPrereq {
                                    id: row.id,
                                    kind: "ld-flag".to_string(),
                                    project: Some(project.to_string()),
                                    key: Some(flag_key.to_string()),
                                    env: if env_val.is_null() {
                                        None
                                    } else {
                                        Some(env_val)
                                    },
                                    text: None,
                                    cause: Some(cause),
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(unmet)
}

/// Create a `prereq_unmet` error if there are unmet prerequisites.
pub fn ensure_prereqs_met(
    conn: &Connection,
    runner: &dyn Runner,
    plan: &str,
    workflow: &str,
    command: &str,
) -> Result<(), CpdError> {
    let unmet = evaluate_unmet(conn, runner, plan, workflow)?;
    if unmet.is_empty() {
        return Ok(());
    }

    let mut parts = Vec::new();
    for u in &unmet {
        if u.kind == "manual" {
            parts.push(format!("manual: {}", u.text.as_deref().unwrap_or("")));
        } else if u.kind == "ld-flag" {
            parts.push(format!(
                "ld-flag {}/{}",
                u.project.as_deref().unwrap_or(""),
                u.key.as_deref().unwrap_or("")
            ));
        }
    }

    let message = format!(
        "{workflow}: {} external prerequisites unmet, so cpd {command} cannot run: {}",
        unmet.len(),
        parts.join("; ")
    );

    let unmet_json = serde_json::to_value(&unmet).map_err(|e| CpdError::bad_row(e.to_string()))?;

    Err(CpdError::new(ErrorCode::PrereqUnmet, message)
        .with("plan", plan)
        .with("workflow", workflow)
        .with("command", command)
        .with("unmet", unmet_json))
}
