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
//!   after it comes the test process's own `PATH` without any cpd wrapper directory
//!   ([`inherited_path`]), so a run gives the same result from any Session;
//! - temporary git repos with a fixed identity and no global or system config.
//!
//! Use it from a test file with `use crate::common;`.
//!
//! Writing a script and executing it must not overlap with spawning in one test
//! process (ETXTBSY, see [`write_executable`]): write scripts with
//! [`write_executable`] and spawn only through [`spawn`] / [`output`].

#![allow(dead_code)] // Every test file compiles this module whole and uses a subset of it.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Mutex, Once, OnceLock, PoisonError, RwLock};
use std::time::Duration;

use clap::Parser;
use cpd::cli::Cli;
use cpd_core::clock::{Clock, SystemClock};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::git_guard;
use cpd_core::store::Store;
use serde_json::Value;
use tempfile::TempDir;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub mod causes;
pub mod cockpit;
pub mod engine;
pub mod fx;
pub mod healthy;
pub mod inbox;
pub mod newer_migration;
pub mod pi;
pub mod screen;
pub mod session;
pub mod tree;
pub mod wrapper;

/// The cpd binary Cargo built for this test run.
pub const CPD: &str = env!("CARGO_BIN_EXE_cpd");

/// Shell for a fake `pi`: `reply` reads the answer text on stdin and prints pi's `--mode json` stream for a one-shot
/// run (shapes recorded from pi 1.3.0) whose final assistant message holds it.
pub const PI_REPLY: &str = r##"reply() {
  text=$(sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/$/\\n/' | tr -d '\n')
  printf '%s\n' '{"type":"session","version":3,"id":"x","timestamp":"2026-10-19T01:59:25.851Z","cwd":"/"}' '{"type":"agent_start"}'
  printf '{"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm"},{"type":"text","text":"%s"}],"usage":{"input":2,"output":4,"cacheRead":100,"cacheWrite":631,"reasoning":1,"totalTokens":737,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0.25}},"stopReason":"stop"}}\n' "$text"
  printf '%s\n' '{"type":"agent_settled","aborted":false}'
}"##;

/// Shell for the curating distiller's answer (contract §4.6 step 4): one valid JSON line, with no retirement and no
/// doc candidate, for every record of `batch.md` (the current directory is the Context's digest directory).
pub const CURATES_ALL: &str = r##"while IFS= read -r line; do
  case "$line" in "["*) id=${line%%]*}; id=${id#[}
    echo "{\"id\":\"$id\",\"topic\":\"t\",\"short\":\"s $id\",\"retire\":null}" ;;
  esac
done < batch.md"##;

/// A fake `pi` that prints `answer` (shell) as the final assistant message of a JSON stream.
pub fn pi_says(answer: &str) -> String {
    format!("{PI_REPLY}\n{{\n{answer}\n}} | reply")
}

/// The longest any fake sleeps, so the leftovers of a killed test run end by themselves.
pub const SLEEP_SECONDS: u64 = 120;

/// The engine tests' tick, `watch.tick_seconds` (its minimum, contract §8.2): an absence window of a few whole ticks
/// ([`TestEnv::wait_whole_ticks`]) is a few tenths of a second.
pub const TICK_SECONDS: &str = "0.1";

/// The variable an engine test starts `cpd watch` with, so that E5 polls [`TICK_SECONDS`] after the previous poll
/// ended (`CPD_TEST_PR_POLL`, contract §1.2 "Test overrides"), not every `pr_poll_seconds` (whole seconds): one poll
/// about every tick, which [`TestEnv::wait_whole_ticks`] counts.
pub const PR_POLL_EVERY_TICK: (&str, &str) = ("CPD_TEST_PR_POLL", TICK_SECONDS);

/// The poll periods of a whole Plan's processes, as test overrides (contract §1.2), for a test that runs an engine or
/// a Plan end to end: the engine's E5 every tick and the wrappers (pi's file, a fork's source) every 50 ms. What such a
/// test waits for is the next look of each. (The drivers' polls and kill pauses have no override: a test of a driver
/// runs it in this process on the `ManualClock`.)
pub const PLAN_POLLS: [(&str, &str); 3] = [
    PR_POLL_EVERY_TICK,
    ("CPD_TEST_FILE_POLL", "0.05"),
    ("CPD_TEST_SOURCE_POLL", "0.05"),
];

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
/// stopped, except in an env that had a tmux server (`tmux_used`): there the last step stops every process
/// that runs with the env's `CPD_HOME` (the server and its panes inherit it), so that a pane of its tmux
/// never outlives the test, even if `list-panes` failed or the server's SIGHUP came late (a pane the
/// Gate's leak check, `scripts/gate-run.sh`, found as `zsh -c sleep 120`). Each stop is SIGTERM, then SIGKILL to what is left after a grace of 1 s.
///
/// `sh teardown.sh <pgid>` tears the env down and removes its directory; `sh teardown.sh watch
/// <pid>` (the reaper, the group's leader) waits until `<pid>` is gone or the directory is, and
/// then tears down, from a session of its own, if the directory is still there. `dir`, `socket`,
/// `tmuxdir` and `sqlite3` are prepended by [`TestEnv::new`]. Idempotent.
///
/// On Linux liveness and the environment come from `/proc` and the second stage starts with
/// `setsid`. On macOS (`uname` is Darwin; no /proc, no `setsid`) `alive` is `kill -0` plus
/// `ps -o stat=` (a Z is dead), `mine` reads `ps -E -o command=` (an Apple platform binary hides its
/// environment there: with none shown `mine` is true only if its command line holds the env's directory), and the second
/// stage starts a session with perl's `POSIX::setsid`.
const TEARDOWN_SH: &str = r#"
home=$dir/cpd-home

# alive PID: running (a zombie is dead).
alive() {
  if [ "$(uname)" = Darwin ]; then
    kill -0 "$1" 2>/dev/null || return 1
    _l=$(ps -o stat= -p "$1" 2>/dev/null) || return 1
    case $_l in ""|*Z*|*X*) return 1 ;; esac
    return 0
  fi
  _l=
  { read -r _l < "/proc/$1/stat"; } 2>/dev/null || return 1
  case ${_l##*) } in Z*|X*) return 1 ;; esac
}

# mine PID: PID runs with this env's CPD_HOME.
mine() {
  if [ "$(uname)" = Darwin ]; then
    _l=$(ps -E -p "$1" -o command= 2>/dev/null) || return 1
    case $_l in *" CPD_HOME=$home"|*" CPD_HOME=$home "*) return 0 ;; esac
    # An environment shown without this CPD_HOME is another's. None shown (no PATH or HOME) is a platform
    # binary hiding it: then only its command line can say, and it is the env's only if it holds the env's directory.
    # (A recorded pid of a driver that has ended is often another process by now: pids are recycled within minutes.)
    case $_l in *" PATH="*|*" HOME="*) return 1 ;; esac
    _c=$(ps -o command= -p "$1" 2>/dev/null) || return 1
    case $_c in *"$dir"*) return 0 ;; esac
    return 1
  fi
  grep -qzxF "CPD_HOME=$home" "/proc/$1/environ" 2>/dev/null
}

# The pids cpd records for its detached children: the drivers of the Attempts not ended, the
# engine's distill runs and its tmux restart. Not checked with `mine` yet.
recorded() {
  if [ -f "$home/cpd.db" ]; then
    # macOS: a WAL store with no -wal/-shm file cannot be opened -readonly (CANTOPEN); it is opened
    # for writing with query_only, which creates the two files in the env's directory.
    if [ "$(uname)" = Darwin ]; then
      "$sqlite3" -cmd 'PRAGMA query_only=1' -cmd '.timeout 5000' "$home/cpd.db" \
        "SELECT driver_pid FROM attempts WHERE status <> 'ended' AND driver_pid IS NOT NULL"
    else
      "$sqlite3" -readonly -cmd '.timeout 5000' "$home/cpd.db" \
        "SELECT driver_pid FROM attempts WHERE status <> 'ended' AND driver_pid IS NOT NULL"
    fi
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

# stop_tmux: stops the tmux server and its panes (a session each), whose pids tmux gives. The
# supervising hooks go first: a hook that runs while the server dies starts a new server, where
# no teardown looks (hygiene.md 3a).
stop_tmux() {
  # The env's socket and the ones a test derives from its name (`<socket>-outer-<name>`).
  for _s in "$tmuxdir"/tmux-*/"$socket" "$tmuxdir"/tmux-*/"$socket"-*; do
    [ -S "$_s" ] || continue
    _n=${_s##*/}
    for _h in pane-died window-unlinked; do
      TMUX_TMPDIR="$tmuxdir" tmux -L "$_n" set-hook -gu "$_h" >/dev/null 2>&1
    done
    _g=
    for _p in $(TMUX_TMPDIR="$tmuxdir" tmux -L "$_n" list-panes -a -F '#{pid} #{pane_pid}' 2>/dev/null); do
      _g="$_g -$_p"
    done
    TMUX_TMPDIR="$tmuxdir" tmux -L "$_n" kill-server >/dev/null 2>&1
    stop $_g
  done
}

# tmux_used: this env ever had a tmux server: its socket file is there (a server that was killed
# leaves its socket file behind, so a test that killed its own server still shows).
tmux_used() {
  for _s in "$tmuxdir"/tmux-*/"$socket" "$tmuxdir"/tmux-*/"$socket"-*; do
    [ -S "$_s" ] && return 0
  done
  return 1
}

# tagged: the pids of the processes that run with this env's CPD_HOME, but this one: the tmux server
# and its panes inherit it from the cpd command or the test that started the server.
tagged() {
  if [ "$(uname)" = Darwin ]; then
    ps -axww -E -o pid=,command= 2>/dev/null | while read -r _p _c; do
      case " $_c " in *" CPD_HOME=$home "*) [ "$_p" -eq "$$" ] || echo "$_p" ;; esac
    done
  else
    for _e in /proc/[0-9]*/environ; do
      _p=${_e#/proc/}
      _p=${_p%/environ}
      { grep -qzxF "CPD_HOME=$home" "$_e" && [ "$_p" -ne "$$" ] && echo "$_p"; } 2>/dev/null
    done
  fi
}

# teardown PGID: stops the env's group and its tmux server, then what cpd detached (now that
# nothing is left to start more), then the tmux server again (a driver may have started one
# since), then, for an env that had a tmux server, every process left with the env's CPD_HOME
# (a pane that `list-panes` did not give, or that outlived its server's SIGHUP), and removes the directory.
teardown() {
  unset TMUX
  _tmux=
  ! tmux_used || _tmux=1
  stop "-$1"
  stop_tmux
  _r=
  for _p in $(recorded); do
    ! mine "$_p" || _r="$_r -$_p $_p"
  done
  stop $_r
  stop_tmux
  [ -z "$_tmux" ] || stop $(tagged)
  # A tmux dir outside the env (a long env path): only this env's socket file goes, never tmux's own directory.
  [ "$tmuxdir" = "$dir/tmux" ] || rm -f "$tmuxdir/tmux-$(id -u)/$socket" "$tmuxdir/tmux-$(id -u)/$socket"-*
  # The fake tmux's state files of this socket and the derived ones (`fake-tmux-<socket>.db*`, `fake-tmux-<socket>-*`).
  rm -f "$tmuxdir/fake-tmux-$socket".db* "$tmuxdir/fake-tmux-$socket"-*
  rm -rf "$dir"
}

if [ "${1-}" = watch ]; then
  while [ -d "$dir" ] && alive "$2"; do sleep 1; done
  [ -d "$dir" ] || exit 0
  # The teardown stops this group, so it runs outside it.
  if [ "$(uname)" = Darwin ]; then
    # No setsid command: perl forks (this process leads its group, so it cannot setsid), the
    # child starts a session and runs the teardown, the parent returns at once, like setsid.
    exec perl -MPOSIX -e 'fork and exit 0; POSIX::setsid() != -1 or exit 1; exec @ARGV' sh "$0" "$$" </dev/null >/dev/null 2>&1
  fi
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

/// Where every env's directory is made: the `TMPDIR` of the test process when set (the Gate's run directory,
/// `scripts/gate-run.sh`, which fails the run for what is left in it), else a tmpfs, so the `fsync`s of the store's writes (several per `cpd` command
/// that writes) and git's cost no disk flush. On a disk shared with builds and other test runs one such `fsync` waits
/// for their writeback, from tens of ms to seconds; no test claims durability across a power loss.
fn envs_dir() -> PathBuf {
    // Normalised: a TMPDIR that ends in a slash, joined with `/name` by a script, makes a `//` the components drop.
    std::env::var_os("TMPDIR")
        .filter(|d| !d.is_empty())
        .map_or_else(
            || PathBuf::from("/dev/shm"),
            |d| PathBuf::from(d).components().collect(),
        )
}

/// A `TestEnv` root this process made, in the registry [`sweep_envs`] walks at process exit.
struct EnvRoot {
    root: PathBuf,
    next: AtomicPtr<EnvRoot>,
}

/// Every env root this process made, newest first. A bare env ([`TestEnv::bare`], every `Fx`) starts no reaper, so a
/// test process that ends without dropping its `TestEnv` (a leaked `Fx` held by a `'static` or a detached thread, a
/// `mem::forget`, a `std::process::exit`) would leave its `cpd-test-*` for the Gate's leak check. The [`sweep_envs`]
/// registered here removes it. A reaped env is torn down by its `Drop`, or by its reaper when the test is killed; this
/// only ever removes a directory of this process that is still there.
static ENV_ROOTS: AtomicPtr<EnvRoot> = AtomicPtr::new(std::ptr::null_mut());

/// The `atexit` handler: removes every env root still on disk. Lock-free and allocation-light, so it cannot deadlock
/// or panic the exiting thread.
extern "C" fn sweep_envs() {
    let mut node = ENV_ROOTS.load(Ordering::SeqCst);
    while !node.is_null() {
        // SAFETY: every node is a `Box` leaked by `register_env` and never freed; only its fields are read here.
        let found = unsafe { &*node };
        let _ = fs::remove_dir_all(&found.root);
        node = found.next.load(Ordering::SeqCst);
    }
}

/// Puts `root` on the exit-sweep list, registering [`sweep_envs`] on the first call. See [`ENV_ROOTS`].
fn register_env(root: &Path) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: `sweep_envs` is an `extern "C"` function of no arguments that touches no lock and cannot unwind;
        // `atexit` calls it once at process exit, on every way out including `std::process::exit`.
        unsafe { nix::libc::atexit(sweep_envs) };
    });
    let node = Box::into_raw(Box::new(EnvRoot {
        root: root.to_path_buf(),
        next: AtomicPtr::new(std::ptr::null_mut()),
    }));
    loop {
        let head = ENV_ROOTS.load(Ordering::SeqCst);
        // SAFETY: `node` is our own fresh `Box`, and its `next` is written only before the CAS publishes it.
        unsafe { (*node).next.store(head, Ordering::SeqCst) };
        if ENV_ROOTS
            .compare_exchange(head, node, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return;
        }
    }
}

/// The Gate's run id (`scripts/gate-run.sh`), which every socket name carries so that the run finds its own.
fn gate_run_id() -> Option<String> {
    std::env::var("GATE_RUN").ok().filter(|id| !id.is_empty())
}

static NEXT_SOCKET: AtomicUsize = AtomicUsize::new(0);

/// Whether `dir` holds a cpd `git` wrapper, of any version: they all start with [`git_guard::SIGNATURE`].
fn is_wrapper_dir(dir: &Path) -> bool {
    let Ok(file) = fs::File::open(dir.join("git")) else {
        return false;
    };
    let mut head = Vec::new();
    file.take(git_guard::SIGNATURE.len() as u64)
        .read_to_end(&mut head)
        .is_ok_and(|_| head == git_guard::SIGNATURE.as_bytes())
}

/// `path` without the directories that hold a cpd `git` wrapper.
pub fn without_wrapper_dirs(path: &OsStr) -> OsString {
    std::env::join_paths(std::env::split_paths(path).filter(|dir| !is_wrapper_dir(dir)))
        .expect("a PATH without some of its entries")
}

/// The test process's own `PATH` without any cpd wrapper directory, the one `PATH` every test builds on. A Session
/// that runs the tests (a Lead running the Gate) has its `$CPD_HOME/bin` first on its `PATH`, and a test's `git` would
/// find that wrapper where it looks for the real git: a fake `git` that execs "the real git" would exec the wrapper,
/// which would find the fake again, for ever; a `PATH` narrowed to a few tools would hold the wrapper and no git.
/// Read it here, never `std::env::var_os("PATH")`, to find a real tool or to build a `PATH`.
pub fn inherited_path() -> &'static OsStr {
    static PATH: OnceLock<OsString> = OnceLock::new();
    PATH.get_or_init(|| without_wrapper_dirs(&std::env::var_os("PATH").unwrap_or_default()))
}

/// The tools a test never runs for real: `TestEnv::new` installs a deny shim for each.
pub const DENIED_TOOLS: [&str; 5] = ["gh", "pi", "curl", "wget", "ssh"];

/// What a fake `cpd watch --ensure` prints when the engine started.
pub const WATCH_OK: &str = "echo '{\"ok\": true, \"started\": true, \"pane\": \"%9\"}'; exit 0";

/// The exit status of a deny shim.
pub const DENIED_EXIT: i32 = 97;

/// Pull request `acme/pay` #812 as the poll's queries print it: open, titled `Pay the bills`, no review, no checks, with the
/// `mergeable` and `mergeStateStatus` GitHub reports (`MERGEABLE`/`CLEAN`, `CONFLICTING`/`DIRTY`, `MERGEABLE`/`BEHIND`, ...).
fn pay_node(mergeable: &str, merge_state: &str) -> String {
    format!(
        r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":812,"url":"https://github.com/acme/pay/pull/812","title":"Pay the bills","isDraft":false,"state":"OPEN","createdAt":"opened","updatedAt":"U1","mergedAt":null,"reviewDecision":null,"mergeable":"{mergeable}","mergeStateStatus":"{merge_state}","viewerDidAuthor":true,"viewerLatestReviewRequest":null,"commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    )
}

/// The comments read's answer when a test scripted none: an empty read (updatedAt `U1`) of every alias the query names.
pub fn empty_comments(query: &str) -> String {
    let empty = serde_json::json!({"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []});
    let mut data = serde_json::json!({"viewer": {"login": "me"}});
    for i in 0..query.matches(": repository(").count() {
        data[format!("p{i}")] = serde_json::json!({"pullRequest": {"id": format!("PR_{i}"), "updatedAt": "U1", "reviewThreads": empty, "comments": empty, "reviews": empty}});
    }
    serde_json::json!({"data": data}).to_string()
}

/// The PR poll's search answer (commands.md §9.2) for the one pull request a fixture stored, [`pay_node`]: the one thing a test of the
/// update Job varies is what GitHub says of its mergeability.
pub fn pay_search(mergeable: &str, merge_state: &str) -> String {
    format!(
        r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}"#,
        pay_node(mergeable, merge_state)
    )
}

/// The poll's lookup answer (§7.6 step 3), which `pr open` also reads for the pull request it just opened: [`pay_node`] as alias `p0`.
pub fn pay_lookup() -> String {
    format!(
        r#"{{"data":{{"p0":{{"pullRequest":{}}}}}}}"#,
        pay_node("MERGEABLE", "CLEAN")
    )
}

/// The proxy variables, dropped from every command (either case).
const PROXY_VARS: [&str; 3] = ["HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"];

/// Held exclusively while a script is being written, shared while a child is
/// being spawned. It guards no data: poisoning (a test panicked while holding
/// it) leaves nothing inconsistent, so it is ignored.
static SCRIPT_WRITE_VS_SPAWN: RwLock<()> = RwLock::new(());

/// The counter that names the temporary file [`write_executable`] and
/// [`copy_executable`] rename over their target: unique within this process.
static TEMP_NAME: AtomicUsize = AtomicUsize::new(0);

/// The path of a temporary file beside `path`, in the same directory so that the
/// rename over the target never crosses a filesystem.
fn temp_beside(path: &Path) -> PathBuf {
    let n = TEMP_NAME.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{name}.tmp-{}-{n}", std::process::id()))
}

/// Writes the executable script `path` (mode 0755).
///
/// The script is written to a temporary file beside `path` and renamed over it,
/// never opened in place: a file that is being executed (an ELF for its whole
/// life, a script until its interpreter has exec'd) cannot be opened for writing
/// and the open fails with ETXTBSY ("Text file busy"), while a rename over it is
/// allowed. The rename is atomic, so a reader sees the old or the new file, never
/// a half-written one.
///
/// Tests also run in parallel threads of one process. `fork` copies every open file
/// descriptor, and close-on-exec acts only at the child's `exec`. A child forked
/// by a concurrent test while the temporary file is open for writing therefore holds
/// a write descriptor to it for a moment, but that temporary file is never exec'd.
/// So the temporary file is written and closed under the exclusive lock, and every
/// spawn ([`spawn`], [`output`]) takes the shared lock until the child has exec'd.
/// Never retry instead.
pub fn write_executable(path: &Path, contents: &str) {
    let _exclusive = SCRIPT_WRITE_VS_SPAWN
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    let temp = temp_beside(path);
    fs::write(&temp, contents).expect("write script");
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o755)).expect("chmod script");
    fs::rename(&temp, path).expect("rename script over its target");
}

/// Copies the executable `from` to `to` (an install of a binary the test later runs), under
/// the same exclusive lock and for the same reason as [`write_executable`]: to a temporary
/// file beside `to` (which `fs::copy` gives the source's mode) and renamed over it, so a
/// running `to` is never opened for writing.
pub fn copy_executable(from: &str, to: &Path) {
    let _exclusive = SCRIPT_WRITE_VS_SPAWN
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    let temp = temp_beside(to);
    fs::copy(from, &temp).expect("copy executable");
    fs::rename(&temp, to).expect("rename executable over its target");
}

/// A fake holographic bridge (docs/research/context-holographic.md `## Bridge`): a shell script
/// cpd spawns as `<context_engine_python> <context_engine_bridge> [--db <path>]`, speaking JSON
/// lines. It logs every request to [`FakeBridge::requests`] and answers each command with the
/// content of `<responses>/<cmd>` (or `<responses>/<cmd>-<n>`, n from 1, for the nth call of a
/// repeated command such as `search`). A command with no response file answers `{"ok":true}`.
/// The caller scripts the engine's answers; a test never runs holographic.
pub struct FakeBridge {
    /// The directory the fake and its files live in.
    pub root: PathBuf,
    /// The script cpd runs as the bridge.
    pub script: PathBuf,
    /// `requests.jsonl`: every request line, in order.
    pub log: PathBuf,
    /// One file per command (and `<cmd>-<n>` per repeated call).
    pub responses: PathBuf,
    /// `context_engine_dir`: the fake's data directory; the fake ignores it.
    pub data_dir: PathBuf,
}

/// The shell body of [`FakeBridge`] (see its documentation).
const BRIDGE_SHIM: &str = r#"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  no_space=$(printf '%s' "$line" | tr -d ' ')
  cmd=$(printf '%s' "$no_space" | sed -n 's/.*"cmd":"\([A-Za-z_]*\)".*/\1/p')
  if [ "$cmd" = "search" ]; then
    n=$(cat "$resp/count-search" 2>/dev/null || echo 0)
    n=$((n + 1))
    printf '%s' "$n" > "$resp/count-search"
    f="$resp/search-$n"
    [ -f "$f" ] || f="$resp/search"
  else
    f="$resp/$cmd"
  fi
  if [ -f "$f" ]; then cat "$f"; else printf '%s\n' '{"ok":true}'; fi
done
"#;

impl FakeBridge {
    /// Writes the fake under `env.root()` and returns it.
    pub fn new(env: &TestEnv) -> FakeBridge {
        let root = env.root().join("fake-bridge");
        let bridge = FakeBridge {
            script: root.join("bridge.py"),
            log: root.join("requests.jsonl"),
            responses: root.join("responses"),
            data_dir: root.join("engine"),
            root,
        };
        fs::create_dir_all(&bridge.responses).expect("create the fake bridge responses");
        fs::create_dir_all(&bridge.data_dir).expect("create the fake engine data dir");
        bridge.install(BRIDGE_SHIM);
        bridge
    }

    /// Writes the bridge script with `body` as its shell body (the shim, or a failing one).
    pub fn install(&self, body: &str) {
        write_executable(
            &self.script,
            &format!(
                "#!/bin/sh\nlog={}\nresp={}\n{body}\n",
                sh_quote(&self.log.to_string_lossy()),
                sh_quote(&self.responses.to_string_lossy())
            ),
        );
    }

    /// Answers the first call of `cmd` with the one-line `json`.
    pub fn reply(&self, cmd: &str, json: &str) {
        fs::write(self.responses.join(cmd), format!("{json}\n")).expect("write a bridge response");
    }

    /// Answers the `n`th (1-based) call of `cmd` with the one-line `json`.
    pub fn reply_n(&self, cmd: &str, n: usize, json: &str) {
        fs::write(
            self.responses.join(format!("{cmd}-{n}")),
            format!("{json}\n"),
        )
        .expect("write a bridge response");
    }

    /// Every request line the fake answered, parsed.
    pub fn requests(&self) -> Vec<Value> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                serde_json::from_str(line)
                    .unwrap_or_else(|e| panic!("a request line is not JSON ({e}): {line}"))
            })
            .collect()
    }

    /// The `cmd` of every request the fake answered, in order.
    pub fn commands(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|request| request["cmd"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// The `config.toml` keys that point cpd at this fake: the bridge, `/bin/sh` as the interpreter,
    /// and the fake's data directory, with the engine enabled.
    pub fn config(&self) -> String {
        format!(
            "context_engine_enabled = true\ncontext_engine_bridge = \"{}\"\ncontext_engine_python = \"/bin/sh\"\ncontext_engine_dir = \"{}\"\n",
            self.script.display(),
            self.data_dir.display()
        )
    }
}

/// `cmd.spawn()`, never overlapping with [`write_executable`]. `Command::spawn`
/// returns once the child has exec'd (or failed to), so the shared lock covers
/// the whole window in which the child holds copies of this process's descriptors.
pub fn spawn(cmd: &mut Command) -> io::Result<Child> {
    let _shared = SCRIPT_WRITE_VS_SPAWN
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    cmd.spawn()
}

/// `cmd.output()` (stdout and stderr captured, stdin null), spawning through
/// [`spawn`]: the lock is released before the child is waited for.
pub fn output(cmd: &mut Command) -> io::Result<Output> {
    spawn(
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?
    .wait_with_output()
}

/// Like [`output`], but writes `stdin` to the child (then closes it) and bounds
/// the wait: the child is killed and reaped when it outlives `deadline`, and
/// `io::ErrorKind::TimedOut` is returned. The bridge tests cannot use [`output`],
/// which nulls stdin.
pub fn output_with_stdin(
    cmd: &mut Command,
    stdin: &[u8],
    deadline: Duration,
) -> io::Result<Output> {
    use std::io::Write;
    let mut child = spawn(
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    {
        let mut pipe = child.stdin.take().expect("stdin is piped");
        let _ = pipe.write_all(stdin);
        // Dropping the pipe closes it: the child sees EOF.
    }
    let done = poll(deadline, || child.try_wait().transpose());
    match done {
        Some(status) => {
            status?;
            child.wait_with_output()
        }
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the child did not exit before its deadline",
            ))
        }
    }
}

/// How long [`wait_until`] and [`wait_screen`] wait: the one wait policy of the suite, long
/// enough for a loaded machine. A wait that needs longer says so with [`wait_until_for`].
pub const WAIT: Duration = Duration::from_secs(30);

/// How often a wait looks again: 10 ms, so a wait ends at most a few ms after its signal (half a poll on average),
/// against a look that costs about 1 ms (a read of the store on a read-only connection, a file, `/proc`) up to a few
/// ms (a `tmux capture-pane`).
pub const POLL: Duration = Duration::from_millis(10);

/// Waits until `ready` is true, at most [`WAIT`]; panics `timed out waiting for <what>`.
pub fn wait_until(what: &str, ready: impl FnMut() -> bool) {
    wait_until_for(what, WAIT, ready);
}

/// [`wait_until`] with its own `timeout`.
pub fn wait_until_for(what: &str, timeout: Duration, ready: impl FnMut() -> bool) {
    wait_until_on(&SystemClock, what, timeout, ready);
}

/// [`wait_until_for`] on `clock`: a test of the wait itself passes a `ManualClock`, whose time is virtual.
pub fn wait_until_on(
    clock: &dyn Clock,
    what: &str,
    timeout: Duration,
    mut ready: impl FnMut() -> bool,
) {
    poll_on(clock, timeout, || ready().then_some(()))
        .unwrap_or_else(|| panic!("timed out waiting for {what}"));
}

/// The suite's one poll loop: looks at `ready` every [`POLL`] until it gives a value, and at
/// least once; `None` once `timeout` has passed without one. A wait panics on `None` ([`wait_until`],
/// [`wait_some`], or its own message); a check that something does not happen within a window
/// asserts `None`.
pub fn poll<T>(timeout: Duration, ready: impl FnMut() -> Option<T>) -> Option<T> {
    poll_on(&SystemClock, timeout, ready)
}

/// [`poll`] on `clock`: the time it reads and the naps it takes are the clock's.
pub fn poll_on<T>(
    clock: &dyn Clock,
    timeout: Duration,
    mut ready: impl FnMut() -> Option<T>,
) -> Option<T> {
    let deadline = clock.now() + timeout;
    loop {
        if let Some(value) = ready() {
            return Some(value);
        }
        if clock.now() >= deadline {
            return None;
        }
        clock.sleep(POLL);
    }
}

/// Waits until some process is blocked in `flock(2)` on the file `path` (a waiter in the
/// kernel's own table, `/proc/locks`: a line `<n>: -> FLOCK ... <dev>:<inode> ...`), at most
/// [`WAIT`]. The positive signal for "this call is waiting for the lock", where a test then
/// asserts what has not happened yet.
pub fn wait_for_flock_waiter(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    let inode = fs::metadata(path)
        .expect("the lock file exists")
        .ino()
        .to_string();
    wait_until(
        &format!("a process to wait for the flock on {}", path.display()),
        || {
            let locks = fs::read_to_string("/proc/locks").expect("read /proc/locks");
            locks.lines().any(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                fields.contains(&"->")
                    && fields.iter().any(|f| {
                        f.rsplit(':').next() == Some(inode.as_str()) && f.matches(':').count() == 2
                    })
            })
        },
    );
}

/// Waits until `ready` gives a value, at most `timeout`. `Err(state)` is "not yet" and says
/// where it is; the last `state` is in the panic message.
pub fn wait_some<T>(what: &str, timeout: Duration, ready: impl FnMut() -> Result<T, String>) -> T {
    wait_some_on(&SystemClock, what, timeout, ready)
}

/// [`wait_some`] on `clock` (see [`wait_until_on`]).
pub fn wait_some_on<T>(
    clock: &dyn Clock,
    what: &str,
    timeout: Duration,
    mut ready: impl FnMut() -> Result<T, String>,
) -> T {
    let mut state = String::new();
    poll_on(clock, timeout, || ready().map_err(|s| state = s).ok())
        .unwrap_or_else(|| panic!("timed out waiting for {what}: {state}"))
}

/// Waits until `done` accepts the screen `screen` reads, at most [`WAIT`]; returns that
/// screen. A timeout prints the last one.
pub fn wait_screen(
    what: &str,
    mut screen: impl FnMut() -> Vec<String>,
    mut done: impl FnMut(&[String]) -> bool,
) -> Vec<String> {
    wait_some(what, WAIT, || {
        let rows = screen();
        if done(&rows) {
            Ok(rows)
        } else {
            Err(format!("the screen is:\n{}", rows.join("\n")))
        }
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
            assert_eq!(
                words.first(),
                Some(&"cpd"),
                "details.next `{next}` is not a cpd command: {run:?}"
            );
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

/// A cpd `git` wrapper (`$CPD_HOME/bin/git`, [`git_guard::WRAPPER`]) in a directory of its own, and the way to run it
/// as a Session does. Shared by the `pr` tests of the wrapper and the harness's test of what a killed test leaves.
pub struct GitWrapper {
    env: TestEnv,
    home: CpdHome,
}

/// How long one call of the wrapper may take: a few `sh` builtins and one `exec`, milliseconds. A call that is not
/// over in this is wrappers that `exec` each other for ever (hs-d341), not a slow machine.
pub const GIT_WRAPPER_BOUND: Duration = Duration::from_secs(10);

impl GitWrapper {
    /// In a [`TestEnv`], so that a call runs in the env's process group and a killed test's reaper stops it (hygiene.md 1b).
    pub fn new() -> GitWrapper {
        let env = TestEnv::empty();
        let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
        fs::create_dir_all(home.bin_dir()).unwrap();
        write_executable(&home.git_wrapper(), git_guard::WRAPPER);
        GitWrapper { env, home }
    }

    pub fn bin(&self) -> PathBuf {
        self.home.bin_dir()
    }

    /// Runs `git <args>` the way a Session does, with `path` as `PATH`: (exit code, stdout, stderr). Panics when the
    /// call is not over in [`GIT_WRAPPER_BOUND`].
    pub fn run(&self, path: &OsString, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let mut cmd = self.env.command(self.home.git_wrapper());
        cmd.args(args)
            .current_dir(cwd)
            .env("PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = spawn(&mut cmd).unwrap();
        if poll(GIT_WRAPPER_BOUND, || child.try_wait().unwrap()).is_none() {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!(
                "`git {args:?}` through {} is not over after {GIT_WRAPPER_BOUND:?}: wrappers that exec each other, PATH {path:?}",
                self.home.git_wrapper().display()
            );
        }
        // Already reaped: `wait_with_output` reads what is left in the pipes.
        let out = child.wait_with_output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    }

    pub fn session_path(&self) -> OsString {
        git_guard::path_with_bin(&self.home, Some(inherited_path().to_owned())).into()
    }
}

/// Room the env's `TMUX_TMPDIR` policy leaves for the sockets a test derives from [`TestEnv::tmux_socket`] (`bare.rs`:
/// `<socket>-outer-<name>`, at most 7 + 10 characters; `tui_merge.rs`: `<socket>-outer`): they live in the same
/// directory, so they must fit in `sun_path` too. The teardown removes them (`<socket>-*`).
const DERIVED_SOCKET_ROOM: usize = 20;

/// The `TMUX_TMPDIR` of the env rooted at `root` whose socket is `socket`: [`tmux_tmpdir_for`] with [`DERIVED_SOCKET_ROOM`].
fn env_tmux_tmpdir(root: &Path, socket: &str) -> PathBuf {
    let uid = fs::metadata(root).expect("stat the env").uid();
    tmux_tmpdir_for(
        root,
        uid,
        &format!("{socket}{}", "-".repeat(DERIVED_SOCKET_ROOM)),
    )
}

/// The `TMUX_TMPDIR` for an env rooted at `root`: `<root>/tmux`, or `/tmp` when the socket path would pass 100
/// characters (measured on the canonical path, which tmux resolves; macOS's `sun_path` holds 103).
pub fn tmux_tmpdir_for(root: &Path, uid: u32, socket: &str) -> PathBuf {
    let tmux = root.join("tmux");
    let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if canonical
        .join(format!("tmux/tmux-{uid}/{socket}"))
        .as_os_str()
        .len()
        > 100
    {
        PathBuf::from("/tmp")
    } else {
        tmux
    }
}

/// A `flock(1)` for where there is none (macOS), in perl, which ships with macOS: `flock [-n] [-x] [--no-fork] <file>
/// <command> [args...]` holds an exclusive lock on `<file>` across the `exec` of the command.
const FLOCK_SHIM: &str = r#"#!/usr/bin/perl
use strict;
use warnings;
use Fcntl qw(:flock);
my $nb = 0;
while (@ARGV && $ARGV[0] =~ /^-/) {
    my $o = shift @ARGV;
    if ($o eq '-n' || $o eq '--nonblock') { $nb = 1 }
    elsif ($o eq '--no-fork' || $o eq '-x' || $o eq '--exclusive') { }
    else { print STDERR "flock shim: unsupported option $o\n"; exit 64 }
}
@ARGV >= 2 or do { print STDERR "usage: flock [-n] [--no-fork] [-x] <file> <command> [args...]\n"; exit 64 };
my $file = shift @ARGV;
$^F = 255;
open(my $fh, '>>', $file) or do { print STDERR "flock shim: $file: $!\n"; exit 1 };
flock($fh, LOCK_EX | ($nb ? LOCK_NB : 0)) or exit 1;
exec @ARGV or do { print STDERR "flock shim: $ARGV[0]: $!\n"; exit 127 };
"#;

/// Writes the perl `flock` shim to `<dir>/flock`.
pub fn write_flock_shim(dir: &Path) {
    write_executable(&dir.join("flock"), FLOCK_SHIM);
}

/// Whether the inherited `PATH` holds a `flock`.
fn has_flock() -> bool {
    std::env::split_paths(inherited_path()).any(|d| d.join("flock").is_file())
}

pub struct TestEnv {
    dir: TempDir,
    tmux_socket: String,
    /// Whether `TMUX_TMPDIR` is `<root>/tmux` whatever the root's length: set by an `Fx` on fake ports, whose tmux has no
    /// socket (the 100-character limit of [`tmux_tmpdir_for`] is a real server's) and must not share `/tmp`.
    pub(crate) tmux_in_root: bool,
    /// The env's process group: the reaper's pid; none for a [`TestEnv::bare`] env, which starts no process.
    pgid: Option<u32>,
    /// The reaper ([`TestEnv::empty`]), until the teardown stops it; none for a [`TestEnv::bare`] env.
    reaper: Mutex<Option<Child>>,
    /// The read-only connection [`TestEnv::read`] reads on, kept open across reads (a new connection parses the whole
    /// schema at its first query: ~10-15 ms a read), with the identity (device, inode) of the `cpd.db` it opened.
    reader: Mutex<Option<((u64, u64), rusqlite::Connection)>>,
}

/// The `sqlite3` CLI, which the teardown reads the env's store with: found on `PATH` at the
/// first [`TestEnv::new`] of the test process, which panics when there is none.
fn sqlite3() -> &'static Path {
    static SQLITE3: OnceLock<PathBuf> = OnceLock::new();
    SQLITE3.get_or_init(|| {
        std::env::split_paths(inherited_path())
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
        assert!(
            !line.contains('\n'),
            "stdout has more than one line: {self:?}"
        );
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
    /// Where there is no `flock` (macOS) the harness needs perl, for the shim it writes to `bin/`.
    pub fn empty() -> TestEnv {
        TestEnv::build(true)
    }

    /// [`TestEnv::empty`] without a process: no reaper and no teardown script, so nothing is started and `Drop` only
    /// removes the directory. For [`fx::Fx`], whose commands run in the test process; a process a command of it starts
    /// is not stopped by any teardown.
    pub fn bare() -> TestEnv {
        TestEnv::build(false)
    }

    fn build(reaped: bool) -> TestEnv {
        let envs_dir = envs_dir();
        let dir = tempfile::Builder::new()
            .prefix("cpd-test-")
            .tempdir_in(&envs_dir)
            .unwrap_or_else(|e| panic!("a temp dir in {}: {e}", envs_dir.display()));
        // On the exit-sweep list from before anything can panic: a test process that ends without dropping this env
        // still leaves no `cpd-test-*` behind ([`sweep_envs`]).
        register_env(dir.path());
        let n = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        let tmux_socket = match gate_run_id() {
            Some(run) => format!("cpdt-{run}-{}-{n}", std::process::id()),
            None => format!("cpdt-{}-{n}", std::process::id()),
        };
        let reaper = reaped.then(|| {
            let sqlite3 = sqlite3();
            let script = dir.path().join("teardown.sh");
            let tmuxdir = env_tmux_tmpdir(dir.path(), &tmux_socket);
            fs::write(
                &script,
                format!(
                    "dir={}\nsocket={}\ntmuxdir={}\nsqlite3={}\n{TEARDOWN_SH}",
                    sh_quote(&dir.path().to_string_lossy()),
                    sh_quote(&tmux_socket),
                    sh_quote(&tmuxdir.to_string_lossy()),
                    sh_quote(&sqlite3.to_string_lossy())
                ),
            )
            .expect("write teardown script");
            start_reaper(&script)
        });
        let env = TestEnv {
            dir,
            tmux_socket,
            tmux_in_root: false,
            pgid: reaper.as_ref().map(Child::id),
            reaper: Mutex::new(reaper),
            reader: Mutex::new(None),
        };
        for sub in ["cpd-home", "home", "bin", "tmux", "repos"] {
            fs::create_dir_all(env.dir.path().join(sub)).expect("create test dir");
        }
        for tool in DENIED_TOOLS {
            let path = env.bin().join(tool);
            write_executable(
                &path,
                &format!(
                    "#!/bin/sh\necho 'no fake installed for {tool}' >&2\nexit {DENIED_EXIT}\n"
                ),
            );
        }
        // Every test env has the owner `acme` (an empty policy file: cpd's default), for `context new --owner acme`.
        fs::create_dir_all(env.cpd_home().join("policy/acme")).expect("create the acme directory");
        fs::write(env.cpd_home().join("policy/acme/owner.toml"), "")
            .expect("write the acme owner file");
        fs::write(
            env.home().join(".gitconfig"),
            "[user]\n\tname = cpd test\n\temail = test@cpd.invalid\n[init]\n\tdefaultBranch = main\n[maintenance]\n\tauto = false\n[gc]\n\tauto = 0\n",
        )
        .expect("write .gitconfig");
        if !has_flock() {
            write_flock_shim(&env.bin());
        }
        env
    }

    fn teardown_script(&self) -> PathBuf {
        self.root().join("teardown.sh")
    }

    /// Stops the reaper, then runs the teardown ([`TEARDOWN_SH`]): stops the env's process
    /// group, its tmux server and the detached children cpd records, then removes the
    /// directory. Idempotent. Every env that a test builds is dropped (or torn down) before the
    /// test process ends, so the Gate's leak check finds no `cpd-test-*` behind.
    pub fn teardown(&self) {
        if let Some(mut reaper) = self
            .reaper
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            // By its pid alone, and reaped: a zombie in the group would hold the teardown for its grace.
            let _ = reaper.kill();
            let _ = reaper.wait();
        }
        if self.teardown_script().exists() {
            // stderr stays the test's: a failed read of the store shows in its output.
            let mut teardown = Command::new("sh");
            teardown
                .arg(self.teardown_script())
                .arg(
                    self.pgid
                        .expect("a teardown script has a reaper")
                        .to_string(),
                )
                .current_dir("/")
                .stdin(Stdio::null())
                .stdout(Stdio::null());
            let _ = spawn(&mut teardown).and_then(|mut child| child.wait());
        }
        let _ = fs::remove_dir_all(self.root());
        // A child the teardown missed (a late supervising `cpd watch --ensure`, a process that left the group) can
        // leave the directory there or make it again after the `rm`: stop the env's process group and remove it once
        // more, so the `Drop` truly removes it.
        if let (true, Some(pgid)) = (self.root().exists(), self.pgid) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pgid as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
            let _ = fs::remove_dir_all(self.root());
        }
    }

    /// The temporary directory everything lives in.
    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Replaces the owner `owner`'s policy file (every test env has `acme`, an empty one) with `toml`.
    pub fn owner_policy(&self, owner: &str, toml: &str) {
        let dir = self.cpd_home().join("policy").join(owner);
        fs::create_dir_all(&dir).expect("create the owner policy directory");
        fs::write(dir.join("owner.toml"), toml).expect("write the owner policy file");
    }

    /// Replaces the repo `<owner>/<name>`'s policy file (contract §8.3), creating its directories, with `toml`.
    pub fn repo_policy(&self, owner: &str, name: &str, toml: &str) {
        let dir = self.cpd_home().join("policy").join(owner);
        fs::create_dir_all(&dir).expect("create the repo policy directory");
        fs::write(dir.join(format!("{name}.toml")), toml).expect("write the repo policy file");
    }

    /// A `keyed_repo` whose repo policy file is removed: its repo has no repo scope (it inherits its owner).
    pub fn unconfigured_repo(&self, name: &str) -> PathBuf {
        let repo = self.keyed_repo(name);
        let _ = fs::remove_file(self.cpd_home().join(format!("policy/acme/{name}.toml")));
        repo
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

    /// The `TMUX_TMPDIR` of this env's private tmux server: in the env, or `/tmp` when its socket path would be too long.
    pub fn tmux_tmpdir(&self) -> PathBuf {
        if self.tmux_in_root {
            return self.root().join("tmux");
        }
        env_tmux_tmpdir(self.root(), self.tmux_socket())
    }

    /// The `flock` the env's commands run: the shim in `bin/` where the system has none, else the real one.
    pub fn flock(&self) -> PathBuf {
        let shim = self.bin().join("flock");
        if shim.is_file() {
            shim
        } else {
            PathBuf::from("flock")
        }
    }

    /// The private tmux server's socket name (`tmux -L <name>`), to put in
    /// `config.toml` as `tmux.socket`. Its socket file is under this env's own
    /// `TMUX_TMPDIR`.
    pub fn tmux_socket(&self) -> &str {
        &self.tmux_socket
    }

    /// A `Command` for `program` with this env's isolated environment:
    /// `CPD_*`, `PI_*`, `GIT_*`, `XDG_*`, `GH_*`, `GITHUB_*`, `SSH_*`,
    /// the proxy variables (`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, either case),
    /// `TMUX`, `TMUX_PANE` and `ZDOTDIR` removed, then
    /// `CPD_HOME`, `HOME`, `PATH`, `TMUX_TMPDIR`, `TZ` (a POSIX rule: jiff reads `TZ` first, so no child reads
    /// /etc/localtime and every child draws times in EST/EDT) and git's config set. It runs in the env's
    /// process group, which the teardown stops: a process that leaves it (`setsid`) is
    /// stopped only when cpd records it ([`TEARDOWN_SH`]).
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut cmd = Command::new(program);
        let (remove, set) = self.isolation();
        for key in remove {
            cmd.env_remove(key);
        }
        for (key, value) in set {
            cmd.env(key, value);
        }
        cmd.current_dir(self.root()).stdin(Stdio::null());
        if let Some(pgid) = self.pgid {
            cmd.process_group(pgid as i32);
        }
        cmd
    }

    /// The environment of every command run here, as the variables of this process to remove (the `CPD_*`, `PI_*`, ...
    /// of [`TestEnv::command`]) and the ones to set: the one place that says what isolated means, for the `Command` of
    /// [`TestEnv::command`] and for the process [`fx::Fx`] runs `cpd` in.
    pub fn isolation(&self) -> (Vec<OsString>, Vec<(&'static str, OsString)>) {
        let mut remove: Vec<OsString> = ["TMUX", "TMUX_PANE", "ZDOTDIR"]
            .map(OsString::from)
            .to_vec();
        for (key, _) in std::env::vars_os() {
            let k = key.to_string_lossy();
            let listed = ["CPD_", "PI_", "GIT_", "XDG_", "GH_", "GITHUB_", "SSH_"]
                .iter()
                .any(|p| k.starts_with(p))
                || PROXY_VARS.iter().any(|v| k.eq_ignore_ascii_case(v));
            if listed && !remove.contains(&key) {
                remove.push(key);
            }
        }
        let mut paths = vec![self.bin()];
        paths.extend(std::env::split_paths(inherited_path()));
        let set = vec![
            ("PATH", std::env::join_paths(paths).expect("PATH")),
            ("CPD_HOME", self.cpd_home().into()),
            ("HOME", self.home().into()),
            ("TMUX_TMPDIR", self.tmux_tmpdir().into()),
            ("TZ", "EST5EDT,M3.2.0,M11.1.0".into()),
            (
                "FAKE_TMUX_SERVER_PID",
                std::process::id().to_string().into(),
            ),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("GIT_CONFIG_GLOBAL", self.home().join(".gitconfig").into()),
            ("GIT_TERMINAL_PROMPT", "0".into()),
        ];
        (remove, set)
    }

    /// The store of this env's `$CPD_HOME`, opened (migrated) now.
    pub fn store(&self) -> Store {
        Store::open(
            &CpdHome::resolve(Some(self.cpd_home().into()), None).unwrap(),
            5000,
        )
        .unwrap()
    }

    /// `f` on a read-only connection to this env's store, for a test that only looks: no
    /// [`Store::open`] (its migration and schema check cost ~30 ms per open in a debug build).
    /// One connection serves every read (each statement sees the last commit); it is opened again
    /// when `cpd.db` is another file than the one it opened (replaced, or removed and created).
    /// Panics when the store does not exist or `f` fails.
    pub fn read<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<T>) -> T {
        use std::os::unix::fs::MetadataExt;
        let db = self.cpd_home().join("cpd.db");
        let file = fs::metadata(&db)
            .map(|m| (m.dev(), m.ino()))
            .unwrap_or_else(|e| panic!("stat {}: {e}", db.display()));
        let mut reader = self.reader.lock().unwrap_or_else(PoisonError::into_inner);
        if reader.as_ref().is_none_or(|(opened, _)| *opened != file) {
            let conn = rusqlite::Connection::open_with_flags(
                &db,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap_or_else(|e| panic!("open {} read-only: {e}", db.display()));
            conn.busy_timeout(Duration::from_secs(5))
                .expect("set the busy timeout");
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

    /// Writes `$CPD_HOME/config.toml` as given: the default curation floor is 25, so a test that curates a small Context
    /// says `context_curation_min_records = 1` in `text`.
    pub fn write_config(&self, text: &str) {
        fs::create_dir_all(self.cpd_home()).expect("create CPD_HOME");
        fs::write(self.cpd_home().join("config.toml"), text).expect("write config.toml");
    }

    /// The Tier models the tests use, `p/opus`, `p/sonnet`, `p/haiku` and `p/gemini`, as the machine policy's `[model]`
    /// (`policy/host.toml`, appended to what it holds; a `host.toml` with a `[model]` is left as it is), (pro-d27).
    pub fn write_models(&self) {
        let path = self.cpd_home().join("policy").join("host.toml");
        fs::create_dir_all(path.parent().unwrap()).expect("create the policy directory");
        let held = fs::read_to_string(&path).unwrap_or_default();
        if held.contains("[model]") {
            return;
        }
        let model =
            "[model]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\nG = \"p/gemini\"\n";
        fs::write(&path, format!("{held}{model}")).expect("write host.toml");
    }

    /// One cached model list, `run/models-quiet.json` (contract §1.4), `polled_at` `now`: E14 is not due before the newest
    /// `polled_at` is a week old (§3.7), so an engine a test starts runs no helper and reports nothing about one. A test of
    /// E14 itself does not call this.
    pub fn quiet_models(&self, now: OffsetDateTime) {
        let polled_at = now.format(&Rfc3339).expect("format polled_at");
        let file = serde_json::json!({
            "version": 1, "provider": "quiet", "polled_at": polled_at, "pi_version": "1.0.0",
            "source": {"kind": "copilot-available", "auth": "oauth", "base": null},
            "ids": ["quiet-model"]
        });
        let run = self.cpd_home().join("run");
        fs::create_dir_all(&run).expect("create the run directory");
        fs::write(run.join("models-quiet.json"), file.to_string())
            .expect("write models-quiet.json");
    }

    /// `config.toml` of machine `m1` on this env's private tmux server. `extra` is more TOML
    /// before the `[tmux]` table (top-level keys, or tables).
    pub fn write_tmux_config(&self, extra: &str) {
        self.write_config(&format!(
            "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n{extra}\n[tmux]\nsocket = \"{}\"\n",
            self.tmux_socket()
        ));
    }

    /// Gives `plan` an outline whose `done_when` covers every PR with a draft Job, which a Plan review needs
    /// (commands.md §4.6, §4.13; hs-d502). Run it right before `plan review`; the same outline twice changes nothing.
    pub fn outline_for_review(&self, plan: &str) {
        self.outline_with(plan, |args| self.cpd(args));
    }

    /// [`TestEnv::outline_for_review`] with the commands run by `cpd` (the binary, or an [`fx::Fx`]'s process).
    pub fn outline_with(&self, plan: &str, cpd: impl Fn(&[&str]) -> Run) {
        let shown = cpd(&["plan", "show", "--plan", plan]).ok();
        let mut done_when = serde_json::Map::new();
        for job in shown["jobs"].as_array().expect("plan show lists the jobs") {
            if job["status"] == "draft" {
                done_when.insert(
                    job["pr"].as_str().expect("a Job's Workflow").to_owned(),
                    serde_json::json!(["Done"]),
                );
            }
        }
        let outline = serde_json::json!({"goal": "Goal", "done_when": done_when});
        let path = self.root().join(format!("outline-{plan}.json"));
        fs::write(&path, outline.to_string()).expect("write the outline");
        cpd(&[
            "plan",
            "outline",
            "--file",
            &path.display().to_string(),
            "--plan",
            plan,
        ])
        .ok();
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
        let out = output(
            self.command(CPD)
                .args(args)
                .envs(vars.iter().copied())
                .current_dir(cwd),
        )
        .expect("run cpd");
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
        let partial = template.with_extension(format!(
            "partial-{}-{}",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        copy_tree(&repo, &partial);
        if fs::rename(&partial, &template).is_err() {
            fs::remove_dir_all(&partial).expect("remove the partial git repo template");
        }
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
        let name = repo
            .file_name()
            .expect("a repo directory")
            .to_string_lossy()
            .into_owned();
        let url = format!("https://github.com/acme/{name}.git");
        self.git(repo, &["remote", "add", "origin", &url]);
        self.git(repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        if !self
            .cpd_home()
            .join(format!("policy/acme/{name}.toml"))
            .exists()
        {
            self.repo_policy("acme", &name, "");
        }
        url
    }

    /// Makes git reach `origin` at the local bare repo `bare`, while its configured URL stays a
    /// GitHub one (the repo's key): `url.<bare>.insteadOf = <that URL>`.
    pub fn local_origin(&self, repo: &Path, bare: &Path) {
        let url = self.github_origin(repo);
        self.git(
            repo,
            &["config", &format!("url.{}.insteadOf", bare.display()), &url],
        );
    }

    /// Removes `origin` from a repo whose `origin` is only the GitHub URL `keyed_repo` gave it
    /// (no `local_origin`): for what runs after `pr add` and must find no remote, as before keying.
    pub fn drop_unreachable_origin(&self, repo: &Path) {
        // Exit 1 and no output when nothing matches.
        let rewritten = output(
            self.command("git")
                .args(["config", "--get-regexp", r"^url\..*\.insteadof$"])
                .current_dir(repo),
        )
        .expect("git config");
        if rewritten.stdout.is_empty() && self.git(repo, &["remote"]).lines().any(|r| r == "origin")
        {
            self.git(repo, &["remote", "remove", "origin"]);
        }
    }

    /// Kills this env's tmux server, after unsetting the supervising hooks (`pane-died`, `window-unlinked`): a hook that
    /// runs while the server dies starts a new one, where no teardown looks (hygiene.md 3a).
    pub fn kill_tmux_server(&self) {
        for hook in ["pane-died", "window-unlinked"] {
            self.tmux(&["set-hook", "-gu", hook]);
        }
        self.tmux(&["kill-server"]);
    }

    /// `tmux -L <private socket> <args>` in this env.
    pub fn tmux(&self, args: &[&str]) -> Output {
        output(
            self.command("tmux")
                .arg("-L")
                .arg(&self.tmux_socket)
                .args(args),
        )
        .expect("run tmux")
    }

    /// Makes `tmux` on this env's PATH the fake tmux: a symlink `bin/tmux` to the built `fake-tmux`. Idempotent.
    pub fn fake_tmux(&self) {
        let link = self.bin().join("tmux");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-tmux"), &link)
            .expect("link bin/tmux to the fake tmux");
    }

    /// The fake's state of this env's socket (`None` while no server exists).
    pub fn tmux_state(&self) -> Option<cpd::fakes::tmux::State> {
        cpd::fakes::tmux::State::open(&self.tmux_tmpdir(), self.tmux_socket())
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
        let Ok(entries) = fs::read_dir(self.calls_dir(name)) else {
            return Vec::new();
        };
        // A call `<n>` is complete once `<n>.done` exists; `<n>.rec` holds its argv.
        let mut records: Vec<(u64, PathBuf)> = entries
            .map(|e| e.expect("read calls dir").path())
            .filter(|p| p.extension().is_some_and(|e| e == "done"))
            .filter_map(|p| {
                Some((
                    p.file_stem()?.to_str()?.parse().ok()?,
                    p.with_extension("rec"),
                ))
            })
            .collect();
        records.sort();
        records
            .into_iter()
            .map(|(_, p)| {
                let text = String::from_utf8(fs::read(p).expect("read call record"))
                    .expect("calls are UTF-8");
                text.split_terminator('\0').map(str::to_owned).collect()
            })
            .collect()
    }

    /// The poll searches the fake `gh` has recorded: one per `cpd pr poll`, which engine E5 runs
    /// about once per tick when the engine runs with [`PR_POLL_EVERY_TICK`].
    pub fn poll_searches(&self) -> usize {
        self.calls("gh")
            .iter()
            .filter(|c| c.get(3).is_some_and(|q| q.contains("search(type")))
            .count()
    }

    /// Waits until the engine has run `ticks` whole ticks (E1-E8) since this call: the poll of
    /// a tick comes before E7, so once poll `n + 1` has begun, the tick of poll `n` is over.
    /// The positive signal an absence test waits for before it asserts that nothing happened.
    /// Needs an engine with [`PR_POLL_EVERY_TICK`] (else each whole tick waits for a whole `pr_poll_seconds`) and a
    /// fake `gh` that answers the search.
    pub fn wait_whole_ticks(&self, ticks: usize) {
        let from = self.poll_searches();
        wait_until(
            &format!(
                "{ticks} whole engine ticks (polls {from} -> {})",
                from + ticks + 1
            ),
            || self.poll_searches() > from + ticks,
        );
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
        let path = dir.join(format!(
            "cpd-store-{}-{}.{}.db",
            binary.len(),
            binary.mtime(),
            binary.mtime_nsec()
        ));
        if path.exists() {
            return path;
        }
        let env = TestEnv::empty();
        env.write_config("version = 1\nmachine = \"m1\"\n");
        env.cpd(&["events", "list"]).ok();
        let db = env.cpd_home().join("cpd.db");
        assert!(
            !env.cpd_home().join("cpd.db-wal").exists(),
            "cpd left a WAL beside the template store"
        );
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
        let binary = fs::metadata(std::env::current_exe().expect("this test binary"))
            .expect("stat this test binary");
        format!(
            "{}-{}.{}",
            binary.len(),
            binary.mtime(),
            binary.mtime_nsec()
        )
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

// ---- A caller without a UTF-8 locale (hs-d463; contract §6.1 "Every tmux call is a UTF-8 client"): shared by the
// session, engine, tooling and tui binaries.

/// The caller's locale of a `cpd` or tmux call: what the incident's launchd agent had (nothing), and the cases around it.
#[derive(Clone, Copy, Debug)]
pub enum Locale {
    /// No `LANG`, no `LANGUAGE`, no `LC_*`: the launchd agent of the incident, which sets only `PATH`.
    Unset,
    /// `LANG` set and empty.
    EmptyLang,
    /// `LANG=C`.
    LangC,
    /// `LC_ALL=C`, which tmux reads before `LANG`: a `LANG` of cpd's could not undo it.
    LcAllC,
}

pub const LOCALES: [Locale; 4] = [
    Locale::Unset,
    Locale::EmptyLang,
    Locale::LangC,
    Locale::LcAllC,
];

impl Locale {
    /// `cmd` with every locale variable of this process removed, then this locale's own.
    pub fn apply(self, cmd: &mut std::process::Command) -> &mut std::process::Command {
        for (key, _) in std::env::vars_os() {
            if key == "LANG" || key == "LANGUAGE" || key.to_string_lossy().starts_with("LC_") {
                cmd.env_remove(&key);
            }
        }
        match self {
            Locale::Unset => cmd,
            Locale::EmptyLang => cmd.env("LANG", ""),
            Locale::LangC => cmd.env("LANG", "C"),
            Locale::LcAllC => cmd.env("LC_ALL", "C"),
        }
    }

    /// This locale's variables, to set on a call that runs in this process (`Fx::cpd_with`).
    pub fn vars(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Locale::Unset => &[],
            Locale::EmptyLang => &[("LANG", "")],
            Locale::LangC => &[("LANG", "C")],
            Locale::LcAllC => &[("LC_ALL", "C")],
        }
    }

    /// The same as `env` words for `env -i`: what a wrapper script passes to the real tmux.
    pub fn env_words(self) -> &'static str {
        match self {
            Locale::Unset => "",
            Locale::EmptyLang => "LANG=",
            Locale::LangC => "LANG=C",
            Locale::LcAllC => "LC_ALL=C",
        }
    }
}

/// What a caller built with `env -i` must pass on for the Gate's tripwire (`scripts/tripwire/tmux`) to know the test:
/// `NAME='value'` words for `NEXTEST_BINARY_ID` and `NEXTEST_TEST_NAME` (the pair it matches against `allow.txt`; without
/// them it denies the call as an unidentified caller) and `TMPDIR` (where it logs a denied call for the Gate to print).
/// Only the variables that are set: the same words, leading space included, for `env -i PATH=... {words} tmux`.
pub fn tripwire_env_words() -> String {
    ["NEXTEST_BINARY_ID", "NEXTEST_TEST_NAME", "TMPDIR"]
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| format!(" {key}={}", sh_quote(&v)))
        })
        .collect()
}

/// The first `tmux` of the real `PATH`.
pub fn real_tmux() -> std::path::PathBuf {
    std::env::split_paths(inherited_path())
        .map(|dir| dir.join("tmux"))
        .find(|p| p.is_file())
        .expect("tmux on PATH")
}
