//! Repo-declared exclusive resources: the lock files and the holder record (contract §1.4, §8.3 "The `resources`
//! domain") and the commands `cpd resource list` and `cpd resource run` (commands.md §30).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{OwnerLogin, PlanId, RepoName};
use crate::lock;
use crate::policy::{Declared, ResourceScope};

pub mod cmd;
pub use cmd::{Command, run};

#[cfg(test)]
mod tests;

/// The reserved name of the built-in resource: the host's Worker slots (contract §8.3 "The built-in resource `workers`").
pub const WORKERS: &str = "workers";

/// The built-in resource `workers` as `cpd resource list` and `job show` read it: a host pool of `concurrency` slots, whose
/// tenants are the Jobs of the `job_tenants` view (contract §3.4 "Tenancy"). It has no lock file: the Ledger is the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workers {
    /// The config's `concurrency`.
    pub slots: u32,
    /// One record per tenant Job, `{by: attempt}` of its latest Attempt, in start order (`started_at`, then Attempt id).
    /// More than `slots` while an urgent Job holds the slot beyond.
    pub holders: Vec<Holder>,
}

/// The Job tenants of the pool whose identity text is `pool` ([`identity`], or [`WORKERS`]), each as the holder of its latest Attempt,
/// in start order. The one query every reader of the tenancy shares (E3, the starter, `resource list`, `job show`, `@cpd_workers`).
pub fn tenants(conn: &rusqlite::Connection, pool: &str) -> Result<Vec<Holder>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT a.plan, a.job, a.id, a.started_at FROM job_tenants t
           JOIN attempts a ON a.plan = t.plan AND a.job = t.job
            AND a.number = (SELECT max(number) FROM attempts WHERE plan = t.plan AND job = t.job)
          WHERE t.pool = ?1 ORDER BY a.started_at, a.id",
    )?;
    let holders = stmt
        .query_map([pool], |r| {
            Ok(Holder::Attempt {
                plan: r.get(0)?,
                job: r.get(1)?,
                attempt: r.get(2)?,
                at: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(holders)
}

/// The Worker slots now: `concurrency` slots, the tenant Jobs of `workers` as holders.
pub fn workers(conn: &rusqlite::Connection, concurrency: u32) -> Result<Workers, CpdError> {
    Ok(Workers {
        slots: concurrency,
        holders: tenants(conn, WORKERS)?,
    })
}

impl Workers {
    /// The pool E3 schedules on: `free` = slots no tenant holds, `urgent_free` = the one beyond, while the tenants are at most `slots`.
    pub fn pool(&self) -> crate::driver::schedule::Pool {
        let active = u32::try_from(self.holders.len()).unwrap_or(u32::MAX);
        crate::driver::schedule::Pool {
            free: self.slots.saturating_sub(active),
            urgent_free: u32::from(active <= self.slots),
        }
    }
}

/// The holder record of a held resource (contract §1.4): what the taker wrote into the lock file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "lowercase")]
pub enum Holder {
    /// The starter of an Attempt whose Job lists the resource.
    Attempt {
        plan: String,
        job: String,
        attempt: String,
        at: String,
    },
    /// `cpd resource run`.
    Command {
        pid: u32,
        command: String,
        session: Option<String>,
        attempt: Option<String>,
        at: String,
    },
    /// A held lock whose record is empty or does not parse.
    Unknown,
}

/// A pool of slots: the level that declares the resource owns it (contract §8.3, D2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pool {
    /// One pool for every repo of the machine.
    Host { name: String, slots: u8 },
    /// One pool for every repo of the owner.
    Owner {
        owner: OwnerLogin,
        name: String,
        slots: u8,
    },
    /// One pool per repo.
    Repo {
        owner: OwnerLogin,
        repo: RepoName,
        name: String,
        slots: u8,
    },
}

impl Pool {
    /// The pool of the resource `declared` for the repo `owner/repo`: the declaring level owns it.
    pub fn of(declared: &Declared, owner: &OwnerLogin, repo: &RepoName) -> Pool {
        let name = declared.name.clone();
        let slots = declared.slots;
        match declared.scope {
            ResourceScope::Host => Pool::Host { name, slots },
            ResourceScope::Owner => Pool::Owner {
                owner: owner.clone(),
                name,
                slots,
            },
            ResourceScope::Repo => Pool::Repo {
                owner: owner.clone(),
                repo: repo.clone(),
                name,
                slots,
            },
        }
    }

    /// The declaring level, which owns the pool.
    pub fn scope(&self) -> ResourceScope {
        match self {
            Pool::Host { .. } => ResourceScope::Host,
            Pool::Owner { .. } => ResourceScope::Owner,
            Pool::Repo { .. } => ResourceScope::Repo,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Pool::Host { name, .. } | Pool::Owner { name, .. } | Pool::Repo { name, .. } => name,
        }
    }

    pub fn slots(&self) -> u8 {
        match self {
            Pool::Host { slots, .. } | Pool::Owner { slots, .. } | Pool::Repo { slots, .. } => {
                *slots
            }
        }
    }
}

/// The pools of the resources `resources` declares for the repo `owner/repo`, in name order.
pub fn pools_of(
    resources: &crate::policy::Resources,
    owner: &OwnerLogin,
    repo: &RepoName,
) -> Vec<Pool> {
    resources
        .declared
        .iter()
        .map(|d| Pool::of(d, owner, repo))
        .collect()
}

/// The text that names `pool` in `job_pools.pool` (contract §3.4 "Resources of a Job" item 3): `host:<name>`, `owner:<login>:<name>`
/// or `repo:<owner>/<repo>:<name>`. Injective: the first field names the level, and a login, a repo name and a resource name
/// contain no `:` while a repo name contains no `/`.
pub fn identity(pool: &Pool) -> String {
    match pool {
        Pool::Host { name, .. } => format!("host:{name}"),
        Pool::Owner { owner, name, .. } => format!("owner:{owner}:{name}"),
        Pool::Repo {
            owner, repo, name, ..
        } => format!("repo:{owner}/{repo}:{name}"),
    }
}

/// The free slots of `pool` (contract §1.4 "The free slots of a pool"): its `slots` minus the Job tenants of the pool (the rows of
/// the view `job_tenants` whose `pool` is its [`identity`]) minus the held command slot files of the pool, never below 0. The
/// starter and `resource run` both decide with it, under the pool's [`Guard`].
pub fn free_slots(
    conn: &rusqlite::Connection,
    home: &CpdHome,
    pool: &Pool,
) -> Result<u32, CpdError> {
    let jobs = tenants(conn, &identity(pool))?.len();
    let commands = holders(home, pool)?.iter().filter(|h| h.is_some()).count();
    let taken = u32::try_from(jobs + commands).unwrap_or(u32::MAX);
    Ok(u32::from(pool.slots()).saturating_sub(taken))
}

/// The guard file of `pool` (contract §1.4): `resource-guard.<scope>.<owner>.<repo>.<name>.lock`, only the parts the scope has.
fn guard_path(home: &CpdHome, pool: &Pool) -> PathBuf {
    let file = match pool {
        Pool::Host { name, .. } => format!("resource-guard.host.{name}.lock"),
        Pool::Owner { owner, name, .. } => format!("resource-guard.owner.{owner}.{name}.lock"),
        Pool::Repo {
            owner, repo, name, ..
        } => format!("resource-guard.repo.{owner}.{repo}.{name}.lock"),
    };
    home.locks_dir().join(file)
}

/// The exclusive guard of one pool, held while a starter or `resource run` decides the free slots and takes one. The file exists
/// only while held: its taker removes it before the lock drops.
#[derive(Debug)]
pub struct Guard {
    path: PathBuf,
    _file: File,
}

impl Guard {
    /// Waits for the guard of `pool`, which another taker holds only for an instant. After the lock the taker checks that its
    /// descriptor and the path are the same file (device and inode), else the file was removed under it and it takes again.
    pub fn take(home: &CpdHome, pool: &Pool) -> Result<Guard, CpdError> {
        use std::os::unix::fs::MetadataExt;
        let path = guard_path(home, pool);
        fs::create_dir_all(home.locks_dir()).map_err(|e| lock_failed(&home.locks_dir(), e))?;
        loop {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .map_err(|e| lock_failed(&path, e))?;
            lock::blocking(&file).map_err(|e| lock_failed(&path, e))?;
            let same = match (file.metadata(), fs::metadata(&path)) {
                (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
                _ => false,
            };
            if same {
                return Ok(Guard { path, _file: file });
            }
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        // Removed while the lock is still held; the lock drops with the descriptor.
        let _ = fs::remove_file(&self.path);
    }
}

/// The Job tenants and held command slots of `pool` as one list for `resource list` and `resource_busy`: the held command slots
/// first, each with its slot number, then the Job tenants with none.
pub fn occupants(
    conn: &rusqlite::Connection,
    home: &CpdHome,
    pool: &Pool,
) -> Result<Vec<(Option<usize>, Holder)>, CpdError> {
    let mut out: Vec<(Option<usize>, Holder)> = holders(home, pool)?
        .into_iter()
        .enumerate()
        .filter_map(|(i, h)| h.map(|h| (Some(i + 1), h)))
        .collect();
    out.extend(
        tenants(conn, &identity(pool))?
            .into_iter()
            .map(|h| (None, h)),
    );
    Ok(out)
}

/// The slots taken by one [`take`], one per pool: each lock stays held until this is dropped (or the descriptor is handed on).
#[derive(Debug)]
pub struct Held {
    files: Vec<(String, u8, File)>,
}

impl Held {
    /// The names held, in name order.
    pub fn names(&self) -> Vec<&str> {
        self.files.iter().map(|(n, _, _)| n.as_str()).collect()
    }

    /// The slot taken of the pool `name`, if it is one of these.
    pub fn slot(&self, name: &str) -> Option<u8> {
        self.files
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, slot, _)| *slot)
    }

    /// The descriptor of the lock of `name`, if it is one of these.
    pub fn fd(&self, name: &str) -> Option<RawFd> {
        self.files
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, _, f)| f.as_raw_fd())
    }
}

/// The outcome of [`take`]: a slot of each pool, or none and the first pool found full with the holders of its slots.
#[derive(Debug)]
pub enum Take {
    Held(Held),
    Busy {
        resource: String,
        holders: Vec<Option<Holder>>,
    },
}

/// The lock file of slot `slot` (from 1) of `pool` (contract §1.4): `resource.<scope>.<owner>.<repo>.<name>.<slot>.lock`
/// (only the parts the scope has). Injective: the scope word comes first, and an owner login, a resource name and a slot
/// never contain a dot, so the owner is the first part after the scope, the slot and the name the last two, and the repo
/// (which may contain dots) is what lies between.
pub fn lock_path(home: &CpdHome, pool: &Pool, slot: u8) -> PathBuf {
    let file = match pool {
        Pool::Host { name, .. } => format!("resource.host.{name}.{slot}.lock"),
        Pool::Owner { owner, name, .. } => format!("resource.owner.{owner}.{name}.{slot}.lock"),
        Pool::Repo {
            owner, repo, name, ..
        } => format!("resource.repo.{owner}.{repo}.{name}.{slot}.lock"),
    };
    home.locks_dir().join(file)
}

fn lock_failed(path: &std::path::Path, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::StoreFailed,
        format!("Cannot use the resource lock {}: {e}.", path.display()),
    )
    .with("lock", path.display().to_string())
}

/// The holder a record in a held lock's file names; [`Holder::Unknown`] when it is empty or does not parse.
fn parse(text: &str) -> Holder {
    serde_json::from_str(text).unwrap_or(Holder::Unknown)
}

/// The holder the record in the held lock file at `path` names.
fn record_of(path: &std::path::Path) -> Holder {
    parse(&fs::read_to_string(path).unwrap_or_default())
}

/// True for a command holder whose pid (within 1..=i32::MAX) no longer exists: its slot lock is held only by a leaked fd.
fn dead_command(holder: &Holder) -> bool {
    let Holder::Command { pid, .. } = holder else {
        return false;
    };
    i32::try_from(*pid)
        .ok()
        .filter(|p| *p > 0)
        .is_some_and(|p| {
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), None)
                == Err(nix::errno::Errno::ESRCH)
        })
}

/// Takes the lowest free slot of every pool of `pools` (in name order, all or none) by a non-blocking exclusive try-lock,
/// then writes `holder` into each lock file.
pub fn take(home: &CpdHome, pools: &[Pool], holder: &Holder) -> Result<Take, CpdError> {
    let mut sorted: Vec<&Pool> = pools.iter().collect();
    sorted.sort_by(|a, b| a.name().cmp(b.name()));
    sorted.dedup_by(|a, b| a.name() == b.name());
    fs::create_dir_all(home.locks_dir()).map_err(|e| lock_failed(&home.locks_dir(), e))?;
    let mut files = Vec::new();
    for pool in sorted {
        let mut taken = None;
        for slot in 1..=pool.slots() {
            let path = lock_path(home, pool, slot);
            let open = || {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&path)
                    .map_err(|e| lock_failed(&path, e))
            };
            let mut file = open()?;
            if !lock::try_lock(&file).map_err(|e| lock_failed(&path, e))? {
                // Held: a command whose pid is dead only has its lock kept by a leaked fd; take a fresh inode.
                if !dead_command(&record_of(&path)) {
                    continue;
                }
                let _ = fs::remove_file(&path);
                file = open()?;
                if !lock::try_lock(&file).map_err(|e| lock_failed(&path, e))? {
                    continue;
                }
            }
            taken = Some((slot, file));
            break;
        }
        let Some((slot, file)) = taken else {
            return Ok(Take::Busy {
                resource: pool.name().to_owned(),
                holders: holders(home, pool)?,
            });
        };
        files.push((pool.name().to_owned(), slot, file, pool));
    }
    let record = serde_json::to_string(holder).expect("a holder serialises");
    for (_, slot, file, pool) in &mut files {
        let path = lock_path(home, pool, *slot);
        file.set_len(0)
            .and_then(|()| file.write_all(record.as_bytes()))
            .map_err(|e| lock_failed(&path, e))?;
    }
    Ok(Take::Held(Held {
        files: files.into_iter().map(|(n, s, f, _)| (n, s, f)).collect(),
    }))
}

/// The resources a Job's Spec lists and the repo they belong to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    /// The Job's Workflow.
    pub workflow: String,
    pub owner: OwnerLogin,
    pub repo: RepoName,
    /// The names, sorted.
    pub names: Vec<String>,
}

/// The resources Job `key` of `plan` lists in its Spec, with its Workflow's repo; `None` when it lists none.
pub fn wanted(
    conn: &rusqlite::Connection,
    plan: &PlanId,
    key: &str,
) -> Result<Option<Wanted>, CpdError> {
    use rusqlite::OptionalExtension;
    // The row's columns: workflow, the listed resources JSON, owner, repo.
    type Found = (String, Option<String>, Option<String>, Option<String>);
    // An explore Job has no Workflow (the join finds no row): it lists no resources.
    let found: Option<Found> = conn.query_row(
        "SELECT j.workflow, j.resources, c.owner, r.name FROM jobs j JOIN workflows r ON r.plan = j.plan AND r.key = j.workflow
           JOIN plans p ON p.id = j.plan JOIN contexts c ON c.id = p.context WHERE j.plan = ?1 AND j.key = ?2",
        [plan.as_str(), key],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).optional()?;
    let Some((workflow, listed, owner, repo)) = found else {
        return Ok(None);
    };
    let Some(listed) = listed else {
        return Ok(None);
    };
    let mut names: Vec<String> = serde_json::from_str(&listed).map_err(|_| {
        CpdError::bad_row(format!("Job {key} of Plan {plan} has resources {listed:?}"))
    })?;
    names.sort();
    let (Some(owner), Some(repo)) = (owner, repo) else {
        return Err(CpdError::bad_row(format!(
            "Job {key} of Plan {plan} lists resources but its Workflow {workflow} has no repo"
        )));
    };
    Ok(Some(Wanted {
        workflow,
        owner: OwnerLogin::parse(&owner)?,
        repo: RepoName::parse(&repo)?,
        names,
    }))
}

/// The holders of the slots of `pool`, in slot order: `None` for a free slot (whatever its file still holds)
/// or one held only by a command whose pid is dead (its file is unlinked), else its record.
pub fn holders(home: &CpdHome, pool: &Pool) -> Result<Vec<Option<Holder>>, CpdError> {
    (1..=pool.slots())
        .map(|slot| holder_of_slot(home, pool, slot))
        .collect()
}

fn holder_of_slot(home: &CpdHome, pool: &Pool, slot: u8) -> Result<Option<Holder>, CpdError> {
    let path = lock_path(home, pool, slot);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(lock_failed(&path, e)),
    };
    // A shared try-lock, released at once (dropped): refused only while an exclusive one is held.
    if lock::shared(&file).map_err(|e| lock_failed(&path, e))? {
        return Ok(None);
    }
    let holder = record_of(&path);
    if dead_command(&holder) {
        let _ = fs::remove_file(&path);
        return Ok(None);
    }
    Ok(Some(holder))
}
