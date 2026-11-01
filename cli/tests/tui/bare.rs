//! Bare `cpd` (commands.md §13.3, hs-d259): without a terminal it is the usage error; in a terminal it starts the
//! engine, builds or repairs the cockpit, restarts views that run an older binary and attaches; inside the cockpit's
//! own server it attaches nothing.
//!
//! Every test runs in the test process (`Fx`, on the fake ports and the fake tmux server). No terminal is the usage
//! error of the command tree; in a terminal the test calls `cpd_tui::bare` itself (`common::cockpit::bare`, with the
//! engine's own restart). The attach is an `exec` that replaces the process: the `FakeSpawn` records it and fails it,
//! so a test reads the `tmux -u -L <socket> -f /dev/null attach -t cpd` line from `fx.fakes.spawn.launches()` and the
//! error `bare` returns. The engine's `watch --ensure` is the `RunCommand` the cockpit helper records (`ran`).
//!
//! A server older than the installed tmux is the route's answer (`OlderServer`): `display -p #{version}` says `1.0` while
//! it is armed and until `kill-server`; `-V` says the installed tmux (the route's `3.6a`); the server's own binary is the
//! file the `FakeSpawn` says the server's pid runs.

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::cockpit::{NO_RESTART, RESTART, bare, ran, route_the_restarts_cpd_tui, tui};
use common::fx::Fx;
use common::session::GoSignals;
use common::tree::fx;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::host::spawn::{Launch, Pid};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::{Store, WriteTxn};
use nix::fcntl::{Flock, FlockArg};
use serde_json::Value;
use time::OffsetDateTime;
use time::macros::format_description;

/// An `Fx` with the config the cockpit needs, and what bare `cpd` reads of the process: the installed `cpd` (the one
/// that started this process, `exe::started_as`) and the restart's `cpd tui`.
fn terminal() -> Fx {
    let fx = fx();
    cpd_core::host::exe::record_start();
    route_the_restarts_cpd_tui(&fx);
    fx
}

/// The attaches (every `exec`) the `FakeSpawn` recorded.
fn attached(fx: &Fx) -> Vec<Launch> {
    fx.fakes.spawn.launches()
}

/// The arguments of the attach of the cockpit's session on the env's socket.
fn attach_args(fx: &Fx) -> Vec<String> {
    [
        "-u",
        "-L",
        fx.env.tmux_socket(),
        "-f",
        "/dev/null",
        "attach",
        "-t",
        "cpd",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// The attach `bare` execs with `program`: `tmux` or the server's own binary. Its client has no `TMUX`, `TMUX_PANE`,
/// `CPD_SESSION` or `CPD_ATTEMPT` (hs-d463).
fn attach(fx: &Fx, program: &str) -> Launch {
    let env_remove = ["CPD_ATTEMPT", "CPD_SESSION", "TMUX", "TMUX_PANE"]
        .map(str::to_owned)
        .to_vec();
    Launch {
        program: program.to_owned(),
        args: attach_args(fx),
        env_remove,
        ..Launch::default()
    }
}

/// Bare `cpd` in a terminal: what it wrote to stderr, after it ended in the attach that the `FakeSpawn` failed.
fn in_a_terminal(fx: &Fx) -> String {
    let (result, stderr) = fx.with_stderr(|| bare(fx, RESTART));
    let error = result.expect_err("the attach is an exec: it returns only when it fails");
    assert_eq!(
        (error.code(), error.message()),
        (ErrorCode::TmuxFailed, "Cannot run tmux attach: faked exec."),
        "{error:?}"
    );
    stderr
}

/// `(view, pid)` of the live panes of `cpd:main` that have a view, from the fake server.
fn view_pids_in(fx: &Fx) -> Vec<(String, u32)> {
    let out = fx.tmux(&[
        "list-panes",
        "-t",
        "=cpd:main",
        "-F",
        "#{@cpd_view} #{pane_pid} #{pane_dead}",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut views: Vec<(String, u32)> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f.len() == 3 && !f[0].is_empty() && f[2] == "0")
                .then(|| Some((f[0].to_owned(), f[1].parse().ok()?)))?
        })
        .collect();
    views.sort();
    views
}

#[test]
fn without_a_terminal_it_is_the_usage_error_and_does_nothing() {
    let fx = fx();
    let run = fx.cpd_without_a_terminal(&[]);
    assert_eq!(run.exit, 2, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "usage");
    assert_eq!(error["message"], "A command is required: see details.help.");
    assert!(error["details"]["help"].is_string());
    assert!(
        !fx.tmux(&["list-sessions"]).status.success(),
        "a tmux server was started"
    );
    assert!(fx.calls("cpd").is_empty() && attached(&fx).is_empty());
}

#[test]
fn with_a_terminal_only_on_stdin_it_is_the_usage_error() {
    // The view of the terminal is `on_a_terminal()`: stdin and stdout both. A test process cannot give itself a pty
    // (nix's `term` feature is off), so stdin is the harness's own and stdout is the one the command prints to: the
    // error is printed as JSON there, as it was on the redirect of the pane.
    let fx = fx();
    let run = fx.cpd_without_a_terminal(&[]);
    assert_eq!(run.exit, 2, "{run:?}");
    let printed: serde_json::Value = serde_json::from_str(run.stdout.trim()).unwrap();
    assert_eq!(printed["error"]["code"], "usage");
    assert!(
        !fx.tmux(&["list-sessions"]).status.success(),
        "a tmux server was started"
    );
}

// What the recording `RunCommand` of `inside_the_cockpit_it_does_not_nest_an_attach` ran.
thread_local! {
    static RAN: RefCell<Vec<Vec<OsString>>> = const { RefCell::new(Vec::new()) };
}

fn record(argv: Vec<OsString>) -> Result<Output, CpdError> {
    RAN.with(|ran| ran.borrow_mut().push(argv));
    Ok(Output::Success(serde_json::Map::new()))
}

#[test]
fn inside_the_cockpit_it_does_not_nest_an_attach() {
    let fx = fx();
    // the cockpit's four views, running a binary that is not the installed one
    fx.cockpit();
    let tree = fx
        .panes()
        .into_iter()
        .find(|p| p.view == "tree")
        .unwrap()
        .pane;
    let widgets = fx.tmux(&[
        "split-window",
        "-d",
        "-t",
        &tree,
        "-P",
        "-F",
        "#{pane_id}",
        "cpd tui view widgets",
    ]);
    assert!(widgets.status.success());
    let widgets = String::from_utf8(widgets.stdout).unwrap();
    assert!(
        fx.tmux(&["set", "-p", "-t", widgets.trim(), "@cpd_view", "widgets"])
            .status
            .success()
    );
    cpd_core::host::exe::record_start();
    let old = fx.env.root().join("old-cpd");
    fs::write(&old, "an older cpd").unwrap();
    let before = view_pids_in(&fx);
    assert_eq!(
        before.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(),
        ["chat", "plan", "tree", "widgets"]
    );
    for (_, pid) in &before {
        fx.fakes.spawn.set_exe_of(Pid(*pid), &old);
    }
    // this terminal is in the cockpit's server
    fx.export(
        "TMUX",
        &format!(
            "{}/{},4242,0",
            fx.env.tmux_tmpdir().display(),
            fx.env.tmux_socket()
        ),
    );
    fx.export("TMUX_PANE", "%999");

    let (result, stderr) = fx.with_stderr(|| {
        Ports::scope(&fx.fakes.ports, || {
            cpd_tui::bare("test", record, NO_RESTART)
        })
    });
    assert!(
        matches!(result, Ok(Output::Owned)),
        "it prints nothing and attaches nothing"
    );
    assert!(attached(&fx).is_empty(), "{:?}", attached(&fx));
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.contains("tmux switch-client -t cpd"), "{stderr}");
    // The follow-ups that are safe from inside still ran: the engine, and the views of the older binary restarted.
    assert_eq!(
        RAN.with(|ran| ran.borrow().clone()),
        [["cpd", "watch", "--ensure"].map(OsString::from).to_vec()]
    );
    let after = view_pids_in(&fx);
    assert_eq!(after.len(), 4, "{after:?}");
    assert!(
        after.iter().zip(&before).all(|(a, b)| a.1 != b.1),
        "{before:?} -> {after:?}"
    );
}

#[test]
fn in_a_terminal_it_builds_the_cockpit_starts_the_engine_and_attaches() {
    let fx = terminal();
    assert_eq!(in_a_terminal(&fx), "");

    assert_eq!(attached(&fx), [attach(&fx, "tmux")]);
    assert_eq!(
        view_pids_in(&fx)
            .iter()
            .map(|(v, _)| v.as_str())
            .collect::<Vec<_>>(),
        ["chat", "plan", "tree", "widgets"]
    );
    // The engine is started by the recorded `cpd watch --ensure` (its own session is `watch --ensure`'s).
    assert_eq!(ran(), [["cpd", "watch", "--ensure"]]);
}

#[test]
fn it_removes_an_older_cpds_statusbar_pane() {
    let fx = terminal();
    assert_eq!(in_a_terminal(&fx), "");
    // A main built by an older cpd: the StatusBar pane at the bottom (the StatusBar is now the status line).
    let tree = fx
        .panes()
        .into_iter()
        .find(|p| p.view == "tree")
        .unwrap()
        .pane;
    let bar = fx.tmux(&[
        "split-window",
        "-d",
        "-v",
        "-f",
        "-l",
        "1",
        "-t",
        &tree,
        "-P",
        "-F",
        "#{pane_id}",
        "sleep",
        "3600",
    ]);
    assert!(
        bar.status.success(),
        "{}",
        String::from_utf8_lossy(&bar.stderr)
    );
    let bar = String::from_utf8(bar.stdout).unwrap().trim().to_owned();
    assert!(
        fx.tmux(&["set", "-p", "-t", &bar, "@cpd_view", "statusbar"])
            .status
            .success()
    );
    assert_eq!(view_pids_in(&fx).len(), 5);

    assert_eq!(in_a_terminal(&fx), "");
    assert!(
        !fx.panes().iter().any(|p| p.pane == bar),
        "the old StatusBar pane is gone: {:?}",
        fx.panes()
    );
    assert_eq!(
        view_pids_in(&fx)
            .iter()
            .map(|(v, _)| v.as_str())
            .collect::<Vec<_>>(),
        ["chat", "plan", "tree", "widgets"]
    );
    assert_eq!(attached(&fx).len(), 2);
}

/// The views of `cpd:main` run `exe`, as the `FakeSpawn` says (what `/proc/<pid>/exe` is for the real one).
fn views_run(fx: &Fx, exe: &Path) -> Vec<(String, u32)> {
    let views = view_pids_in(fx);
    for (_, pid) in &views {
        fx.fakes.spawn.set_exe_of(Pid(*pid), exe);
    }
    views
}

/// The pane pids of `window` of the fake server.
fn pane_pids_of_window(fx: &Fx, target: &str) -> Vec<String> {
    let out = fx.tmux(&["list-panes", "-t", target, "-F", "#{pane_id} #{pane_pid}"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn it_restarts_views_that_run_an_older_binary_once() {
    let fx = terminal();
    // The installed cpd is the one that started this process; the cockpit is built by an older one: its views run `old`.
    let installed = cpd_core::host::exe::started_as().unwrap().to_owned();
    let old = fx.env.root().join("old-cpd");
    fs::write(&old, "an older cpd").unwrap();
    tui(&fx, &[]);
    let before = views_run(&fx, &old);
    assert_eq!(before.len(), 4, "{before:?}");
    // The engine's pane, in its own session.
    fx.tmux(&[
        "new-session",
        "-d",
        "-s",
        "cpd-work",
        "-n",
        "watch",
        "cpd",
        "watch",
    ]);
    let engine = pane_pids_of_window(&fx, "=cpd-work:watch");

    assert_eq!(in_a_terminal(&fx), "");
    let after = views_run(&fx, &installed);
    assert_eq!(
        after.iter().map(|(v, _)| v).collect::<Vec<_>>(),
        before.iter().map(|(v, _)| v).collect::<Vec<_>>()
    );
    assert!(
        after.iter().zip(&before).all(|(a, b)| a.1 != b.1),
        "{before:?} -> {after:?}"
    );
    assert_eq!(
        pane_pids_of_window(&fx, "=cpd-work:watch"),
        engine,
        "the engine is not touched"
    );
    assert_eq!(attached(&fx).len(), 1);

    // Nothing is old now (the restarted views run the installed binary): the next one restarts nothing.
    assert_eq!(in_a_terminal(&fx), "");
    assert_eq!(view_pids_in(&fx), after);
    assert_eq!(attached(&fx).len(), 2);
}

// ---------- the tmux restart (contract §6.1, hs-d294) ----------

/// The server of the tests below is the *old* one. `older_server` gives its binary (a file of the env's, which the
/// `FakeSpawn` says the server's pid runs, as `/proc/<pid>/exe` is for the real one); until [`OlderServer::arm`] it is as
/// new as the installed tmux, so the cockpit is built as any test builds it. Once armed the route says that server runs
/// tmux 1.0, until a `kill-server` (the restart's) ends it: the server built after runs the installed one's version.
struct OlderServer {
    armed: Arc<AtomicBool>,
    program: PathBuf,
}

fn older_server(fx: &Fx) -> OlderServer {
    let dir = fx.env.root().join("old-tmux");
    fs::create_dir_all(&dir).unwrap();
    let program = dir.join("tmux");
    fs::write(&program, "an older tmux").unwrap();
    // The fake server's pid is this process's.
    fx.fakes.spawn.set_exe_of(Pid(std::process::id()), &program);
    let armed = Arc::new(AtomicBool::new(false));
    let (ended, says) = (armed.clone(), armed.clone());
    let runner = (*fx.fakes.runner).clone();
    // A matcher that never matches: it only sees the `kill-server` go by.
    let runner = runner.on(
        "tmux",
        move |call| {
            if call.args.iter().any(|a| a == "kill-server") {
                ended.store(false, Ordering::SeqCst);
            }
            false
        },
        Reply::ok(""),
    );
    let _ = runner.on(
        "tmux",
        move |call| {
            says.load(Ordering::SeqCst)
                && call
                    .args
                    .ends_with(&["display".into(), "-p".into(), "#{version}".into()])
        },
        Reply::ok("1.0\n"),
    );
    OlderServer { armed, program }
}

impl OlderServer {
    /// The server that runs now is the one that is older than the installed tmux.
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }
}

/// `(session, state, pane)` of every Lead and Manager, sorted by id.
fn states(fx: &Fx) -> Vec<(String, String, Option<String>)> {
    store_of(&fx.env)
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, state, tmux_pane FROM sessions WHERE role != 'worker' ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn home_of(env: &common::TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn store_of(env: &common::TestEnv) -> Store {
    Store::open(&home_of(env), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .unwrap()
}

/// Context `ctx` with its Manager `manager1`, and Plans `pay` (Lead `lead1`) and
/// `other` (Lead `lead2`), all idle. Every Session's cwd is the env's root. With `attempt`, Job `api`
/// of `pay` has a running Attempt in that status, whose driver lock is held by the returned guard.
fn fixture(env: &common::TestEnv, attempt: Option<&str>) -> Option<Flock<fs::File>> {
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
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .unwrap();
        Flock::lock(file, FlockArg::LockExclusiveNonblock)
            .map_err(|(_, e)| e)
            .unwrap()
    })
}

/// Builds the cockpit and puts `lead1` in the chat slot, `lead2` in a parked window and the Manager
/// in its Context window, each in a pane running `cpd session run <id>` (recorded by the fake server).
fn cockpit_with_sessions(fx: &Fx) {
    tui(fx, &[]);
    for id in ["manager1", "lead2", "lead1"] {
        fx.cpd(&["session", "open", id]).ok();
    }
}

/// `(session, window name)` of every pane that has a Session, sorted.
fn session_windows(fx: &Fx) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = fx
        .panes()
        .into_iter()
        .filter(|p| !p.session.is_empty())
        .map(|p| (p.session, p.window_name))
        .collect();
    found.sort();
    found
}

/// The `kill-server` calls so far, by the program that made them: the installed `tmux` or another one.
fn kill_server_calls(fx: &Fx) -> Vec<String> {
    fx.calls("tmux")
        .into_iter()
        .filter(|c| c.args.iter().any(|a| a == "kill-server"))
        .map(|c| c.program)
        .collect()
}

fn pending_file(fx: &Fx) -> PathBuf {
    fx.env.cpd_home().join("run/tmux-restart.pending")
}

/// The windows the Sessions are in when `cockpit_with_sessions` has run.
const PLACES: [(&str, &str); 3] = [
    ("lead1", "main"),
    ("lead2", "park-other"),
    ("manager1", "ctx-ctx"),
];

fn places() -> Vec<(String, String)> {
    PLACES
        .iter()
        .map(|(s, w)| ((*s).to_owned(), (*w).to_owned()))
        .collect()
}

#[test]
fn when_it_is_not_safe_nothing_is_killed_the_restart_is_marked_pending_and_the_servers_binary_attaches()
 {
    for (what, lead_state, attempt) in [
        ("a starting Attempt", "idle", Some("starting")),
        ("a running Attempt", "idle", Some("running")),
        ("a merging Attempt", "idle", Some("merging")),
        ("a working Lead", "working", None),
        ("a blocked Lead", "blocked", None),
    ] {
        let fx = terminal();
        let older = older_server(&fx);
        let _driver = fixture(&fx.env, attempt);
        cockpit_with_sessions(&fx);
        // The engine stops a Session that has no pane: the state is set once they have panes.
        store_of(&fx.env).write("test", |tx| exec(tx, &format!("UPDATE sessions SET state = '{lead_state}', state_label = iif('{lead_state}' = 'blocked', 'a dialog', NULL) WHERE id = 'lead1'"))).unwrap();
        let panes = fx.panes();
        older.arm();

        let stderr = in_a_terminal(&fx);
        assert_eq!(kill_server_calls(&fx), Vec::<String>::new(), "{what}");
        assert_eq!(fx.panes(), panes, "{what}");
        // The attach is by the server's own tmux: the installed one must not attach.
        assert_eq!(
            attached(&fx),
            [attach(&fx, &older.program.display().to_string())],
            "{what}"
        );
        assert_eq!(stderr.lines().count(), 1, "{what}: {stderr}");
        assert!(
            stderr.contains("attaching with the server's own tmux")
                && stderr.contains("the restart happens once nothing is in the way"),
            "{what}: {stderr}"
        );
        let pending: Value =
            serde_json::from_str(&fs::read_to_string(pending_file(&fx)).unwrap()).unwrap();
        assert_eq!(
            (pending["server"].as_str(), pending["failed"].is_null()),
            (Some("1.0"), true),
            "{what}"
        );
    }
}

#[test]
fn a_server_that_is_not_older_needs_no_restart_and_a_stale_pending_file_is_cleared() {
    let fx = terminal();
    tui(&fx, &[]);
    fs::create_dir_all(pending_file(&fx).parent().unwrap()).unwrap();
    fs::write(pending_file(&fx), r#"{"at":"t","server":"1.0","installed":"9.9","failed":{"at":"t","code":"tmux_failed","reason":"it broke.","reported":true}}"#).unwrap();
    let stderr = in_a_terminal(&fx);
    assert_eq!(kill_server_calls(&fx), Vec::<String>::new());
    assert!(
        stderr.contains("The tmux restart failed") && stderr.contains("it broke."),
        "{stderr}"
    );
    assert!(!pending_file(&fx).exists());
}

/// A restart reopens the Sessions of the parked windows (`reopen_one`): the pane gets `@cpd_session` (the go signal: the
/// wrapper's `session run` waits for it, then its pi reports `idle`) only once the store records the pane. A report that
/// lands before leaves a row with no pane that engine E2 stops (hs-d238). The route's matcher writes down what the store
/// says of each Session as the signal is set on its pane (`GoSignals`).
#[test]
fn a_restart_records_the_pane_of_a_parked_session_before_it_gets_the_go_signal() {
    let fx = terminal();
    let older = older_server(&fx);
    let _driver = fixture(&fx.env, None);
    cockpit_with_sessions(&fx);
    older.arm();
    let signals = GoSignals::record(&fx);

    let stderr = in_a_terminal(&fx);
    assert!(stderr.contains("3 Session(s) reopened"), "{stderr}");
    // lead1 is the chat slot's, reopened by `session open` (its own test); the others are parked.
    for id in ["lead2", "manager1"] {
        signals.assert_the_store_had_the_pane(id);
    }
}

#[test]
fn a_real_older_tmux_server_is_restarted_onto_the_installed_tmux() {
    // hs-n285: the first real restart (tmux 3.4 -> 3.8) died on the old server's last words. The old server's last words
    // are tmux's; what is ours is that everything said to the old server is said by its own binary, and that the cockpit and
    // every Session are back, in their places, on the server built by the installed one.
    let fx = terminal();
    let older = older_server(&fx);
    let _driver = fixture(&fx.env, None);
    cockpit_with_sessions(&fx);
    assert_eq!(session_windows(&fx), places());
    let old_version = fx.tmux(&["display", "-p", "#{version}"]);
    assert_eq!(
        String::from_utf8(old_version.stdout).unwrap().trim(),
        "3.6a"
    );
    older.arm();
    assert_eq!(
        String::from_utf8(fx.tmux(&["display", "-p", "#{version}"]).stdout)
            .unwrap()
            .trim(),
        "1.0"
    );

    let stderr = in_a_terminal(&fx);
    // The server was killed once, by its own binary (a call of the installed tmux to the old server is one the restart must not make).
    assert_eq!(
        kill_server_calls(&fx),
        [older.program.display().to_string()]
    );
    assert!(
        stderr.contains("restarted the tmux server onto tmux 3.6a; 3 Session(s) reopened."),
        "{stderr}"
    );
    // The new server runs the installed tmux's version, and the installed tmux attaches to it.
    assert_eq!(
        String::from_utf8(fx.tmux(&["display", "-p", "#{version}"]).stdout)
            .unwrap()
            .trim(),
        "3.6a"
    );
    assert_eq!(
        attached(&fx)
            .iter()
            .map(|l| l.program.as_str())
            .collect::<Vec<_>>(),
        ["tmux"]
    );
    assert_eq!(session_windows(&fx), places());
    // Every Lead and Manager has a pane that is in the server (the Sessions' idle reports are their pi adapter's).
    let panes: Vec<String> = fx.panes().into_iter().map(|p| p.pane).collect();
    for (id, _, pane) in states(&fx) {
        assert!(
            pane.as_deref()
                .is_some_and(|p| panes.iter().any(|live| live == p)),
            "{id}: {pane:?} in {panes:?}"
        );
    }
    assert!(!pending_file(&fx).exists());
}
