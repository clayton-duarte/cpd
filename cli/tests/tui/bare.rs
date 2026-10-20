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

use common::{CPD, SLEEP_SECONDS, TestEnv, copy_executable, wait_until};
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

impl Terminal<'_> {
    /// Waits for the command to exit; its exit code. The shell creates the file before it
    /// writes the code (`echo $? > file`), so the file existing is not the code being there:
    /// the wait is for the whole line.
    fn exit_code(&self) -> i32 {
        let mut code = None;
        wait_until("the command to exit", || {
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

/// Whether the process `pid` runs the file `exe` (device and inode).
fn runs(pid: u32, exe: &str) -> bool {
    let (Ok(proc), Ok(file)) = (fs::metadata(format!("/proc/{pid}/exe")), fs::metadata(exe)) else { return false };
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

/// The server of the tests below is the *old* one: `older_server` copies the installed tmux to its own
/// path and, until `arm`, the fake `tmux` on `PATH` runs that copy for every call, so the server the
/// cockpit is built on has the copy as its own binary (`/proc/<pid>/exe`), as an older tmux has on hs.
/// Once armed, the fake says that server runs tmux 1.0 (and no other), and runs the installed tmux for
/// everything else: a call of the installed tmux to the old server is a call the restart must not make.
fn older_server(env: &TestEnv) -> PathBuf {
    let old = env.root().join("old-tmux");
    fs::create_dir_all(&old).unwrap();
    copy_executable(real_tmux().to_str().unwrap(), &old.join("tmux"));
    older_server_run_by(env, &old.join("tmux"))
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

/// The server that runs now is the one that is older than the installed tmux.
fn arm(env: &TestEnv, flag: &Path) {
    fs::write(flag, server_pid(env)).unwrap();
    arm_real(env, flag);
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
fn when_it_is_safe_an_older_server_is_restarted_every_session_comes_back_in_its_place_and_plain_tmux_attaches() {
    let env = env();
    fake_cpd_with_sleeping_sessions(&env);
    let flag = older_server(&env);
    let _driver = fixture(&env, None);
    cockpit_with_sessions(&env);
    assert_eq!(session_windows(&env), places());
    let before = server_pid(&env);
    let lead1_pane = tmux_out(&env, &["list-panes", "-a", "-F", "#{pane_id} #{@cpd_session}"]).lines().find(|l| l.ends_with(" lead1")).unwrap().to_owned();
    arm(&env, &flag);

    let t = run_in_terminal(&env, "restart", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    // Bare `cpd` restarts in its own process and attaches only when that is done, and it exits 0 whatever the
    // restart did (A3): its one line says whether the restart succeeded, so it is read before the server is.
    // A restart that failed after the kill leaves no server, and the line is then the only message there is.
    let line = t.stderr();
    assert!(line.contains("restarted the tmux server") && line.contains("3 Session(s) reopened"), "{line}");
    assert_eq!(line.lines().count(), 1, "{line}");
    assert_eq!(kill_server_calls(&env), 0, "the old server is killed by its own binary, not by the installed tmux on PATH");
    assert_ne!(server_pid(&env), before, "the server was not restarted");
    assert_eq!(session_windows(&env), places());
    let slot = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_session}"]);
    assert!(slot.lines().any(|l| l == "lead1"), "{slot}");
    assert_ne!(tmux_out(&env, &["list-panes", "-a", "-F", "#{pane_id} #{@cpd_session}"]).lines().find(|l| l.ends_with(" lead1")), Some(lead1_pane.as_str()));
    // The cockpit is whole: its views, and a Lead (not the placeholder) in the chat slot.
    assert_eq!(view_pids(&env).iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["plan", "tree", "widgets"]);
    // Each pane runs `cpd session run <id>`: the fake cpd was called again for each Session.
    for id in ["lead1", "lead2", "manager1"] {
        let runs = env.calls("cpd").iter().filter(|c| c.as_slice() == ["session", "run", id]).count();
        assert_eq!(runs, 2, "{id}: {:?}", env.calls("cpd"));
    }
    // The store follows the panes.
    for (id, window) in PLACES {
        let pane: String = store_of(&env).read(|c| Ok(c.query_row("SELECT tmux_pane FROM sessions WHERE id = ?1", [id], |r| r.get(0))?)).unwrap();
        let in_window = tmux_out(&env, &["list-panes", "-a", "-F", "#{pane_id} #{window_name}"]);
        assert!(in_window.lines().any(|l| l == format!("{pane} {window}")), "{id}: {pane} not in {window}: {in_window}");
    }
    assert!(!pending_file(&env).exists());
    // Each Session is `stopped` with no pane until its new pi reports `idle` (the fake's `session run` does,
    // as the adapter does at start-up), and then idle, in the pane the store records.
    wait_idle_in_live_panes(&env, &["lead1", "lead2", "manager1"]);
    // The server is as new as the installed tmux: the installed tmux attaches.
    assert_eq!(attach_calls(&env).len(), 1);
    // The engine runs again in the new server.
    assert_eq!(tmux_out(&env, &["list-panes", "-t", "=cpd-work:watch", "-F", "#{pane_dead}"]).trim(), "0");
}

#[test]
fn when_it_is_not_safe_nothing_is_killed_the_restart_is_marked_pending_and_the_servers_binary_attaches() {
    for (what, lead_state, attempt) in [
        ("a starting Attempt", "idle", Some("starting")),
        ("a running Attempt", "idle", Some("running")),
        ("a merging Attempt", "idle", Some("merging")),
        ("a working Lead", "working", None),
        ("a blocked Lead", "blocked", None),
    ] {
        let env = env();
        fake_cpd_with_sleeping_sessions(&env);
        let flag = older_server(&env);
        let _driver = fixture(&env, attempt);
        cockpit_with_sessions(&env);
        // The engine stops a Session that has no pane: the state is set once they have panes.
        store_of(&env).write("test", |tx| exec(tx, &format!("UPDATE sessions SET state = '{lead_state}', state_label = iif('{lead_state}' = 'blocked', 'a dialog', NULL) WHERE id = 'lead1'"))).unwrap();
        let (before, panes) = (server_pid(&env), tmux_out(&env, &["list-panes", "-a", "-F", "#{pane_id} #{@cpd_session}"]));
        arm(&env, &flag);

        let t = run_in_terminal(&env, "hold", false, "");
        wait_until(&format!("the attach ({what})"), || {
            let clients = env.tmux(&["list-clients", "-t", "=cpd", "-F", "#{client_pid}"]);
            clients.status.success() && !clients.stdout.is_empty()
        });
        assert_eq!(kill_server_calls(&env), 0, "{what}");
        assert_eq!(server_pid(&env), before, "{what}");
        assert_eq!(tmux_out(&env, &["list-panes", "-a", "-F", "#{pane_id} #{@cpd_session}"]), panes, "{what}");
        assert!(attach_calls(&env).is_empty(), "{what}: the installed tmux must not attach");
        let line = t.stderr();
        assert_eq!(line.lines().count(), 1, "{what}: {line}");
        assert!(
            line.contains("attaching with the server's own tmux") && line.contains("the restart happens once nothing is in the way"),
            "{what}: {line}"
        );
        let pending: serde_json::Value = serde_json::from_str(&fs::read_to_string(pending_file(&env)).unwrap()).unwrap();
        assert_eq!((pending["server"].as_str(), pending["failed"].is_null()), (Some("1.0"), true), "{what}");
    }
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

fn run_dir_file(env: &TestEnv, name: &str) -> String {
    fs::read_to_string(env.cpd_home().join("run").join(name)).unwrap_or_default()
}

fn count_events(env: &TestEnv, kind: &str) -> i64 {
    store_of(env).read(|c| Ok(c.query_row("SELECT count(*) FROM events WHERE kind = ?1", [kind], |r| r.get(0))?)).unwrap()
}

/// Ends Attempt `api-a1`, as its driver does.
fn end_attempt(env: &TestEnv) {
    let at = now();
    store_of(env)
        .write("test", |tx| {
            exec(tx, &format!("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = '{at}' WHERE id = 'api-a1';"))
        })
        .unwrap();
}

fn set_lead_state(env: &TestEnv, state: &str) {
    let label = if state == "blocked" { "'a dialog'" } else { "NULL" };
    store_of(env).write("test", |tx| exec(tx, &format!("UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = 'lead1'"))).unwrap();
}

/// `cpd tui` on a server that is older than the installed tmux, with a running Attempt and a blocked
/// Lead in the way: the restart is marked pending and the engine, which runs, holds and waits.
fn pending_restart_in_the_way(env: &TestEnv) -> (PathBuf, Flock<fs::File>) {
    fake_cpd_with_sleeping_sessions(env);
    let flag = older_server(env);
    let driver = fixture(env, Some("running")).unwrap();
    cockpit_with_sessions(env);
    set_lead_state(env, "blocked");
    arm(&env, &flag);
    env.cpd(&["tui"]).ok();
    assert!(pending_file(env).exists(), "cpd tui marks the restart pending");
    (flag, driver)
}

#[test]
fn once_nothing_runs_the_engine_restarts_the_server_detached_and_every_session_is_back_in_its_place() {
    let env = env();
    let (_flag, driver) = pending_restart_in_the_way(&env);
    let before = server_pid(&env);
    let started = count_events(&env, "watch-started");

    // The Attempt runs, then ends; the Lead stays blocked: the restart waits for both.
    // time-claim: the pending restart holds while an Attempt runs and a Lead is blocked; the engine ticks every 1 s, so 3.5 s is three ticks without kill-server; no per-poll signal from the engine for a tick that held.
    std::thread::sleep(Duration::from_millis(3500));
    assert_eq!((kill_server_calls(&env), server_pid(&env)), (0, before.clone()));
    end_attempt(&env);
    drop(driver);
    // time-claim: the pending restart holds while an Attempt runs and a Lead is blocked (a dialog is open); the engine ticks every 1 s, so 3.5 s is three ticks without kill-server; no per-poll signal from the engine for a tick that held.
    std::thread::sleep(Duration::from_millis(3500));
    assert_eq!((kill_server_calls(&env), server_pid(&env)), (0, before.clone()), "a blocked Lead has a dialog open");
    assert!(pending_file(&env).exists());

    set_lead_state(&env, "idle");
    wait_until("the restart to finish", || !pending_file(&env).exists());
    assert_ne!(server_pid(&env), before, "the server was not restarted");
    assert_eq!(kill_server_calls(&env), 0, "the old server is killed by its own binary, not by the installed tmux on PATH");
    assert_eq!(session_windows(&env), places());
    assert!(tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_session}"]).lines().any(|l| l == "lead1"));
    assert_eq!(view_pids(&env).iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["plan", "tree", "widgets"]);
    for id in ["lead1", "lead2", "manager1"] {
        let runs = env.calls("cpd").iter().filter(|c| c.as_slice() == ["session", "run", id]).count();
        assert_eq!(runs, 2, "{id}");
    }
    let log = run_dir_file(&env, "tmux-restart.log");
    assert!(log.contains("restarting the tmux server") && log.contains("reopened 3 Session(s); done"), "{log}");
    // `cpd tui` started the engine again, in the new server.
    wait_until("the new engine", || count_events(&env, "watch-started") == started + 1);
    assert_eq!(tmux_out(&env, &["list-panes", "-t", "=cpd-work:watch", "-F", "#{pane_dead}"]).trim(), "0");
    assert_eq!(env.cpd(&["events", "list", "--kind", "error"]).ok()["events"], serde_json::json!([]), "no error Event at all");
}

#[test]
fn a_tmux_failure_of_the_workers_step_while_a_restart_is_pending_is_no_error_and_the_options_are_set_on_the_new_server() {
    let env = env();
    let (flag, driver) = pending_restart_in_the_way(&env);
    // The restart takes the server away under the step: `list-sessions` fails as tmux's does while it dies.
    let fails = PathBuf::from(format!("{}.list-sessions-fails", flag.display()));
    let hits = || fs::read_to_string(format!("{}.hits", flag.display())).map_or(0, |t| t.lines().count());
    fs::write(&fails, "").unwrap();
    // The Attempt ends, so the counts change and the step calls tmux; the blocked Lead holds the restart.
    end_attempt(&env);
    drop(driver);
    // The step failed, and asks again at the next tick.
    wait_until("the step to fail twice", || hits() >= 2);
    assert_eq!(count_events(&env, "error"), 0, "{:?}", env.cpd(&["events", "list", "--kind", "error"]));
    assert!(pending_file(&env).exists());

    // The server is back: the restart runs and the new server has the options.
    fs::remove_file(&fails).unwrap();
    set_lead_state(&env, "idle");
    wait_until("the restart to finish", || !pending_file(&env).exists());
    wait_until("the options on the new server", || env.tmux(&["show", "-gv", "@cpd_workers"]).stdout.starts_with(b"#[dim]workers"));
    assert_eq!(count_events(&env, "error"), 0, "{:?}", env.cpd(&["events", "list", "--kind", "error"]));
}

#[test]
fn a_failed_restart_keeps_the_pending_file_with_its_reason_and_raises_one_error_event() {
    let env = env();
    let (flag, driver) = pending_restart_in_the_way(&env);
    // Lead two's parked window cannot be made after the restart.
    fs::write(format!("{}.no-park-other", flag.display()), "").unwrap();
    end_attempt(&env);
    drop(driver);
    set_lead_state(&env, "idle");

    wait_until("the failure to be reported", || count_events(&env, "error") >= 1);
    // The engine that runs again raises it once.
    // time-claim: the engine raises the failure once, not again on later ticks; 3.5 s is three 1 s ticks; no per-poll signal from the engine for a tick that raised nothing.
    std::thread::sleep(Duration::from_millis(3500));
    assert_eq!(count_events(&env, "error"), 1);
    let pending: serde_json::Value = serde_json::from_str(&fs::read_to_string(pending_file(&env)).unwrap()).unwrap();
    let reason = pending["failed"]["reason"].as_str().unwrap_or_else(|| panic!("{pending}\n{}\n{:?}", run_dir_file(&env, "tmux-restart.log"), env.cpd(&["events", "list", "--kind", "error"])));
    assert!(reason.contains("were not reopened") && reason.contains("lead2"), "{pending}");
    assert_eq!((pending["failed"]["code"].as_str(), pending["failed"]["reported"].as_bool()), (Some("tmux_failed"), Some(true)));
    let events = env.cpd(&["events", "list", "--kind", "error"]).ok();
    let event = &events["events"][0];
    assert_eq!((event["data"]["source"].as_str(), event["data"]["code"].as_str()), (Some("watch"), Some("tmux_failed")));
    assert!(event["data"]["message"].as_str().unwrap().contains("lead2"), "{event}");
    assert!(run_dir_file(&env, "tmux-restart.log").contains("failed:"));
    // The cockpit and the other Sessions did come back; `cpd doctor` says what failed.
    assert_eq!(session_windows(&env).iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["lead1", "manager1"]);
    wait_idle_in_live_panes(&env, &["lead1", "manager1"]);
    // The one that was not reopened says so: stopped, no pane (what engine E2 writes for a pane that is gone).
    let lead2 = states(&env).into_iter().find(|(id, _, _)| id == "lead2").unwrap();
    assert_eq!((lead2.1.as_str(), lead2.2), ("stopped", None));
    let doctor = env.cpd(&["doctor"]);
    // The tools of the test's PATH are missing, so `doctor` itself fails: its checks are in the failure's details.
    let json = doctor.json();
    let checks = if json["ok"] == true { &json["checks"] } else { &json["error"]["details"]["checks"] };
    let check = checks.as_array().unwrap().iter().find(|c| c["name"] == "tmux_restart").cloned().unwrap_or_else(|| panic!("{doctor:?}"));
    assert_eq!(check["ok"], false);
    assert!(check["message"].as_str().unwrap().contains("The tmux restart failed"), "{check}");
}

#[test]
fn a_session_the_panes_do_not_show_but_the_store_records_is_reopened_too() {
    // pr-links' case (hs-n285): the restart's list of Sessions had one fewer than the store's rows with a pane,
    // and that Lead came back with no pane.
    let env = env();
    fake_cpd_with_sleeping_sessions(&env);
    let flag = older_server(&env);
    let _driver = fixture(&env, None);
    cockpit_with_sessions(&env);
    let pane = tmux_out(&env, &["list-panes", "-a", "-F", "#{pane_id} #{@cpd_session}"])
        .lines()
        .find_map(|l| l.strip_suffix(" lead2").map(str::to_owned))
        .unwrap();
    tmux_out(&env, &["set", "-p", "-u", "-t", &pane, "@cpd_session"]);
    assert!(!session_windows(&env).iter().any(|(s, _)| s == "lead2"), "the pane must not show its Session");
    assert_eq!(states(&env).iter().find(|(id, _, _)| id == "lead2").unwrap().2.as_deref(), Some(pane.as_str()), "the store records it");
    arm(&env, &flag);

    let t = run_in_terminal(&env, "recorded", false, "");
    assert_eq!(t.exit_code(), 0, "{}", t.stderr());
    assert!(t.stderr().contains("3 Session(s) reopened"), "{}", t.stderr());
    assert_eq!(session_windows(&env), places());
    wait_idle_in_live_panes(&env, &["lead1", "lead2", "manager1"]);
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
