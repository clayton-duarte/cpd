//! The harness itself: isolation of `$CPD_HOME`, git, tmux and the fakes.

mod common;

use common::{SLEEP_SECONDS, TestEnv};

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
fn cpd_runs_and_prints_one_json_value() {
    let env = TestEnv::new();
    let run = env.cpd(&["--version"]);
    assert_eq!(run.ok()["version"], env!("CARGO_PKG_VERSION"));
    let run = env.cpd(&["no-such-command"]);
    assert_eq!(run.error()["code"], "usage");
    assert_eq!(run.exit, 2);
}

#[test]
fn git_repo_has_one_commit_on_main() {
    let env = TestEnv::new();
    let repo = env.git_repo("pay");
    assert!(repo.starts_with(env.root()));
    assert_eq!(env.git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(env.git(&repo, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(env.git(&repo, &["log", "-1", "--format=%ae"]), "test@cpd.invalid");
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
    let socket_dir = env.root().join("tmux");
    let out = env.tmux(&["new-session", "-d", "-s", "t", "sleep 60"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(env.tmux(&["has-session", "-t", "=t"]).status.success());
    assert_ne!(env.tmux_socket(), "cpd");

    // The socket file is under this env's TMUX_TMPDIR, not the user's.
    let found = std::fs::read_dir(&socket_dir)
        .unwrap()
        .flat_map(|uid_dir| std::fs::read_dir(uid_dir.unwrap().path()).unwrap())
        .any(|entry| entry.unwrap().file_name() == env.tmux_socket());
    assert!(found, "no socket {} under {}", env.tmux_socket(), socket_dir.display());

    let pid = String::from_utf8(env.tmux(&["display-message", "-p", "#{pid}"]).stdout).unwrap();
    let pid = pid.trim().to_owned();
    assert!(!pid.is_empty());
    drop(env);
    let gone = (0..50).any(|_| {
        let alive = std::process::Command::new("kill").args(["-0", &pid]).stderr(std::process::Stdio::null()).status().unwrap();
        if alive.success() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        !alive.success()
    });
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
    for entry in std::fs::read_dir(env!("CARGO_MANIFEST_DIR").to_owned() + "/tests").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
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

/// The text of every test file of this crate but this one, as (path, line number, line).
fn test_source_lines() -> Vec<(String, usize, String)> {
    let mut lines = Vec::new();
    for entry in std::fs::read_dir(env!("CARGO_MANIFEST_DIR").to_owned() + "/tests").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") || path.file_name().is_some_and(|n| n == "harness.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            lines.push((path.display().to_string(), n + 1, line.to_owned()));
        }
    }
    lines
}

/// The lines of the test files containing any of `needles`, as `path:line: text`.
fn test_lines_with(needles: &[&str]) -> Vec<String> {
    test_source_lines()
        .into_iter()
        .filter(|(_, _, line)| needles.iter().any(|n| line.contains(n)))
        .map(|(path, n, line)| format!("{path}:{n}: {}", line.trim()))
        .collect()
}

#[test]
fn no_test_mutates_the_process_environment() {
    // Test threads share one environment: `set_var` there races every `getenv` (tests-build-G3).
    // A test sets a variable on the `Command` it spawns.
    let found = test_lines_with(&["set_var(", "remove_var("]);
    assert!(found.is_empty(), "use Command::env, not the process environment:\n{}", found.join("\n"));
}

/// The milliseconds of the literal duration a `sleep(` on `line` waits (`from_millis(n)`, `from_secs(n)`), if it has one.
fn literal_sleep_ms(line: &str) -> Option<u64> {
    let after = &line[line.find("sleep(")? + 6..];
    let number = |prefix: &str| -> Option<u64> {
        let rest = &after[after.find(prefix)? + prefix.len()..];
        rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
    };
    number("from_millis(").or_else(|| number("from_secs(").map(|s| s * 1000))
}

#[test]
fn no_test_sleeps_for_a_result_or_bounds_the_time_a_result_takes() {
    // A fixed sleep before an assertion proves nothing on a loaded machine, and an upper bound on elapsed time fails on
    // one (tests-build-W4): wait for the result (`wait_until`, a whole engine tick, a rendezvous). A sleep of 250 ms or
    // more, or an upper bound, stands only for a claim about time itself, and says so in a `// time-claim: <what it
    // asserts>; no per-poll signal from <X>` comment on its line or the five above it.
    let lines = test_source_lines();
    let mut found = Vec::new();
    for (i, (path, n, line)) in lines.iter().enumerate() {
        let code = line.trim();
        if code.starts_with("//") {
            continue;
        }
        let long_sleep = literal_sleep_ms(code).is_some_and(|ms| ms >= 250);
        let upper_bound = code.contains("assert") && code.contains("elapsed()") && code.contains(" < ");
        if !(long_sleep || upper_bound) {
            continue;
        }
        let claimed = lines[i.saturating_sub(5)..=i].iter().any(|(p, _, l)| p == path && l.contains("time-claim:"));
        if !claimed {
            found.push(format!("{path}:{n}: {code}"));
        }
    }
    assert!(found.is_empty(), "wait for the result, or give the `// time-claim:` it asserts:\n{}", found.join("\n"));
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

#[test]
fn the_shared_helpers_are_defined_only_in_the_harness() {
    // One wait policy, one refusal check, one fixture writer, one store query (tests-build-G2, commands-G4): a file
    // that needs another shape calls the harness's and wraps it, it does not copy it.
    let found = test_lines_with(&[
        "fn wait_until(",
        "fn wait_until_for(",
        "fn refused(run: &Run, code: &str, exit: i32)",
        "fn sql(&self",
        "fn sql(env: &TestEnv",
        "fn sql(store: &mut Store",
        "fn query<T",
    ]);
    assert!(found.is_empty(), "use common::wait_until, wait_until_for, refused, sql and TestEnv::query:\n{}", found.join("\n"));
}

// ---- teardown: a dropped or killed test leaves no tmux server, process, dir or reaper ----

const HELPER_GATE: &str = "CPD_HARNESS_KILLED_TEST_HELPER";
const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a helper env put up: the dir and the pids of its tmux server, a detached process
/// under its cwd and a detached process with the dir in its command line.
struct Up {
    dir: std::path::PathBuf,
    pids: Vec<(&'static str, u32)>,
}

fn put_up(env: &TestEnv) -> Up {
    let out = env.tmux(&["new-session", "-d", "-s", "t", "sleep 100"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let tmux = String::from_utf8(env.tmux(&["display-message", "-p", "#{pid}"]).stdout).unwrap();
    let detached = |cmd: &mut std::process::Command| {
        cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap().id()
    };
    // Under the dir by its cwd only.
    let by_cwd = detached(env.command("setsid").args(["sleep", "100"]));
    // Under the dir by its command line only (a shell loop that never execs).
    let marker = env.root().join("driver");
    let by_cmdline = detached(
        env.command("setsid")
            .args(["sh", "-c", "while :; do sleep 1; done"])
            .arg(&marker)
            .current_dir("/"),
    );
    Up {
        dir: env.root().to_owned(),
        pids: vec![("tmux server", tmux.trim().parse().unwrap()), ("cwd process", by_cwd), ("cmdline process", by_cmdline)],
    }
}

fn alive(pid: u32) -> bool {
    // A zombie is dead; an unreadable /proc entry is gone.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| s.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z') && !rest.starts_with('X')))
}

/// Every live process whose command line mentions `dir` (the reaper's is `sh <dir>/teardown.sh watch <pid>`).
fn processes_mentioning(dir: &std::path::Path) -> Vec<u32> {
    let needle = dir.to_string_lossy().into_owned().into_bytes();
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
    let start = std::time::Instant::now();
    loop {
        let mut left: Vec<String> = up.pids.iter().filter(|(_, p)| alive(*p)).map(|(n, p)| format!("{n} {p}")).collect();
        if up.dir.exists() {
            left.push(format!("dir {}", up.dir.display()));
        }
        left.extend(processes_mentioning(&up.dir).into_iter().map(|p| format!("process {p} (reaper?) mentioning the dir")));
        if left.is_empty() || start.elapsed() > WAIT {
            return left;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Not a test of its own: the child process the killed-test test kills. Does nothing
/// unless the gate variable is set.
#[test]
fn killed_test_helper() {
    if std::env::var_os(HELPER_GATE).is_none() {
        return;
    }
    let env = TestEnv::new();
    let up = put_up(&env);
    // libtest prints "test <name> ... " first, without a newline.
    println!();
    println!("DIR {}", up.dir.display());
    for (name, pid) in &up.pids {
        println!("PID {name}|{pid}");
    }
    println!("READY");
    std::thread::sleep(std::time::Duration::from_secs(SLEEP_SECONDS));
    drop(env);
}

#[test]
fn a_killed_test_leaves_no_tmux_server_process_dir_or_reaper() {
    use std::io::{BufRead, BufReader};

    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "killed_test_helper", "--nocapture", "--test-threads=1"])
        .env(HELPER_GATE, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut dir = None;
    let mut pids = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        if line == "READY" {
            break;
        }
        if let Some(d) = line.strip_prefix("DIR ") {
            dir = Some(std::path::PathBuf::from(d));
        } else if let Some(p) = line.strip_prefix("PID ") {
            let (name, pid) = p.split_once('|').unwrap();
            pids.push((match name { "tmux server" => "tmux server", "cwd process" => "cwd process", _ => "cmdline process" }, pid.parse().unwrap()));
        }
    }
    let up = Up { dir: dir.expect("the helper printed no DIR"), pids };
    assert_eq!(up.pids.len(), 3, "the helper printed no READY with its pids");
    assert!(up.pids.iter().all(|(_, p)| alive(*p)), "everything is up before the kill");
    assert!(up.dir.exists());
    assert!(!processes_mentioning(&up.dir).is_empty(), "the reaper is up before the kill");

    child.kill().unwrap(); // SIGKILL: no Drop runs
    child.wait().unwrap();

    let left = leftovers_after_wait(&up);
    assert!(left.is_empty(), "a killed test left behind: {left:?}");
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
    let mut n = 0;
    common::wait_until("three polls", || {
        n += 1;
        n == 3
    });
    let caught = std::panic::catch_unwind(|| common::wait_until_for("never", std::time::Duration::from_millis(150), || false)).unwrap_err();
    assert!(caught.downcast_ref::<String>().is_some_and(|m| m.contains("timed out waiting for never")));
}

#[test]
fn wait_some_says_the_last_state_on_timeout() {
    let mut n = 0;
    let got = common::wait_some("a value", std::time::Duration::from_secs(5), || {
        n += 1;
        if n < 3 { Err(format!("poll {n}")) } else { Ok(n) }
    });
    assert_eq!(got, 3);
    let caught = std::panic::catch_unwind(|| common::wait_some::<()>("x", std::time::Duration::from_millis(150), || Err("stuck".into()))).unwrap_err();
    assert!(caught.downcast_ref::<String>().is_some_and(|m| m.contains("timed out waiting for x: stuck")));
}

#[test]
fn sql_writes_the_envs_store() {
    let env = TestEnv::new();
    env.write_tmux_config("");
    env.cpd(&["context", "new", "c1", "--title", "Before"]).ok();
    env.sql("UPDATE contexts SET title = 'After' WHERE id = 'c1'");
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"][0]["title"], "After");
}

#[test]
fn a_new_env_has_a_migrated_store_of_its_own_and_an_empty_env_has_none() {
    let a = TestEnv::new();
    let b = TestEnv::new();
    for env in [&a, &b] {
        let store = env.store();
        assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    }
    a.write_tmux_config("");
    b.write_tmux_config("");
    a.cpd(&["context", "new", "c1", "--title", "Only in a"]).ok();
    assert_eq!(a.cpd(&["context", "list"]).ok()["contexts"].as_array().unwrap().len(), 1);
    assert_eq!(b.cpd(&["context", "list"]).ok()["contexts"], serde_json::json!([]));

    let empty = TestEnv::empty();
    assert_eq!(std::fs::read_dir(empty.cpd_home()).unwrap().count(), 0, "an empty env's CPD_HOME is empty");
}
