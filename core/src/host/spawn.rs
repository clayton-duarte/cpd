//! The process seam of what [`Runner`](super::runner::Runner) is not: a child that outlives the call, is waited for
//! later, is signalled, or replaces the process (docs/implementing.md, "Ports, Spawn and run_with").
//! [`ProcessSpawn`] is the real one; [`FakeSpawn`] starts nothing and records.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader};
use std::os::fd::{BorrowedFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, fcntl};
use nix::sys::signal::{self, kill};
use nix::unistd::Pid as NixPid;

use super::priority::idle_prefix;
use crate::error::{CpdError, ErrorCode};

/// A process id, as `Spawn` hands it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pid(pub u32);

/// A signal `Spawn::signal` sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
}

/// What to start: the program, its argv, its environment and how it is detached.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub setsid: bool,
    pub stdout_to: Option<PathBuf>,
    pub hand_over: Option<RawFd>,
    pub idle: bool,
    /// The child keeps this process's stdin, stdout and stderr (a Lead's pi owns the pane's terminal) and is not
    /// reaped by `Spawn`: its parent waits for it, by pid (`session run`'s `PiProc`). `setsid`, `stdout_to` and
    /// `hand_over` are not combined with it.
    pub foreground: bool,
}

pub trait Spawn: Send + Sync {
    /// A detached process or a child; the real one reaps it.
    fn spawn(&self, l: &Launch) -> io::Result<Pid>;
    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>>;
    fn signal(&self, p: Pid, s: Signal) -> io::Result<()>;
    /// Zombie-aware.
    fn alive(&self, p: Pid) -> bool;
    fn exe_of(&self, p: Pid) -> Option<PathBuf>;
    fn cmdline(&self, p: Pid) -> Option<String>;
    /// The pids of the live processes of this user that carry `key=value` in their environment (`cpd gate finish`
    /// finds a run's leftovers by `GATE_RUN=<id>`).
    fn with_env(&self, key: &str, value: &str) -> Vec<Pid>;
    /// The parent of `p`, `None` when `p` is gone or the OS does not tell.
    fn ppid(&self, p: Pid) -> Option<Pid>;
    /// The value of `key` in the environment of `p`, `None` when it has none or the OS does not show it (on macOS an
    /// Apple platform binary hides its environment).
    fn env_of(&self, p: Pid, key: &str) -> Option<String>;
    /// Every live descendant of `root`, not `root` itself, in pid order (the stall rule kills a Worker's tool call).
    fn descendants(&self, root: Pid) -> Vec<Pid>;
    /// Returns only on failure.
    fn exec(&self, l: &Launch) -> io::Error;
    /// [`Spawn::exec`] that must leave the descriptor `keep` open and inheritable across the exec (`cpd resource run`
    /// holds its lock on it). The real one returns only on failure, so `Ok` is for a fake that returns.
    fn exec_keeping(&self, l: &Launch, keep: RawFd) -> io::Result<()> {
        let _ = keep;
        Err(self.exec(l))
    }
    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)>;
    /// The path of cpd's own executable.
    fn self_exe(&self) -> Result<PathBuf, CpdError>;
    /// Which implementation this is, by name, for a test of `Ports` (`"ProcessSpawn"` for the real one).
    fn kind(&self) -> &'static str {
        "other"
    }
}

/// The executable path recorded when this process started (hs-d151, fix 3): the one `ProcessSpawn::self_exe` answers.
static RECORDED: OnceLock<PathBuf> = OnceLock::new();

/// Records `path` as cpd's own executable; the first record wins.
pub(super) fn record_start_as(path: PathBuf) {
    let _ = RECORDED.set(path);
}

/// The recorded path, or `spawn_failed` when `record_start` found none.
pub(super) fn recorded() -> Result<&'static std::path::Path, CpdError> {
    RECORDED.get().map(PathBuf::as_path).ok_or_else(|| {
        CpdError::new(ErrorCode::SpawnFailed, "The path of cpd's own executable was not recorded at process start (`current_exe` failed).")
    })
}

/// The real one: `Command`, `setsid`, `kill(2)`, `/proc` or `libproc`, `exec`.
pub struct ProcessSpawn;

/// The descriptor a driver finds its lock on (`hand_over`).
const HAND_OVER_FD: RawFd = 3;

/// The exit status of each child `spawn` and `stream` started, `None` while it runs: a thread per child waits for it
/// (the reaper), so it never stays a zombie, and `try_wait` reads what the thread found.
type Reaped = Arc<Mutex<Option<ExitStatus>>>;

fn reaped() -> &'static Mutex<HashMap<u32, Reaped>> {
    static TABLE: OnceLock<Mutex<HashMap<u32, Reaped>>> = OnceLock::new();
    TABLE.get_or_init(Mutex::default)
}

/// The `Command` of `l`, stdin from `/dev/null`; stdout and stderr are the caller's to set.
fn command(l: &Launch) -> Command {
    let prefix: &[&str] = if l.idle { idle_prefix() } else { &[] };
    let mut command = match prefix.split_first() {
        Some((first, rest)) => {
            let mut c = Command::new(first);
            c.args(rest).arg(&l.program);
            c
        }
        None => Command::new(&l.program),
    };
    command.args(&l.args);
    for (k, v) in &l.env_set {
        command.env(k, v);
    }
    for k in &l.env_remove {
        command.env_remove(k);
    }
    if let Some(cwd) = &l.cwd {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::null());
    let (setsid, fd) = (l.setsid, l.hand_over);
    if setsid || fd.is_some() {
        // SAFETY: the closure runs in the forked child before exec and only makes async-signal-safe system calls; it
        // allocates nothing.
        unsafe {
            command.pre_exec(move || {
                if setsid {
                    nix::unistd::setsid().map_err(io::Error::from)?;
                }
                if let Some(fd) = fd {
                    if fd != HAND_OVER_FD {
                        Errno::result(nix::libc::dup2(fd, HAND_OVER_FD)).map_err(io::Error::from)?;
                    }
                    // dup2 clears close-on-exec; when fd already was 3 it did not run.
                    fcntl(BorrowedFd::borrow_raw(HAND_OVER_FD), FcntlArg::F_SETFD(FdFlag::empty())).map_err(io::Error::from)?;
                }
                Ok(())
            });
        }
    }
    command
}

/// Starts the reaper of `child`: it waits for it on a thread of its own and keeps the status.
fn reap(mut child: std::process::Child) -> Pid {
    let pid = child.id();
    let status: Reaped = Arc::default();
    reaped().lock().unwrap_or_else(|e| e.into_inner()).insert(pid, Arc::clone(&status));
    std::thread::spawn(move || {
        let found = child.wait().ok();
        *status.lock().unwrap_or_else(|e| e.into_inner()) = found;
    });
    Pid(pid)
}

fn raw(p: Pid) -> Option<NixPid> {
    // 0 and the negatives would signal a process group, not a process.
    i32::try_from(p.0).ok().filter(|p| *p > 0).map(NixPid::from_raw)
}

impl Spawn for ProcessSpawn {
    fn spawn(&self, l: &Launch) -> io::Result<Pid> {
        let mut command = command(l);
        if l.foreground {
            command.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
            return Ok(Pid(command.spawn()?.id()));
        }
        match &l.stdout_to {
            Some(path) => {
                let log = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
                command.stdout(log.try_clone()?).stderr(log);
            }
            None => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        Ok(reap(command.spawn()?))
    }

    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>> {
        let table = reaped().lock().unwrap_or_else(|e| e.into_inner());
        let status = table.get(&p.0).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("process {} is not a child of this process", p.0)))?;
        let found = *status.lock().unwrap_or_else(|e| e.into_inner());
        Ok(found)
    }

    fn signal(&self, p: Pid, s: Signal) -> io::Result<()> {
        let pid = raw(p).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a process id", p.0)))?;
        let s = match s {
            Signal::Term => signal::Signal::SIGTERM,
            Signal::Kill => signal::Signal::SIGKILL,
        };
        kill(pid, s).map_err(io::Error::from)
    }

    /// Whether process `pid` runs: `kill(pid, 0)` finds it (a process of another user, `EPERM`, exists too) and it is
    /// not a zombie (ended, not yet reaped). Linux reads `/proc/<pid>/stat`; macOS reads the BSD status through
    /// `libproc` (`SZOMB`).
    fn alive(&self, p: Pid) -> bool {
        let Some(pid) = raw(p) else { return false };
        if matches!(kill(pid, None), Err(Errno::ESRCH)) {
            return false;
        }
        #[cfg(target_os = "macos")]
        {
            use libproc::bsd_info::BSDInfo;
            // SZOMB, <sys/proc.h>: the process has ended and waits to be reaped.
            const SZOMB: u32 = 5;
            match libproc::proc_pid::pidinfo::<BSDInfo>(pid.as_raw(), 0) {
                Ok(info) => info.pbi_status != SZOMB,
                // `kill` found it, `libproc` does not: it was reaped between the two.
                Err(_) => false,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", p.0)) else { return true };
            // `pid (comm) state …`; comm may hold spaces and parentheses.
            stat.rfind(')').and_then(|i| stat[i + 1..].split_whitespace().next()) != Some("Z")
        }
    }

    #[cfg(target_os = "macos")]
    fn exe_of(&self, p: Pid) -> Option<PathBuf> {
        libproc::proc_pid::pidpath(i32::try_from(p.0).ok()?).ok().map(PathBuf::from)
    }

    #[cfg(not(target_os = "macos"))]
    fn exe_of(&self, p: Pid) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/exe", p.0)).ok()
    }

    /// macOS: `libproc` has no argv reader (only the name and path), so `ps -o command=` gives it.
    #[cfg(target_os = "macos")]
    fn cmdline(&self, p: Pid) -> Option<String> {
        let out = Command::new("ps").args(["-o", "command=", "-p", &p.0.to_string()]).output().ok()?;
        let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (!line.is_empty()).then_some(line)
    }

    /// `/proc/<pid>/cmdline`, its arguments joined by spaces.
    #[cfg(not(target_os = "macos"))]
    fn cmdline(&self, p: Pid) -> Option<String> {
        let raw = std::fs::read(format!("/proc/{}/cmdline", p.0)).ok()?;
        let words: Vec<String> = raw.split(|&b| b == 0).filter(|w| !w.is_empty()).map(|w| String::from_utf8_lossy(w).into_owned()).collect();
        (!words.is_empty()).then(|| words.join(" "))
    }

    /// Linux reads `/proc/<pid>/environ` (it survives `setsid`, `nohup` and a tmux server, which inherits its
    /// starter's environment); macOS has no `/proc`, so `ps -axww -E` prints each process's environment after its
    /// command. Not a match: this process, its ancestors and its descendants, and a process that is no longer alive
    /// (a scan's own `ps` has exited when the pids are examined).
    fn with_env(&self, key: &str, value: &str) -> Vec<Pid> {
        let entry = format!("{key}={value}");
        let own = std::process::id();
        let mut related: std::collections::HashSet<u32> = crate::driver::stall::descendants(own).into_iter().collect();
        related.insert(own);
        let mut up = crate::driver::stall::stat(own).map(|(ppid, _)| ppid);
        while let Some(p) = up.filter(|p| *p > 1) {
            if !related.insert(p) {
                break;
            }
            up = crate::driver::stall::stat(p).map(|(ppid, _)| ppid);
        }
        let mut found: Vec<u32> = scan_env(&entry).into_iter().filter(|p| !related.contains(p)).collect();
        found.retain(|p| self.alive(Pid(*p)));
        found.sort_unstable();
        found.into_iter().map(Pid).collect()
    }

    fn ppid(&self, p: Pid) -> Option<Pid> {
        crate::driver::stall::stat(p.0).map(|(ppid, _)| Pid(ppid))
    }

    fn descendants(&self, root: Pid) -> Vec<Pid> {
        crate::driver::stall::descendants(root.0).into_iter().map(Pid).collect()
    }

    /// macOS: `ps -E -p <pid> -o command=` prints the environment after the command.
    #[cfg(target_os = "macos")]
    fn env_of(&self, p: Pid, key: &str) -> Option<String> {
        let out = Command::new("ps").args(["-E", "-p", &p.0.to_string(), "-o", "command="]).output().ok()?;
        let prefix = format!("{key}=");
        String::from_utf8_lossy(&out.stdout).split_whitespace().find_map(|w| w.strip_prefix(&prefix).map(str::to_owned))
    }

    #[cfg(not(target_os = "macos"))]
    fn env_of(&self, p: Pid, key: &str) -> Option<String> {
        let raw = std::fs::read(format!("/proc/{}/environ", p.0)).ok()?;
        let prefix = format!("{key}=");
        raw.split(|&b| b == 0).find_map(|e| String::from_utf8_lossy(e).strip_prefix(&prefix).map(str::to_owned))
    }

    fn exec(&self, l: &Launch) -> io::Error {
        let mut command = command(l);
        // The new program owns this process's terminal (a reloaded `session run` reads its stdin): `command` sets null.
        command.stdin(Stdio::inherit());
        if let Some(path) = &l.stdout_to {
            match std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|f| Ok((f.try_clone()?, f))) {
                Ok((out, err)) => {
                    command.stdout(out).stderr(err);
                }
                Err(e) => return e,
            }
        }
        // `exec` replaces this process: it keeps this process's terminal as its stdin (`command` starts from `/dev/null`).
        command.stdin(Stdio::inherit());
        command.exec()
    }

    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)> {
        let mut command = command(l);
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = command.spawn()?;
        let out = child.stdout.take().ok_or_else(|| io::Error::other("no stdout pipe"))?;
        Ok((reap(child), Box::new(BufReader::new(out))))
    }

    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        recorded().map(PathBuf::from)
    }

    fn kind(&self) -> &'static str {
        "ProcessSpawn"
    }
}

/// The pids that carry the exact entry `entry` (`KEY=value`) in their environment.
#[cfg(target_os = "macos")]
fn scan_env(entry: &str) -> Vec<u32> {
    let Ok(out) = Command::new("ps").args(["-axww", "-E", "-o", "pid=,command="]).output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?.parse().ok()?;
            words.any(|w| w == entry).then_some(pid)
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn scan_env(entry: &str) -> Vec<u32> {
    crate::driver::stall::all_pids()
        .into_iter()
        .filter(|pid| {
            // Another user's environ is unreadable: not ours to list.
            std::fs::read(format!("/proc/{pid}/environ")).is_ok_and(|raw| raw.split(|&b| b == 0).any(|e| e == entry.as_bytes()))
        })
        .collect()
}

/// What `FakeSpawn` knows of a pid it handed out.
enum Fate {
    Running,
    Exited(ExitStatus),
}

#[derive(Default)]
struct Table {
    last_pid: u32,
    launches: Vec<Launch>,
    signals: Vec<(Pid, Signal)>,
    pids: HashMap<Pid, Fate>,
    self_exe: Option<PathBuf>,
    exes: HashMap<Pid, PathBuf>,
    cmdlines: HashMap<Pid, String>,
    /// The `env_set` of each launch that got a pid.
    envs: HashMap<Pid, Vec<(String, String)>>,
    /// The parent of a pid, where a test said so (`FakeSpawn::add_process`).
    parents: HashMap<Pid, Pid>,
    /// Every `exec_keeping` call.
    execs: Vec<ExecRecord>,
    /// Run by `exec_keeping` after it records, before it answers.
    exec_hook: Option<ExecHook>,
    /// The failure `exec_keeping` answers, when a test said so.
    exec_fails: Option<io::ErrorKind>,
}

/// A call of [`Spawn::exec_keeping`] on the fake: what was exec-ed and the state of the descriptor kept at the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecRecord {
    pub launch: Launch,
    pub keep: RawFd,
    /// Whether `keep` was not close-on-exec at the call; `None` when it was not open.
    pub inheritable: Option<bool>,
}

type ExecHook = Arc<dyn Fn(&ExecRecord) + Send + Sync>;

impl Table {
    /// Records `l` and hands out the next pid, running.
    fn start(&mut self, l: &Launch) -> Pid {
        self.launches.push(l.clone());
        self.last_pid += 1;
        let pid = Pid(self.last_pid);
        self.pids.insert(pid, Fate::Running);
        self.envs.insert(pid, l.env_set.clone());
        pid
    }
}

/// The fake: pids from a counter, a table of launches and their state, the record of signals. Starts nothing.
/// Configured through `&self`, so a test can hold it in an `Arc` beside the `Ports` that share it.
pub struct FakeSpawn {
    table: Mutex<Table>,
}

impl Default for FakeSpawn {
    fn default() -> FakeSpawn {
        FakeSpawn::new()
    }
}

/// The first pid the fake hands out: well above 1, which is never a cpd child.
const FIRST_FAKE_PID: u32 = 1000;

impl FakeSpawn {
    pub fn new() -> FakeSpawn {
        FakeSpawn { table: Mutex::new(Table { last_pid: FIRST_FAKE_PID - 1, ..Table::default() }) }
    }

    fn table(&self) -> std::sync::MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every `Launch` that `spawn`, `stream` and `exec` received, in order.
    pub fn launches(&self) -> Vec<Launch> {
        self.table().launches.clone()
    }

    /// Every call of `exec_keeping`, in order.
    pub fn execs(&self) -> Vec<ExecRecord> {
        self.table().execs.clone()
    }

    /// Runs `hook` inside each `exec_keeping`, after it records and before it answers: the process "is exec-ed" there.
    pub fn on_exec(&self, hook: impl Fn(&ExecRecord) + Send + Sync + 'static) {
        self.table().exec_hook = Some(Arc::new(hook));
    }

    /// Makes `exec_keeping` fail with `kind`.
    pub fn fail_exec(&self, kind: io::ErrorKind) {
        self.table().exec_fails = Some(kind);
    }

    /// `launches` without the engine's `cpd janitor` launch (step E16 starts one on the first tick of every engine start).
    pub fn launches_but_janitor(&self) -> Vec<Launch> {
        self.launches().into_iter().filter(|l| l.args != ["janitor"]).collect()
    }

    /// Every `(pid, signal)` that `signal` received, in order.
    pub fn signals(&self) -> Vec<(Pid, Signal)> {
        self.table().signals.clone()
    }

    /// Marks `pid` exited with `code`: `try_wait` then returns the status and `alive` is false.
    pub fn exit(&self, pid: Pid, code: i32) {
        self.table().pids.insert(pid, Fate::Exited(ExitStatus::from_raw(code << 8)));
    }

    /// What `self_exe` returns; until it is called `self_exe` is an `Err`.
    pub fn set_self_exe(&self, path: impl Into<PathBuf>) {
        self.table().self_exe = Some(path.into());
    }

    /// What `exe_of(pid)` returns.
    pub fn set_exe_of(&self, pid: Pid, path: impl Into<PathBuf>) {
        self.table().exes.insert(pid, path.into());
    }

    /// What `cmdline(pid)` returns.
    pub fn set_cmdline(&self, pid: Pid, line: &str) {
        self.table().cmdlines.insert(pid, line.to_owned());
    }

    /// What `ppid(pid)` returns: `parent`, for a pid a `Launch` started (`add_process` names its parent itself).
    pub fn set_parent(&self, pid: Pid, parent: Pid) {
        self.table().parents.insert(pid, parent);
    }

    /// A running process that no `Launch` started: a child of `parent` (a pid the test names, a pane's) whose `cmdline` is `line`.
    /// It counts in `descendants(parent)` until it exits (`exit`, or a signal).
    pub fn add_process(&self, parent: Pid, line: &str) -> Pid {
        let mut table = self.table();
        table.last_pid += 1;
        let pid = Pid(table.last_pid);
        table.pids.insert(pid, Fate::Running);
        table.parents.insert(pid, parent);
        table.cmdlines.insert(pid, line.to_owned());
        pid
    }
}

impl Spawn for FakeSpawn {
    fn spawn(&self, l: &Launch) -> io::Result<Pid> {
        Ok(self.table().start(l))
    }

    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>> {
        match self.table().pids.get(&p) {
            Some(Fate::Running) => Ok(None),
            Some(Fate::Exited(status)) => Ok(Some(*status)),
            None => Err(io::Error::new(io::ErrorKind::NotFound, format!("process {} was not started by this FakeSpawn", p.0))),
        }
    }

    fn signal(&self, p: Pid, s: Signal) -> io::Result<()> {
        let mut table = self.table();
        table.signals.push((p, s));
        let number = match s {
            Signal::Term => signal::Signal::SIGTERM,
            Signal::Kill => signal::Signal::SIGKILL,
        } as i32;
        match table.pids.get_mut(&p) {
            Some(fate) => {
                // A pid that already exited stays as it exited.
                if matches!(fate, Fate::Running) {
                    *fate = Fate::Exited(ExitStatus::from_raw(number));
                }
                Ok(())
            }
            None => Err(io::Error::new(io::ErrorKind::NotFound, format!("process {} was not started by this FakeSpawn", p.0))),
        }
    }

    fn alive(&self, p: Pid) -> bool {
        matches!(self.table().pids.get(&p), Some(Fate::Running))
    }

    fn exe_of(&self, p: Pid) -> Option<PathBuf> {
        self.table().exes.get(&p).cloned()
    }

    fn cmdline(&self, p: Pid) -> Option<String> {
        self.table().cmdlines.get(&p).cloned()
    }

    fn with_env(&self, key: &str, value: &str) -> Vec<Pid> {
        let table = self.table();
        let mut pids: Vec<Pid> = table
            .envs
            .iter()
            .filter(|(pid, env)| matches!(table.pids.get(pid), Some(Fate::Running)) && env.iter().any(|(k, v)| k == key && v == value))
            .map(|(pid, _)| *pid)
            .collect();
        pids.sort_unstable();
        pids
    }

    fn ppid(&self, p: Pid) -> Option<Pid> {
        self.table().parents.get(&p).copied()
    }

    fn descendants(&self, root: Pid) -> Vec<Pid> {
        let table = self.table();
        let mut found: Vec<Pid> = table
            .parents
            .keys()
            .copied()
            .filter(|pid| matches!(table.pids.get(pid), Some(Fate::Running)))
            .filter(|pid| {
                let mut up = table.parents.get(pid);
                while let Some(parent) = up {
                    if *parent == root {
                        return true;
                    }
                    up = table.parents.get(parent);
                }
                false
            })
            .collect();
        found.sort_unstable();
        found
    }

    fn env_of(&self, p: Pid, key: &str) -> Option<String> {
        self.table().envs.get(&p)?.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn exec(&self, l: &Launch) -> io::Error {
        self.table().launches.push(l.clone());
        io::Error::other("faked exec")
    }

    fn exec_keeping(&self, l: &Launch, keep: RawFd) -> io::Result<()> {
        // SAFETY: only probes the descriptor number.
        let flags = fcntl(unsafe { BorrowedFd::borrow_raw(keep) }, FcntlArg::F_GETFD).ok();
        let inheritable = flags.map(|f| !FdFlag::from_bits_truncate(f).contains(FdFlag::FD_CLOEXEC));
        let record = ExecRecord { launch: l.clone(), keep, inheritable };
        let (hook, fails) = {
            let mut table = self.table();
            table.launches.push(l.clone());
            table.execs.push(record.clone());
            (table.exec_hook.clone(), table.exec_fails)
        };
        if let Some(hook) = hook {
            hook(&record);
        }
        fails.map_or(Ok(()), |kind| Err(io::Error::from(kind)))
    }

    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn BufRead + Send>)> {
        let pid = self.table().start(l);
        Ok((pid, Box::new(io::Cursor::new(Vec::new()))))
    }

    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        self.table().self_exe.clone().ok_or_else(|| CpdError::new(ErrorCode::SpawnFailed, "The FakeSpawn has no self_exe configured."))
    }
}
