//! The integration-test harness: runs the built `cpd` in an isolated
//! environment and parses its JSON.
//!
//! Every `TestEnv` lives in its own temporary directory and holds:
//! - `$CPD_HOME` and `$HOME` there, never the real `~/.cpd` or home;
//! - a private tmux server: its own `TMUX_TMPDIR` and socket name, killed on drop,
//!   so the cockpit server `-L cpd` is never touched;
//! - a process group that every [`TestEnv::command`] joins, and a teardown (`teardown.sh`,
//!   run by `Drop` and by the reaper, the group's leader, when the test process is killed)
//!   that stops that group, the tmux server and the detached children cpd records, then
//!   removes the directory (see [`TEARDOWN_SH`]);
//! - a `bin/` directory first on `PATH`, for fake `pi` and `gh` scripts that
//!   record their calls; `gh`, `pi`, `curl`, `wget` and `ssh` start as shims that
//!   exit 97 (`no fake installed for <name>`), and no `GH_*`, `GITHUB_*`, `SSH_*`
//!   or proxy variable reaches a command, so no test reaches pi or the network;
//! - temporary git repos with a fixed identity and no global or system config.
//!
//! Use it from a test file with `use crate::common;`.
//!
//! Writing a script and executing it must not overlap with spawning in one test
//! process (ETXTBSY, see [`write_executable`]): write scripts with
//! [`write_executable`] and spawn only through [`spawn`] / [`output`].

#![allow(dead_code)] // Every test file compiles this module whole and uses a subset of it.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError, RwLock};
use std::time::{Duration, Instant};

use clap::Parser;
use cpd::cli::Cli;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::Value;
use tempfile::TempDir;

/// The cpd binary Cargo built for this test run.
pub const CPD: &str = env!("CARGO_BIN_EXE_cpd");

/// The longest any fake sleeps, so the leftovers of a killed test run end by themselves.
pub const SLEEP_SECONDS: u64 = 120;

/// The teardown script. It finds nothing by scanning: it stops what the env tracks.
///
/// - The env's process group: its leader is the reaper, and every [`TestEnv::command`] joins it.
/// - The private tmux server and its panes (a session each), whose pids tmux gives.
/// - The detached children cpd records (each started with `setsid`, so out of the group): the
///   drivers of the Attempts not ended (`attempts.driver_pid` in the env's store), and the engine's
///   distill runs and tmux restart (`run/context-digests/*/distill.pid`, `run/tmux-restart.pid`).
///   Each is signalled only while `/proc/<pid>/environ` holds the env's `CPD_HOME` (a recorded pid
///   may have been reused). They are stopped after the group (nothing is left to start more), and
///   the tmux server once more after them (a driver may have started it again).
///
/// A process that leaves the group (`setsid`, `setpgid`) and that cpd does not record is not
/// stopped. Each stop is SIGTERM, then SIGKILL to what is left after a grace of 1 s.
///
/// `sh teardown.sh <pgid>` tears the env down and removes its directory; `sh teardown.sh watch
/// <pid>` (the reaper, the group's leader) waits until `<pid>` is gone or the directory is, and
/// then tears down, from a session of its own, if the directory is still there. `dir`, `socket`
/// and `sqlite3` are prepended by [`TestEnv::new`]. Linux only (`/proc`). Idempotent.
const TEARDOWN_SH: &str = r#"
home=$dir/cpd-home

# alive PID: running (a zombie is dead).
alive() {
  _l=
  { read -r _l < "/proc/$1/stat"; } 2>/dev/null || return 1
  case ${_l##*) } in Z*|X*) return 1 ;; esac
}

# mine PID: PID runs with this env's CPD_HOME.
mine() { grep -qzxF "CPD_HOME=$home" "/proc/$1/environ" 2>/dev/null; }

# The pids cpd records for its detached children: the drivers of the Attempts not ended, the
# engine's distill runs and its tmux restart. Not checked with `mine` yet.
recorded() {
  if [ -f "$home/cpd.db" ]; then
    "$sqlite3" -readonly -cmd '.timeout 5000' "$home/cpd.db" \
      "SELECT driver_pid FROM attempts WHERE status <> 'ended' AND driver_pid IS NOT NULL"
  fi
  for _f in "$home"/run/context-digests/*/distill.pid "$home/run/tmux-restart.pid"; do
    [ -f "$_f" ] && read -r _p < "$_f" && printf '%s\n' "$_p"
  done
}

# left TARGET...: some TARGET (a -PGID or a PID) is still there.
left() {
  for _t; do kill -0 "$_t" 2>/dev/null && return 0; done
  return 1
}

# stop TARGET...: SIGTERM to each -PGID (a process group) and PID, then SIGKILL to what is
# left after the grace; to a PID only while it is still this env's.
stop() {
  [ $# -gt 0 ] || return 0
  kill -TERM "$@" 2>/dev/null
  _n=0
  while [ "$_n" -lt 20 ] && left "$@"; do sleep 0.05; _n=$((_n + 1)); done
  for _t; do
    case $_t in -*) kill -KILL "$_t" ;; *) ! mine "$_t" || kill -KILL "$_t" ;; esac
  done 2>/dev/null
}

# stop_tmux: stops the tmux server and its panes (a session each), whose pids tmux gives.
stop_tmux() {
  for _s in "$dir"/tmux/*/"$socket"; do
    [ -S "$_s" ] || return 0
    _g=
    for _p in $(TMUX_TMPDIR="$dir/tmux" tmux -L "$socket" list-panes -a -F '#{pid} #{pane_pid}' 2>/dev/null); do
      _g="$_g -$_p"
    done
    TMUX_TMPDIR="$dir/tmux" tmux -L "$socket" kill-server >/dev/null 2>&1
    stop $_g
  done
}

# teardown PGID: stops the env's group and its tmux server, then what cpd detached (now that
# nothing is left to start more), then the tmux server again (a driver may have started one
# since), and removes the directory.
teardown() {
  unset TMUX
  stop "-$1"
  stop_tmux
  _r=
  for _p in $(recorded); do
    ! mine "$_p" || _r="$_r -$_p $_p"
  done
  stop $_r
  stop_tmux
  rm -rf "$dir"
}

if [ "${1-}" = watch ]; then
  while [ -d "$dir" ] && alive "$2"; do sleep 1; done
  [ -d "$dir" ] || exit 0
  # The teardown stops this group, so it runs outside it.
  exec setsid sh "$0" "$$" </dev/null >/dev/null 2>&1
fi
teardown "$1"
exit 0
"#;

/// Starts the reaper `sh <script> watch <this process>`, the leader of a new process group
/// (the env's), with stdio on /dev/null: it runs the teardown once this test process is gone,
/// so a killed test leaves nothing behind. In the test process's session, which a group that
/// a command joins must be.
fn start_reaper(script: &Path) -> Child {
    let mut cmd = Command::new("sh");
    cmd.arg(script)
        .arg("watch")
        .arg(std::process::id().to_string())
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    spawn(&mut cmd).expect("start the reaper")
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
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

/// How often a wait looks again.
const POLL: Duration = Duration::from_millis(100);

/// Waits until `ready` is true, at most [`WAIT`]; panics `timed out waiting for <what>`.
pub fn wait_until(what: &str, ready: impl FnMut() -> bool) {
    wait_until_for(what, WAIT, ready);
}

/// [`wait_until`] with its own `timeout`.
pub fn wait_until_for(what: &str, timeout: Duration, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
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
    let deadline = Instant::now() + timeout;
    loop {
        match ready() {
            Ok(value) => return value,
            Err(state) => assert!(Instant::now() < deadline, "timed out waiting for {what}: {state}"),
        }
        std::thread::sleep(POLL);
    }
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
    /// The env's process group: the reaper's pid.
    pgid: u32,
    /// The reaper ([`TestEnv::new`]), until the teardown stops it.
    reaper: Mutex<Option<Child>>,
}

/// The `sqlite3` CLI, which the teardown reads the env's store with: found on `PATH` at the
/// first [`TestEnv::new`] of the test process, which panics when there is none.
fn sqlite3() -> &'static Path {
    static SQLITE3: OnceLock<PathBuf> = OnceLock::new();
    SQLITE3.get_or_init(|| {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .map(|dir| dir.join("sqlite3"))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("the test harness needs the sqlite3 CLI on PATH (its teardown reads the env's store); install sqlite3"))
    })
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
    pub fn new() -> TestEnv {
        let sqlite3 = sqlite3();
        let dir = tempfile::Builder::new().prefix("cpd-test-").tempdir().expect("temp dir");
        let n = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        let tmux_socket = format!("cpdt-{}-{n}", std::process::id());
        let script = dir.path().join("teardown.sh");
        fs::write(
            &script,
            format!(
                "dir={}\nsocket={}\nsqlite3={}\n{TEARDOWN_SH}",
                sh_quote(&dir.path().to_string_lossy()),
                sh_quote(&tmux_socket),
                sh_quote(&sqlite3.to_string_lossy())
            ),
        )
        .expect("write teardown script");
        let reaper = start_reaper(&script);
        let env = TestEnv { dir, tmux_socket, pgid: reaper.id(), reaper: Mutex::new(Some(reaper)) };
        for sub in ["cpd-home", "home", "bin", "tmux", "repos"] {
            fs::create_dir_all(env.dir.path().join(sub)).expect("create test dir");
        }
        for tool in DENIED_TOOLS {
            let path = env.bin().join(tool);
            write_executable(&path, &format!("#!/bin/sh\necho 'no fake installed for {tool}' >&2\nexit {DENIED_EXIT}\n"));
        }
        // Every test env has the owner `acme` (an empty policy file: cpd's default), for `context new --owner acme`.
        fs::create_dir_all(env.cpd_home().join("policy/owners")).expect("create the owners directory");
        fs::write(env.cpd_home().join("policy/owners/acme.toml"), "").expect("write the acme owner file");
        fs::write(
            env.home().join(".gitconfig"),
            "[user]\n\tname = cpd test\n\temail = test@cpd.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .expect("write .gitconfig");
        env
    }

    fn teardown_script(&self) -> PathBuf {
        self.root().join("teardown.sh")
    }

    /// Stops the reaper, then runs the teardown ([`TEARDOWN_SH`]): stops the env's process
    /// group, its tmux server and the detached children cpd records, then removes the
    /// directory. Idempotent.
    pub fn teardown(&self) {
        if let Some(mut reaper) = self.reaper.lock().unwrap_or_else(PoisonError::into_inner).take() {
            // By its pid alone, and reaped: a zombie in the group would hold the teardown for its grace.
            let _ = reaper.kill();
            let _ = reaper.wait();
        }
        if self.teardown_script().exists() {
            // stderr stays the test's: a failed read of the store shows in its output.
            let mut teardown = Command::new("sh");
            teardown.arg(self.teardown_script()).arg(self.pgid.to_string()).current_dir("/").stdin(Stdio::null()).stdout(Stdio::null());
            let _ = spawn(&mut teardown).and_then(|mut child| child.wait());
        }
        let _ = fs::remove_dir_all(self.root());
    }

    /// The temporary directory everything lives in.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Replaces the owner `owner`'s policy file (every test env has `acme`, an empty one) with `toml`.
    pub fn owner_policy(&self, owner: &str, toml: &str) {
        fs::write(self.cpd_home().join(format!("policy/owners/{owner}.toml")), toml).expect("write the owner policy file");
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
    /// `CPD_HOME`, `HOME`, `PATH`, `TMUX_TMPDIR` and git's config set. It runs in the env's
    /// process group, which the teardown stops: a process that leaves it (`setsid`) is
    /// stopped only when cpd records it ([`TEARDOWN_SH`]).
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
            .stdin(Stdio::null())
            .process_group(self.pgid as i32);
        cmd
    }

    /// The store of this env's `$CPD_HOME`, opened (migrated) now.
    pub fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.cpd_home().into()), None).unwrap(), 5000).unwrap()
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
        let out = output(self.command(CPD).args(args).current_dir(cwd)).expect("run cpd");
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

    /// A new git repo `repos/<name>` on branch `main` with one commit (`README.md`).
    pub fn git_repo(&self, name: &str) -> PathBuf {
        let repo = self.root().join("repos").join(name);
        fs::create_dir_all(&repo).expect("create repo dir");
        self.git(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join("README.md"), format!("# {name}\n")).expect("write README");
        self.git(&repo, &["add", "-A"]);
        self.git(&repo, &["commit", "-q", "-m", "initial"]);
        repo
    }

    /// `git_repo` that `pr add` can key: its `origin` is `https://github.com/acme/<name>.git`
    /// (`acme` is every test Context's owner), with `refs/remotes/origin/main` at the commit.
    /// Nothing is reachable at that URL: see `local_origin`.
    pub fn keyed_repo(&self, name: &str) -> PathBuf {
        let repo = self.git_repo(name);
        self.github_origin(&repo);
        repo
    }

    /// The configured URL of `origin`, a GitHub one: adds `acme/<the repo's directory name>` when the
    /// repo has no `origin`, e.g. after `drop_unreachable_origin`.
    pub fn github_origin(&self, repo: &Path) -> String {
        if self.git(repo, &["remote"]).lines().any(|r| r == "origin") {
            return self.git(repo, &["config", "--get", "remote.origin.url"]);
        }
        let name = repo.file_name().expect("a repo directory").to_string_lossy().into_owned();
        let url = format!("https://github.com/acme/{name}.git");
        self.git(repo, &["remote", "add", "origin", &url]);
        self.git(repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        url
    }

    /// Makes git reach `origin` at the local bare repo `bare`, while its configured URL stays a
    /// GitHub one (the repo's key): `url.<bare>.insteadOf = <that URL>`.
    pub fn local_origin(&self, repo: &Path, bare: &Path) {
        let url = self.github_origin(repo);
        self.git(repo, &["config", &format!("url.{}.insteadOf", bare.display()), &url]);
    }

    /// Removes `origin` from a repo whose `origin` is only the GitHub URL `keyed_repo` gave it
    /// (no `local_origin`): for what runs after `pr add` and must find no remote, as before keying.
    pub fn drop_unreachable_origin(&self, repo: &Path) {
        // Exit 1 and no output when nothing matches.
        let rewritten = output(self.command("git").args(["config", "--get-regexp", r"^url\..*\.insteadof$"]).current_dir(repo))
            .expect("git config");
        if rewritten.stdout.is_empty() && self.git(repo, &["remote"]).lines().any(|r| r == "origin") {
            self.git(repo, &["remote", "remove", "origin"]);
        }
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
    /// once per tick when the config says `tick_seconds = 1` and `pr_poll_seconds = 1`.
    pub fn poll_searches(&self) -> usize {
        self.calls("gh").iter().filter(|c| c.get(3).is_some_and(|q| q.contains("search(type"))).count()
    }

    /// Waits until the engine has run `ticks` whole ticks (E1-E8) since this call: the poll of
    /// a tick comes before E7, so once poll `n + 1` has begun, the tick of poll `n` is over.
    /// The positive signal an absence test waits for before it asserts that nothing happened.
    /// Needs `tick_seconds = 1` and `pr_poll_seconds = 1` and a fake `gh` that answers the search.
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

fn assert_ok(program: &str, args: &[&str], out: &Output) {
    assert!(
        out.status.success(),
        "{program} {args:?} failed with {}: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}
