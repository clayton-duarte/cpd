//! `cpd gate finish`: the Gate's leak check, run by `scripts/gate-run.sh` after its command (docs/implementing.md,
//! "The split: `scripts/gate-run.sh` and `cpd gate finish`"; hygiene.md, "The Gate leak check").
//!
//! The run's directory is `$TMPDIR` (the wrapper exported `TMPDIR=$RUN`); the tmux sockets are searched in
//! `/tmp/tmux-<uid>` and `$TMUX_TMPDIR/tmux-<uid>`; the run's processes are the union of `Spawn::with_env("GATE_RUN", <id>)` and `Spawn::with_env("TMPDIR", <run dir>)`.

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

use crate::error::{CpdError, ErrorCode};
use crate::host::spawn::{Pid, Signal};
use crate::ports::Ports;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Check what the run left behind, stop and remove it, report it (the Gate's leak check)
    Finish(FinishArgs),
}

#[derive(Args, Debug)]
pub struct FinishArgs {
    /// The run id (`GATE_RUN`, 6 characters)
    #[arg(long)]
    pub run: String,
    /// The exit status of the run's command
    #[arg(long)]
    pub status: i32,
}

/// What `finish` decided: the exit status of the whole run and the text for stderr (empty for a clean run).
#[derive(Debug, PartialEq, Eq)]
pub struct Finished {
    pub status: i32,
    pub report: String,
}

/// The exit status of a run that left something behind (or called the real tmux) while its command succeeded.
pub const LEAK_STATUS: i32 = 64;

/// `finish` on the environment's `TMPDIR`, which must be the run's own directory, `gate-run.<id>`: this command
/// removes it with everything in it, so a `TMPDIR` that is not that (a hand run in `/tmp`) is refused.
pub fn run(ports: &Ports, command: Command) -> Result<Finished, CpdError> {
    match command {
        Command::Finish(a) => {
            let dir = run_dir();
            let wanted = format!("gate-run.{}", a.run);
            if dir.as_deref().and_then(Path::file_name).and_then(|n| n.to_str()) != Some(wanted.as_str()) {
                return Err(CpdError::new(
                    ErrorCode::Usage,
                    format!("TMPDIR must be the run's directory `{wanted}` (scripts/gate-run.sh sets it); it is {dir:?}."),
                ));
            }
            Ok(finish(ports, &a.run, a.status))
        }
    }
}

/// The run's directory: `$TMPDIR`, which `scripts/gate-run.sh` exported.
fn run_dir() -> Option<PathBuf> {
    std::env::var_os("TMPDIR").filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// What a leftover process shows when the OS does not tell its test.
const NO_TEST: &str = "(not in its environment, or not shown by this system)";

/// The first `n` characters of `s`.
fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn command_of(ports: &Ports, pid: Pid) -> String {
    cut(&ports.spawn.cmdline(pid).unwrap_or_default(), 100)
}

/// `<pid> <command> < <pid> <command> …`: the ancestors of `pid` up to pid 1.
fn parents_of(ports: &Ports, pid: Pid) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut seen = vec![pid];
    let mut next = ports.spawn.ppid(pid);
    while let Some(q) = next.filter(|q| q.0 >= 1 && !seen.contains(q)) {
        out.push(format!("{} {}", q.0, command_of(ports, q)));
        if q.0 <= 1 {
            break;
        }
        seen.push(q);
        next = ports.spawn.ppid(q);
    }
    out.join(" < ")
}

/// The sockets named `*-<id>-*` in tmux's directories: `/tmp/tmux-<uid>` and `$TMUX_TMPDIR/tmux-<uid>`.
/// The uid in the directory names is this user's: the owner of the run's directory, which the wrapper made.
fn sockets_of(run_id: &str, run_dir: Option<&Path>) -> Vec<PathBuf> {
    let Some(uid) = run_dir.and_then(|d| fs::metadata(d).ok()).map(|m| m.uid()) else { return Vec::new() };
    let mut dirs = vec![PathBuf::from(format!("/tmp/tmux-{uid}"))];
    if let Some(root) = std::env::var_os("TMUX_TMPDIR").filter(|v| !v.is_empty()) {
        let dir = Path::new(&root).join(format!("tmux-{uid}"));
        let dir = PathBuf::from(dir.to_string_lossy().replace("//", "/"));
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    let marker = format!("-{run_id}-");
    let mut found = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        let mut names: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(&marker))
            .filter(|e| e.file_type().is_ok_and(|t| t.is_socket()))
            .map(|e| e.path())
            .collect();
        names.sort();
        found.extend(names);
    }
    found
}

/// The entries of the run's directory, hidden ones included, by name.
fn paths_of(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    paths
}

/// The leak check of run `run_id` whose command exited with `status`: the run's own leftovers are listed, reported,
/// stopped and removed, and nothing else is touched. The run's directory is `$TMPDIR`.
/// Its processes are those with `GATE_RUN=<id>` plus those with `TMPDIR=<run dir>` (exact match), deduplicated, in pid
/// order, live only.
pub fn finish(ports: &Ports, run_id: &str, status: i32) -> Finished {
    let dir = run_dir();
    let mut status = status;
    let mut report = String::new();

    // The tripwire's log (scripts/tripwire/tmux): any line means a test called the real tmux.
    if let Some(dir) = &dir {
        let log = dir.join("tripwire.log");
        if fs::read_to_string(&log).is_ok_and(|t| !t.is_empty()) {
            let text = fs::read_to_string(&log).unwrap_or_default();
            report += &format!("gate-run: FAIL: a test called the real tmux:\n{text}");
            if !text.ends_with('\n') {
                report.push('\n');
            }
            if status == 0 {
                status = LEAK_STATUS;
            }
        }
        let _ = fs::remove_file(&log);
    }

    let mut found: Vec<Pid> = ports.spawn.with_env("GATE_RUN", run_id);
    if let Some(dir) = &dir {
        found.extend(ports.spawn.with_env("TMPDIR", &dir.to_string_lossy()));
    }
    found.sort();
    found.dedup();
    let pids: Vec<Pid> = found.into_iter().filter(|p| ports.spawn.alive(*p)).collect();
    let sockets = sockets_of(run_id, dir.as_deref());
    let paths = dir.as_deref().map(paths_of).unwrap_or_default();
    let count = pids.len() + sockets.len() + paths.len();
    if count == 0 {
        if let Some(dir) = &dir {
            let _ = fs::remove_dir(dir);
        }
        return Finished { status, report };
    }

    let mut listing = String::new();
    for &pid in &pids {
        let ppid = ports.spawn.ppid(pid).map(|p| p.0.to_string()).unwrap_or_default();
        let test = ports.spawn.env_of(pid, "NEXTEST_TEST_NAME").filter(|t| !t.is_empty());
        let _ = writeln!(listing, "  process {} (ppid {ppid}) {}", pid.0, command_of(ports, pid));
        let _ = writeln!(listing, "    NEXTEST_TEST_NAME={}", test.as_deref().unwrap_or(NO_TEST));
        let _ = writeln!(listing, "    parents: {}", parents_of(ports, pid));
    }
    for socket in &sockets {
        let _ = writeln!(listing, "  socket  {}", socket.display());
    }
    for path in &paths {
        let _ = writeln!(listing, "  path    {}", path.display());
    }
    let _ = writeln!(report, "gate-run: FAIL: this run (GATE_RUN={run_id}) left {count} things behind:");
    report += &listing;
    if let Some(first) = pids.first() {
        // A hint that names a command this OS has: `/proc` where it exists, `ps -E` where it does not.
        let how = if Path::new("/proc/self").exists() {
            format!("tr '\\0' '\\n' </proc/{}/environ | grep NEXTEST_TEST_NAME", first.0)
        } else {
            format!("ps -E -p {} -o command= | tr ' ' '\\n' | grep NEXTEST_TEST_NAME", first.0)
        };
        let _ = writeln!(
            report,
            "gate-run: the test that left them: grep the id in the process environment (NEXTEST_TEST_NAME), e.g. `{how}`"
        );
    }

    // Only the pids, sockets and paths listed above.
    for &pid in &pids {
        let _ = ports.spawn.signal(pid, Signal::Kill);
    }
    for socket in &sockets {
        let _ = fs::remove_file(socket);
    }
    let removed = sockets.len() + paths.len() + usize::from(dir.is_some());
    if let Some(dir) = &dir {
        let _ = fs::remove_dir_all(dir);
    }
    let _ = writeln!(
        report,
        "gate-run: stopped {} process(es) and removed {removed} path(s) of this run (nothing else was touched)",
        pids.len()
    );
    if status == 0 {
        status = LEAK_STATUS;
    }
    Finished { status, report }
}

#[cfg(test)]
mod tests;
