//! `cpd resource list` and `cpd resource run` (commands.md §29).

use serde_json::json;

use std::os::fd::{BorrowedFd, RawFd};
use std::path::{Path, PathBuf};
use std::time::Duration;

use nix::fcntl::{FcntlArg, FdFlag, fcntl};

use super::{Holder, Take, lock_path, read, take};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::spawn::Launch;
use crate::ids::{OwnerLogin, RepoName};
use crate::output::Output;
use crate::policy::{PolicyKey, repo_key, repo_path, resolve};
use crate::ports::Ports;

/// The `resource` commands (commands.md §29).
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// The repo's declared resources and who holds them (§29.1)
    List,
    /// Take a resource and run a command holding it (§29.2)
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
}

impl Repo {
    fn label(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

fn usage(message: impl Into<String>) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

fn here(ports: &Ports, home: &CpdHome) -> Result<(PathBuf, Repo), CpdError> {
    let cwd = std::env::current_dir().map_err(|e| usage(format!("Cannot read the current directory: {e}.")))?;
    let top = crate::git::run::run_with(&*ports.runner, &cwd, &["rev-parse", "--show-toplevel"])?;
    if top.code != 0 {
        return Err(usage(format!("cpd resource: {} is not in a git worktree.", cwd.display())));
    }
    let (owner, name) = repo_key(Path::new(top.stdout.trim()))?;
    let mut declared = resolve(home, &PolicyKey::Repo(owner.clone(), name.clone()))?.policy.resources.names;
    declared.sort();
    Ok((cwd, Repo { owner, name, declared }))
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
    let mut resources = Vec::new();
    for name in &repo.declared {
        let holder = read(&home, &repo.owner, &repo.name, name)?;
        resources.push(json!({"name": name, "held": holder.is_some(), "holder": holder}));
    }
    Ok(Output::success(json!({"repo": repo.label(), "resources": resources})))
}

fn run_command(ports: &Ports, args: RunArgs) -> Result<Output, CpdError> {
    if args.command.is_empty() {
        return Err(usage("cpd resource run: a command must follow `--`."));
    }
    if args.wait == Some(0) {
        return Err(usage("cpd resource run: --wait is a positive number of minutes."));
    }
    let home = CpdHome::from_env()?;
    let (cwd, repo) = here(ports, &home)?;
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

    // An Attempt that holds the resource takes nothing more.
    if let Some(id) = &attempt
        && matches!(read(&home, &repo.owner, &repo.name, &args.name)?, Some(Holder::Attempt { attempt, .. }) if &attempt == id)
    {
        let path = lock_path(&home, &repo.owner, &repo.name, &args.name);
        let file = std::fs::File::open(&path).map_err(|e| lock_failed(&path, e))?;
        return exec(ports, &launch, &std::os::fd::AsRawFd::as_raw_fd(&file), &args.name);
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
    let held = loop {
        match take(&home, &repo.owner, &repo.name, &[args.name.as_str()], &holder)? {
            Take::Held(held) => break held,
            Take::Busy { resource, holder } => {
                if tries_left == 0 {
                    return Err(CpdError::new(
                        ErrorCode::ResourceBusy,
                        format!("The resource {resource} of {} is held by another process.", repo.label()),
                    )
                    .with("resource", resource)
                    .with("repo", repo.label())
                    .with("holder", serde_json::to_value(holder).expect("a holder serialises"))
                    .with("waited_minutes", minutes));
                }
                tries_left -= 1;
                ports.clock.sleep(Duration::from_secs(1));
            }
        }
    };
    let fd = held.fd(&args.name).expect("the resource was taken");
    exec(ports, &launch, &fd, &args.name)
}

fn lock_failed(path: &Path, e: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot use the resource lock {}: {e}.", path.display()))
        .with("lock", path.display().to_string())
}

/// Leaves `fd` (and only it) inheritable, then replaces this process with the command. Returns only when that
/// failed (or on a fake that returns).
fn exec(ports: &Ports, launch: &Launch, fd: &RawFd, name: &str) -> Result<Output, CpdError> {
    // SAFETY: `fd` is open for the whole call: its owner is the caller's `Held` or `File`.
    let borrowed = unsafe { BorrowedFd::borrow_raw(*fd) };
    let flags = fcntl(borrowed, FcntlArg::F_GETFD).map_err(|e| usage(format!("Cannot read the lock descriptor flags: {e}.")))?;
    let flags = FdFlag::from_bits_truncate(flags) & !FdFlag::FD_CLOEXEC;
    fcntl(borrowed, FcntlArg::F_SETFD(flags)).map_err(|e| usage(format!("Cannot keep the lock descriptor across exec: {e}.")))?;
    match ports.spawn.exec_keeping(launch, *fd) {
        Ok(()) => Ok(Output::Owned),
        Err(e) => Err(CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot run {} holding the resource {name}: {e}.", launch.program),
        )
        .with("argv", [vec![launch.program.clone()], launch.args.clone()].concat())),
    }
}
