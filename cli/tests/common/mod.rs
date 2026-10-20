//! The integration-test harness: runs the built `cpd` in an isolated
//! environment and parses its JSON.
//!
//! Every `TestEnv` lives in its own temporary directory and holds:
//! - `$CPD_HOME` and `$HOME` there, never the real `~/.cpd` or home;
//! - a private tmux server: its own `TMUX_TMPDIR` and socket name, killed on drop,
//!   so the cockpit server `-L cpd` is never touched;
//! - a teardown that kills the tmux server and every process whose command line or cwd
//!   is under the directory, then removes the directory: run in-process by `Drop`
//!   ([`TestEnv::teardown`]), and by a detached reaper (`teardown.sh`) when the test
//!   process is killed;
//! - a `bin/` directory first on `PATH`, for fake `pi` and `gh` scripts that
//!   record their calls; `gh`, `pi`, `curl`, `wget` and `ssh` start as shims that
//!   exit 97 (`no fake installed for <name>`), and no `GH_*`, `GITHUB_*`, `SSH_*`
//!   or proxy variable reaches a command, so no test reaches pi or the network;
//! - temporary git repos with a fixed identity and no global or system config.
//!
//! Use it from a test file with `mod common;`.
//!
//! Writing a script and executing it must not overlap with spawning in one test
//! process (ETXTBSY, see [`write_executable`]): write scripts with
//! [`write_executable`] and spawn only through [`spawn`] / [`output`].

#![allow(dead_code)] // Every test file compiles this module whole and uses a subset of it.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::os::unix::process::CommandExt;
use std::sync::{Mutex, OnceLock, PoisonError, RwLock};
use std::time::{Duration, Instant};

use clap::Parser;
use cpd::cli::Cli;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use serde_json::Value;
use tempfile::TempDir;

/// The cpd binary Cargo built for this test run.
pub const CPD: &str = env!("CARGO_BIN_EXE_cpd");

/// The longest any fake sleeps, so the leftovers of a killed test run end by themselves.
pub const SLEEP_SECONDS: u64 = 120;

/// The views' re-read period in the tests, as `tmux new-session -e` takes it (contract §1.2 "Test overrides"): a
/// view test waits on the screen for a change no Event announces, a few of these, not the release 2 s.
pub const VIEW_REFRESH: &str = "CPD_TEST_VIEW_REFRESH=0.1";

/// The engine tests' tick, `watch.tick_seconds` (its minimum, contract §8.2): an absence window of a few whole ticks
/// ([`TestEnv::wait_whole_ticks`]) is a few tenths of a second.
pub const TICK_SECONDS: &str = "0.1";

/// The variable an engine test starts `cpd watch` with, so that E5 polls [`TICK_SECONDS`] after the previous poll
/// ended (`CPD_TEST_PR_POLL`, contract §1.2 "Test overrides"), not every `pr_poll_seconds` (whole seconds): one poll
/// about every tick, which [`TestEnv::wait_whole_ticks`] counts.
pub const PR_POLL_EVERY_TICK: (&str, &str) = ("CPD_TEST_PR_POLL", TICK_SECONDS);

/// The poll periods of a whole Plan's processes, as test overrides (contract §1.2), for a test that runs an engine or
/// a Plan end to end: the engine's E5 every tick, A9's kill pauses (the engine's and the drivers'), the drivers (A4, a
/// review Job's Lead) and the wrappers (pi's file, a fork's source) every 50 ms. What such a test waits for is the
/// next look of each.
pub const PLAN_POLLS: [(&str, &str); 6] = [
    PR_POLL_EVERY_TICK,
    ("CPD_TEST_DRIVER_POLL", "0.05"),
    ("CPD_TEST_LEAD_POLL", "0.05"),
    ("CPD_TEST_KILL_PAUSE", "0.05"),
    ("CPD_TEST_FILE_POLL", "0.05"),
    ("CPD_TEST_SOURCE_POLL", "0.05"),
];

/// The reaper's teardown script: `sh teardown.sh watch <pid>` waits until `<pid>` (the test
/// process) is gone or the directory is, and then tears down if the directory is still there;
/// `sh teardown.sh` tears down at once. The same teardown as [`TestEnv::teardown`], which
/// `Drop` runs in-process (no process to start, one `/proc` scan): this one runs only when
/// the test process was killed. `dir` and `socket` are prepended by [`TestEnv::new`]. Never
/// kills the script itself, its ancestors or its descendants. Linux only (`/proc`). Idempotent.
const TEARDOWN_SH: &str = r#"
me=$$

# stat_of PID: sets st (state) and pp (parent pid); fails when the process is gone.
stat_of() {
  _l=
  { read -r _l < "/proc/$1/stat"; } 2>/dev/null || return 1
  _l=${_l##*) }
  set -- $_l
  st=$1 pp=$2
}

# alive PID: running (a zombie is dead).
alive() { stat_of "$1" && [ "$st" != Z ] && [ "$st" != X ]; }

protected=" "
_a=$me
while [ "$_a" -gt 0 ]; do
  protected="$protected$_a "
  stat_of "$_a" || break
  _a=$pp
done

mine() {
  _m=$1
  while [ "$_m" -gt 1 ]; do
    [ "$_m" = "$me" ] && return 0
    stat_of "$_m" || return 1
    _m=$pp
  done
  return 1
}

# The live processes whose command line contains $dir or whose cwd is under it.
targets() {
  for t in $({ grep -laF -- "$dir" /proc/[0-9]*/cmdline; find /proc -mindepth 2 -maxdepth 2 -name cwd -lname "$dir*"; } 2>/dev/null | sed -n 's|^/proc/\([0-9][0-9]*\)/.*|\1|p' | sort -u); do
    [ "$t" -gt 1 ] || continue
    case "$protected" in *" $t "*) continue ;; esac
    mine "$t" && continue
    alive "$t" || continue
    printf '%s\n' "$t"
  done
}

teardown() {
  unset TMUX
  TMUX_TMPDIR="$dir/tmux" tmux -L "$socket" kill-server >/dev/null 2>&1
  round=0
  while [ "$round" -lt 5 ]; do
    round=$((round + 1))
    pids=$(targets)
    [ -n "$pids" ] || break
    kill -TERM $pids 2>/dev/null
    n=0
    while [ "$n" -lt 10 ]; do
      any=
      for k in $pids; do alive "$k" && any=1; done
      [ -z "$any" ] && break
      sleep 0.1
      n=$((n + 1))
    done
    kill -KILL $pids 2>/dev/null
  done
  rm -rf "$dir"
}

if [ "${1-}" = watch ]; then
  while [ -d "$dir" ] && alive "$2"; do sleep 1; done
  [ -d "$dir" ] || exit 0
fi
teardown
exit 0
"#;

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Spawns `cmd` with stdio on /dev/null through [`spawn`] (the harness's one
/// write-vs-spawn lock) and waits for it.
fn run_quietly(cmd: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    spawn(cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()))?.wait()
}

static NEXT_SOCKET: AtomicUsize = AtomicUsize::new(0);

/// The tools a test never runs for real: `TestEnv::new` installs a deny shim for each.
pub const DENIED_TOOLS: [&str; 5] = ["gh", "pi", "curl", "wget", "ssh"];

/// What a fake `cpd watch --ensure` prints when the engine started.
pub const WATCH_OK: &str = "echo '{\"ok\": true, \"started\": true, \"pane\": \"%9\"}'; exit 0";

/// The exit status of a deny shim.
pub const DENIED_EXIT: i32 = 97;

/// The proxy variables, dropped from every command (either case).
const PROXY_VARS: [&str; 3] = ["HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"];

/// Held exclusively while a script is being written, shared while a child is
/// being spawned. It guards no data: poisoning (a test panicked while holding
/// it) leaves nothing inconsistent, so it is ignored.
static SCRIPT_WRITE_VS_SPAWN: RwLock<()> = RwLock::new(());

/// Writes the executable script `path` (mode 0755).
///
/// Tests run in parallel threads of one process. `fork` copies every open file
/// descriptor, and close-on-exec acts only at the child's `exec`. A child forked
/// by a concurrent test while this file is open for writing therefore holds a
/// write descriptor to it for a moment, and an `exec` of the script in that
/// moment fails with ETXTBSY ("Text file busy"). So the file is written and
/// closed under the exclusive lock, and every spawn ([`spawn`], [`output`])
/// takes the shared lock until the child has exec'd. Never retry instead.
pub fn write_executable(path: &Path, contents: &str) {
    let _exclusive = SCRIPT_WRITE_VS_SPAWN.write().unwrap_or_else(PoisonError::into_inner);
    fs::write(path, contents).expect("write script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod script");
}

/// Copies the executable `from` to `to` (an install of a binary the test later runs), under
/// the same exclusive lock and for the same reason as [`write_executable`].
pub fn copy_executable(from: &str, to: &Path) {
    let _exclusive = SCRIPT_WRITE_VS_SPAWN.write().unwrap_or_else(PoisonError::into_inner);
    fs::copy(from, to).expect("copy executable");
}

/// `cmd.spawn()`, never overlapping with [`write_executable`]. `Command::spawn`
/// returns once the child has exec'd (or failed to), so the shared lock covers
/// the whole window in which the child holds copies of this process's descriptors.
pub fn spawn(cmd: &mut Command) -> io::Result<Child> {
    let _shared = SCRIPT_WRITE_VS_SPAWN.read().unwrap_or_else(PoisonError::into_inner);
    cmd.spawn()
}

/// `cmd.output()` (stdout and stderr captured, stdin null), spawning through
/// [`spawn`]: the lock is released before the child is waited for.
pub fn output(cmd: &mut Command) -> io::Result<Output> {
    spawn(cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()))?.wait_with_output()
}

/// Makes the file `path` look last modified `by` ago, for a code that waits for a quiet file
/// (a session file nobody wrote to for a while): set the time, do not sleep until it has passed.
pub fn backdate(path: &Path, by: Duration) {
    let file = fs::OpenOptions::new().write(true).open(path).expect("open the file to backdate");
    file.set_modified(std::time::SystemTime::now() - by).expect("set the modification time");
}

/// How long [`wait_until`] and [`wait_screen`] wait: the one wait policy of the suite, long
/// enough for a loaded machine. A wait that needs longer says so with [`wait_until_for`].
pub const WAIT: Duration = Duration::from_secs(30);

/// How often a wait looks again: 10 ms, so a wait ends at most a few ms after its signal (half a poll on average),
/// against a look that costs about 1 ms (a read of the store on a read-only connection, a file, `/proc`) up to a few
/// ms (a `tmux capture-pane`).
const POLL: Duration = Duration::from_millis(10);

/// Waits until `ready` is true, at most [`WAIT`]; panics `timed out waiting for <what>`.
pub fn wait_until(what: &str, ready: impl FnMut() -> bool) {
    wait_until_for(what, WAIT, ready);
}

/// [`wait_until`] with its own `timeout`.
pub fn wait_until_for(what: &str, timeout: Duration, mut ready: impl FnMut() -> bool) {
    poll(timeout, || ready().then_some(())).unwrap_or_else(|| panic!("timed out waiting for {what}"));
}

/// The suite's one poll loop: looks at `ready` every [`POLL`] until it gives a value, and at
/// least once; `None` once `timeout` has passed without one. A wait panics on `None` ([`wait_until`],
/// [`wait_some`], or its own message); a check that something does not happen within a window
/// asserts `None`.
pub fn poll<T>(timeout: Duration, mut ready: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = ready() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL);
    }
}

/// Waits until some process is blocked in `flock(2)` on the file `path` (a waiter in the
/// kernel's own table, `/proc/locks`: a line `<n>: -> FLOCK ... <dev>:<inode> ...`), at most
/// [`WAIT`]. The positive signal for "this call is waiting for the lock", where a test then
/// asserts what has not happened yet.
pub fn wait_for_flock_waiter(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    let inode = fs::metadata(path).expect("the lock file exists").ino().to_string();
    wait_until(&format!("a process to wait for the flock on {}", path.display()), || {
        let locks = fs::read_to_string("/proc/locks").expect("read /proc/locks");
        locks.lines().any(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            fields.contains(&"->") && fields.iter().any(|f| f.rsplit(':').next() == Some(inode.as_str()) && f.matches(':').count() == 2)
        })
    });
}

/// Waits until `ready` gives a value, at most `timeout`. `Err(state)` is "not yet" and says
/// where it is; the last `state` is in the panic message.
pub fn wait_some<T>(what: &str, timeout: Duration, mut ready: impl FnMut() -> Result<T, String>) -> T {
    let mut state = String::new();
    poll(timeout, || ready().map_err(|s| state = s).ok()).unwrap_or_else(|| panic!("timed out waiting for {what}: {state}"))
}

/// Waits until `done` accepts the screen `screen` reads, at most [`WAIT`]; returns that
/// screen. A timeout prints the last one.
pub fn wait_screen(what: &str, mut screen: impl FnMut() -> Vec<String>, mut done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    wait_some(what, WAIT, || {
        let rows = screen();
        if done(&rows) { Ok(rows) } else { Err(format!("the screen is:\n{}", rows.join("\n"))) }
    })
}

/// A refusal: `run` is `ok: false` with `code` and `exit`; returns its `error` object.
///
/// `details.next`, when there is one, is a command for the caller to run: it is parsed with
/// the real command tree ([`Cli`]), so a `next` that names a flag or an argument that does not
/// exist fails here, in every test that checks a refusal. (It is parsed, not executed: a
/// `next` such as `cpd plan unarchive` would change the state the test goes on to read.)
pub fn refused(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    match &error["details"]["next"] {
        Value::Null => {}
        Value::String(next) => {
            let words: Vec<&str> = next.split_whitespace().collect();
            assert_eq!(words.first(), Some(&"cpd"), "details.next `{next}` is not a cpd command: {run:?}");
            if let Err(e) = Cli::try_parse_from(&words) {
                panic!("details.next `{next}` does not parse: {e}");
            }
        }
        other => panic!("details.next is not a string: {other}"),
    }
    error
}

/// Runs `sql` as the store's own write, on `store`.
pub fn sql(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

pub struct TestEnv {
    dir: TempDir,
    tmux_socket: String,
    /// The reaper ([`TestEnv::start_reaper`]), until the teardown stops it.
    reaper: Mutex<Option<Child>>,
    /// The reaper's start: every process this env starts starts at or after it.
    since: OnceLock<Since>,
    /// The read-only connection [`TestEnv::read`] reads on, kept open across reads (a new connection parses the whole
    /// schema at its first query: ~10-15 ms a read), with the identity (device, inode) of the `cpd.db` it opened.
    reader: Mutex<Option<((u64, u64), rusqlite::Connection)>>,
}

/// When a process started: its pid and its start time.
#[derive(Clone, Copy, Debug)]
struct Since {
    /// The kernel hands out pids in increasing order, wrapping at `pid_max`: until the counter
    /// wraps, a process started later has a larger pid.
    pid: i32,
    /// Clock ticks since boot (`/proc/<pid>/stat` field 22).
    ticks: u64,
}

/// The result of one `cpd` run.
#[derive(Debug)]
pub struct Run {
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// stdout as exactly one JSON value on one line (contract §1.2); panics otherwise.
    pub fn json(&self) -> Value {
        let line = self
            .stdout
            .strip_suffix('\n')
            .unwrap_or_else(|| panic!("stdout does not end with one newline: {self:?}"));
        assert!(!line.contains('\n'), "stdout has more than one line: {self:?}");
        serde_json::from_str(line).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {self:?}"))
    }

    /// The success envelope's fields; panics unless `ok` is true and the exit 0.
    pub fn ok(&self) -> Value {
        let json = self.json();
        assert_eq!(json["ok"], true, "{self:?}");
        assert_eq!(self.exit, 0, "{self:?}");
        json
    }

    /// The failure's `error` object; panics unless `ok` is false and the exit non-zero.
    pub fn error(&self) -> Value {
        let json = self.json();
        assert_eq!(json["ok"], false, "{self:?}");
        assert_ne!(self.exit, 0, "{self:?}");
        json["error"].clone()
    }
}

impl TestEnv {
    /// A new env whose `$CPD_HOME` already has a store: a copy of [`store_template`], so the
    /// first command opens it instead of creating and migrating one (~0.2 s). A test of what
    /// happens before the store exists uses [`TestEnv::empty`].
    pub fn new() -> TestEnv {
        let env = TestEnv::empty();
        fs::copy(store_template(), env.cpd_home().join("cpd.db")).expect("copy the store template");
        env
    }

    /// A new env whose `$CPD_HOME` is an empty directory: the first command creates the store.
    pub fn empty() -> TestEnv {
        let dir = tempfile::Builder::new().prefix("cpd-test-").tempdir().expect("temp dir");
        let n = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        let env = TestEnv { dir, tmux_socket: format!("cpdt-{}-{n}", std::process::id()), reaper: Mutex::new(None), since: OnceLock::new(), reader: Mutex::new(None) };
        for sub in ["cpd-home", "home", "bin", "tmux", "repos"] {
            fs::create_dir_all(env.dir.path().join(sub)).expect("create test dir");
        }
        for tool in DENIED_TOOLS {
            let path = env.bin().join(tool);
            write_executable(&path, &format!("#!/bin/sh\necho 'no fake installed for {tool}' >&2\nexit {DENIED_EXIT}\n"));
        }
        fs::write(
            env.home().join(".gitconfig"),
            "[user]\n\tname = cpd test\n\temail = test@cpd.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .expect("write .gitconfig");
        let script = format!(
            "dir={}\nsocket={}\n{TEARDOWN_SH}",
            sh_quote(&env.root().to_string_lossy()),
            sh_quote(&env.tmux_socket)
        );
        fs::write(env.teardown_script(), script).expect("write teardown script");
        env.start_reaper();
        env
    }

    fn teardown_script(&self) -> PathBuf {
        self.root().join("teardown.sh")
    }

    /// Starts the reaper: `teardown.sh watch`, detached (a session of its own, stdio on
    /// /dev/null), that runs the teardown once this test process is gone, so a killed test
    /// leaves nothing behind. It is a child of the test process, so [`TestEnv::teardown`]
    /// knows its pid and stops it first: it is never one of the processes the teardown
    /// searches for and waits out.
    fn start_reaper(&self) {
        let mut cmd = Command::new("sh");
        cmd.arg(self.teardown_script())
            .arg("watch")
            .arg(std::process::id().to_string())
            .current_dir("/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: setsid(2) is async-signal-safe, and nothing else runs between fork and exec.
        unsafe {
            cmd.pre_exec(|| nix::unistd::setsid().map(drop).map_err(io::Error::from));
        }
        let reaper = spawn(&mut cmd).expect("start the reaper");
        let pid = reaper.id() as i32;
        let since = Since { pid, ticks: start_ticks(Pid::from_raw(pid)).expect("the reaper's start time") };
        self.since.set(since).expect("one reaper per env");
        *self.reaper.lock().unwrap_or_else(PoisonError::into_inner) = Some(reaper);
    }

    /// Stops the reaper, kills this env's tmux server, then every process started since the env
    /// whose command line or cwd is under its directory (SIGTERM, SIGKILL after a grace of up to
    /// 1 s; never the test process or its ancestors), then removes the directory. Idempotent.
    /// In-process: the reaper's `teardown.sh` does the same when the test process was killed.
    pub fn teardown(&self) {
        if let Some(mut reaper) = self.reaper.lock().unwrap_or_else(PoisonError::into_inner).take() {
            // Its session is its process group: the shell and its `sleep`.
            let _ = killpg(Pid::from_raw(reaper.id() as i32), Signal::SIGKILL);
            let _ = reaper.wait();
        }
        let socket = self.root().join("tmux").join(format!("tmux-{}", uid())).join(&self.tmux_socket);
        if socket.exists() {
            let _ = run_quietly(self.command("tmux").arg("-L").arg(&self.tmux_socket).arg("kill-server").current_dir("/"));
        }
        for _ in 0..5 {
            let pids = processes_under(self.root(), self.since.get().copied());
            if pids.is_empty() {
                break;
            }
            for &pid in &pids {
                let _ = kill(pid, Signal::SIGTERM);
            }
            poll(Duration::from_secs(1), || (!pids.iter().any(|&pid| alive(pid))).then_some(()));
            for &pid in &pids {
                let _ = kill(pid, Signal::SIGKILL);
            }
        }
        let _ = fs::remove_dir_all(self.root());
    }

    /// The temporary directory everything lives in.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// `$CPD_HOME` of every command run here (empty at first).
    pub fn cpd_home(&self) -> PathBuf {
        self.root().join("cpd-home")
    }

    /// `$HOME` of every command run here.
    pub fn home(&self) -> PathBuf {
        self.root().join("home")
    }

    /// The directory first on `PATH`, holding the fake programs.
    pub fn bin(&self) -> PathBuf {
        self.root().join("bin")
    }

    /// The private tmux server's socket name (`tmux -L <name>`), to put in
    /// `config.toml` as `tmux.socket`. Its socket file is under this env's own
    /// `TMUX_TMPDIR`.
    pub fn tmux_socket(&self) -> &str {
        &self.tmux_socket
    }

    /// A `Command` for `program` with this env's isolated environment:
    /// `CPD_*`, `PI_*`, `GIT_*`, `XDG_*`, `PLANNOTATOR_*`, `GH_*`, `GITHUB_*`, `SSH_*`,
    /// the proxy variables (`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, either case),
    /// `TMUX` and `TMUX_PANE` removed, then
    /// `CPD_HOME`, `HOME`, `PATH`, `TMUX_TMPDIR` and git's config set.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut cmd = Command::new(program);
        for (key, _) in std::env::vars_os() {
            let k = key.to_string_lossy();
            let drop = ["CPD_", "PI_", "GIT_", "XDG_", "PLANNOTATOR_", "GH_", "GITHUB_", "SSH_"]
                .iter()
                .any(|p| k.starts_with(p))
                || PROXY_VARS.iter().any(|v| k.eq_ignore_ascii_case(v))
                || k == "TMUX"
                || k == "TMUX_PANE";
            if drop {
                cmd.env_remove(&key);
            }
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![self.bin()];
        paths.extend(std::env::split_paths(&path));
        cmd.env("PATH", std::env::join_paths(paths).expect("PATH"))
            .env("CPD_HOME", self.cpd_home())
            .env("HOME", self.home())
            .env("TMUX_TMPDIR", self.root().join("tmux"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.home().join(".gitconfig"))
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(self.root())
            .stdin(Stdio::null());
        cmd
    }

    /// The store of this env's `$CPD_HOME`, opened (migrated) now.
    pub fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    /// `f` on a read-only connection to this env's store, for a test that only looks: no
    /// [`Store::open`] (its migration and schema check cost ~30 ms per open in a debug build).
    /// One connection serves every read (each statement sees the last commit); it is opened again
    /// when `cpd.db` is another file than the one it opened (replaced, or removed and created).
    /// Panics when the store does not exist or `f` fails.
    pub fn read<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> T {
        use std::os::unix::fs::MetadataExt;
        let db = self.cpd_home().join("cpd.db");
        let file = fs::metadata(&db).map(|m| (m.dev(), m.ino())).unwrap_or_else(|e| panic!("stat {}: {e}", db.display()));
        let mut reader = self.reader.lock().unwrap_or_else(PoisonError::into_inner);
        if reader.as_ref().is_none_or(|(opened, _)| *opened != file) {
            let conn = rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap_or_else(|e| panic!("open {} read-only: {e}", db.display()));
            conn.busy_timeout(Duration::from_secs(5)).expect("set the busy timeout");
            *reader = Some((file, conn));
        }
        let (_, conn) = reader.as_ref().expect("opened above");
        f(conn).unwrap_or_else(|e| panic!("read {}: {e}", db.display()))
    }

    /// The first column of the one row `sql` returns, read with [`TestEnv::read`].
    pub fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.read(|conn| conn.query_row(sql, [], |r| r.get(0)))
    }

    /// Runs `sql` as a write of this env's store (a fixture).
    pub fn sql(&self, text: &str) {
        sql(&mut self.store(), text);
    }

    /// Writes `$CPD_HOME/config.toml`.
    pub fn write_config(&self, text: &str) {
        fs::create_dir_all(self.cpd_home()).expect("create CPD_HOME");
        fs::write(self.cpd_home().join("config.toml"), text).expect("write config.toml");
    }

    /// `config.toml` of machine `m1` on this env's private tmux server. `extra` is more TOML
    /// before the `[tmux]` table (top-level keys, or tables).
    pub fn write_tmux_config(&self, extra: &str) {
        self.write_config(&format!(
            "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n{extra}\n[tmux]\nsocket = \"{}\"\n",
            self.tmux_socket()
        ));
    }

    /// Runs `cpd <args>` from this env's root.
    pub fn cpd(&self, args: &[&str]) -> Run {
        self.cpd_in(self.root(), args)
    }

    /// Runs `cpd <args>` from `cwd`.
    pub fn cpd_in(&self, cwd: &Path, args: &[&str]) -> Run {
        self.cpd_with(cwd, args, &[])
    }

    /// Runs `cpd <args>` from `cwd` with the variables `vars` added to its environment.
    pub fn cpd_with(&self, cwd: &Path, args: &[&str], vars: &[(&str, &str)]) -> Run {
        let out = output(self.command(CPD).args(args).envs(vars.iter().copied()).current_dir(cwd)).expect("run cpd");
        Run {
            exit: out.status.code().expect("cpd was killed by a signal"),
            stdout: String::from_utf8(out.stdout).expect("stdout is UTF-8"),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// Runs `program` in this env and panics unless it exits 0; returns stdout.
    pub fn run_ok(&self, cwd: &Path, program: &str, args: &[&str]) -> String {
        let out = output(self.command(program).args(args).current_dir(cwd)).expect(program);
        assert_ok(program, args, &out);
        String::from_utf8(out.stdout).expect("stdout is UTF-8")
    }

    /// `git -C <dir> <args>`; panics unless it exits 0; returns trimmed stdout.
    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.run_ok(dir, "git", args).trim_end().to_owned()
    }

    /// A new git repo `repos/<name>` on branch `main` with one commit (`README.md`): a copy of
    /// [`git_repo_template`] (one git process instead of three), made here when there is none.
    pub fn git_repo(&self, name: &str) -> PathBuf {
        let repo = self.root().join("repos").join(name);
        let template = git_repo_template(name);
        if template.exists() {
            copy_tree(&template, &repo);
            // The index holds the template's inodes and times: refresh it, so that plumbing
            // (`diff-index`, `diff-files`) sees the clean tree it is.
            self.git(&repo, &["update-index", "-q", "--refresh"]);
            return repo;
        }
        fs::create_dir_all(&repo).expect("create repo dir");
        self.git(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join("README.md"), format!("# {name}\n")).expect("write README");
        self.git(&repo, &["add", "-A"]);
        self.git(&repo, &["commit", "-q", "-m", "initial"]);
        // Put it in place for the next env (atomic: a concurrent maker's whole copy wins).
        let partial = template.with_extension(format!("partial-{}-{}", std::process::id(), NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)));
        copy_tree(&repo, &partial);
        if fs::rename(&partial, &template).is_err() {
            fs::remove_dir_all(&partial).expect("remove the partial git repo template");
        }
        repo
    }

    /// `tmux -L <private socket> <args>` in this env.
    pub fn tmux(&self, args: &[&str]) -> Output {
        output(self.command("tmux").arg("-L").arg(&self.tmux_socket).args(args)).expect("run tmux")
    }

    /// Installs `bin/<name>`: a shell script that records its argv (see
    /// [`TestEnv::calls`]) and then runs `body` (`sh` code; `"$@"` are its arguments).
    ///
    /// A call is recorded atomically using only `sh` builtins, because some tests
    /// run cpd with `PATH` set to `bin/` alone (no coreutils, no `flock`). The call
    /// claims the next free number `<n>` by creating `<n>.rec` exclusively (`set -C`:
    /// `O_EXCL`, so two calls never share a number and the number is the call order),
    /// writes its argv there (each argument ended by NUL), and only then creates
    /// `<n>.done`. [`TestEnv::calls`] reads the `.done` records, so it never sees a
    /// call half written.
    pub fn fake(&self, name: &str, body: &str) -> PathBuf {
        let path = self.bin().join(name);
        let dir = self.calls_dir(name);
        fs::create_dir_all(&dir).expect("create calls dir");
        let script = format!(
            r#"#!/bin/sh
_d='{dir}'
_n=1
for _f in "$_d"/*.rec; do [ -e "$_f" ] && _n=$((_n+1)); done
set -C
until for _a in "$@"; do printf '%s\0' "$_a"; done 2>/dev/null > "$_d/$_n.rec"; do
  [ -e "$_d/$_n.rec" ] || {{ echo "fake {name}: cannot record the call" >&2; exit 97; }}
  _n=$((_n+1))
done
true > "$_d/$_n.done"
set +C
unset _d _n _f _a
{body}
"#,
            dir = dir.display(),
        );
        write_executable(&path, &script);
        path
    }

    /// A fake `cpd` on `PATH` for panes that hold views: `cpd tui view chat` and `cpd tui view
    /// history <id>` just sleep; everything else is the built binary.
    pub fn fake_cpd_views(&self) {
        self.fake("cpd", &format!("case \"$1 $2 $3\" in \"tui view chat\"|\"tui view history\") exec sleep {SLEEP_SECONDS};; esac\nexec '{CPD}' \"$@\""));
    }

    /// A fake `cpd` on `PATH` for panes that run a Session: `session run <id>` records its
    /// environment in `<root>/pane-<id>.env` and sleeps (the pane's program); `watch --ensure`
    /// runs the shell code `watch` (for example [`WATCH_OK`]); everything else is the built binary.
    pub fn fake_cpd_panes(&self, watch: &str) {
        self.fake(
            "cpd",
            &format!(
                "case \"$1 $2\" in\n\
                 \"session run\") env > '{root}/pane-'\"$3\".env; exec sleep {SLEEP_SECONDS};;\n\
                 \"watch --ensure\") {watch};;\n\
                 esac\nexec '{CPD}' \"$@\"",
                root = self.root().display()
            ),
        );
    }

    /// A fake `pi` on `PATH`.
    pub fn fake_pi(&self, body: &str) -> PathBuf {
        self.fake("pi", body)
    }

    /// A fake `gh` on `PATH`.
    pub fn fake_gh(&self, body: &str) -> PathBuf {
        self.fake("gh", body)
    }

    /// Every call of the fake `name` so far, each as its argv (without the program),
    /// in the order the calls reached the recorder: calls that do not overlap in time
    /// are in the order they ran; overlapping calls are in some one fixed order.
    pub fn calls(&self, name: &str) -> Vec<Vec<String>> {
        let Ok(entries) = fs::read_dir(self.calls_dir(name)) else { return Vec::new() };
        // A call `<n>` is complete once `<n>.done` exists; `<n>.rec` holds its argv.
        let mut records: Vec<(u64, PathBuf)> = entries
            .map(|e| e.expect("read calls dir").path())
            .filter(|p| p.extension().is_some_and(|e| e == "done"))
            .filter_map(|p| Some((p.file_stem()?.to_str()?.parse().ok()?, p.with_extension("rec"))))
            .collect();
        records.sort();
        records
            .into_iter()
            .map(|(_, p)| {
                let text = String::from_utf8(fs::read(p).expect("read call record")).expect("calls are UTF-8");
                text.split_terminator('\0').map(str::to_owned).collect()
            })
            .collect()
    }

    /// The poll searches the fake `gh` has recorded: one per `cpd pr poll`, which engine E5 runs
    /// about once per tick when the engine runs with [`PR_POLL_EVERY_TICK`].
    pub fn poll_searches(&self) -> usize {
        self.calls("gh").iter().filter(|c| c.get(3).is_some_and(|q| q.contains("search(type"))).count()
    }

    /// Waits until the engine has run `ticks` whole ticks (E1-E8) since this call: the poll of
    /// a tick comes before E7, so once poll `n + 1` has begun, the tick of poll `n` is over.
    /// The positive signal an absence test waits for before it asserts that nothing happened.
    /// Needs an engine with [`PR_POLL_EVERY_TICK`] (else each whole tick waits for a whole `pr_poll_seconds`) and a
    /// fake `gh` that answers the search.
    pub fn wait_whole_ticks(&self, ticks: usize) {
        let from = self.poll_searches();
        wait_until(&format!("{ticks} whole engine ticks (polls {from} -> {})", from + ticks + 1), || self.poll_searches() > from + ticks);
    }

    fn calls_dir(&self, name: &str) -> PathBuf {
        self.bin().join(format!(".{name}.calls"))
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Kills the tmux server, the env's processes and removes its directory.
        self.teardown();
    }
}

/// A store as `cpd` creates it in an empty `$CPD_HOME` (migrated, checkpointed, no WAL), made
/// once per build of `cpd` and shared by the test processes: a file under Cargo's
/// `CARGO_TARGET_TMPDIR` named after the binary's size and modification time. A process that
/// finds none makes one in an [`TestEnv::empty`] env with the real binary and renames it into
/// place (atomic: concurrent makers each write a whole one).
fn store_template() -> &'static Path {
    static TEMPLATE: OnceLock<PathBuf> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        use std::os::unix::fs::MetadataExt;
        let binary = fs::metadata(CPD).expect("stat the cpd binary");
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let path = dir.join(format!("cpd-store-{}-{}.{}.db", binary.len(), binary.mtime(), binary.mtime_nsec()));
        if path.exists() {
            return path;
        }
        let env = TestEnv::empty();
        env.write_config("version = 1\nmachine = \"m1\"\n");
        env.cpd(&["events", "list"]).ok();
        let db = env.cpd_home().join("cpd.db");
        assert!(!env.cpd_home().join("cpd.db-wal").exists(), "cpd left a WAL beside the template store");
        let partial = dir.join(format!("cpd-store-{}.partial", std::process::id()));
        fs::copy(&db, &partial).expect("copy the new store");
        fs::rename(&partial, &path).expect("put the store template in place");
        path
    })
}

/// Where [`TestEnv::git_repo`] keeps its template of repo `name`: under Cargo's
/// `CARGO_TARGET_TMPDIR`, named after this test binary's size and modification time (the recipe
/// is in the binary), shared by its test processes.
fn git_repo_template(name: &str) -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    static KEY: OnceLock<String> = OnceLock::new();
    let key = KEY.get_or_init(|| {
        let binary = fs::metadata(std::env::current_exe().expect("this test binary")).expect("stat this test binary");
        format!("{}-{}.{}", binary.len(), binary.mtime(), binary.mtime_nsec())
    });
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("git-repo-{key}-{name}"))
}

/// Copies the directory tree `from` (directories and regular files) to the new directory `to`.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|e| panic!("create {}: {e}", to.display()));
    for entry in fs::read_dir(from).unwrap_or_else(|e| panic!("read {}: {e}", from.display())) {
        let entry = entry.expect("read a directory entry");
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type().expect("the entry's type").is_dir() {
            copy_tree(&src, &dst);
        } else {
            fs::copy(&src, &dst).unwrap_or_else(|e| panic!("copy {}: {e}", src.display()));
        }
    }
}

/// This process's user id: the owner of `/proc/self`.
fn uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata("/proc/self").expect("stat /proc/self").uid()
}

/// Whether `pid` is running (a zombie is dead; a pid with no `/proc` entry is gone).
fn alive(pid: Pid) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z') && !rest.starts_with('X')))
}

/// When `pid` started, in clock ticks since boot (`/proc/<pid>/stat` field 22); `None` when it is gone.
fn start_ticks(pid: Pid) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split(' ').nth(19)?.parse().ok()
}

/// The live processes of this user that started at or after `since` (every process when
/// `None`) and whose command line contains `dir` or whose cwd is under it, but the test process
/// and its ancestors: one pass over `/proc`, in this process (the teardown's search). Processes
/// of other users are skipped (they cannot be signalled), and so are the older ones without
/// reading their command line: on a busy machine those reads are the search's cost.
///
/// The kernel hands out pids in increasing order up to `pid_max`, then wraps. So while its
/// counter (`ns_last_pid`, read after the listing) is at or above `since.pid`, every process
/// started since has a pid between the two, and the entries outside are not even read; once it
/// has wrapped, every entry is (a whole cycle of `pid_max`, millions of processes, within one
/// env does not happen).
fn processes_under(dir: &Path, since: Option<Since>) -> Vec<Pid> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let uid = uid();
    let needle = dir.as_os_str().as_bytes();
    let mut protected = vec![nix::unistd::getpid()];
    let mut parent = nix::unistd::getppid();
    while parent.as_raw() > 1 && !protected.contains(&parent) {
        protected.push(parent);
        let Ok(stat) = fs::read_to_string(format!("/proc/{parent}/stat")) else { break };
        let Some(ppid) = stat.rsplit_once(") ").and_then(|(_, rest)| rest.split(' ').nth(1)?.parse().ok()) else { break };
        parent = Pid::from_raw(ppid);
    }
    let Ok(entries) = fs::read_dir("/proc") else { return Vec::new() };
    let listed: Vec<i32> = entries.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok()).collect();
    let last_pid: Option<i32> = fs::read_to_string("/proc/sys/kernel/ns_last_pid").ok().and_then(|s| s.trim().parse().ok());
    let pids = match (since, last_pid) {
        (Some(since), Some(last)) if last >= since.pid => since.pid..=last,
        _ => 2..=i32::MAX,
    };
    let min_ticks = since.map_or(0, |s| s.ticks);
    listed
        .into_iter()
        .filter(|pid| pids.contains(pid))
        .map(Pid::from_raw)
        .filter(|pid| pid.as_raw() > 1 && !protected.contains(pid))
        .filter(|pid| {
            let proc_dir = PathBuf::from(format!("/proc/{pid}"));
            let mine = || fs::metadata(&proc_dir).is_ok_and(|m| m.uid() == uid);
            let recent = || start_ticks(*pid).is_some_and(|t| t >= min_ticks);
            let by_cmdline = || fs::read(proc_dir.join("cmdline")).is_ok_and(|c| c.windows(needle.len()).any(|w| w == needle));
            let by_cwd = || fs::read_link(proc_dir.join("cwd")).is_ok_and(|cwd| cwd.starts_with(dir));
            mine() && recent() && (by_cmdline() || by_cwd()) && alive(*pid)
        })
        .collect()
}

fn assert_ok(program: &str, args: &[&str], out: &Output) {
    assert!(
        out.status.success(),
        "{program} {args:?} failed with {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}
