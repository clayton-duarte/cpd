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

use crate::common;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{CPD, SLEEP_SECONDS, TestEnv, copy_executable, wait_until, wait_until_for};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use nix::fcntl::{Flock, FlockArg};
use time::OffsetDateTime;
use time::macros::format_description;

fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [watch]\ntick_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
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

/// How long a `cpd` in a terminal may take to exit: a restart of a real tmux server that brings every session back.
/// Alone these tests take 11-41 s (16 `bare::` tests at once, 14 cores); in the Gate they ran 66-77 s and the 30 s
/// of `common::WAIT` was not enough. Only `bare::` waits this long (see the test group in `.config/nextest.toml`).
const EXIT_WAIT: Duration = Duration::from_secs(90);

impl Terminal<'_> {
    /// Waits for the command to exit; its exit code. The shell creates the file before it
    /// writes the code (`echo $? > file`), so the file existing is not the code being there:
    /// the wait is for the whole line.
    fn exit_code(&self) -> i32 {
        let mut code = None;
        wait_until_for("the command to exit", EXIT_WAIT, || {
            code = fs::read_to_string(&self.code).ok().and_then(|text| text.strip_suffix('\n')?.parse().ok());
            code.is_some()
        });
        code.unwrap()
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

/// The path of the executable process `pid` runs: `/proc/<pid>/exe` where it exists, `lsof` (whose first
/// `n` line of the `txt` descriptors is the executable) on macOS.
fn exe_of(pid: u32) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let out = std::process::Command::new("lsof").args(["-a", "-p", &pid.to_string(), "-d", "txt", "-Fn"]).output().ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        return text.lines().find_map(|l| l.strip_prefix('n')).map(PathBuf::from);
    }
    fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// Whether the process `pid` runs the file `exe` (device and inode).
fn runs(pid: u32, exe: &str) -> bool {
    let (Some(proc), Ok(file)) = (exe_of(pid).and_then(|p| fs::metadata(p).ok()), fs::metadata(exe)) else { return false };
    (proc.dev(), proc.ino()) == (file.dev(), file.ino())
}

/// Waits until the four views of `cpd:main` all run `exe`; their pids.
fn wait_views_run(env: &TestEnv, exe: &str) -> Vec<(String, u32)> {
    let mut views = Vec::new();
    wait_until(&format!("the views to run {exe}"), || {
        views = view_pids(env);
        views.len() == 4 && views.iter().all(|(_, pid)| runs(*pid, exe))
    });
    views
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
    assert_eq!(attach_calls(&env), [["-u", "-L", socket, "-f", "/dev/null", "attach", "-t", "cpd"]]);
    assert_eq!(view_pids(&env).iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["chat", "plan", "tree", "widgets"]);
    // The engine runs in its own session.
    assert_eq!(tmux_out(&env, &["list-windows", "-t", "=cpd-work", "-F", "#{window_name}"]).trim(), "watch");
    assert_eq!(tmux_out(&env, &["list-panes", "-t", "=cpd-work:watch", "-F", "#{pane_dead}"]).trim(), "0");
}

#[test]
fn it_removes_an_older_cpds_statusbar_pane() {
    let env = env();
    assert_eq!(run_in_terminal(&env, "first", false, "").exit_code(), 0);
    // A main built by an older cpd: the StatusBar pane at the bottom (the StatusBar is now the status line).
    let tree = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_id} #{@cpd_view}"])
        .lines()
        .find_map(|l| l.strip_suffix(" tree").map(str::to_owned))
        .unwrap();
    let bar = tmux_out(&env, &["split-window", "-d", "-v", "-f", "-l", "1", "-t", &tree, "-P", "-F", "#{pane_id}", "sleep", "3600"]);
    let bar = bar.trim();
    tmux_out(&env, &["set", "-p", "-t", bar, "@cpd_view", "statusbar"]);
    assert_eq!(view_pids(&env).len(), 5);

    let t = run_in_terminal(&env, "second", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    let panes = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_id}"]);
    assert!(!panes.lines().any(|p| p == bar), "the old StatusBar pane is gone: {panes}");
    assert_eq!(view_pids(&env).iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["chat", "plan", "tree", "widgets"]);
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


// ---------- the tmux restart (contract §6.1, hs-d294) ----------

/// The `cpd` of the panes: a Lead's `session run` reports `idle` as its pi's adapter does at start-up
/// (commands.md §15.2), once `arm` has said the server is the old one (the panes of the cockpit's first
/// build report nothing, so a test's own states are not overwritten), then sleeps; everything else is
/// the built binary.
fn fake_cpd_with_sleeping_sessions(env: &TestEnv) {
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2\" in \"session run\") [ -e '{root}/adapter-reports' ] && '{CPD}' session state idle --session \"$3\" >/dev/null 2>&1; exec sleep {SLEEP_SECONDS};; esac\nexec '{CPD}' \"$@\"",
            root = env.root().display()
        ),
    );
}

/// [`older_server`], with `old` as the program that builds the server (an older tmux for real).
fn older_server_run_by(env: &TestEnv, old: &Path) -> PathBuf {
    let flag = env.root().join("older-server");
    fs::write(format!("{}.build", flag.display()), "").unwrap();
    fake_tmux(
        env,
        &format!(
            "if [ -e '{flag}.build' ]; then exec '{old}' \"$@\"; fi\n\
             if [ -e '{flag}' ]; then\n\
               case \" $* \" in\n\
                 *\" display -p #{{version}} \"*)\n\
                   pid=$('{real}' -L '{socket}' -f /dev/null display -p '#{{pid}}' 2>/dev/null)\n\
                   [ \"$pid\" = \"$(cat '{flag}')\" ] && {{ echo 1.0; exit 0; }};;\n\
               esac\n\
             fi\n\
             if [ -e '{flag}.list-sessions-fails' ]; then\n\
               case \" $* \" in *\" list-sessions \"*) echo hit >> '{flag}.hits'; echo 'server exited unexpectedly' >&2; exit 1;; esac\n\
             fi\n\
             if [ -e '{flag}.record-go-signals' ]; then\n\
               a=''; b=''; c=''; d=''; e=''; for x in \"$@\"; do a=$b; b=$c; c=$d; d=$e; e=$x; done\n\
               if [ \"$b\" = -t ] && [ \"$d\" = @cpd_session ] && [ -n \"$e\" ]; then\n\
                 echo \"$c $e $('{CPD}' session show \"$e\" 2>&1 | tr -d '\\n')\" >> '{flag}.go-signals'\n\
               fi\n\
             fi\n\
             if [ -e '{flag}.no-park-other' ]; then\n\
               case \" $* \" in *\" new-window \"*\" -n park-other \"*) echo 'no window for you' >&2; exit 1;; esac\n\
             fi",
            flag = flag.display(),
            old = old.display(),
            real = real_tmux().display(),
            socket = env.tmux_socket(),
        ),
    );
    flag
}

/// [`arm`] for a server that really is older: the fake need not say so.
fn arm_real(env: &TestEnv, flag: &Path) {
    fs::write(env.root().join("adapter-reports"), "").unwrap();
    fs::remove_file(format!("{}.build", flag.display())).unwrap();
}

/// `(session, state, pane)` of every Lead and Manager, sorted by id.
fn states(env: &TestEnv) -> Vec<(String, String, Option<String>)> {
    store_of(env)
        .read(|c| {
            let mut stmt = c.prepare("SELECT id, state, tmux_pane FROM sessions WHERE role != 'worker' ORDER BY id")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// Waits until every one of `ids` is `idle` with a pane that exists in the server, as the adapter of
/// the pi of its new pane reports it.
fn wait_idle_in_live_panes(env: &TestEnv, ids: &[&str]) {
    let mut seen = (Vec::new(), String::new());
    let done = common::poll(common::WAIT, || {
        seen = (states(env), tmux_out(env, &["list-panes", "-a", "-F", "#{pane_id} #{@cpd_session}"]));
        let live = |p: &str| seen.1.lines().any(|l| l.split(' ').next() == Some(p));
        seen.0
            .iter()
            .filter(|(id, _, _)| ids.contains(&id.as_str()))
            .all(|(_, state, pane)| state == "idle" && pane.as_deref().is_some_and(live))
            .then_some(())
    });
    assert!(
        done.is_some(),
        "timed out waiting for {ids:?} to be idle in live panes: rows {:?}, panes {:?}\nsession-state and session-moved Events: {}",
        seen.0,
        seen.1,
        env.cpd(&["events", "list", "--kind", "session-state", "--kind", "session-moved"]).stdout
    );
}

fn home_of(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn store_of(env: &TestEnv) -> Store {
    Store::open(&home_of(env), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn now() -> String {
    OffsetDateTime::now_utc().format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z")).unwrap()
}

/// Context `ctx` with its Manager `manager1`, and Plans `pay` (Lead `lead1`) and
/// `other` (Lead `lead2`), all idle. Every Session's cwd is the env's root. With `attempt`, Job `api`
/// of `pay` has a running Attempt in that status, whose driver lock is held by the returned guard.
fn fixture(env: &TestEnv, attempt: Option<&str>) -> Option<Flock<fs::File>> {
    let cwd = env.root().display().to_string();
    let at = now();
    store_of(env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{at}', 'm1');
                     INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
                       ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '{cwd}', 'O3', 'idle', '{at}', '{at}'),
                       ('lead2', 'lead', 'ctx', 'other', NULL, 'Lead two', '{cwd}', 'O3', 'idle', '{at}', '{at}'),
                       ('manager1', 'manager', 'ctx', NULL, NULL, 'Manager', '{cwd}', 'O5', 'idle', '{at}', '{at}');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
                       ('pay', 'ctx', 'Pay', 'grounding', 'lead1', '{at}'),
                       ('other', 'ctx', 'Other', 'grounding', 'lead2', '{at}');"
                ),
            )?;
            if let Some(status) = attempt {
                exec(
                    tx,
                    &format!(
                        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                           VALUES ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {id}, '{at}');
                         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                           status, appended, fingerprint, created_txn, created_at)
                           VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {id}, '{at}');
                         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid, submitted_sha, submitted_at)
                           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', '{status}', 'cpd/pay--api-a1', '/w', '{at}', 4242, {sha}, {sha_at});
                         {running}",
                        fp = "a".repeat(64),
                        running = if status == "starting" {
                            String::new()
                        } else {
                            format!(
                                "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
                                 VALUES (cpd_txn(), '{at}', 'attempt-status', 'ctx', 'pay', 'api', 'api-a1', 'running',
                                         '{{\"from\":\"starting\",\"to\":\"running\"}}');"
                            )
                        },
                        id = tx.id(),
                        sha = if status == "merging" { "'abc'" } else { "NULL" },
                        sha_at = if status == "merging" { format!("'{at}'") } else { "NULL".to_owned() },
                    ),
                )?;
            }
            Ok(())
        })
        .unwrap();
    attempt.map(|_| {
        let path = home_of(env).attempt_lock(&"pay".parse().unwrap(), &"api-a1".parse().unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
        Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap()
    })
}

/// Builds the cockpit and puts `lead1` in the chat slot, `lead2` in a parked window and the Manager
/// in its Context window, each in a pane running `cpd session run <id>`.
fn cockpit_with_sessions(env: &TestEnv) {
    env.cpd(&["tui"]).ok();
    for id in ["manager1", "lead2", "lead1"] {
        env.cpd(&["session", "open", id]).ok();
    }
}

/// `(session, window name)` of every pane that has a Session, sorted.
fn session_windows(env: &TestEnv) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> =
        tmux_out(env, &["list-panes", "-a", "-F", "#{@cpd_session}\t#{window_name}"])
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .filter(|(session, _)| !session.is_empty())
            .map(|(s, w)| (s.to_owned(), w.to_owned()))
            .collect();
    found.sort();
    found
}

fn server_pid(env: &TestEnv) -> String {
    tmux_out(env, &["display", "-p", "#{pid}"]).trim().to_owned()
}

fn kill_server_calls(env: &TestEnv) -> usize {
    env.calls("tmux").iter().filter(|c| c.iter().any(|a| a == "kill-server")).count()
}

fn pending_file(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("run/tmux-restart.pending")
}

/// The windows the Sessions are in when `cockpit_with_sessions` has run.
const PLACES: [(&str, &str); 3] = [("lead1", "main"), ("lead2", "park-other"), ("manager1", "ctx-ctx")];

fn places() -> Vec<(String, String)> {
    PLACES.iter().map(|(s, w)| ((*s).to_owned(), (*w).to_owned())).collect()
}

#[test]
fn a_server_that_is_not_older_needs_no_restart_and_a_stale_pending_file_is_cleared() {
    let env = env();
    env.cpd(&["tui"]).ok();
    fs::write(pending_file(&env), r#"{"at":"t","server":"1.0","installed":"9.9","failed":{"at":"t","code":"tmux_failed","reason":"it broke.","reported":true}}"#).unwrap();
    let t = run_in_terminal(&env, "stale", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    assert_eq!(kill_server_calls(&env), 0);
    assert!(t.stderr().contains("The tmux restart failed") && t.stderr().contains("it broke."), "{}", t.stderr());
    assert!(!pending_file(&env).exists());
}

/// An installed tmux that is older than the one on `PATH`, at a known path; else why there is none.
fn older_real_tmux() -> Result<PathBuf, String> {
    use std::io::Write;
    const KNOWN: &str = "/usr/bin/tmux";
    let version = |program: &Path| -> Result<String, String> {
        let out = std::process::Command::new(program).arg("-V").output().map_err(|e| format!("{} -V: {e}", program.display()))?;
        String::from_utf8_lossy(&out.stdout).trim().strip_prefix("tmux ").map(str::to_owned).ok_or_else(|| format!("{} -V printed nothing usable", program.display()))
    };
    let result = version(Path::new(KNOWN)).and_then(|old| {
        let installed = version(&real_tmux())?;
        if cpd_core::host::tmux::older_version(&old, &installed) {
            Ok(PathBuf::from(KNOWN))
        } else {
            Err(format!("{KNOWN} is tmux {old}, not older than the tmux on PATH ({installed})"))
        }
    });
    if let Err(why) = &result {
        // Written to the stderr handle, which the test harness does not capture: seen in every run.
        let _ = writeln!(std::io::stderr(), "\nNOT APPLICABLE (a real older tmux): {why}");
    }
    result
}

#[test]
fn a_real_older_tmux_server_is_restarted_onto_the_installed_tmux() {
    // hs-n285: the first real restart (tmux 3.4 -> 3.8) died on the old server's last words.
    let Ok(older) = older_real_tmux() else { return };
    let env = env();
    fake_cpd_with_sleeping_sessions(&env);
    let flag = older_server_run_by(&env, &older);
    let _driver = fixture(&env, None);
    cockpit_with_sessions(&env);
    assert_eq!(session_windows(&env), places());
    let old_version = tmux_out(&env, &["display", "-p", "#{version}"]).trim().to_owned();
    let installed = String::from_utf8(std::process::Command::new(real_tmux()).arg("-V").output().unwrap().stdout).unwrap();
    assert!(!installed.contains(&old_version), "the server runs {old_version}, the installed tmux is {installed}");
    let before = server_pid(&env);
    arm_real(&env, &flag);

    let t = run_in_terminal(&env, "real-older", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    assert_ne!(server_pid(&env), before, "the server was not restarted");
    assert_eq!(kill_server_calls(&env), 0);
    let new_version = tmux_out(&env, &["display", "-p", "#{version}"]);
    assert!(installed.contains(new_version.trim()), "the new server runs {new_version}, the installed tmux is {installed}");
    assert_eq!(session_windows(&env), places());
    wait_idle_in_live_panes(&env, &["lead1", "lead2", "manager1"]);
    assert!(!pending_file(&env).exists());
}
