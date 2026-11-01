//! Rule S (contract §7.4, hs-d243): the Lead worktree follows its Lead Branch,
//! in any order. Whoever moves a Lead Branch (merge step 9, R2, `pr rebase`)
//! and the PR's clean-up call [`sync_lead`]; it takes no lock of cpd's.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::run::{Out, run_with};
use super::{TAIL_LINES, tail};
use crate::clock::Clock;
use crate::error::{CpdError, ErrorCode};
use crate::host::runner::Runner;
use crate::ports::Ports;

/// How many rounds rule S runs before it gives up (contract §7.4).
pub const SYNC_ROUNDS: u32 = 20;

/// How a rule S that did not stop ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Synced {
    /// The index and the working tree hold the tree of the Lead Branch's tip.
    Done,
    /// [`SYNC_ROUNDS`] rounds found the index busy: nothing to report, the
    /// next mover finishes it.
    Busy,
}

/// What one round saw.
enum Round {
    Done,
    /// `index.lock` was held, or the index changed under the round: try again.
    Busy,
    /// `read-tree` moved the tree: look again.
    Moved,
}

/// Rule S on the Lead worktree `lead`, which has `branch` checked out
/// (contract §7.4): up to 20 rounds of `H = rev-parse HEAD`; working tree ≠
/// index → stop; `T = write-tree`; `T = H^{tree}` → done; `T` not the tree of
/// an entry of `branch`'s reflog → stop; `read-tree -m -u T H`.
///
/// A stop is `Err` with `lead_worktree_stale` (`worktree`, `stderr`, `fix`);
/// so is any git call that fails for a reason other than a held `index.lock`.
/// Nothing is overwritten on a stop. A round that meets a held `index.lock`
/// sleeps 100–500 ms. A "working tree ≠ index" verdict counts only when
/// `index.lock` was absent and the index file unchanged around it, so that a
/// concurrent rule S's half-written tree is never taken for local changes.
pub fn sync_lead(lead: &Path, branch: &str) -> Result<Synced, CpdError> {
    sync_lead_with(
        &*Ports::current().runner,
        &*Ports::current().clock,
        lead,
        branch,
    )
}

/// [`sync_lead`] on `runner`, napping on `clock` (a test passes a `FakeRunner` and a `ManualClock`).
pub fn sync_lead_with(
    runner: &dyn Runner,
    clock: &dyn Clock,
    lead: &Path,
    branch: &str,
) -> Result<Synced, CpdError> {
    let index = git_path(runner, lead, "index")?;
    for _ in 0..SYNC_ROUNDS {
        match round(runner, lead, branch, &index)? {
            Round::Done => return Ok(Synced::Done),
            Round::Moved => {}
            Round::Busy => pause(clock),
        }
    }
    Ok(Synced::Busy)
}

fn round(runner: &dyn Runner, lead: &Path, branch: &str, index: &Path) -> Result<Round, CpdError> {
    let run = |lead: &Path, args: &[&str]| run_with(runner, lead, args);
    let before = Probe::read(index);
    if before.locked {
        return Ok(Round::Busy);
    }
    let Some(head) = checked(
        runner,
        lead,
        &["rev-parse", "--verify", "HEAD"],
        status_fix(lead),
    )?
    else {
        return Ok(Round::Busy);
    };
    let head = head.trim().to_owned();

    // `--no-optional-locks`: the check writes no refreshed index, so it neither
    // changes the index under the probe nor takes `index.lock` from a mover.
    let diff =
        run(lead, &["--no-optional-locks", "diff", "--quiet"]).map_err(|e| from_error(lead, &e))?;
    match diff.code {
        0 => {}
        1 => {
            if Probe::read(index) != before {
                return Ok(Round::Busy);
            }
            return Err(stale(lead, "", local_changes_fix(lead)));
        }
        _ => return Err(stale(lead, &diff.stderr, local_changes_fix(lead))),
    }

    let Some(tree) = checked(runner, lead, &["write-tree"], status_fix(lead))? else {
        return Ok(Round::Busy);
    };
    let tree = tree.trim().to_owned();
    let Some(head_tree) = checked(
        runner,
        lead,
        &["rev-parse", "--verify", &format!("{head}^{{tree}}")],
        status_fix(lead),
    )?
    else {
        return Ok(Round::Busy);
    };
    if tree == head_tree.trim() {
        return Ok(Round::Done);
    }

    let reflog = run(
        lead,
        &["log", "-g", "--format=%T", &format!("refs/heads/{branch}")],
    )
    .map_err(|e| from_error(lead, &e))?;
    if reflog.code != 0 || !reflog.stdout.lines().any(|t| t.trim() == tree) {
        return Err(stale(lead, "", local_changes_fix(lead)));
    }

    let fix = format!(
        "Move away the files in the way, then run: git -C {} read-tree -m -u {tree} {head}",
        lead.display()
    );
    let Some(_) = checked(runner, lead, &["read-tree", "-m", "-u", &tree, &head], fix)? else {
        return Ok(Round::Busy);
    };
    Ok(Round::Moved)
}

/// `git -C <lead> <args>`: `Some(stdout)` on exit 0, `None` when it failed on a
/// held `index.lock`, else `lead_worktree_stale` with git's stderr and `fix`.
fn checked(
    runner: &dyn Runner,
    lead: &Path,
    args: &[&str],
    fix: String,
) -> Result<Option<String>, CpdError> {
    let out: Out = run_with(runner, lead, args).map_err(|e| from_error(lead, &e))?;
    if out.code == 0 {
        return Ok(Some(out.stdout));
    }
    if out.stderr.contains("index.lock") {
        return Ok(None);
    }
    Err(stale(lead, &out.stderr, fix))
}

/// The absolute path of `name` in the worktree's git dir (`index` lives in
/// `.git/worktrees/<id>/` for a linked worktree).
fn git_path(runner: &dyn Runner, lead: &Path, name: &str) -> Result<PathBuf, CpdError> {
    let out = run_with(
        runner,
        lead,
        &["rev-parse", "--path-format=absolute", "--git-path", name],
    )
    .map_err(|e| from_error(lead, &e))?;
    if out.code != 0 {
        return Err(stale(lead, &out.stderr, status_fix(lead)));
    }
    Ok(PathBuf::from(out.stdout.trim()))
}

/// What the index file looked like at one moment: whether `index.lock` was
/// held, and the index's identity (a rewrite renames a new file over it).
#[derive(Debug, PartialEq, Eq)]
struct Probe {
    locked: bool,
    index: Option<(u64, u64, i64, i64, u64)>,
}

impl Probe {
    fn read(index: &Path) -> Probe {
        let mut lock = index.as_os_str().to_owned();
        lock.push(".lock");
        let index = std::fs::metadata(index)
            .ok()
            .map(|m| (m.dev(), m.ino(), m.mtime(), m.mtime_nsec(), m.len()));
        Probe {
            locked: Path::new(&lock).exists(),
            index,
        }
    }
}

/// A random 100–500 ms (contract §7.4 step 6).
fn pause(clock: &dyn Clock) {
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() % 401);
    clock.sleep(Duration::from_millis(100 + u64::from(jitter)));
}

fn local_changes_fix(lead: &Path) -> String {
    format!(
        "The Lead worktree has changes of its own: inspect them with git -C {0} status; to keep only the Lead Branch, run: git -C {0} reset --hard HEAD",
        lead.display()
    )
}

fn status_fix(lead: &Path) -> String {
    format!(
        "Check the Lead worktree with git -C {} status",
        lead.display()
    )
}

/// A git that could not run (`git_failed` from [`run`]) is a stop too.
fn from_error(lead: &Path, e: &CpdError) -> CpdError {
    let stderr = e
        .details()
        .get("stderr")
        .and_then(|v| v.as_str())
        .unwrap_or(e.message())
        .to_owned();
    stale(lead, &stderr, status_fix(lead))
}

/// `lead_worktree_stale`: the Lead Branch moved but the Lead worktree `lead`
/// was not brought up to it (contract §7.4 rule S).
pub fn stale(lead: &Path, stderr: &str, fix: String) -> CpdError {
    CpdError::new(
        ErrorCode::LeadWorktreeStale,
        format!(
            "The Lead Branch moved but the Lead worktree {} was not updated to it.",
            lead.display()
        ),
    )
    .with("worktree", lead.display().to_string())
    .with("stderr", tail(stderr, TAIL_LINES))
    .with("fix", fix)
}
