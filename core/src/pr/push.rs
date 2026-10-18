//! `pr push` (commands.md §9.9, contract §7.6): the Gate, then a plain push of
//! the Lead Branch. No rebase, never forced.

use serde_json::json;

use super::row::{PlanRow, PrRow};
use super::{Args, Env, gate, illegal_phase, lock, require_clean};
use crate::error::{CpdError, ErrorCode};
use crate::git::{self, TAIL_LINES, tail};
use crate::output::Output;
use crate::plan::Phase;

const COMMAND: &str = "pr push";

pub(super) fn push(args: Args) -> Result<Output, CpdError> {
    let id = args.plan.require()?.clone();
    let mut env = Env::open()?;

    let plan = env.plan(&id)?;
    let key = require_open(&plan, &args)?.key.clone();

    // Under the per-repo lock, after R, so that no merge interleaves and none is unrecorded.
    let lock = lock(&mut env, require_open(&plan, &args)?, COMMAND)?;
    let plan = env.plan(&id)?;
    let pr = require_open(&plan, &args)?;
    let lead = pr.lead(&id)?.clone();
    require_clean(&id, pr, &lead)?;
    gate(&env, &id, pr, &lead, &format!("pr push --pr {key}"))?;

    let branch = pr.lead_branch.as_str();
    let out = git::run::run(&lead, &["push", "--porcelain", "origin", branch])?;
    let pushed = if out.code == 0 {
        status_flag(&out.stdout) != Some('=')
    } else if remote_moved(&out.stdout) {
        return Err(CpdError::new(
            ErrorCode::PushRejected,
            format!("The remote {branch} is not an ancestor of the local one, so it moved: nothing was pushed. Run cpd pr rebase --pr {key}."),
        )
        .with("plan", id.as_str())
        .with("pr", key.as_str())
        .with("stderr", tail(&out.stderr, TAIL_LINES)));
    } else {
        return Err(CpdError::new(ErrorCode::GitFailed, format!("git push exited {}.", out.code))
            .with("argv", json!(["git", "-C", lead.display().to_string(), "push", "--porcelain", "origin", branch]))
            .with("exit", out.code)
            .with("stderr", tail(&out.stderr, TAIL_LINES)));
    };
    let sha = git::run::rev_parse(&lead, &format!("refs/heads/{branch}"))?;
    drop(lock);

    Ok(Output::success(json!({"plan": id, "pr": key, "pushed": pushed, "sha": sha})))
}

/// The porcelain flag of the pushed ref: `=` up to date, ` ` fast-forward, `*` new, `!` rejected.
/// Each ref's line is `<flag>\t<from>:<to>\t<summary>`.
fn status_flag(stdout: &str) -> Option<char> {
    ref_line(stdout).and_then(|l| l.chars().next())
}

/// Whether git refused the ref because the remote has commits the branch lacks:
/// `[rejected]` as `non-fast-forward` or `fetch first`.
fn remote_moved(stdout: &str) -> bool {
    ref_line(stdout).is_some_and(|l| l.starts_with('!') && l.contains("[rejected]") && (l.contains("(non-fast-forward)") || l.contains("(fetch first)")))
}

fn ref_line(stdout: &str) -> Option<&str> {
    stdout.lines().find(|l| l.chars().nth(1) == Some('\t'))
}

/// The selected PR must be `open` (`illegal_transition`); a Plan that has not
/// been locked is `illegal_phase`.
fn require_open<'a>(plan: &'a PlanRow, args: &Args) -> Result<&'a PrRow, CpdError> {
    if matches!(plan.phase, Phase::Grounding | Phase::Plan) {
        return Err(illegal_phase(plan, COMMAND, "deploy"));
    }
    let pr = plan.select(&args.pr)?;
    if pr.status != "open" {
        return Err(pr.refuse(&plan.id, COMMAND, "open"));
    }
    Ok(pr)
}
