//! The integration-test harness: runs the built `cpd` in an isolated
//! environment and parses its JSON.
//!
//! Every `TestEnv` lives in its own temporary directory and holds:
//! - `$CPD_HOME` and `$HOME` there, never the real `~/.cpd` or home;
//! - a private tmux server: its own `TMUX_TMPDIR` and socket name, killed on drop,
//!   so the cockpit server `-L cpd` is never touched;
//! - a `bin/` directory first on `PATH`, for fake `pi` and `gh` scripts that
//!   record their calls, so no test reaches pi or the network;
//! - temporary git repos with a fixed identity and no global or system config.
//!
//! Use it from a test file with `mod common;`.
//!
//! Writing a script and executing it must not overlap with spawning in one test
//! process (ETXTBSY, see [`write_executable`]): write scripts with
//! [`write_executable`] and spawn only through [`spawn`] / [`output`].

#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{PoisonError, RwLock};

use serde_json::Value;
use tempfile::TempDir;

/// The cpd binary Cargo built for this test run.
pub const CPD: &str = env!("CARGO_BIN_EXE_cpd");

/// The longest any fake sleeps, so the leftovers of a killed test run end by themselves.
pub const SLEEP_SECONDS: u64 = 120;

static NEXT_SOCKET: AtomicUsize = AtomicUsize::new(0);

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

pub struct TestEnv {
    dir: TempDir,
    tmux_socket: String,
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
        let dir = tempfile::Builder::new().prefix("cpd-test-").tempdir().expect("temp dir");
        let n = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        let env = TestEnv { dir, tmux_socket: format!("cpdt-{}-{n}", std::process::id()) };
        for sub in ["cpd-home", "home", "bin", "tmux", "repos"] {
            fs::create_dir_all(env.dir.path().join(sub)).expect("create test dir");
        }
        fs::write(
            env.home().join(".gitconfig"),
            "[user]\n\tname = cpd test\n\temail = test@cpd.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .expect("write .gitconfig");
        env
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
    /// `CPD_*`, `PI_*`, `GIT_*`, `XDG_*`, `PLANNOTATOR_*`, `TMUX` and `TMUX_PANE` removed, then
    /// `CPD_HOME`, `HOME`, `PATH`, `TMUX_TMPDIR` and git's config set.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut cmd = Command::new(program);
        for (key, _) in std::env::vars_os() {
            let k = key.to_string_lossy();
            let drop = ["CPD_", "PI_", "GIT_", "XDG_", "PLANNOTATOR_"].iter().any(|p| k.starts_with(p))
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

    fn calls_dir(&self, name: &str) -> PathBuf {
        self.bin().join(format!(".{name}.calls"))
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Fails harmlessly when no server was started.
        let _ = spawn(self.command("tmux").arg("-L").arg(&self.tmux_socket).arg("kill-server").stderr(Stdio::null()))
            .and_then(|mut child| child.wait());
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
