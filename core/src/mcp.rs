//! `mcp attach` (commands.md §24.1; contract §5.2 `mcp-attached`; pro-d2, pro-d8): one Event addressed to the
//! caller's Session, which its adapter turns into `pi.registerMcpServer`. Nothing else is written.

use clap::{Args, Subcommand};
use rusqlite::OptionalExtension;
use serde_json::json;

use crate::args::SessionRef;
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::output::Output;
use crate::policy::{load_machine, name_ok};

/// The `mcp` commands (commands.md §24).
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Attach a server of this machine's policy to this Session only (§24.1)
    Attach(AttachArgs),
}

#[derive(Args, Debug)]
pub struct AttachArgs {
    /// A server of this machine's policy
    #[arg(value_name = "name")]
    name: String,
    /// The Session to attach it to [default: $CPD_SESSION]
    #[arg(long, value_name = "id", env = "CPD_SESSION", hide_env = true)]
    session: Option<SessionRef>,
}

/// Runs an `mcp` command.
pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Attach(args) => attach(args),
    }
}

fn attach(args: AttachArgs) -> Result<Output, CpdError> {
    let name = args.name;
    if !name_ok(&name) {
        return Err(CpdError::new(ErrorCode::Usage, format!("{name:?} is not a server name: ^[a-z][a-z0-9-]{{0,31}}$.")).with("name", name.as_str()));
    }
    let reference = args
        .session
        .ok_or_else(|| CpdError::new(ErrorCode::SessionRequired, "No Session selected: pass --session <id>, or set CPD_SESSION."))?;
    let mut env = Env::open()?;
    let session = env.store.read(|conn| crate::session::resolve(conn, &reference))?;
    let machine = load_machine(&env.home)?;
    let Some(server) = machine.servers.get(&name) else {
        let available: Vec<&str> = machine.servers.keys().map(String::as_str).collect();
        let listed = if available.is_empty() { "none".to_owned() } else { available.join(", ") };
        return Err(CpdError::new(
            ErrorCode::McpUnknown,
            format!("Machine {} has no MCP server {name}; available: {listed}.", env.config.machine),
        )
        .with("name", name.as_str())
        .with("machine", env.config.machine.as_str())
        .with("available", available));
    };
    let held: Option<(String, String, String, Option<String>)> = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT a.id, a.runner, s.context, s.plan FROM sessions s JOIN attempts a ON a.id = s.attempt WHERE s.id = ?1",
                [session.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?)
    })?;
    if let Some((attempt, stored, context, plan)) = held {
        let runner = crate::config::WorkerRunner::parse(&stored)
            .map_err(|e| CpdError::new(ErrorCode::Usage, format!("Attempt {attempt} has a bad runner: {e}.")).with("attempt", attempt.as_str()))?;
        if runner == crate::config::WorkerRunner::Durable {
            let message = format!("Server {name} was not attached: Attempt {attempt} runs on the durable runner, which has no MCP client, so no tool would arrive.");
            let mut details = serde_json::Map::new();
            details.insert("name".to_owned(), json!(name));
            details.insert("session".to_owned(), json!(session));
            details.insert("attempt".to_owned(), json!(attempt));
            details.insert("runner".to_owned(), json!(runner.as_str()));
            env.store.write("mcp attach", |tx| {
                let mut new = NewEvent::error(message.clone(), "mcp attach", ErrorCode::RunnerNoTools.as_str(), &message, &details)
                    .context(crate::ids::ContextId::parse(&context)?);
                if let Some(plan) = &plan {
                    new = new.plan(crate::ids::PlanId::parse(plan)?);
                }
                append(tx, new)?;
                Ok(())
            })?;
            let mut err = CpdError::new(ErrorCode::RunnerNoTools, message);
            for (k, v) in details {
                err = err.with(&k, v);
            }
            return Err(err);
        }
    }
    let event = env.store.write("mcp attach", |tx| {
        let (context, plan): (String, Option<String>) = tx
            .query_row("SELECT context, plan FROM sessions WHERE id = ?1", [session.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?
            .ok_or_else(|| CpdError::new(ErrorCode::SessionNotFound, format!("No Session {session}.")).with("session", session.as_str()))?;
        let mut new = NewEvent::mcp_attached(format!("Server {name} attached to Session {session}."), &name, server)
            .session(session.clone())
            .context(crate::ids::ContextId::parse(&context)?);
        if let Some(plan) = plan {
            new = new.plan(crate::ids::PlanId::parse(&plan)?);
        }
        append(tx, new)
    })?;
    Ok(Output::success(json!({"attached": {"name": name, "session": session, "event": event}})))
}
