//! `policy show` (commands.md §22.1): the resolved policy of one key, read-only.

use std::path::PathBuf;

use clap::Args;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Map, Value};

use super::resolve::{machine_path, owner_path, repo_path};
use super::domain::{
    BUILD, FEEDBACK_CHECKS, GATE_PR, LIST, MERGE, PATHS, POST_CHANNEL, POST_ENABLED, POST_MERGED_REACTION, POST_TEMPLATE, PRE_COMMIT,
    PRE_MERGE, PRE_START, RUN,
};
use super::{audience, check_file_names, load_machine, resolve, Domain, DomainSource, Machine, PolicyKey};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::ids::{ContextId, OwnerLogin, PlanId, PrKey, RepoName, SessionId};
use crate::output::Output;
use crate::store::Store;

/// The flags that select a policy key (`policy show`, `tiers`).
#[derive(Args, Debug)]
pub struct KeyArgs {
    /// The PR of the selected Plan whose repo policy to show
    #[arg(long, value_name = "key", conflicts_with_all = ["context", "owner"])]
    pr: Option<PrKey>,
    /// The Plan of `--pr` [default: $CPD_PLAN]
    #[arg(long, value_name = "id", env = "CPD_PLAN", hide_env = true)]
    plan: Option<PlanId>,
    /// The Context whose owner's policy to show
    #[arg(long, value_name = "id", conflicts_with_all = ["pr", "owner"])]
    context: Option<ContextId>,
    /// The owner whose policy to show
    #[arg(long, value_name = "login", conflicts_with_all = ["pr", "context"])]
    owner: Option<OwnerLogin>,
    /// With `--owner`: the repo of that owner
    #[arg(long, value_name = "name")]
    repo: Option<RepoName>,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    #[command(flatten)]
    key: KeyArgs,
}

impl KeyArgs {
    /// The key the flags select: `None` when none is given. `--repo` without `--owner` is `usage`.
    pub fn select(&self, store: &mut Store) -> Result<Option<PolicyKey>, CpdError> {
        if self.repo.is_some() && self.owner.is_none() {
            return Err(CpdError::new(ErrorCode::Usage, "--repo goes with --owner only."));
        }
        if let Some(pr) = &self.pr {
            let plan = self.plan.as_ref().ok_or_else(|| {
                CpdError::new(ErrorCode::PlanRequired, "No Plan selected: pass --plan <id> or set CPD_PLAN.")
            })?;
            Ok(Some(store.read(|conn| key_of_pr(conn, plan.as_str(), pr.as_str()))?))
        } else if let Some(context) = &self.context {
            Ok(Some(store.read(|conn| context_owner(conn, context.as_str()).map(PolicyKey::Owner))?))
        } else if let Some(owner) = &self.owner {
            Ok(Some(match &self.repo {
                Some(name) => PolicyKey::Repo(owner.clone(), name.clone()),
                None => PolicyKey::Owner(owner.clone()),
            }))
        } else {
            Ok(None)
        }
    }
}

fn not_found(code: ErrorCode, what: &str, field: &str, id: &str) -> CpdError {
    CpdError::new(code, format!("No {what} {id}.")).with(field, id)
}

fn context_owner(conn: &Connection, context: &str) -> Result<OwnerLogin, CpdError> {
    let owner: Option<Option<String>> = conn
        .query_row("SELECT owner FROM contexts WHERE id = ?1", [context], |r| r.get(0))
        .optional()?;
    match owner {
        None => Err(not_found(ErrorCode::ContextNotFound, "Context", "context", context)),
        Some(None) => Err(super::owner_unset(&ContextId::parse(context)?)),
        Some(Some(owner)) => OwnerLogin::parse(&owner),
    }
}

/// `Repo(owner, name)`, or `Owner(owner)` for a PR whose `name` is null.
fn pr_key(owner: OwnerLogin, name: Option<String>) -> Result<PolicyKey, CpdError> {
    Ok(match name {
        Some(name) => PolicyKey::Repo(owner, RepoName::parse(&name)?),
        None => PolicyKey::Owner(owner),
    })
}

/// The key of PR `pr` of Plan `plan`, and of the Context of the Plan.
fn key_of_pr(conn: &Connection, plan: &str, pr: &str) -> Result<PolicyKey, CpdError> {
    let context: Option<String> =
        conn.query_row("SELECT context FROM plans WHERE id = ?1", [plan], |r| r.get(0)).optional()?;
    let context = context.ok_or_else(|| not_found(ErrorCode::PlanNotFound, "Plan", "plan", plan))?;
    let name: Option<Option<String>> = conn
        .query_row("SELECT name FROM prs WHERE plan = ?1 AND key = ?2", [plan, pr], |r| r.get(0))
        .optional()?;
    let name = name.ok_or_else(|| {
        CpdError::new(ErrorCode::PrNotFound, format!("No PR {pr} in Plan {plan}.")).with("plan", plan).with("pr", pr)
    })?;
    pr_key(context_owner(conn, &context)?, name)
}

/// The Session's role and key (no flag): a Manager or a Lead its Context's owner, a Worker its Job's PR.
fn key_of_session(conn: &Connection, session: &SessionId) -> Result<(&'static str, PolicyKey), CpdError> {
    let row: Option<(String, String, Option<String>, Option<String>)> = conn
        .query_row("SELECT role, context, plan, attempt FROM sessions WHERE id = ?1", [session.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    let (role, context, plan, attempt) =
        row.ok_or_else(|| not_found(ErrorCode::SessionNotFound, "Session", "session", session.as_str()))?;
    let owner = context_owner(conn, &context)?;
    match (role.as_str(), plan, attempt) {
        ("manager", ..) => Ok(("manager", PolicyKey::Owner(owner))),
        ("lead", ..) => Ok(("lead", PolicyKey::Owner(owner))),
        ("worker", Some(plan), Some(attempt)) => {
            let name: Option<String> = conn.query_row(
                "SELECT p.name FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
                   JOIN prs p ON p.plan = j.plan AND p.key = j.pr WHERE a.plan = ?1 AND a.id = ?2",
                [&plan, &attempt],
                |r| r.get(0),
            )?;
            Ok(("worker", pr_key(owner, name)?))
        }
        _ => Err(CpdError::new(ErrorCode::SessionMalformed, format!("Session {session} has role {role} and no Attempt."))),
    }
}

fn session_from_env() -> Result<SessionId, CpdError> {
    std::env::var("CPD_SESSION")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| SessionId::parse(&v))
        .transpose()?
        .ok_or_else(|| {
            CpdError::new(ErrorCode::SessionRequired, "No Session selected: pass a flag, or set CPD_SESSION.")
        })
}

pub(super) fn show(args: ShowArgs) -> Result<Output, CpdError> {
    let Env { home, mut store, config, .. } = Env::open()?;
    let (role, key) = match args.key.select(&mut store)? {
        Some(key) => (None, key),
        None => {
            let session = session_from_env()?;
            let (role, key) = store.read(|conn| key_of_session(conn, &session))?;
            (Some(role), key)
        }
    };
    check_file_names(&home)?;
    let resolved = resolve(&home, &key)?;
    let machine = load_machine(&home)?;

    // The files read, machine first (`host.toml`); the owner's exists (resolve refused otherwise).
    let mut files = vec![machine_path(&home), owner_path(&home, key.owner())];
    if let PolicyKey::Repo(owner, name) = &key {
        files.push(repo_path(&home, owner, name));
    }
    let files: Vec<PathBuf> = files.iter().filter_map(|f| std::fs::canonicalize(f).ok()).collect();

    let mut policy = Map::new();
    for &domain in Domain::ALL {
        let source = resolved.sources.get(domain);
        let keys = typed_keys(domain, &resolved, source);
        policy.insert(
            domain.name().to_owned(),
            json!({
                "source": source.source.to_string(),
                "audience": audience(domain).iter().map(|r| r.as_str()).collect::<Vec<_>>(),
                "keys": keys,
                "text": resolved.policy.text(domain),
            }),
        );
    }
    let (owner, repo) = match &key {
        PolicyKey::Owner(o) => (o.as_str(), None),
        PolicyKey::Repo(o, n) => (o.as_str(), Some(n.as_str())),
    };
    let mut out = json!({
        "key": {"owner": owner, "repo": repo},
        "role": role,
        "files": files,
        "policy": policy,
        "machine": {"name": config.machine.as_str(), "mcp": {"source": machine.source.to_string(), "servers": machine.servers}},
        "markdown": machine_markdown(config.machine.as_str(), &machine),
    });
    if !resolved.warnings.is_empty() {
        out["warnings"] = json!(resolved.warnings);
    }
    Ok(Output::success(out))
}

/// The typed keys of `domain` with value and source; a key the winning file leaves out has
/// the domain's source followed by ` (key default)` (contract §8.3 "Precedence and replace").
/// The left-out keys come from [`super::domain::Set::fill`], so no file is read again.
fn typed_keys(domain: Domain, resolved: &super::Resolved, source: &DomainSource) -> Value {
    fn value(v: &impl serde::Serialize) -> Value {
        serde_json::to_value(v).expect("a policy value serializes")
    }
    let entries: Vec<(&str, Value)> = match domain {
        Domain::Gates => {
            let g = &resolved.policy.gate;
            vec![(MERGE, value(&g.merge)), (GATE_PR, value(&g.pr))]
        }
        Domain::Pr => {
            let pr = &resolved.policy.pr;
            vec![
                (FEEDBACK_CHECKS, value(&pr.feedback.checks)),
                (POST_ENABLED, value(&pr.post.enabled)),
                (POST_CHANNEL, value(&pr.post.channel)),
                (POST_TEMPLATE, value(&pr.post.template)),
                (POST_MERGED_REACTION, value(&pr.post.merged_reaction)),
            ]
        }
        Domain::Hooks => {
            let h = &resolved.policy.hooks;
            vec![(PRE_START, value(&h.pre_start)), (PRE_COMMIT, value(&h.pre_commit)), (PRE_MERGE, value(&h.pre_merge))]
        }
        Domain::TestCommands => {
            let t = resolved.policy.test_commands.as_ref();
            vec![
                (BUILD, value(&t.map(|t| &t.build))),
                (RUN, value(&t.map(|t| &t.run))),
                (LIST, value(&t.and_then(|t| t.list.as_ref()))),
            ]
        }
        Domain::Model => {
            let m = &resolved.policy.model;
            vec![("O", value(&m.o)), ("S", value(&m.s)), ("H", value(&m.h)), ("G", value(&m.g))]
        }
        Domain::Protected => vec![(PATHS, value(&resolved.policy.protected.paths))],
        Domain::Resources => vec![("names", value(&resolved.policy.resources.names))],
        _ => Vec::new(),
    };
    let mut keys = Map::new();
    for (name, value) in entries {
        let source = if source.left_out.contains(&name) {
            format!("{} (key default)", source.source)
        } else {
            source.source.to_string()
        };
        keys.insert(name.to_owned(), json!({"value": value, "source": source}));
    }
    Value::Object(keys)
}

/// The `## Machine <name>` section: each server's name, `url` and `exposure`.
fn machine_markdown(name: &str, machine: &Machine) -> String {
    let mut md = format!("## Machine {name}\n\n");
    if machine.servers.is_empty() {
        md.push_str("No MCP servers.\n");
    }
    for (server, entry) in &machine.servers {
        let exposure = entry.exposure.map_or(String::new(), |e| {
            format!(", exposure {}", serde_json::to_value(e).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default())
        });
        md.push_str(&format!("- {server}: {}{exposure}\n", entry.url));
    }
    md
}
