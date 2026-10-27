//! `cpd resource list` and `cpd resource run` (commands.md §30).

use serde_json::json;

use std::os::fd::{BorrowedFd, RawFd};
use std::path::PathBuf;
use std::time::Duration;

use nix::fcntl::{FcntlArg, FdFlag, fcntl};

use super::{Guard, Holder, Pool, Take, free_slots, occupants, take, tenants};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::spawn::Launch;
use crate::ids::{OwnerLogin, RepoName};
use crate::output::Output;
use crate::policy::{PolicyKey, repo_key, repo_path, resolve};
use crate::ports::Ports;

/// The `resource` commands (commands.md §30).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// The repo's declared resources and who holds them (§30.1)
    List,
    /// Take a resource and run a command holding it (§30.2)
    Run(RunArgs),
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// A resource the repo's policy declares
    pub name: String,
    /// Try again every second for this many minutes [default: 0]
    #[arg(long, value_name = "minutes")]
    pub wait: Option<u64>,
    /// The command and its arguments, after `--`
    #[arg(last = true, value_name = "command")]
    pub command: Vec<String>,
}

/// The repo of the current directory and its declared resource names, sorted.
struct Repo {
    owner: OwnerLogin,
    name: RepoName,
    declared: Vec<String>,
    pools: Vec<Pool>,
}

impl Repo {
    fn pool(&self, name: &str) -> &Pool {
        self.pools.iter().find(|p| p.name() == name).expect("a declared resource has a pool")
    }

    fn label(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

fn usage(message: impl Into<String>) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

fn here(ports: &Ports, home: &CpdHome) -> Result<(PathBuf, Repo), CpdError> {
    let cwd = std::env::current_dir().map_err(|e| usage(format!("Cannot read the current directory: {e}.")))?;
    let top = crate::git::run::run_with(&*ports.runner, &cwd, &["rev-parse", "--git-common-dir"])?;
    if top.code != 0 {
        return Err(usage(format!("cpd resource: {} is not in a git worktree.", cwd.display())));
    }
    let (owner, name) = repo_key(&cwd)?;
    let resources = resolve(home, &PolicyKey::Repo(owner.clone(), name.clone()))?.policy.resources;
    let declared = resources.names.clone();
    let pools = super::pools_of(&resources, &owner, &name);
    Ok((cwd, Repo { owner, name, declared, pools }))
}

pub fn run(ports: &Ports, command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List => list(ports),
        Command::Run(args) => run_command(ports, args),
    }
}

fn list(ports: &Ports) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let (_, repo) = here(ports, &home)?;
    let mut env = crate::env::Env::open_home_with(ports, home.clone())?;
    let concurrency = env.config.concurrency;
    let workers = env.store.read(|conn| super::workers(conn, concurrency))?;
    let mut resources = Vec::new();
    for name in &repo.declared {
        let pool = repo.pool(name);
        let held = env.store.read(|conn| occupants(conn, &home, pool))?;
        let free = u32::from(pool.slots()).saturating_sub(u32::try_from(held.len()).unwrap_or(u32::MAX));
        let holders: Vec<_> = held.into_iter().map(|(slot, holder)| json!({"slot": slot, "holder": holder})).collect();
        resources.push(json!({"name": name, "scope": pool.scope(), "slots": pool.slots(), "free": free, "holders": holders}));
    }
    // The built-in `workers`: host scope, one holder per tenant Job, in the name order with the declared ones.
    let held: Vec<_> = workers.holders.iter().enumerate().map(|(i, h)| json!({"slot": i + 1, "holder": h})).collect();
    let free = workers.slots.saturating_sub(u32::try_from(workers.holders.len()).unwrap_or(u32::MAX));
    let entry = json!({"name": super::WORKERS, "scope": "host", "slots": workers.slots, "free": free, "holders": held});
    let at = resources.iter().position(|r| r["name"].as_str().is_some_and(|n| n > super::WORKERS)).unwrap_or(resources.len());
    resources.insert(at, entry);
    Ok(Output::success(json!({"repo": repo.label(), "resources": resources})))
}

fn run_command(ports: &Ports, args: RunArgs) -> Result<Output, CpdError> {
    if args.command.is_empty() {
        return Err(usage("cpd resource run: a command must follow `--`."));
    }
    if args.wait == Some(0) {
        return Err(usage("cpd resource run: --wait is a positive number of minutes."));
    }
    if args.name == super::WORKERS {
        return Err(usage("cpd resource run: `workers` is the built-in resource, taken only by the starter of an Attempt."));
    }
    let home = CpdHome::from_env()?;
    let (cwd, repo) = here(ports, &home)?;
    let mut env = crate::env::Env::open_home_with(ports, home.clone())?;
    if !repo.declared.contains(&args.name) {
        let file = repo_path(&home, &repo.owner, &repo.name);
        return Err(CpdError::new(
            ErrorCode::ResourceUndeclared,
            format!(
                "The resource {} is not declared by the policy of {} ({}). Declare it as [resources.{}] in {}.",
                args.name,
                repo.label(),
                if repo.declared.is_empty() { "none are declared".to_owned() } else { repo.declared.join(", ") },
                args.name,
                file.display()
            ),
        )
        .with("resource", args.name.as_str())
        .with("repo", repo.label())
        .with("declared", repo.declared.clone())
        .with("file", file.display().to_string()));
    }
    let launch = Launch {
        program: args.command[0].clone(),
        args: args.command[1..].to_vec(),
        cwd: Some(cwd),
        ..Launch::default()
    };
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let (session, attempt) = (var("CPD_SESSION"), var("CPD_ATTEMPT"));

    // An Attempt whose Job is a tenant of the pool takes nothing more (commands.md §30.2): the Job holds a slot already.
    if let Some(id) = &attempt {
        let identity = super::identity(repo.pool(&args.name));
        let own = env.store.read(|conn| Ok(tenants(conn, &identity)?.iter().any(|h| matches!(h, Holder::Attempt { attempt, .. } if attempt == id))))?;
        if own {
            let e = ports.spawn.exec(&launch);
            return Err(spawn_failed(&launch, &args.name, e));
        }
    }

    let holder = Holder::Command {
        pid: std::process::id(),
        command: args.command.join(" "),
        session,
        attempt,
        at: crate::clock::timestamp(ports.clock.utc()),
    };
    let minutes = args.wait.unwrap_or(0);
    let mut tries_left = minutes * 60;
    let pool = repo.pool(&args.name);
    let held = loop {
        // The free count and the take are one step under the pool's guard, as the starter's (contract §1.4).
        let taken = {
            let _guard = Guard::take(&home, pool)?;
            if env.store.read(|conn| free_slots(conn, &home, pool))? > 0 {
                match take(&home, std::slice::from_ref(pool), &holder)? {
                    Take::Held(held) => Some(held),
                    Take::Busy { .. } => None,
                }
            } else {
                None
            }
        };
        match taken {
            Some(held) => break held,
            None => {
                if tries_left == 0 {
                    let holders = env.store.read(|conn| occupants(conn, &home, pool))?;
                    let holders: Vec<_> = holders.into_iter().map(|(slot, holder)| json!({"slot": slot, "holder": holder})).collect();
                    return Err(CpdError::new(
                        ErrorCode::ResourceBusy,
                        format!("The resource {} of {} is held by another process.", args.name, repo.label()),
                    )
                    .with("resource", args.name.as_str())
                    .with("repo", repo.label())
                    .with("holders", holders)
                    .with("waited_minutes", minutes));
                }
                tries_left -= 1;
                ports.clock.sleep(Duration::from_secs(1));
            }
        }
    };
    let fd = held.fd(&args.name).expect("the resource was taken");
    exec(ports, &launch, fd, &args.name)
}

fn spawn_failed(launch: &Launch, name: &str, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::SpawnFailed, format!("Cannot run {} holding the resource {name}: {e}.", launch.program))
        .with("argv", [vec![launch.program.clone()], launch.args.clone()].concat())
}

/// Leaves `fd` (and only it) inheritable, then replaces this process with the command. Returns only when that
/// failed (or on a fake that returns).
fn exec(ports: &Ports, launch: &Launch, fd: RawFd, name: &str) -> Result<Output, CpdError> {
    // SAFETY: `fd` is open for the whole call: its owner is the caller's `Held` or `File`.
    let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
    let flags = fcntl(borrowed, FcntlArg::F_GETFD).map_err(|e| usage(format!("Cannot read the lock descriptor flags: {e}.")))?;
    let flags = FdFlag::from_bits_truncate(flags) & !FdFlag::FD_CLOEXEC;
    fcntl(borrowed, FcntlArg::F_SETFD(flags)).map_err(|e| usage(format!("Cannot keep the lock descriptor across exec: {e}.")))?;
    match ports.spawn.exec_keeping(launch, fd) {
        // The real exec never returns: an `Ok` is a fake, which answers like any command.
        Ok(()) => Ok(Output::success(json!({"resource": name}))),
        Err(e) => Err(spawn_failed(launch, name, e)),
    }
}
