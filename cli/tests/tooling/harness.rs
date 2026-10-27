//! The harness itself: isolation of `$CPD_HOME`, git, tmux and the fakes. (The shared `write_config` sets no curation
//! floor: the default is 25, so a test that curates a small Context says `context_curation_min_records = 1`, hs-d592.)

use crate::common;

use crate::guards::{test_files, test_lines_with};
use common::fx::Fx;
use common::{SLEEP_SECONDS, TestEnv};
use cpd_core::clock::ManualClock;

#[test]
fn commands_get_an_isolated_environment() {
    let env = TestEnv::new();
    let out = env
        .command("sh")
        .args(["-c", "printf '%s|%s|%s|%s' \"$CPD_HOME\" \"$HOME\" \"${CPD_PLAN-unset}\" \"${TMUX-unset}\""])
        .env("CPD_PLAN", "leak") // set on the Command after isolation, so it shows
        .output()
        .unwrap();
    let got = String::from_utf8(out.stdout).unwrap();
    let parts: Vec<&str> = got.split('|').collect();
    assert_eq!(parts[0], env.cpd_home().to_str().unwrap());
    assert_eq!(parts[1], env.home().to_str().unwrap());
    assert_eq!(parts[2], "leak");
    assert_eq!(parts[3], "unset");

    // Without an explicit override, inherited CPD_* variables are removed.
    let out = env.command("sh").args(["-c", "env"]).output().unwrap();
    let vars = String::from_utf8(out.stdout).unwrap();
    for line in vars.lines() {
        let key = line.split('=').next().unwrap();
        assert!(
            !(key.starts_with("PI_") || key.starts_with("GIT_DIR") || key == "TMUX_PANE"),
            "{key} leaked"
        );
        if key.starts_with("CPD_") {
            assert_eq!(key, "CPD_HOME");
        }
    }
}

#[test]
fn git_repo_has_one_commit_on_main() {
    // Twice: the second repo is a copy of the template (the first makes it if there is none).
    for _ in 0..2 {
        let env = TestEnv::new();
        let repo = env.git_repo("pay");
        assert!(repo.starts_with(env.root()));
        assert_eq!(env.git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
        assert_eq!(env.git(&repo, &["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(env.git(&repo, &["log", "-1", "--format=%ae"]), "test@cpd.invalid");
        assert_eq!(std::fs::read_to_string(repo.join("README.md")).unwrap(), "# pay\n");
        // Clean to plumbing, which trusts the index's stat data (first: `status` refreshes the
        // index), and to porcelain.
        env.git(&repo, &["diff-files", "--quiet"]);
        env.git(&repo, &["diff-index", "--quiet", "HEAD", "--"]);
        assert_eq!(env.git(&repo, &["status", "--porcelain"]), "");
    }
}

#[test]
fn fakes_shadow_the_real_programs_and_record_calls() {
    let env = TestEnv::new();
    env.fake_pi("echo pi-fake");
    env.fake_gh("echo '{\"number\": 1}'; exit 3");
    let out = env.command("pi").args(["--fork", "a file", ""]).output().unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "pi-fake\n");
    env.command("pi").output().unwrap();
    let out = env.command("gh").args(["pr", "list"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));

    assert_eq!(env.calls("pi"), vec![vec!["--fork".to_owned(), "a file".to_owned(), String::new()], vec![]]);
    assert_eq!(env.calls("gh"), vec![vec!["pr".to_owned(), "list".to_owned()]]);
    assert!(env.calls("git").is_empty());
}

#[test]
fn tmux_server_is_private_and_killed_on_drop() {
    let env = TestEnv::new();
    let socket_dir = env.tmux_tmpdir();
    let out = env.tmux(&["new-session", "-d", "-s", "t", "sleep 60"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(env.tmux(&["has-session", "-t", "=t"]).status.success());
    assert_ne!(env.tmux_socket(), "cpd");

    // The socket file is under this env's TMUX_TMPDIR, not the user's.
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(env.root()).unwrap());
    let socket = socket_dir.join(format!("tmux-{uid}")).join(env.tmux_socket());
    assert!(socket.exists(), "no socket {} under {}", env.tmux_socket(), socket_dir.display());

    let pid = String::from_utf8(env.tmux(&["display-message", "-p", "#{pid}"]).stdout).unwrap();
    let pid = pid.trim().to_owned();
    assert!(!pid.is_empty());
    drop(env);
    let pid: u32 = pid.parse().unwrap();
    let gone = common::poll(std::time::Duration::from_secs(5), || (!alive(pid)).then_some(())).is_some();
    assert!(gone, "the tmux server {pid} outlived its TestEnv");
}

#[test]
fn concurrent_fake_calls_are_recorded_whole() {
    use std::collections::BTreeSet;

    const CALLS: usize = 16;
    let env = TestEnv::new();
    env.fake("tmux", "exit 0");
    // Every call gets several long arguments of its own, so a torn record is
    // visible as an argv that is none of these.
    let argv = |i: usize| -> Vec<String> { (0..8).map(|j| format!("call{i}-arg{j}-{}", "x".repeat(400))).collect() };
    for _round in 0..5 {
        let children: Vec<_> = (0..CALLS).map(|i| env.command("tmux").args(argv(i)).spawn().unwrap()).collect();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
    }
    let got = env.calls("tmux");
    assert_eq!(got.len(), CALLS * 5);
    let expected: BTreeSet<_> = (0..CALLS).map(argv).collect();
    for call in &got {
        assert!(expected.contains(call), "a recorded call is none of the argvs given: {} args, first {:.20?}", call.len(), call.first());
    }
    assert_eq!(got.iter().collect::<BTreeSet<_>>().len(), CALLS, "every argv was recorded");
}

#[test]
fn calls_come_back_in_the_order_they_started() {
    let env = TestEnv::new();
    env.fake("tmux", "exit 0");
    for i in 0..30 {
        env.command("tmux").arg(i.to_string()).status().unwrap();
    }
    let got: Vec<String> = env.calls("tmux").into_iter().map(|c| c[0].clone()).collect();
    assert_eq!(got, (0..30).map(|i| i.to_string()).collect::<Vec<_>>());
}

#[test]
fn no_fake_sleeps_longer_than_sleep_seconds() {
    let mut found = Vec::new();
    for path in test_files() {
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            for (at, _) in line.match_indices("sleep ") {
                let digits: String = line[at + 6..].chars().take_while(char::is_ascii_digit).collect();
                if digits.parse::<u64>().is_ok_and(|s| s > SLEEP_SECONDS) {
                    found.push(format!("{}:{}: sleep {digits}", path.display(), n + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "fakes must sleep at most SLEEP_SECONDS ({SLEEP_SECONDS}):\n{}", found.join("\n"));
}

#[test]
fn executables_are_written_and_copied_only_through_the_harness() {
    // ETXTBSY is handled once, in `common` (`write_executable`, `copy_executable`, `spawn`, `output`;
    // tests-build-W5): no file keeps its own lock, copy helper or chmod after a write.
    let found = test_lines_with(&[
        "from_mode(0o755)",
        "fs::copy(CPD",
        "ONE_AT_A_TIME",
        "fn copy_executable",
        "fn executable(",
        "ETXTBSY",
        "Text file busy",
    ]);
    assert!(found.is_empty(), "write executables with common::write_executable and copy_executable:\n{}", found.join("\n"));
}

// ---- teardown: a dropped or killed test leaves no tmux server, process, dir or reaper ----

const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a helper env put up: the dir and the pids of what its teardown must stop.
struct Up {
    dir: std::path::PathBuf,
    pids: Vec<(String, u32)>,
}

/// Starts `argv` as cpd starts its detached children: in a session of its own (`setsid`), so out
/// of the env's process group, and not a child of the test process. Returns its pid once it is
/// in that session.
fn detached(env: &TestEnv, argv: &[&str]) -> u32 {
    let mut cmd = env.command("sh");
    // macOS has no `setsid` command: perl's POSIX::setsid, then exec, is the same process in a new session.
    let script = if cfg!(target_os = "macos") {
        r#"perl -MPOSIX -e 'POSIX::setsid(); exec @ARGV' "$@" </dev/null >/dev/null 2>&1 & echo $!"#
    } else {
        r#"setsid "$@" </dev/null >/dev/null 2>&1 & echo $!"#
    };
    cmd.args(["-c", script, "sh"]).args(argv);
    let out = common::output(&mut cmd).unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pid: u32 = String::from_utf8(out.stdout).unwrap().trim().parse().unwrap();
    // `/proc/<pid>/stat`: `<pid> (<comm>) <state> <ppid> <pgrp> <session> ...`.
    let session = || {
        if cfg!(target_os = "macos") {
            // POSIX.pm has no getsid and `ps -o sess=` prints 0: a setsid'd process leads its own group,
            // and a background job of a non-interactive sh does not, so pgid == pid shows the new session.
            let out = std::process::Command::new("ps").args(["-o", "pgid=", "-p", &pid.to_string()]).output().ok()?;
            String::from_utf8(out.stdout).ok()?.trim().parse::<u32>().ok()
        } else {
            std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?.rsplit_once(") ")?.1.split(' ').nth(3)?.parse::<u32>().ok()
        }
    };
    common::wait_until(&format!("process {pid} to be in a session of its own"), || session() == Some(pid));
    pid
}

/// Records `pid` as the driver of Attempt `api-a1` (running) in the env's store.
fn record_driver(env: &TestEnv, pid: u32) {
    let cwd = env.root().display();
    let mut store = env.store();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead', '{cwd}', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {id}, 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {id}, 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
                   VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', {pid}, 't');",
                fp = "a".repeat(64),
                id = tx.id()
            ))?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();
}

/// Puts up in `env` what its teardown must stop, each found its own way: the tmux server and
/// its pane (through tmux), a process of the env's group, and two detached processes cpd
/// records: a driver (`attempts.driver_pid`) and a distill run (its pid file). The tmux server
/// that the driver starts when it is stopped mentions the dir in its command line.
fn put_up(env: &TestEnv) -> Up {
    let out = env.tmux(&["new-session", "-d", "-s", "t", "sleep 100"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let tmux = |format: &str| -> u32 { String::from_utf8(env.tmux(&["display-message", "-p", format]).stdout).unwrap().trim().parse().unwrap() };
    let group = common::output(env.command("sh").args(["-c", "sleep 100 >/dev/null 2>&1 & echo $!"])).unwrap();
    let group = String::from_utf8(group.stdout).unwrap().trim().parse().unwrap();
    // A driver that starts the tmux server again when it is stopped, as a driver may (its Worker's window).
    let trapped = env.root().join("driver-trapped");
    let restart = format!(
        "trap 'tmux -L {} new-session -d \"sleep 100 # {}\"; exit 0' TERM; : > '{}'; while :; do sleep 1 & wait $!; done",
        env.tmux_socket(),
        env.root().display(),
        trapped.display()
    );
    let driver = detached(env, &["sh", "-c", &restart]);
    common::wait_until("the driver to trap SIGTERM", || trapped.exists());
    record_driver(env, driver);
    // A distill run the engine started, recorded as the engine records it.
    // Its command line holds the env's dir (the teardown stops a recorded pid only if it is the env's, and a platform
    // binary like `sleep` shows nothing of it on macOS); the `:` after the sleep keeps the shell from exec'ing it.
    let distill = detached(env, &["sh", "-c", &format!("sleep 100; : '{}'", env.root().display())]);
    let home = cpd_core::home::CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let pid_file = home.distill_pid(&cpd_core::ids::ContextId::parse("ctx").unwrap());
    std::fs::create_dir_all(pid_file.parent().unwrap()).unwrap();
    std::fs::write(pid_file, format!("{distill}\n")).unwrap();
    Up {
        dir: env.root().to_owned(),
        pids: [("tmux server", tmux("#{pid}")), ("tmux pane", tmux("#{pane_pid}")), ("group process", group), ("driver", driver), ("distill run", distill)]
            .map(|(name, pid)| (name.to_owned(), pid))
            .into(),
    }
}

fn alive(pid: u32) -> bool {
    if cfg!(target_os = "macos") {
        // `kill -0` finds the pid; `ps -o stat=` prints nothing for a gone process and Z for a zombie.
        let signalled = std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        let stat = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
        return signalled && !stat.is_empty() && !stat.starts_with('Z');
    }
    // A zombie is dead; an unreadable /proc entry is gone.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z') && !rest.starts_with('X')))
}

/// Every live process whose command line mentions `dir` (the reaper's is `sh <dir>/teardown.sh watch <pid>`).
/// A scan of all of /proc: the tests' own check, never the teardown's.
fn processes_mentioning(dir: &std::path::Path) -> Vec<u32> {
    let needle = dir.to_string_lossy().into_owned().into_bytes();
    if cfg!(target_os = "macos") {
        // BSD `ps` has no /proc: `pid=,command=` of every process.
        let out = std::process::Command::new("ps").args(["-axo", "pid=,command="]).output().unwrap();
        return String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.trim_start().split_once(' '))
            .filter(|(_, cmd)| cmd.contains(dir.to_string_lossy().as_ref()))
            .filter_map(|(pid, _)| pid.parse::<u32>().ok())
            .collect();
    }
    std::fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.windows(needle.len()).any(|w| w == needle.as_slice()))
        })
        .collect()
}

/// Waits up to WAIT until nothing of `up` is left; returns what is left otherwise.
fn leftovers_after_wait(up: &Up) -> Vec<String> {
    let mut left = Vec::new();
    common::poll(WAIT, || {
        left = up.pids.iter().filter(|(_, p)| alive(*p)).map(|(n, p)| format!("{n} {p}")).collect();
        if up.dir.exists() {
            left.push(format!("dir {}", up.dir.display()));
        }
        left.extend(processes_mentioning(&up.dir).into_iter().map(|p| format!("process {p} (reaper?) mentioning the dir")));
        left.is_empty().then_some(())
    });
    left
}

#[test]
fn a_dropped_env_leaves_no_tmux_server_process_dir_or_reaper() {
    let env = TestEnv::new();
    let up = put_up(&env);
    assert!(up.pids.iter().all(|(_, p)| alive(*p)));
    assert!(!processes_mentioning(&up.dir).is_empty(), "the reaper is up before the drop");
    drop(env);
    assert!(!up.dir.exists(), "the dir is removed when drop returns");
    let left = leftovers_after_wait(&up);
    assert!(left.is_empty(), "a dropped env left behind: {left:?}");
}

/// Kills `pid` when dropped: a test that fails midway leaves nothing it started.
struct KillOnDrop(u32);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = std::process::Command::new("kill").args(["-KILL", &self.0.to_string()]).status();
    }
}

/// Kills the helper child when dropped (a failed assertion unwinds through it).
struct KillChild(std::process::Child);

impl Drop for KillChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts this binary as the helper test `name` (gated by `gate`), with `extra` variables; its stdout is piped.
fn start_helper(name: &str, gate: &str, extra: &[(&str, &std::ffi::OsStr)]) -> KillChild {
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env(gate, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    KillChild(cmd.spawn().unwrap())
}

/// The `KEY value` lines the helper prints before `READY`.
fn helper_lines(helper: &mut KillChild, key: &str) -> Vec<String> {
    use std::io::{BufRead, BufReader};
    let mut found = Vec::new();
    for line in BufReader::new(helper.0.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        if line == "READY" {
            break;
        }
        if let Some(v) = line.strip_prefix(&format!("{key} ")) {
            found.push(v.to_owned());
        }
    }
    found
}

// ---- hygiene.md 1b: the git wrapper tests run through the env's group and reaper ----

const WRAPPER_HELPER_GATE: &str = "CPD_HARNESS_WRAPPER_HELPER";

/// Not a test of its own: runs a `common::GitWrapper` whose directory and another's hold old-version wrappers that
/// `exec` each other for ever (hs-d341), the call of `Wrapper::run` blocked in its thread. Does nothing unless gated.
#[test]
fn wrapper_helper() {
    if std::env::var_os(WRAPPER_HELPER_GATE).is_none() {
        return;
    }
    let (first, second) = (common::GitWrapper::new(), common::GitWrapper::new());
    for (me, other) in [(&first, &second), (&second, &first)] {
        common::write_executable(&me.bin().join("git"), &format!("#!/bin/sh\nexec '{}/git' \"$@\"\n", other.bin().display()));
    }
    let path = std::env::join_paths([first.bin(), second.bin()]).unwrap();
    let dirs = [first.bin(), second.bin()];
    std::thread::scope(|s| {
        s.spawn(|| first.run(&path, std::path::Path::new("/"), &["status"]));
        // One process, which `exec`s from one wrapper to the other: it mentions one directory at a time.
        common::wait_until("the looping wrappers to run", || dirs.iter().any(|d| !processes_mentioning(d).is_empty()));
        println!();
        for d in &dirs {
            println!("BIN {}", d.display());
        }
        println!("READY");
        std::thread::sleep(std::time::Duration::from_secs(SLEEP_SECONDS));
    });
}

#[test]
fn a_killed_test_leaves_no_git_wrapper_process() {
    let mut helper = start_helper("harness::wrapper_helper", WRAPPER_HELPER_GATE, &[]);
    let dirs: Vec<std::path::PathBuf> = helper_lines(&mut helper, "BIN").into_iter().map(Into::into).collect();
    // Whatever the outcome, what the helper left is stopped and its temporary directories removed.
    // (A `tempfile` directory, `.tmp*`, or an env's, `cpd-test-*`: never a directory that is not the helper's.)
    let _sweep = Sweep(
        dirs.iter()
            .flat_map(|d| d.ancestors())
            .filter(|a| a.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".tmp") || n.to_string_lossy().starts_with("cpd-test-")))
            .map(ToOwned::to_owned)
            .collect(),
    );
    assert_eq!(dirs.len(), 2, "the helper printed no READY with its wrapper directories");
    // The wrapper `exec`s from one script to the other: its command line is empty for a moment at each `exec`.
    let wrappers = common::poll(WAIT, || Some(dirs.iter().flat_map(|d| processes_mentioning(d)).collect::<Vec<u32>>()).filter(|w| !w.is_empty()))
        .expect("a wrapper runs before the kill");

    helper.0.kill().unwrap(); // SIGKILL: no Drop runs
    helper.0.wait().unwrap();

    let left = common::poll(WAIT, || wrappers.iter().all(|p| !alive(*p)).then_some(()));
    assert!(left.is_some(), "a killed test left git wrapper processes: {:?}", wrappers.iter().filter(|p| alive(**p)).collect::<Vec<_>>());
}

/// Removes the directories when dropped.
struct DirsOnDrop(Vec<std::path::PathBuf>);

impl Drop for DirsOnDrop {
    fn drop(&mut self) {
        for d in &self.0 {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// Kills every process that mentions one of the directories, then removes them, when dropped.
struct Sweep(Vec<std::path::PathBuf>);

impl Drop for Sweep {
    fn drop(&mut self) {
        for d in &self.0 {
            for pid in processes_mentioning(d) {
                drop(KillOnDrop(pid));
            }
        }
        drop(DirsOnDrop(std::mem::take(&mut self.0)));
    }
}

// ---- hygiene.md 5b: an env is made under the run's TMPDIR ----

#[test]
fn an_env_is_made_under_the_runs_tmpdir() {
    // In this process, under the `Fx`'s lock: `TestEnv` reads the `TMPDIR` of its process, which the `Fx` sets for the
    // one call (no helper process whose environment the test sets).
    let fx = Fx::new();
    let tmp = tempfile::tempdir().unwrap();
    let env = fx.with_vars(&[("TMPDIR", tmp.path().to_str().unwrap())], TestEnv::bare);
    assert_eq!(env.root().parent(), Some(tmp.path()), "the env's directory is not under the TMPDIR of its process");
}

#[test]
fn a_process_that_leaves_the_group_and_is_not_recorded_outlives_the_teardown() {
    // What the teardown does not stop: it finds nothing by scanning, and cpd records only its own detached children.
    let env = TestEnv::new();
    let escaped = detached(&env, &["sleep", "100"]);
    drop(env);
    let outlived = alive(escaped);
    let _ = std::process::Command::new("kill").arg(escaped.to_string()).status();
    assert!(outlived, "an unrecorded process in a session of its own was stopped");
}

#[test]
fn a_recorded_pid_that_is_not_the_envs_is_spared() {
    // A pid cpd recorded for a driver that has ended is another process by the time of the teardown (pids are recycled
    // within minutes on macOS). Such a process hides its environment (an Apple platform binary does), and has nothing
    // of the env in its command line: the teardown must not signal it.
    use std::os::unix::process::CommandExt;
    let env = TestEnv::new();
    let mut other = std::process::Command::new("/bin/sleep");
    other.arg("600").env_clear().process_group(0).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut other = other.spawn().unwrap();
    record_driver(&env, other.id());
    env.teardown();
    let outlived = other.try_wait().unwrap().is_none();
    let _ = other.kill();
    let _ = other.wait();
    assert!(outlived, "the teardown stopped a recorded pid that is not the env's");
}

#[test]
fn teardown_is_idempotent_and_spares_the_test_process() {
    let env = TestEnv::new();
    env.teardown();
    env.teardown();
    assert!(!env.root().exists());
    assert!(alive(std::process::id()));
}

// ---------- refused ----------

fn refusal(next: Option<&str>) -> common::Run {
    let details = next.map_or(serde_json::json!({}), |n| serde_json::json!({"next": n}));
    let json = serde_json::json!({"ok": false, "error": {"code": "archived", "message": "No.", "details": details}});
    common::Run { exit: 3, stdout: format!("{json}\n"), stderr: String::new() }
}

#[test]
fn refused_returns_the_error_of_a_refusal_whose_next_parses() {
    let error = common::refused(&refusal(Some("cpd plan unarchive --plan pay-1")), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd plan unarchive --plan pay-1");
    common::refused(&refusal(None), "archived", 3);
}

#[test]
#[should_panic(expected = "does not parse")]
fn refused_fails_on_a_next_that_does_not_parse() {
    common::refused(&refusal(Some("cpd plan unarchive pay-1")), "archived", 3);
}

#[test]
#[should_panic(expected = "is not a cpd command")]
fn refused_fails_on_a_next_that_is_not_a_cpd_command() {
    common::refused(&refusal(Some("unarchive the plan")), "archived", 3);
}

#[test]
#[should_panic(expected = "assertion")]
fn refused_fails_on_another_code_or_exit() {
    common::refused(&refusal(None), "usage", 3);
}

// ---------- waits ----------

#[test]
fn wait_until_returns_once_ready_and_names_what_it_waited_for_on_timeout() {
    // On a `ManualClock`: the polls and the timeout are virtual time, no wait.
    let clock = ManualClock::new();
    let mut n = 0;
    common::wait_until_on(&clock, "three polls", common::WAIT, || {
        n += 1;
        n == 3
    });
    assert_eq!(clock.naps(), [common::POLL; 2], "three looks are two polls");
    let timeout = std::time::Duration::from_millis(150);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| common::wait_until_on(&clock, "never", timeout, || false))).unwrap_err();
    assert!(caught.downcast_ref::<String>().is_some_and(|m| m.contains("timed out waiting for never")));
    assert!(clock.elapsed() >= 2 * common::POLL + timeout, "it gave up before its timeout: {:?}", clock.elapsed());
}

#[test]
fn wait_some_says_the_last_state_on_timeout() {
    let clock = ManualClock::new();
    let mut n = 0;
    let got = common::wait_some_on(&clock, "a value", std::time::Duration::from_secs(5), || {
        n += 1;
        if n < 3 { Err(format!("poll {n}")) } else { Ok(n) }
    });
    assert_eq!(got, 3);
    let timeout = std::time::Duration::from_millis(150);
    let caught =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| common::wait_some_on::<()>(&clock, "x", timeout, || Err("stuck".into())))).unwrap_err();
    assert!(caught.downcast_ref::<String>().is_some_and(|m| m.contains("timed out waiting for x: stuck")));
}

#[test]
fn sql_writes_the_envs_store() {
    let fx = Fx::new();
    fx.env.write_tmux_config("");
    fx.cpd(&["context", "new", "c1", "--title", "Before", "--owner", "acme"]).ok();
    fx.env.sql("UPDATE contexts SET title = 'After' WHERE id = 'c1'");
    assert_eq!(fx.cpd(&["context", "list"]).ok()["contexts"][0]["title"], "After");
}

#[test]
fn a_new_env_has_a_migrated_store_of_its_own_and_an_empty_env_has_none() {
    let fx = Fx::new();
    let (a, b) = (&fx.env, fx.another_env());
    for env in [a, &b] {
        let store = env.store();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    }
    a.write_tmux_config("");
    b.write_tmux_config("");
    // `b`'s commands run in this process too, with `b`'s `$CPD_HOME`.
    let in_b = |args: &[&str]| fx.cpd_with(b.root(), args, &[("CPD_HOME", b.cpd_home().to_str().unwrap())]);
    fx.cpd(&["context", "new", "c1", "--title", "Only in a", "--owner", "acme"]).ok();
    assert_eq!(fx.cpd(&["context", "list"]).ok()["contexts"].as_array().unwrap().len(), 1);
    assert_eq!(in_b(&["context", "list"]).ok()["contexts"], serde_json::json!([]));

    // `TestEnv::empty` without its reaper process: the same empty `$CPD_HOME`.
    let empty = TestEnv::bare();
    // main's harness gives every env the owner policy `acme` (policy/acme/owner.toml): no store is what makes it empty.
    assert!(!empty.cpd_home().join("cpd.db").exists(), "an empty env has no store");
}

#[test]
fn the_tmux_dir_stays_in_the_env_unless_its_socket_path_would_pass_100_characters() {
    let uid = 501;
    let socket = "cpdt-s";
    let base = tempfile::Builder::new().prefix("tt-").tempdir_in("/tmp").unwrap();
    let base_path = std::fs::canonicalize(base.path()).unwrap();
    let tail = format!("/tmux/tmux-{uid}/{socket}").len();
    // A root whose canonical socket path is `total` characters long.
    let root_with = |total: usize| {
        let pad = total - base_path.as_os_str().len() - 1 - tail;
        let root = base_path.join("p".repeat(pad));
        std::fs::create_dir_all(&root).unwrap();
        root
    };
    let length = |root: &std::path::Path| std::fs::canonicalize(root).unwrap().join(format!("tmux/tmux-{uid}/{socket}")).as_os_str().len();

    let short = base_path.join("env");
    std::fs::create_dir_all(&short).unwrap();
    assert_eq!(common::tmux_tmpdir_for(&short, uid, socket), short.join("tmux"), "a short root keeps its tmux dir");

    let exact = root_with(100);
    assert_eq!(length(&exact), 100);
    assert_eq!(common::tmux_tmpdir_for(&exact, uid, socket), exact.join("tmux"), "exactly 100 characters stays in the env");

    let over = root_with(101);
    assert_eq!(length(&over), 101);
    assert_eq!(common::tmux_tmpdir_for(&over, uid, socket), std::path::PathBuf::from("/tmp"), "101 characters falls back to /tmp");
}

#[test]
fn a_command_does_not_inherit_zdotdir() {
    let env = TestEnv::new();
    let cmd = env.command("true");
    let removed = cmd.get_envs().any(|(key, value)| key == "ZDOTDIR" && value.is_none());
    assert!(removed, "ZDOTDIR is not removed from the command's environment: {:?}", cmd.get_envs().collect::<Vec<_>>());
}

#[test]
fn the_env_gitconfig_turns_automatic_maintenance_off() {
    let env = TestEnv::new();
    let get = |key: &str| {
        let out = common::output(env.command("git").args(["config", "--global", key])).expect("run git");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    assert_eq!(get("maintenance.auto"), "false", "maintenance.auto");
    assert_eq!(get("gc.auto"), "0", "gc.auto");
}

#[test]
fn the_flock_shim_holds_the_lock_across_exec_and_n_fails_fast_while_it_is_held() {
    let env = TestEnv::new();
    let dir = env.root().join("shim");
    std::fs::create_dir_all(&dir).unwrap();
    common::write_flock_shim(&dir);
    let shim = dir.join("flock");
    assert!(shim.is_file(), "write_flock_shim wrote no {}", shim.display());
    let lock = dir.join("lock");
    let marker = dir.join("held");
    // The holder forks no child (`:` is a builtin, then `exec`): the shim's lock fd is inherited by children, so a
    // forked `touch` could still hold the lock after the holder is killed and waited.
    let mut holder = common::spawn(
        env.command(&shim)
            .arg(&lock)
            .args(["sh", "-c"])
            .arg(format!(": > '{}'; exec sleep 60", marker.display())),
    )
    .expect("spawn the shim");
    common::wait_until("the command to run under the lock", || marker.exists());
    let second = common::output(env.command(&shim).arg("-n").arg(&lock).arg("true")).expect("run the second shim");
    assert_eq!(second.status.code(), Some(1), "-n exits 1 at once while the lock is held");
    holder.kill().unwrap();
    holder.wait().unwrap();
    // Evidence: standalone repro (no child), ~1% of runs exit 1 right after kill+wait, gone after 1-3 retries.
    common::wait_until("the kernel to drop the flock a moment after the killed holder is reaped (observed on macOS, ~1%)", || {
        let third = common::output(env.command(&shim).arg("-n").arg(&lock).arg("true")).expect("run the third shim");
        let code = third.status.code();
        assert!(matches!(code, Some(0) | Some(1)), "-n exits 0 (lock free) or 1 (held), not {code:?}");
        code == Some(0)
    });
    let bad = common::output(env.command(&shim).arg("-w").arg("1").arg(&lock).arg("true")).expect("run the shim with another option");
    assert_eq!(bad.status.code(), Some(64), "any other option exits 64");
}
