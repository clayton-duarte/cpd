//! `plan delete` (commands.md §4.14; contract §2.2 "Deleted", hs-d527), and the pieces `context delete` reuses for each
//! of its Plans: what a delete finds ([`Inventory`], [`Targets`]), what refuses it ([`blockers`]), the rows it removes
//! ([`delete_rows`]) and the files, worktrees and branches it removes after the transaction ([`finish`]).
//!
//! The rows go in one transaction behind the tombstone (`plans.deleted_at`, set first: the delete triggers read it,
//! §4.4 "The delete exception"); the files go after it. The paths come from the live rows and from the `delete` rows
//! the triggers left in `history`, whichever exist, so a rerun after a crash needs no row ([`Targets::of_plan`]).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

use super::archive::{Objects, refuse_chat_slot};
use super::{DeleteArgs, Env};
use crate::error::CpdError;
use crate::events::{Counts, NewEvent, append};
use crate::git::run::{branch_exists, ok, run};
use crate::home::CpdHome;
use crate::host::Tmux;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::Output;
use crate::session::{Blocker, archive_blocked, blockers as live_blockers};

const COMMAND: &str = "plan delete";

/// A worktree and the repository it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Worktree {
    repo: Option<PathBuf>,
    path: PathBuf,
}

/// What a delete removes outside the database.
#[derive(Clone, Debug, Default)]
pub(crate) struct Targets {
    worktrees: Vec<Worktree>,
    /// `(repo, branch)`: local branches only.
    branches: Vec<(PathBuf, String)>,
    /// Session files.
    files: Vec<PathBuf>,
    /// Run directories.
    dirs: Vec<PathBuf>,
}

impl Targets {
    fn push_worktree(&mut self, repo: Option<String>, path: Option<String>) {
        if let Some(path) = path {
            let wt = Worktree {
                repo: repo.map(PathBuf::from),
                path: PathBuf::from(path),
            };
            if !self.worktrees.contains(&wt) {
                self.worktrees.push(wt);
            }
        }
    }

    fn push_branch(&mut self, repo: Option<String>, branch: Option<String>) {
        if let (Some(repo), Some(branch)) = (repo, branch) {
            let one = (PathBuf::from(repo), branch);
            if !self.branches.contains(&one) {
                self.branches.push(one);
            }
        }
    }

    fn push_file(&mut self, file: Option<String>) {
        if let Some(file) = file {
            let file = PathBuf::from(file);
            if !self.files.contains(&file) {
                self.files.push(file);
            }
        }
    }

    /// A directory of the run files, removed last.
    pub(crate) fn push_dir(&mut self, dir: PathBuf) {
        if !self.dirs.contains(&dir) {
            self.dirs.push(dir);
        }
    }

    /// Everything `other` has too.
    pub(crate) fn extend(&mut self, other: Targets) {
        for wt in other.worktrees {
            if !self.worktrees.contains(&wt) {
                self.worktrees.push(wt);
            }
        }
        for one in other.branches {
            if !self.branches.contains(&one) {
                self.branches.push(one);
            }
        }
        for file in other.files {
            if !self.files.contains(&file) {
                self.files.push(file);
            }
        }
        for dir in other.dirs {
            self.push_dir(dir);
        }
    }

    /// The files of the Context's Sessions that belong to no Plan (its Managers), from the live rows and from `history`.
    pub(crate) fn of_managers(conn: &Connection, context: &ContextId) -> Result<Targets, CpdError> {
        let mut targets = Targets::default();
        let mut stmt = conn.prepare(
            "SELECT file FROM sessions WHERE context = ?1 AND plan IS NULL
             UNION ALL
             SELECT json_extract(before, '$.file') FROM history
              WHERE tbl = 'sessions' AND op = 'delete' AND json_extract(before, '$.context') = ?1
                AND json_extract(before, '$.plan') IS NULL",
        )?;
        for file in stmt.query_map([context.as_str()], |r| r.get::<_, Option<String>>(0))? {
            targets.push_file(file?);
        }
        Ok(targets)
    }

    /// What `plan` has outside the database: its live rows' paths and those of its rows already deleted.
    pub(crate) fn of_plan(
        conn: &Connection,
        home: &CpdHome,
        plan: &PlanId,
    ) -> Result<Targets, CpdError> {
        let mut targets = Targets::default();
        let p = plan.as_str();
        // The PRs: their repository, Lead worktree and Lead Branch.
        let mut stmt = conn.prepare(
            "SELECT repo, lead_worktree, lead_branch FROM prs WHERE plan = ?1
             UNION ALL
             SELECT json_extract(before, '$.repo'), json_extract(before, '$.lead_worktree'), json_extract(before, '$.lead_branch')
               FROM history WHERE tbl = 'prs' AND op = 'delete' AND json_extract(pk, '$[0]') = ?1",
        )?;
        let prs = stmt
            .query_map([p], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (repo, worktree, branch) in prs {
            targets.push_worktree(Some(repo.clone()), worktree);
            targets.push_branch(Some(repo), branch);
        }
        // The Attempts: their worktree and branch, in the repository of their Job's PR.
        let mut stmt = conn.prepare(
            "SELECT p.repo, a.worktree, a.branch FROM attempts a
               JOIN jobs j ON j.plan = a.plan AND j.key = a.job JOIN prs p ON p.plan = j.plan AND p.key = j.pr
              WHERE a.plan = ?1
             UNION ALL
             SELECT p.repo, a.worktree, a.branch FROM
               (SELECT json_extract(before, '$.job') AS job, json_extract(before, '$.worktree') AS worktree,
                       json_extract(before, '$.branch') AS branch
                  FROM history WHERE tbl = 'attempts' AND op = 'delete' AND json_extract(pk, '$[0]') = ?1) a
               JOIN (SELECT json_extract(before, '$.key') AS key, json_extract(before, '$.pr') AS pr
                       FROM history WHERE tbl = 'jobs' AND op = 'delete' AND json_extract(before, '$.plan') = ?1) j ON j.key = a.job
               JOIN (SELECT json_extract(before, '$.key') AS key, json_extract(before, '$.repo') AS repo
                       FROM history WHERE tbl = 'prs' AND op = 'delete' AND json_extract(pk, '$[0]') = ?1) p ON p.key = j.pr",
        )?;
        let attempts = stmt
            .query_map([p], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (repo, worktree, branch) in attempts {
            targets.push_worktree(Some(repo.clone()), Some(worktree));
            targets.push_branch(Some(repo), Some(branch));
        }
        // The Sessions' files: the Lead's row stays, the others are in `history`.
        let mut stmt = conn.prepare(
            "SELECT file FROM sessions WHERE plan = ?1
             UNION ALL
             SELECT json_extract(before, '$.file') FROM history
              WHERE tbl = 'sessions' AND op = 'delete' AND json_extract(before, '$.plan') = ?1",
        )?;
        for file in stmt.query_map([p], |r| r.get::<_, Option<String>>(0))? {
            targets.push_file(file?);
        }
        targets.push_dir(home.plan_dir(plan));
        Ok(targets)
    }

    /// What of these is still on disk: worktrees, local branches, and files and directories.
    pub(crate) fn left(&self) -> Result<(u64, u64, u64), CpdError> {
        let worktrees = self.worktrees.iter().filter(|w| w.path.exists()).count() as u64;
        let mut branches = 0;
        for (repo, branch) in &self.branches {
            if repo.is_dir() && branch_exists(repo, branch)? {
                branches += 1;
            }
        }
        let files = self
            .files
            .iter()
            .chain(&self.dirs)
            .filter(|f| f.exists())
            .count() as u64;
        Ok((worktrees, branches, files))
    }

    /// `(worktrees, bytes)` that exist now: the count of worktrees and the size of them, the Session files and the run
    /// directories (the disk a delete frees).
    pub(crate) fn measure(&self) -> (u64, u64) {
        let worktrees: Vec<&Worktree> = self.worktrees.iter().filter(|w| w.path.exists()).collect();
        let bytes = worktrees.iter().map(|w| size(&w.path)).sum::<u64>()
            + self
                .files
                .iter()
                .chain(&self.dirs)
                .map(|f| size(f))
                .sum::<u64>();
        (worktrees.len() as u64, bytes)
    }
}

/// The bytes under `path` (its own size for a file), symbolic links not followed; 0 when it is gone.
fn size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().map(|e| size(&e.path())).sum::<u64>() + meta.len()
}

/// Megabytes, rounded to the nearest.
pub(crate) fn megabytes(bytes: u64) -> u64 {
    (bytes + (1 << 19)) >> 20
}

fn io_failed(path: &Path, e: std::io::Error) -> CpdError {
    CpdError::bad_row(format!("Cannot remove {}: {e}.", path.display()))
}

/// A path to remove must be absolute and not the root: a row that holds anything else is damaged, and is not obeyed.
fn absolute(path: &Path) -> Result<(), CpdError> {
    if path.is_absolute() && path.parent().is_some() {
        Ok(())
    } else {
        Err(CpdError::bad_row(format!(
            "{} is not an absolute path to remove.",
            path.display()
        )))
    }
}

fn remove_tree(path: &Path) -> Result<(), CpdError> {
    absolute(path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path).map_err(|e| io_failed(path, e)),
        Ok(_) => fs::remove_file(path).map_err(|e| io_failed(path, e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_failed(path, e)),
    }
}

/// What a [`finish`] removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Removed {
    pub worktrees: u64,
    pub disk_freed_mb: u64,
}

/// Steps 2 to 4 of a delete (commands.md §4.14): the Sessions' files, the worktrees and then the local branches, then the
/// run directories. What is already gone is skipped, so it can be rerun after a crash.
pub(crate) fn finish(targets: &Targets) -> Result<Removed, CpdError> {
    let (worktrees, bytes) = targets.measure();
    for file in &targets.files {
        remove_tree(file)?;
    }
    let mut repos: Vec<&Path> = Vec::new();
    for wt in &targets.worktrees {
        let repo = wt.repo.as_deref().filter(|r| r.is_dir());
        if wt.path.exists() {
            if let Some(repo) = repo {
                // Git's own removal first, so the repository forgets it; the directory is removed below whatever it says.
                let _ = run(
                    repo,
                    &[
                        "worktree",
                        "remove",
                        "--force",
                        "--force",
                        &wt.path.to_string_lossy(),
                    ],
                );
            }
            remove_tree(&wt.path)?;
        }
        if let Some(repo) = repo.filter(|r| !repos.contains(r)) {
            repos.push(repo);
        }
    }
    for repo in repos {
        let _ = run(repo, &["worktree", "prune"]);
    }
    for (repo, branch) in &targets.branches {
        if repo.is_dir() && branch_exists(repo, branch)? {
            ok(repo, &["branch", "-D", branch])?;
        }
    }
    for dir in &targets.dirs {
        remove_tree(dir)?;
    }
    Ok(Removed {
        worktrees,
        disk_freed_mb: megabytes(bytes),
    })
}

/// The rows a Plan delete counts (the Event's and the preview's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inventory {
    pub jobs: u64,
    pub attempts: u64,
    pub sessions: u64,
}

impl Inventory {
    pub(crate) fn of_plan(conn: &Connection, plan: &PlanId) -> Result<Inventory, CpdError> {
        let count = |table: &str| -> Result<u64, CpdError> {
            Ok(conn.query_row(
                &format!("SELECT count(*) FROM {table} WHERE plan = ?1"),
                [plan.as_str()],
                |r| r.get::<_, i64>(0),
            )? as u64)
        };
        Ok(Inventory {
            jobs: count("jobs")?,
            attempts: count("attempts")?,
            sessions: count("sessions")?,
        })
    }

    pub(crate) fn counts(self, worktrees: u64, bytes: u64) -> Counts {
        Counts {
            jobs: self.jobs,
            attempts: self.attempts,
            sessions: self.sessions,
            worktrees,
            disk_freed_mb: megabytes(bytes),
        }
    }
}

/// What stops a delete of `plans` (commands.md §4.14; contract §2.2 "Deleted"): a Plan that is neither archived nor
/// asleep (when `check_asleep`), and what is still live: the archive rule's blockers, an Attempt not `ended` and a PR that
/// is not `merged` or `cancelled`. `sessions` are Sessions of the delete that belong to no Plan (a Manager).
pub(crate) fn blockers(
    conn: &Connection,
    plans: &[PlanId],
    sessions: &[SessionId],
    check_asleep: bool,
) -> Result<Vec<Blocker>, CpdError> {
    let mut found = Vec::new();
    let mut ids = sessions.to_vec();
    for plan in plans {
        let mut stmt = conn.prepare("SELECT id FROM sessions WHERE plan = ?1")?;
        for id in stmt.query_map([plan.as_str()], |r| r.get::<_, String>(0))? {
            ids.push(SessionId::parse(&id?)?);
        }
        if check_asleep {
            let quiet: bool = conn.query_row(
                "SELECT p.archived_at IS NOT NULL OR s.asleep_at IS NOT NULL FROM plans p JOIN sessions s ON s.id = p.lead_session WHERE p.id = ?1",
                [plan.as_str()],
                |r| r.get(0),
            )?;
            if !quiet {
                found.push(Blocker {
                    kind: "plan",
                    id: plan.to_string(),
                    plan: Some(plan.to_string()),
                    reason: format!("Plan {plan} is neither archived nor asleep."),
                });
            }
        }
    }
    found.extend(live_blockers(conn, &ids, plans)?);
    for plan in plans {
        let mut stmt = conn
            .prepare("SELECT id FROM attempts WHERE plan = ?1 AND status <> 'ended' ORDER BY id")?;
        for id in stmt.query_map([plan.as_str()], |r| r.get::<_, String>(0))? {
            let id = id?;
            found.push(Blocker {
                kind: "attempt",
                id: id.clone(),
                plan: Some(plan.to_string()),
                reason: format!("Attempt {id} of Plan {plan} is not ended."),
            });
        }
        let mut stmt = conn.prepare("SELECT key, status FROM prs WHERE plan = ?1 AND status NOT IN ('merged', 'cancelled') ORDER BY key")?;
        for pr in stmt.query_map([plan.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (key, status) = pr?;
            found.push(Blocker {
                kind: "pr",
                id: key.clone(),
                plan: Some(plan.to_string()),
                reason: format!("PR {key} of Plan {plan} is {status}."),
            });
        }
    }
    Ok(found)
}

/// Sets `deleted_at` on the `plans` or `contexts` row `id`, the first write of a delete (§2.2 "Deleted").
pub(crate) fn tombstone(tx: &Connection, table: &str, id: &str) -> Result<(), CpdError> {
    tx.execute(
        &format!("UPDATE {table} SET deleted_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE id = ?1 AND deleted_at IS NULL"),
        [id],
    )?;
    Ok(())
}

/// A Plan's rows once it is tombstoned, in an order that keeps the foreign keys whole. The Lead's Session row stays
/// (`plans.lead_session` references it), as do the Context's records.
pub(crate) fn delete_rows(tx: &Connection, plan: &PlanId) -> Result<(), CpdError> {
    const STATEMENTS: [&str; 15] = [
        "DELETE FROM board_posts WHERE to_session IN (SELECT id FROM sessions WHERE plan = ?1)",
        "DELETE FROM inbox_items WHERE plan = ?1",
        "DELETE FROM events WHERE plan = ?1",
        "DELETE FROM escalations WHERE plan = ?1",
        "DELETE FROM job_addenda WHERE plan = ?1",
        "DELETE FROM attempts WHERE plan = ?1",
        "DELETE FROM dependencies WHERE plan = ?1",
        "DELETE FROM job_pools WHERE plan = ?1",
        "DELETE FROM jobs WHERE plan = ?1",
        "DELETE FROM pr_needs WHERE plan = ?1",
        "DELETE FROM pr_posts WHERE plan = ?1",
        "DELETE FROM jira_transitions WHERE plan = ?1",
        "DELETE FROM prs WHERE plan = ?1",
        "DELETE FROM plan_reviews WHERE plan = ?1",
        "DELETE FROM sessions WHERE plan = ?1 AND id <> (SELECT lead_session FROM plans WHERE id = ?1)",
    ];
    for sql in STATEMENTS {
        tx.execute(sql, [plan.as_str()])?;
    }
    Ok(())
}

/// A delete of what is already deleted (`"already": true`): it writes nothing to the store; with `yes` it finishes the
/// file steps that are left, else it says what is left.
pub(crate) fn rerun(targets: &Targets, yes: bool) -> Result<Output, CpdError> {
    if yes {
        let removed = finish(targets)?;
        return Ok(Output::success(json!({
            "preview": false, "already": true, "worktrees": removed.worktrees, "disk_freed_mb": removed.disk_freed_mb,
        })));
    }
    let (worktrees, branches, files) = targets.left()?;
    let left: Value = json!({"worktrees": worktrees, "branches": branches, "files": files});
    Ok(Output::success(
        json!({"preview": true, "already": true, "left": left}),
    ))
}

/// `plan delete` (§4.14).
pub(super) fn plan_delete(args: DeleteArgs) -> Result<Output, CpdError> {
    let id = args.id;
    let Env {
        home,
        config,
        mut store,
        ..
    } = Env::open()?;
    let deleted: Option<Option<String>> = store.read(|conn| {
        Ok(conn
            .query_row(
                "SELECT deleted_at FROM plans WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )
            .optional()?)
    })?;
    match deleted {
        None => Err(CpdError::plan_not_found(&id)),
        Some(Some(_)) => rerun(
            &store.read(|conn| Targets::of_plan(conn, &home, &id))?,
            args.yes,
        ),
        Some(None) => {
            let (context, objects, inventory, targets, live) = store.read(|conn| {
                let row = super::row::load(conn, &id)?;
                let live = blockers(conn, std::slice::from_ref(&id), &[], true)?;
                Ok((
                    row.context,
                    Objects::of_plan(conn, &id)?,
                    Inventory::of_plan(conn, &id)?,
                    Targets::of_plan(conn, &home, &id)?,
                    live,
                ))
            })?;
            if !live.is_empty() {
                return Err(archive_blocked(&live, None));
            }
            let tmux = Tmux::new(config.tmux.socket.clone());
            refuse_chat_slot(
                &mut store,
                &tmux,
                &config.tmux.cockpit_session,
                &objects.sessions,
            )?;
            let (worktrees, bytes) = targets.measure();
            let counts = inventory.counts(worktrees, bytes);
            let out = json!({
                "preview": !args.yes, "already": false, "jobs": counts.jobs, "attempts": counts.attempts,
                "sessions": counts.sessions, "worktrees": counts.worktrees, "disk_freed_mb": counts.disk_freed_mb,
            });
            if !args.yes {
                return Ok(Output::success(out));
            }
            store.write(COMMAND, |tx| {
                let live = blockers(tx, std::slice::from_ref(&id), &[], true)?;
                if !live.is_empty() {
                    return Err(archive_blocked(&live, None));
                }
                tombstone(tx, "plans", id.as_str())?;
                delete_rows(tx, &id)?;
                let text = format!(
                    "Plan {id} deleted: {} Jobs, {} Attempts, {} Sessions, {} worktrees, ~{} MB.",
                    counts.jobs,
                    counts.attempts,
                    counts.sessions,
                    counts.worktrees,
                    counts.disk_freed_mb
                );
                append(
                    tx,
                    NewEvent::plan_deleted(text, &counts)
                        .plan(id.clone())
                        .context(context.clone()),
                )?;
                Ok(())
            })?;
            // The paths again, now from `history`: where a rerun reads them too.
            finish(&store.read(|conn| Targets::of_plan(conn, &home, &id))?)?;
            Ok(Output::success(out))
        }
    }
}
