//! `cpd prune` (commands.md §26): the clean-up of leftover worktrees whose work reached `origin/main`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use clap::Args as ClapArgs;

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

#[cfg(test)]
mod lead_branch_tests;
#[cfg(test)]
mod tests;

/// `cpd prune [--apply] [--drop <lead-branch>]... <repo>...`
#[derive(ClapArgs, Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Do the removals; without it nothing changes and the command prints what it would do
    #[arg(long)]
    pub apply: bool,
    /// Remove the worktrees of this Lead Branch whether or not they are merged (repeatable)
    #[arg(long = "drop", value_name = "lead-branch")]
    pub drop: Vec<String>,
    /// The repos to clean
    #[arg(required = true, value_name = "repo")]
    pub repos: Vec<PathBuf>,
}

/// What a run printed and the exit code it ends with (commands.md §26: 0, 1 a command failed, 2 usage or the store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub exit: u8,
    pub stdout: String,
    pub stderr: String,
}

/// The deadline of one `git` call (a fetch included).
const GIT: Duration = Duration::from_secs(120);
/// The deadline of `gh pr list`.
const GH: Duration = Duration::from_secs(120);
/// The store's busy timeout, as the config default.
const BUSY_MS: u64 = 5000;
/// How many merged Workflows deep a chain of bases may go before it must reach main.
const MAX_LEVELS: usize = 5;

/// Prunes `args.repos` on `ports`, reading the protected set from the store of `home`.
pub fn run(ports: &Ports, home: &CpdHome, args: &Args) -> Outcome {
    let protected = match protected(ports, home) {
        Ok(p) => p,
        Err(e) => {
            return Outcome {
                exit: 2,
                stdout: String::new(),
                stderr: format!("failed: cannot read the protected set: {e}\n"),
            };
        }
    };
    let mut out = Outcome {
        exit: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    for repo in &args.repos {
        if let Err(e) = prune_repo(ports, args, &protected, repo, &mut out) {
            out.stderr
                .push_str(&format!("failed {}: {e}\n", repo.display()));
            out.exit = 1;
        }
    }
    out
}

/// The command of the binary: [`run`] on the real ports and `$CPD_HOME`.
pub fn command(args: Args) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let out = run(&Ports::current(), &home, &args);
    print!("{}", out.stdout);
    eprint!("{}", out.stderr);
    if out.exit != 0 {
        std::process::exit(i32::from(out.exit));
    }
    Ok(Output::Owned)
}

/// The branches cpd still runs: the Lead Branch of each unmerged Workflow of a Plan that is not done, and
/// `cpd/<plan>--<attempt>` for each Attempt of a Job of such a Workflow.
fn protected(ports: &Ports, home: &CpdHome) -> Result<BTreeSet<String>, CpdError> {
    let mut store = Store::open_with(&*ports.clock, home, BUSY_MS, None)?;
    store.read(|conn| {
        let mut set = BTreeSet::new();
        let mut leads = conn.prepare(
            "SELECT workflows.lead_branch FROM workflows JOIN plans ON plans.id = workflows.plan \
             WHERE plans.phase != 'done' AND workflows.status != 'merged'",
        )?;
        for row in leads.query_map([], |r| r.get::<_, Option<String>>(0))? {
            set.extend(row?);
        }
        let mut attempts = conn.prepare(
            "SELECT attempts.plan, attempts.id FROM attempts \
             JOIN jobs ON jobs.plan = attempts.plan AND jobs.key = attempts.job \
             JOIN workflows ON workflows.plan = jobs.plan AND workflows.key = jobs.workflow \
             JOIN plans ON plans.id = workflows.plan \
             WHERE plans.phase != 'done' AND workflows.status != 'merged'",
        )?;
        for row in
            attempts.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (plan, id) = row?;
            set.insert(format!("cpd/{plan}--{id}"));
        }
        Ok(set)
    })
}

/// A merged pull request of `gh pr list`.
struct Merged {
    number: u64,
    head: String,
    base: String,
    oid: String,
}

struct Worktree {
    path: PathBuf,
    head: String,
    branch: Option<String>,
}

fn git(ports: &Ports, dir: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    ports
        .runner
        .output(&mut cmd, "git", GIT)
        .map_err(|e| failure(&e))
}

/// `git` that must exit 0: its stdout.
fn git_ok(ports: &Ports, dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(ports, dir, args)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(text(&out.stderr))
    }
}

fn failure(e: &Failed) -> String {
    match e {
        Failed::Io(e) => e.to_string(),
        Failed::Timeout(e) => e.to_string(),
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_owned()
}

/// `owner/repo` of a remote URL.
fn slug(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let mut parts = url.rsplit(['/', ':']);
    let repo = parts.next().unwrap_or("");
    match parts.next() {
        Some(owner) if !owner.is_empty() => format!("{owner}/{repo}"),
        _ => url.to_owned(),
    }
}

fn merged_prs(ports: &Ports, slug: &str) -> Result<Vec<Merged>, String> {
    let mut cmd = Command::new("gh");
    cmd.args([
        "pr",
        "list",
        "--repo",
        slug,
        "--state",
        "merged",
        "--limit",
        "500",
        "--json",
        "number,headRefName,baseRefName,headRefOid",
    ]);
    let out = ports
        .runner
        .output(&mut cmd, "gh", GH)
        .map_err(|e| failure(&e))?;
    if !out.status.success() {
        return Err(text(&out.stderr));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("cannot read the output of gh pr list: {e}"))?;
    let list = json
        .as_array()
        .ok_or("cannot read the output of gh pr list: not a list")?;
    let field =
        |v: &serde_json::Value, k: &str| v.get(k).and_then(|s| s.as_str()).map(str::to_owned);
    list.iter()
        .map(|v| {
            Some(Merged {
                number: v.get("number")?.as_u64()?,
                head: field(v, "headRefName")?,
                base: field(v, "baseRefName")?,
                oid: field(v, "headRefOid")?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "cannot read the output of gh pr list: a pull request lacks a field".to_owned())
}

fn worktrees(ports: &Ports, repo: &Path) -> Result<Vec<Worktree>, String> {
    let listing = git_ok(ports, repo, &["worktree", "list", "--porcelain"])?;
    Ok(listing
        .split("\n\n")
        .skip(1)
        .filter(|block| !block.trim().is_empty())
        .map(|block| {
            let mut wt = Worktree {
                path: PathBuf::new(),
                head: String::new(),
                branch: None,
            };
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    wt.path = PathBuf::from(p);
                } else if let Some(h) = line.strip_prefix("HEAD ") {
                    wt.head = h.to_owned();
                } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                    wt.branch = Some(b.to_owned());
                }
            }
            wt
        })
        .collect())
}

/// True when `base` reaches main through merged Workflows.
fn reached(workflows: &[Merged], base: &str, level: usize) -> bool {
    base == "main" || (level < MAX_LEVELS && workflows.iter().any(|p| p.head == base && reached(workflows, &p.base, level + 1)))
}

/// True when a merged Workflow of the Lead Branch of `branch` contains `head` and reached main.
fn merged_via_pr(ports: &Ports, repo: &Path, workflows: &[Merged], branch: &str, head: &str, failed: &mut Vec<String>) -> bool {
    let lead = lead_of(branch);
    let attempt_plan = attempt_plan(branch);
    let legacy = lead
        .strip_prefix("cpd/")
        .and_then(|rest| rest.split_once('/'))
        .map(|(plan, _)| format!("cpd/{plan}"));
    let prefix = format!("{lead}/");
    let plan_prefix = attempt_plan.map(|plan| format!("{plan}/"));
    for workflow in workflows {
        let named = workflow.head == lead
            || workflow.head.starts_with(&prefix)
            || legacy.as_deref() == Some(workflow.head.as_str())
            || plan_prefix.as_deref().is_some_and(|p| workflow.head.starts_with(p));
        if !named || !reached(workflows, &workflow.base, 0) {
            continue;
        }
        let have = matches!(git(ports, repo, &["cat-file", "-e", &format!("{}^{{commit}}", workflow.oid)]), Ok(o) if o.status.success());
        if !have && !matches!(git(ports, repo, &["fetch", "-q", "origin", &format!("pull/{}/head", workflow.number)]), Ok(o) if o.status.success()) {
            failed.push(format!("cannot fetch pull/{}/head", workflow.number));
            continue;
        }
        if matches!(git(ports, repo, &["merge-base", "--is-ancestor", head, &workflow.oid]), Ok(o) if o.status.success()) {
            return true;
        }
    }
    false
}

/// The Plan of an Attempt branch `cpd/<plan>--<attempt>` (starts with `cpd/`, the rest has no `/`).
fn attempt_plan(branch: &str) -> Option<&str> {
    let rest = branch.strip_prefix("cpd/").filter(|r| !r.contains('/'))?;
    rest.split_once("--").map(|(plan, _)| plan)
}

/// The Lead Branch of `branch`: the branch itself, except an Attempt branch, whose Lead Branch is `cpd/<plan>`.
fn lead_of(branch: &str) -> &str {
    match attempt_plan(branch) {
        Some(plan) => &branch[..4 + plan.len()],
        None => branch,
    }
}

fn prune_repo(
    ports: &Ports,
    args: &Args,
    protected: &BTreeSet<String>,
    repo: &Path,
    out: &mut Outcome,
) -> Result<(), String> {
    let url = git_ok(ports, repo, &["remote", "get-url", "origin"])?;
    let slug = slug(&url);
    git_ok(ports, repo, &["fetch", "-q", "origin"])?;
    let workflows = merged_prs(ports, &slug)?;
    let list = worktrees(ports, repo)?;

    let mut failed = Vec::new();
    let (mut removes, mut keeps) = (0, 0);
    for wt in &list {
        let label = wt.branch.as_deref().unwrap_or("detached");
        let path = wt.path.display();
        let mut reason = None;
        let mut dropped = false;
        if wt.branch.as_ref().is_some_and(|b| protected.contains(b)) {
            reason = Some("active");
        }
        if reason.is_none() && wt.path.is_dir() {
            match git_ok(ports, &wt.path, &["status", "--porcelain"]) {
                Err(e) => {
                    out.stdout.push_str(&format!("failed {path}: {e}\n"));
                    failed.push(e);
                    continue;
                }
                Ok(s) if !s.trim().is_empty() => reason = Some("dirty"),
                Ok(_) => {}
            }
        }
        if reason.is_none() {
            let lead = wt.branch.as_deref().map(lead_of);
            if lead.is_some_and(|l| args.drop.iter().any(|d| d == l)) {
                dropped = true;
            } else if matches!(git(ports, repo, &["merge-base", "--is-ancestor", &wt.head, "origin/main"]), Ok(o) if o.status.success()) {
            } else if wt.branch.as_deref().is_some_and(|b| merged_via_pr(ports, repo, &workflows, b, &wt.head, &mut failed)) {
            } else {
                reason = Some("not merged");
            }
        }
        if let Some(reason) = reason {
            out.stdout
                .push_str(&format!("keep {path} ({label}): {reason}\n"));
            keeps += 1;
            continue;
        }
        out.stdout.push_str(&format!(
            "remove {path} ({label}){}\n",
            if dropped { ": dropped" } else { "" }
        ));
        removes += 1;
        if !args.apply {
            continue;
        }
        if let Err(e) = git_ok(
            ports,
            repo,
            &["worktree", "remove", "--force", &wt.path.to_string_lossy()],
        ) {
            out.stdout.push_str(&format!("failed {path}: {e}\n"));
            failed.push(e);
            continue;
        }
        if let Some(branch) = &wt.branch {
            let exists = matches!(git(ports, repo, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")]), Ok(o) if o.status.success());
            let line = format!("branch refs/heads/{branch}");
            let used = worktrees_all(ports, repo)?.lines().any(|l| l == line);
            if exists
                && !used
                && let Err(e) = git_ok(ports, repo, &["branch", "-D", branch])
            {
                out.stdout.push_str(&format!("failed {path}: {e}\n"));
                failed.push(e);
            }
        }
    }
    if args.apply {
        git_ok(ports, repo, &["worktree", "prune"]).map_err(|e| e.to_string())?;
    }
    out.stdout.push_str(&format!(
        "{}: {removes} remove, {keeps} keep\n",
        repo.display()
    ));
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

fn worktrees_all(ports: &Ports, repo: &Path) -> Result<String, String> {
    git_ok(ports, repo, &["worktree", "list", "--porcelain"])
}
