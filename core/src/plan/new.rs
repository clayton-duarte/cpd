//! `plan new` (commands.md §4.1, contract §3.1): registers the Plan and its
//! Lead, writes the `plan` record, the Lead's first message, and starts the
//! Lead in a parked window of the cockpit.

use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::lead::{self, Lead};
use super::row::{self, PlanRow};
use super::{Env, NewArgs, bad_row, read_text_file, write_file};
use crate::args::Text;
use crate::context::render::markdown;
use crate::context::{By, ContextRow, Contexts, Draft, PlanRef, Record};
use crate::error::{CpdError, ErrorCode};
use crate::events::{Kind, NewEvent, append};
use crate::git::lead_branch;
use crate::git::run::{branch_exists, run};
use crate::git::worktree::list as worktrees;
use crate::ids::{PlanId, SessionId};
use crate::output::Output;

pub(super) fn new(args: NewArgs) -> Result<Output, CpdError> {
    let brief = args.brief_file.as_deref().map(read_text_file).transpose()?;
    let mut env = Env::open()?;
    let id = args.id.clone();
    let name = args.name.as_ref().map_or_else(|| id.to_string(), |n| n.as_str().to_owned());
    let jira = args.jira.as_ref().map(|j| j.as_str().to_owned());

    let context = env.store.read(|conn| ContextRow::require(conn, &args.context))?;
    if context.archived_at.is_some() {
        return Err(CpdError::new(
            ErrorCode::Archived,
            format!("Context {} is archived: unarchive it before creating a Plan in it.", args.context),
        )
        .with("context", args.context.as_str())
        .with("next", format!("cpd context unarchive {}", args.context)));
    }
    let repo = main_worktree(&args.repo)?;
    let repo_str = repo.display().to_string();
    let base = resolve_base(&repo, args.base.as_deref())?;
    if let Some(plan) = env.store.read(|conn| row::find(conn, &id))? {
        let lead_name: String = env.store.read(|conn| {
            Ok(conn.query_row("SELECT name FROM sessions WHERE id = ?1", [plan.lead_session.as_str()], |r| r.get(0))?)
        })?;
        let same = plan.context == args.context
            && plan.repo == repo_str
            && plan.title == args.title.as_str()
            && plan.base == base
            && plan.jira == jira
            && lead_name == name;
        if !same {
            return Err(CpdError::new(ErrorCode::PlanExists, format!("Plan {id} exists with other flags.")).with("plan", plan.to_json()));
        }
        return already(&mut env, &plan, brief.as_deref());
    }
    let branch = lead_branch(&id);
    if branch_exists(&repo, &branch)? {
        return Err(CpdError::new(ErrorCode::BranchExists, format!("Branch {branch} already exists in {repo_str}."))
            .with("branch", branch)
            .with("repo", repo_str));
    }

    // 1. The Plan and its Lead, in one transaction.
    let session = SessionId::new_v4();
    let tier = env.config.lead_tier.to_string();
    let (forked_from, grounded_seq) = env.store.write("plan new", |tx| {
        let exists = tx.query_row("SELECT 1 FROM plans WHERE id = ?1", [id.as_str()], |_| Ok(())).optional()?;
        if exists.is_some() {
            return Err(CpdError::new(ErrorCode::PlanExists, format!("Plan {id} was created meanwhile.")).with("plan", id.as_str()));
        }
        let (forked_from, grounded_seq): (Option<String>, Option<i64>) = tx.query_row(
            "SELECT lead_session, grounded_seq FROM contexts WHERE id = ?1",
            [args.context.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.execute(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, forked_from, cwd, tier, tmux_pane,
                                   state, state_label, state_at, created_at)
             SELECT ?1, 'lead', ?2, ?3, NULL, ?4, NULL, ?5, ?6, ?7, NULL, 'stopped', NULL, at, at
               FROM txns WHERE id = cpd_txn()",
            rusqlite::params![session.as_str(), args.context.as_str(), id.as_str(), name, forked_from, repo_str, tier],
        )?;
        tx.execute(
            "INSERT INTO plans (id, context, title, jira, repo, base, lead_branch, phase, lead_session, created_at)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'grounding', ?8, at FROM txns WHERE id = cpd_txn()",
            rusqlite::params![id.as_str(), args.context.as_str(), args.title.as_str(), jira, repo_str, base, branch, session.as_str()],
        )?;
        let text = format!("Plan {id} \"{}\" created in Context {}, phase grounding. Next: cpd plan show --plan {id}", args.title, args.context);
        let data = json!({"title": args.title.as_str(), "repo": repo_str, "base": base, "jira": jira,
                          "lead_session": session, "forked_from": forked_from});
        append(tx, NewEvent::new(Kind::PlanCreated, text, data).plan(id.clone()))?;
        let text = match &forked_from {
            Some(source) => format!("Lead {name} of Plan {id} registered, a fork of the grounded Lead {source}."),
            None => format!("Lead {name} of Plan {id} registered, a fresh Lead."),
        };
        let data = json!({"role": "lead", "name": name, "forked_from": forked_from, "tier": tier});
        append(tx, NewEvent::new(Kind::SessionRegistered, text, data).plan(id.clone()).session(session.clone()))?;
        Ok((forked_from, grounded_seq))
    })?;
    let plan = env.store.read(|conn| row::load(conn, &id))?;

    // 2. The `plan` record; 3. the first message; 4. the Lead's window.
    let record = write_record(&mut env, &plan)?;
    let since = forked_from.as_ref().and(grounded_seq).unwrap_or(0);
    write_message(&mut env, &plan, brief.as_deref(), since)?;
    let started = start_lead(&mut env, &plan, &session, &name)?;
    Ok(Output::success(json!({
        "plan": summary(&plan),
        "lead": {"session": session, "name": name, "forked_from": forked_from,
                 "pane": started.pane.as_str(), "window": started.window.as_str()},
        "record": record,
    })))
}

/// `plan new` again with the same flags: redoes the steps that did not happen
/// (the `plan` record, the first message, the Lead's window) and returns
/// `"already": true`.
fn already(env: &mut Env, plan: &PlanRow, brief: Option<&str>) -> Result<Output, CpdError> {
    let machine = env.config.machine.clone();
    let record: Option<String> = env.store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT id FROM context_records WHERE context = ?1 AND kind = 'plan' AND plan_id = ?2 AND plan_machine = ?3
                 ORDER BY seq LIMIT 1",
                [plan.context.as_str(), plan.id.as_str(), machine.as_str()],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    let record = match record {
        Some(record) => record,
        None => write_record(env, plan)?,
    };
    let (lead, grounded_seq): ((String, Option<String>, String, Option<String>), Option<i64>) = env.store.read(|conn| {
        let lead = conn.query_row(
            "SELECT name, forked_from, state, tmux_pane FROM sessions WHERE id = ?1",
            [plan.lead_session.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let seq = conn.query_row("SELECT grounded_seq FROM contexts WHERE id = ?1", [plan.context.as_str()], |r| r.get(0))?;
        Ok((lead, seq))
    })?;
    let (name, forked_from, state, pane) = lead;
    if !env.home.lead_message(&plan.id).exists() {
        let since = forked_from.as_ref().and(grounded_seq).unwrap_or(0);
        write_message(env, plan, brief, since)?;
    }
    let (pane, window) = match pane {
        None if state == "stopped" => {
            let started = start_lead(env, plan, &plan.lead_session, &name)?;
            (Some(started.pane.as_str().to_owned()), Some(started.window.as_str().to_owned()))
        }
        pane => (pane, env.store.read(|conn| lead::last_window(conn, &plan.lead_session))?),
    };
    Ok(Output::success(json!({
        "plan": summary(plan),
        "lead": {"session": plan.lead_session, "name": name, "forked_from": forked_from, "pane": pane, "window": window},
        "record": record,
        "already": true,
    })))
}

/// The Plan as `plan new` prints it.
fn summary(plan: &PlanRow) -> Value {
    json!({"id": plan.id, "context": plan.context, "title": plan.title, "jira": plan.jira, "repo": plan.repo,
           "base": plan.base, "lead_branch": plan.lead_branch, "phase": plan.phase.as_str(), "lead_session": plan.lead_session})
}

/// Step 2: the Context record of kind `plan` (§4.6), `text` = the title.
/// Returns its id.
fn write_record(env: &mut Env, plan: &PlanRow) -> Result<String, CpdError> {
    let by = By { session: env_id("CPD_SESSION", SessionId::parse)?.or(env_id("PI_SESSION_ID", SessionId::parse)?), plan: env_id("CPD_PLAN", PlanId::parse)? };
    let draft = Draft::Plan(PlanRef { id: plan.id.clone(), machine: env.config.machine.clone(), repo: plan.repo.clone(), jira: plan.jira.clone() });
    let title: Text = plan.title.parse().map_err(|_| bad_row(format!("Plan {} has title {:?}", plan.id, plan.title)))?;
    let written = Contexts::new(&env.home, &env.config, &mut env.store).write(&plan.context, "plan new", by, &title, draft)?;
    Ok(written.record.id.to_string())
}

/// A non-empty variable of the caller's environment, parsed (the writer of a
/// record, contract §2.2 "Context record" `by`).
fn env_id<T>(name: &str, parse: fn(&str) -> Result<T, CpdError>) -> Result<Option<T>, CpdError> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(|v| parse(&v.to_string_lossy())).transpose()
}

/// Step 3: `run/<plan>/lead-message.md`: the header, the brief, then the
/// Context rendered as `context show` does, from index position `since`.
fn write_message(env: &mut Env, plan: &PlanRow, brief: Option<&str>, since: i64) -> Result<(), CpdError> {
    let (context, records) = env.store.read(|conn| {
        let context = ContextRow::require(conn, &plan.context)?;
        let mut stmt = conn.prepare(&format!("SELECT {} FROM context_records WHERE context = ?1 ORDER BY seq", Record::COLUMNS))?;
        let records = stmt.query_map([plan.context.as_str()], Record::from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((context, records))
    })?;
    let mut message = vec![header(plan)];
    if let Some(brief) = brief {
        message.push(brief.trim_end().to_owned());
    }
    message.push(markdown(&context.title, context.jira.as_deref(), &records, since));
    write_file(&env.home.lead_message(&plan.id), &(message.join("\n\n") + "\n"))
}

fn header(plan: &PlanRow) -> String {
    format!(
        "You are the Lead of Plan {} \"{}\" in Context {}. Repo {}, base {}. Phase grounding: follow the cpd-lead skill.",
        plan.id, plan.title, plan.context, plan.repo, plan.base
    )
}

/// Step 4: the Lead's window `park-<plan>` (contract §6.2).
fn start_lead(env: &mut Env, plan: &PlanRow, session: &SessionId, name: &str) -> Result<lead::Started, CpdError> {
    let window = crate::host::layout::park_window(session, Some(&plan.id));
    let lead = Lead { session, context: &plan.context, plan: Some(&plan.id), name, cwd: Path::new(&plan.repo) };
    lead::start(env, &lead, &window, "plan new")
}

fn not_a_git_repo(path: &Path, why: &str) -> CpdError {
    CpdError::new(ErrorCode::NotAGitRepo, format!("{} is not inside a git repository: {why}", path.display()))
        .with("repo", path.display().to_string())
}

/// The realpath of the main worktree of the repo `path` is in
/// (`git rev-parse --git-common-dir`, then the first `git worktree list`
/// entry); `not_a_git_repo` when it is in none.
fn main_worktree(path: &Path) -> Result<PathBuf, CpdError> {
    let out = run(path, &["rev-parse", "--git-common-dir"])?;
    if out.code != 0 {
        return Err(not_a_git_repo(path, out.stderr.lines().next().unwrap_or("git rev-parse failed.")));
    }
    let main = worktrees(path)?.into_iter().next().ok_or_else(|| not_a_git_repo(path, "it has no worktree."))?;
    main.path.canonicalize().map_err(|e| not_a_git_repo(&main.path, &format!("{e}.")))
}

/// `--base`, else the repo's `origin` HEAD branch, else `main`; it must exist
/// as `refs/remotes/origin/<base>`, or `refs/heads/<base>` without `origin`.
fn resolve_base(repo: &Path, base: Option<&str>) -> Result<String, CpdError> {
    let has_origin = run(repo, &["remote", "get-url", "origin"])?.code == 0;
    let base = match base {
        Some(base) => base.to_owned(),
        None if has_origin => {
            let out = run(repo, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])?;
            let head = out.stdout.trim();
            match head.strip_prefix("origin/") {
                Some(branch) if out.code == 0 && !branch.is_empty() => branch.to_owned(),
                _ => "main".to_owned(),
            }
        }
        None => "main".to_owned(),
    };
    let reference = if has_origin { format!("refs/remotes/origin/{base}") } else { format!("refs/heads/{base}") };
    let out = run(repo, &["rev-parse", "--verify", "--quiet", &reference])?;
    if out.code != 0 {
        return Err(CpdError::new(ErrorCode::BaseNotFound, format!("The base {reference} does not exist in {}.", repo.display()))
            .with("base", base)
            .with("ref", reference)
            .with("repo", repo.display().to_string()));
    }
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ContextId;

    #[test]
    fn the_header_names_plan_context_repo_and_base() {
        let plan = PlanRow {
            id: PlanId::parse("pay-4521").unwrap(),
            context: ContextId::parse("payments").unwrap(),
            title: "Retry failed payouts".into(),
            jira: None,
            repo: "/r/pay".into(),
            base: "main".into(),
            lead_branch: "cpd/pay-4521".into(),
            lead_worktree: None,
            phase: super::super::Phase::Grounding,
            lead_session: SessionId::parse("s1").unwrap(),
            pr: None,
            pr_seen: json!({}),
            all_merged_at: None,
            created_at: "t".into(),
            locked_at: None,
            done_at: None,
            archived_at: None,
        };
        assert_eq!(
            header(&plan),
            "You are the Lead of Plan pay-4521 \"Retry failed payouts\" in Context payments. Repo /r/pay, base main. Phase grounding: follow the cpd-lead skill."
        );
    }
}
