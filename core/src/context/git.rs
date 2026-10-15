//! The git commands of a Context store (commands.md §3: "git identity").
//!
//! Private to this area: the `git` module (contract §7) is the repo-worktree
//! wrapper, and a Context store needs only a handful of fixed commands.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use crate::error::{CpdError, ErrorCode};
use crate::ids::MachineName;

/// The variables with which git is told which repository to act on. A parent
/// process such as a git hook may set them; they would send `-C <dir>` to
/// the wrong repository, so they are removed. Others (`GIT_SSH_COMMAND`, …)
/// are the User's and stay.
const REPOSITORY_VARIABLES: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// A Context store's git repository, driven as `cpd <machine>`.
pub struct Git<'a> {
    dir: &'a Path,
    machine: &'a MachineName,
}

impl<'a> Git<'a> {
    pub fn new(dir: &'a Path, machine: &'a MachineName) -> Git<'a> {
        Git { dir, machine }
    }

    /// `git <args>` in the store; `git_failed` when it cannot run or exits non-zero.
    pub fn run(&self, args: &[&str]) -> Result<String, CpdError> {
        let output = self.output(args)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(failed(&self.argv(args), &output))
        }
    }

    /// `git <args>` whose exit code is an answer, not a failure: the output
    /// when it exits 0, `None` when it exits 1.
    pub fn probe(&self, args: &[&str]) -> Result<Option<String>, CpdError> {
        let output = self.output(args)?;
        match output.status.code() {
            Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
            Some(1) => Ok(None),
            _ => Err(failed(&self.argv(args), &output)),
        }
    }

    /// `git <args>` for a command that is expected to fail sometimes (a merge):
    /// the output whatever the exit code.
    pub fn attempt(&self, args: &[&str]) -> Result<Output, CpdError> {
        self.output(args)
    }

    /// The failure of an [`Git::attempt`] that exited non-zero.
    pub fn failure(&self, args: &[&str], output: &Output) -> CpdError {
        failed(&self.argv(args), output)
    }

    /// `git commit -q -m <message>`.
    pub fn commit(&self, message: &str) -> Result<(), CpdError> {
        self.run(&["commit", "-q", "-m", message]).map(drop)
    }

    fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut argv = vec!["git".to_owned()];
        argv.extend(args.iter().map(|a| (*a).to_owned()));
        argv
    }

    /// The identity makes a commit independent of the machine's git
    /// configuration; `commit.gpgsign=false` keeps a User's global signing
    /// setting from prompting inside a command.
    fn output(&self, args: &[&str]) -> Result<Output, CpdError> {
        let machine = self.machine;
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(self.dir)
            .arg("-c")
            .arg(format!("user.name=cpd {machine}"))
            .arg("-c")
            .arg(format!("user.email=cpd@{machine}.invalid"))
            .arg("-c")
            .arg("commit.gpgsign=false")
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        for variable in REPOSITORY_VARIABLES {
            command.env_remove(variable);
        }
        command.output().map_err(|e| {
            CpdError::new(ErrorCode::GitFailed, format!("Cannot run git: {e}."))
                .with("argv", self.argv(args))
                .with("dir", self.dir.display().to_string())
        })
    }
}

/// `git_failed` with `argv`, `exit` and the last 60 lines of stderr (commands.md §1.3).
fn failed(argv: &[String], output: &Output) -> CpdError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    let tail = lines[lines.len().saturating_sub(60)..].join("\n");
    CpdError::new(ErrorCode::GitFailed, format!("{} failed.", argv[..argv.len().min(3)].join(" ")))
        .with("argv", argv.to_vec())
        .with("exit", output.status.code())
        .with("stderr", tail)
}
