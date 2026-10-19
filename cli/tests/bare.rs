//! Bare `cpd` (commands.md §13.3, hs-d259): without a terminal it is the usage
//! error; in a terminal it starts the engine, builds or repairs the cockpit,
//! restarts views that run an older binary and attaches; inside the cockpit's
//! own server it attaches nothing.
//!
//! The terminal is a pane of a second private tmux server (`outer`): the
//! command's stdin and stdout are that pane's pty, its stderr and exit code go
//! to files. A fake `tmux` on `PATH` records every call and ends an `attach`
//! with exit 0, so no test attaches for real, except the one that pins which
//! tmux program attaches.

mod common;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{CPD, TestEnv, copy_executable};

fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    fake_cpd(&env, CPD);
    fake_tmux(&env, "");
    env
}

/// The `cpd` that the panes run: `exe`, recorded.
fn fake_cpd(env: &TestEnv, exe: impl AsRef<Path>) {
    env.fake("cpd", &format!("exec '{}' \"$@\"", exe.as_ref().display()));
}

/// The first `tmux` on the test process's own `PATH`.
fn real_tmux() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap();
    std::env::split_paths(&path).map(|d| d.join("tmux")).find(|p| p.is_file()).expect("tmux on PATH")
}

/// A fake `tmux` that records its calls, ends `attach` with exit 0 and otherwise
/// runs the real one. `extra` is shell code run first.
fn fake_tmux(env: &TestEnv, extra: &str) {
    env.fake("tmux", &format!("{extra}\ncase \" $* \" in *\" attach \"*) exit 0;; esac\nexec '{}' \"$@\"", real_tmux().display()));
}

fn attach_calls(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "attach")).collect()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A command run in a pty: `stdin_stdout` stay the terminal, stderr and the
/// exit code are files.
struct Terminal<'a> {
    env: &'a TestEnv,
    server: Option<String>,
    err: PathBuf,
    code: PathBuf,
    out: PathBuf,
}

impl Drop for Terminal<'_> {
    fn drop(&mut self) {
        if let Some(socket) = &self.server {
            let _ = common::output(self.env.command(real_tmux()).args(["-L", socket, "kill-server"]));
        }
    }
}

/// Runs `cpd` (the real one) with `redirect` after it in a new window of `cockpit_session`
/// of the cockpit's own server (`inside`) or in a session of a second server.
fn run_in_terminal<'a>(env: &'a TestEnv, name: &str, inside: bool, redirect: &str) -> Terminal<'a> {
    let dir = env.root().join(format!("term-{name}"));
    fs::create_dir_all(&dir).unwrap();
    let mut t = Terminal { env, server: None, err: dir.join("err"), code: dir.join("code"), out: dir.join("out") };
    let script = format!("'{CPD}' {redirect} 2> '{}'; echo $? > '{}'", t.err.display(), t.code.display());
    let out = if inside {
        env.tmux(&["new-window", "-d", "-t", "=cpd:", "-n", name, "sh", "-c", &script])
    } else {
        let socket = format!("{}-outer-{name}", env.tmux_socket());
        t.server = Some(socket.clone());
        common::output(env.command(real_tmux()).args(["-L", &socket, "new-session", "-d", "-x", "200", "-y", "50", "sh", "-c", &script]))
            .unwrap()
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    t
}

impl Terminal<'_> {
    /// Waits for the command to exit; its exit code.
    fn exit_code(&self) -> i32 {
        wait_until("the command to exit", || self.code.exists());
        fs::read_to_string(&self.code).unwrap().trim().parse().unwrap()
    }

    fn stderr(&self) -> String {
        fs::read_to_string(&self.err).unwrap_or_default()
    }
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// `(view, pid)` of the panes of `cpd:main` that have a view.
fn view_pids(env: &TestEnv) -> Vec<(String, u32)> {
    let mut views: Vec<(String, u32)> = tmux_out(env, &["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_view} #{pane_pid} #{pane_dead}"])
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            if f.len() != 3 || f[0].is_empty() || f[2] != "0" {
                return None;
            }
            Some((f[0].to_owned(), f[1].parse().ok()?))
        })
        .collect();
    views.sort();
    views
}

/// Whether the process `pid` runs the file `exe` (device and inode).
fn runs(pid: u32, exe: &str) -> bool {
    let (Ok(proc), Ok(file)) = (fs::metadata(format!("/proc/{pid}/exe")), fs::metadata(exe)) else { return false };
    (proc.dev(), proc.ino()) == (file.dev(), file.ino())
}

/// Waits until the five views of `cpd:main` all run `exe`; their pids.
fn wait_views_run(env: &TestEnv, exe: &str) -> Vec<(String, u32)> {
    let mut views = Vec::new();
    wait_until(&format!("the views to run {exe}"), || {
        views = view_pids(env);
        views.len() == 5 && views.iter().all(|(_, pid)| runs(*pid, exe))
    });
    views
}

fn pane_pids_of_window(env: &TestEnv, target: &str) -> Vec<String> {
    tmux_out(env, &["list-panes", "-t", target, "-F", "#{pane_id} #{pane_pid}"]).lines().map(str::to_owned).collect()
}

#[test]
fn without_a_terminal_it_is_the_usage_error_and_does_nothing() {
    let env = env();
    let run = env.cpd(&[]);
    assert_eq!(run.exit, 2, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "usage");
    assert_eq!(error["message"], "A command is required: see details.help.");
    assert!(error["details"]["help"].is_string());
    assert!(!env.tmux(&["list-sessions"]).status.success(), "a tmux server was started");
    assert!(env.calls("cpd").is_empty() && attach_calls(&env).is_empty());
}

#[test]
fn with_a_terminal_only_on_stdin_it_is_the_usage_error() {
    let env = env();
    let t = run_in_terminal(&env, "stdin-only", false, &format!("> '{}'", env.root().join("term-stdin-only/out").display()));
    assert_eq!(t.exit_code(), 2, "{}", t.stderr());
    let printed: serde_json::Value = serde_json::from_str(fs::read_to_string(&t.out).unwrap().trim()).unwrap();
    assert_eq!(printed["error"]["code"], "usage");
    assert!(!env.tmux(&["list-sessions"]).status.success(), "a tmux server was started");
}

#[test]
fn in_a_terminal_it_builds_the_cockpit_starts_the_engine_and_attaches() {
    let env = env();
    let t = run_in_terminal(&env, "build", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    assert_eq!(t.stderr(), "");

    let socket = env.tmux_socket();
    assert_eq!(attach_calls(&env), [["-L", socket, "-f", "/dev/null", "attach", "-t", "cpd"]]);
    assert_eq!(view_pids(&env).iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["chat", "plan", "statusbar", "tree", "widgets"]);
    // The engine runs in its own session.
    assert_eq!(tmux_out(&env, &["list-windows", "-t", "=cpd-work", "-F", "#{window_name}"]).trim(), "watch");
    assert_eq!(tmux_out(&env, &["list-panes", "-t", "=cpd-work:watch", "-F", "#{pane_dead}"]).trim(), "0");
}

#[test]
fn it_adds_a_pane_the_layout_is_missing() {
    let env = env();
    assert_eq!(run_in_terminal(&env, "first", false, "").exit_code(), 0);
    let bar = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_id} #{@cpd_view}"])
        .lines()
        .find_map(|l| l.strip_suffix(" statusbar").map(str::to_owned))
        .unwrap();
    tmux_out(&env, &["kill-pane", "-t", &bar]);
    assert_eq!(view_pids(&env).len(), 4);

    let t = run_in_terminal(&env, "second", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    assert_eq!(view_pids(&env).iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["chat", "plan", "statusbar", "tree", "widgets"]);
    assert_eq!(attach_calls(&env).len(), 2);
}

#[test]
fn it_restarts_views_that_run_an_older_binary_once() {
    let env = env();
    let old = env.root().join("old-cpd");
    copy_executable(CPD, &old);
    let old = old.to_str().unwrap().to_owned();
    // The cockpit is built by an older cpd: its views run `old`.
    fake_cpd(&env, &old);
    env.cpd(&["tui"]).ok();
    let before = wait_views_run(&env, &old);
    let engine = pane_pids_of_window(&env, "=cpd-work:watch");
    // The install: panes started now run the installed binary.
    fake_cpd(&env, CPD);

    let t = run_in_terminal(&env, "update", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    assert_eq!(t.stderr(), "");
    let after = wait_views_run(&env, CPD);
    assert_eq!(after.iter().map(|(v, _)| v).collect::<Vec<_>>(), before.iter().map(|(v, _)| v).collect::<Vec<_>>());
    assert!(after.iter().zip(&before).all(|(a, b)| a.1 != b.1), "{before:?} -> {after:?}");
    assert_eq!(pane_pids_of_window(&env, "=cpd-work:watch"), engine, "the engine is not touched");
    assert_eq!(attach_calls(&env).len(), 1);

    // Nothing is old now: the next one restarts nothing.
    let again = run_in_terminal(&env, "again", false, "");
    assert_eq!(again.exit_code(), 0, "{}", again.stderr());
    assert_eq!(view_pids(&env), after);
    assert_eq!(attach_calls(&env).len(), 2);
}

#[test]
fn inside_the_cockpit_it_does_not_nest_an_attach() {
    let env = env();
    let old = env.root().join("old-cpd");
    copy_executable(CPD, &old);
    let old = old.to_str().unwrap().to_owned();
    fake_cpd(&env, &old);
    env.cpd(&["tui"]).ok();
    let before = wait_views_run(&env, &old);
    fake_cpd(&env, CPD);

    let t = run_in_terminal(&env, "here", true, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    assert!(attach_calls(&env).is_empty(), "{:?}", attach_calls(&env));
    let line = t.stderr();
    assert_eq!(line.lines().count(), 1, "{line}");
    assert!(line.contains("tmux switch-client -t cpd"), "{line}");
    // The follow-ups that are safe from inside still ran.
    let after = wait_views_run(&env, CPD);
    assert!(after.iter().zip(&before).all(|(a, b)| a.1 != b.1), "{before:?} -> {after:?}");
}

#[test]
fn when_the_server_runs_another_tmux_the_servers_own_binary_attaches() {
    let env = env();
    // The installed tmux says it is another version than the server's.
    fake_tmux(&env, "[ \"$1\" = -V ] && { echo 'tmux 1.0'; exit 0; }");
    let t = run_in_terminal(&env, "other", false, "");
    wait_until("the attach", || {
        let clients = env.tmux(&["list-clients", "-t", "=cpd", "-F", "#{client_pid}"]);
        clients.status.success() && !clients.stdout.is_empty()
    });
    assert!(attach_calls(&env).is_empty(), "the installed tmux must not attach: {:?}", attach_calls(&env));
    let line = t.stderr();
    assert!(line.contains("installed tmux is 1.0") && line.contains("attaching with the server's own tmux"), "{line}");
    assert!(!t.code.exists(), "attach replaced the process and is still attached");
    assert_eq!(line.lines().count(), 1);
}
