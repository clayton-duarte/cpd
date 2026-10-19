//! `session run` reloads itself after an install (hs-d33, hs-d38, hs-d49,
//! hs-d151; contract §6.7 "Reload on install"; incident hs-n141), run as a copy
//! of the built binary in a pane of a private tmux server, with a fake `pi`. An
//! install is a new file renamed over the copy; the new file records that it
//! ran (its pid and arguments) and then becomes the built `cpd`. A new cpd
//! re-execs the wrapper at once and the new process adopts pi; only a new pi
//! restarts pi, at the Lead's idle.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{CPD, SLEEP_SECONDS, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

const LEAD: &str = "lead1";
/// A `user_version` the built cpd's verdict refuses (`store_too_new`, contract §4.5):
/// neither legacy nor its own.
const NEWER_SCHEMA: i64 = cpd_core::store::VERSION + 1;
/// The `--version` of a cpd whose verdict opens the store (commands.md §1.2).
const OPENS: &str = r#"echo '{"ok":true,"version":"x","store":{"opens":true,"refusal":null,"unknown":[],"pending":[]}}'; exit 0"#;

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000).unwrap().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// The finished turn the Lead's session file ends with (§6.7 step 1).
const FINISHED: &str = "{\"type\":\"session\"}\n{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";

struct Fixture {
    env: TestEnv,
    /// The copy of `cpd` the pane runs.
    installed: PathBuf,
    session_file: PathBuf,
    pane: String,
}

impl Fixture {
    fn path(&self, name: &str) -> PathBuf {
        self.env.home().join(name)
    }

    fn signals(&self) -> String {
        fs::read_to_string(self.path("pi.signals")).unwrap_or_default()
    }

    /// The pid of each pi started so far.
    fn pi_pids(&self) -> Vec<i32> {
        fs::read_to_string(self.path("pi.pids")).unwrap_or_default().lines().map(|l| l.parse().unwrap()).collect()
    }

    /// The pane's process: what the wrapper is, before and after the reload.
    fn pane_pid(&self) -> String {
        let out = self.env.tmux(&["display-message", "-p", "-t", &self.pane, "#{pane_pid}"]);
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// What the installed file recorded: `<pid> <arguments>` per run.
    fn runs(&self) -> Vec<String> {
        fs::read_to_string(self.path("installed.runs")).map(|t| t.lines().map(str::to_owned).collect()).unwrap_or_default()
    }

    /// A new pi: `install/current-version` rewritten by rename, as pi's own update does.
    fn activate_pi(&self, version: &str) {
        let current = self.path(".pi/agent/install/current-version");
        let new = current.with_extension("tmp");
        fs::write(&new, format!("{version}\n")).unwrap();
        fs::rename(&new, &current).unwrap();
    }

    /// A new install: a new file renamed over the copy. It answers `--version` as
    /// the built `cpd` does (the wrapper asks before it reloads), and records every other call.
    fn install(&self) {
        self.install_script(&format!("exec '{CPD}' --version"));
    }

    /// A `cpd` that cannot open the store installed over the wrapper: its `--version` reports a
    /// refusing verdict (`store.opens` false).
    fn install_older(&self) {
        self.install_script(r#"echo '{"ok":true,"version":"x","store":{"opens":false,"refusal":{"code":"store_too_new","message":"The database was migrated by a newer cpd."},"unknown":[],"pending":[]}}'; exit 0"#);
    }

    fn install_script(&self, on_version: &str) {
        let new = self.installed.with_extension("new");
        fs::write(
            &new,
            format!(
                "#!/bin/sh\ncase \"$1\" in --version) {on_version};; esac\necho \"$$ $*\" >> '{}'\nexec '{CPD}' \"$@\"\n",
                self.path("installed.runs").display()
            ),
        )
        .unwrap();
        fs::set_permissions(&new, fs::Permissions::from_mode(0o755)).unwrap();
        fs::rename(&new, &self.installed).unwrap();
    }

    /// The store as a newer cpd leaves it: a column the old code reads renamed, and a
    /// `user_version` the built cpd's verdict refuses (the migration of hs-n148).
    fn migrate_store(&self) {
        self.store_sql(&format!("ALTER TABLE sessions RENAME COLUMN state TO state_v2; PRAGMA user_version = {NEWER_SCHEMA};"));
    }

    /// Undoes [`Fixture::migrate_store`]: what the built cpd can open again.
    fn unmigrate_store(&self) {
        self.store_sql(&format!(
            "ALTER TABLE sessions RENAME COLUMN state_v2 TO state; PRAGMA user_version = {};",
            cpd_core::store::VERSION
        ));
    }

    /// The adapter's report that the Lead is idle (long ago, so the wrapper's idle rule holds).
    fn go_idle(&self) {
        let home = CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap();
        Store::open(&home, 5000)
            .unwrap()
            .write("fixture", |tx| {
                tx.execute("UPDATE sessions SET state = 'idle', state_at = '2026-01-01T00:00:00Z' WHERE id = ?1", [LEAD])?;
                Ok(())
            })
            .unwrap();
    }

    fn store_sql(&self, sql: &str) {
        let conn = rusqlite::Connection::open(self.env.cpd_home().join("cpd.db")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        conn.execute_batch(sql).unwrap();
    }

    /// What the pane shows: the wrapper's stderr is its terminal.
    fn screen(&self) -> String {
        String::from_utf8(self.env.tmux(&["capture-pane", "-p", "-J", "-t", &self.pane]).stdout).unwrap()
    }
}

/// A grounded Lead (`lead1`) in `state`, with a finished session file, running
/// `session run` from a copy of `cpd` in its own pane, with its fake pi started once.
/// The fake pi records `SIGTERM` in `pi.signals` and exits at it, or when `pi.exit` appears.
fn start(state: &str) -> Fixture {
    start_with(state, "exit 0")
}

/// As [`start`]; `on_term` is what the fake pi runs after it records `SIGTERM`
/// (`exit 0`, or `:` to ignore it). Every pi records its pid in `pi.pids`.
fn start_with(state: &str, on_term: &str) -> Fixture {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[timeouts]\nfork_quiet_seconds = 1\npi_stop_seconds = 3\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    let at = "2026-01-01T00:00:00Z";
    let cwd = env.root().display().to_string();
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000)
        .unwrap()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                   VALUES ('{LEAD}', 'lead', 'ctx', NULL, NULL, 'Grounded', '{cwd}', 'O3', '{state}', '{at}', '{at}');"
            ))?;
            Ok(())
        })
        .unwrap();
    let sessions = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let session_file = sessions.join(format!("2026_{LEAD}.jsonl"));
    fs::write(&session_file, FINISHED).unwrap();
    let current_version = env.home().join(".pi/agent/install/current-version");
    fs::create_dir_all(current_version.parent().unwrap()).unwrap();
    fs::write(&current_version, "1.2.0\n").unwrap();

    let home_dir = env.home().display().to_string();
    env.fake_pi(&format!(
        "trap 'echo TERM >> \"{home_dir}/pi.signals\"; {on_term}' TERM\n\
         echo $$ >> \"{home_dir}/pi.pids\"\n\
         while :; do\n  if [ -e \"{home_dir}/pi.exit\" ]; then rm \"{home_dir}/pi.exit\"; exit 0; fi\n  sleep 0.2\ndone"
    ));

    let bin = env.root().join("installed-bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("cpd");
    fs::copy(CPD, &installed).unwrap();

    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    ok(&["new-session", "-d", "-s", "cpd", "-n", "main", &format!("sleep {SLEEP_SECONDS}")]);
    let command = format!("exec '{}' session run {LEAD}", installed.display());
    let pane = ok(&["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", &command]);
    ok(&["set", "-p", "-t", &pane, "@cpd_session", LEAD]);
    let fixture = Fixture { env, installed, session_file, pane };
    // The fake records its call before it runs its body, so the call alone does not
    // say that the pid is written. The pid is written last, after the trap is set:
    // once it is there, the pi can be signalled and its pid read.
    wait_until("the first pi", || fixture.env.calls("pi").len() == 1 && fixture.pi_pids().len() == 1);
    fixture
}

#[test]
fn an_install_re_execs_an_idle_wrapper_at_once_keeping_pi_and_a_new_pi_then_restarts_it_at_idle() {
    let f = start("idle");
    let pid = f.pane_pid();
    let old_pi = f.pi_pids()[0];
    let session_file = f.session_file.display().to_string();
    assert!(f.env.calls("pi")[0].windows(2).any(|w| w == ["--session", &session_file]), "the first start resumes");
    assert!(f.signals().is_empty());

    f.install();

    wait_until("the new binary", || !f.runs().is_empty());
    assert_eq!(f.runs(), [format!("{pid} session run {LEAD} --adopt {old_pi}")], "the same process re-execed the new file");
    // Several polls: pi runs on, untouched, under the new wrapper.
    std::thread::sleep(Duration::from_millis(2500));
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(alive(old_pi));
    assert_eq!(f.env.calls("pi").len(), 1, "pi was not restarted");
    assert_eq!(f.pane_pid(), pid);

    // The new wrapper still reacts: a new pi restarts the adopted one (it can signal and wait for it).
    f.activate_pi("1.3.0");
    wait_until("pi restarted", || f.env.calls("pi").len() == 2);
    assert_eq!(f.signals(), "TERM\n", "the adopted pi got SIGTERM once");
    let calls = f.env.calls("pi");
    assert!(calls[1].windows(2).any(|w| w == ["--session", &session_file]), "{:?}", calls[1]);
    assert!(!alive(old_pi), "the adopted pi is dead");
    assert_eq!(f.pane_pid(), pid);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

#[test]
fn a_parked_idle_lead_restarts_its_pi_on_its_session_file_after_pi_changes() {
    let f = start("idle");
    let session_file = f.session_file.display().to_string();
    assert!(f.signals().is_empty());
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(f.env.calls("pi").len(), 1, "nothing changed, pi runs on");

    f.activate_pi("1.3.0");

    wait_until("pi restarted", || f.env.calls("pi").len() == 2);
    assert_eq!(f.signals(), "TERM\n", "the old pi got SIGTERM once");
    let calls = f.env.calls("pi");
    assert!(calls[1].windows(2).any(|w| w == ["--session", &session_file]), "{:?}", calls[1]);
    assert!(f.runs().is_empty(), "cpd was not reinstalled");
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

/// The incident of hs-n141 in its simplest form: a Lead that is working while cpd is installed.
#[test]
fn a_working_lead_survives_an_install_the_wrapper_re_execs_and_pi_is_never_signalled() {
    let f = start("working");
    let pid = f.pane_pid();
    let old_pi = f.pi_pids()[0];

    f.install();

    wait_until("the new binary", || !f.runs().is_empty());
    assert_eq!(f.runs(), [format!("{pid} session run {LEAD} --adopt {old_pi}")]);
    assert!(f.screen().contains("new cpd installed"), "{}", f.screen());
    // Several polls: the Lead is working, pi runs on and nothing else happens.
    std::thread::sleep(Duration::from_millis(2500));
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(alive(old_pi));
    assert_eq!(f.pi_pids(), [old_pi], "no other pi");
    assert_eq!(f.env.calls("pi").len(), 1);
    assert_eq!(f.pane_pid(), pid);
    assert_eq!(query::<String>(&f.env, "SELECT state FROM sessions WHERE id = 'lead1'"), "working");

    // The new wrapper owns pi as its child: it sees it die and reports it.
    // (An exit 0 would archive the Lead and end the wrapper.)
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(old_pi), nix::sys::signal::Signal::SIGKILL).unwrap();
    wait_until("the new wrapper to see pi exit", || f.screen().contains("pi exited. Press Enter"));
    assert_eq!(query::<String>(&f.env, "SELECT state FROM sessions WHERE id = 'lead1'"), "stopped");
    assert_eq!(f.runs().len(), 1);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

/// A pi that changed before the install is still restarted at the Lead's idle, by the new wrapper.
#[test]
fn a_pi_that_changed_before_the_install_is_restarted_at_idle_by_the_new_wrapper() {
    let f = start("working");
    let pid = f.pane_pid();
    let old_pi = f.pi_pids()[0];

    f.activate_pi("1.3.0");
    f.install();

    wait_until("the new binary", || !f.runs().is_empty());
    assert_eq!(f.runs(), [format!("{pid} session run {LEAD} --adopt {old_pi} --pi-stale")]);
    std::thread::sleep(Duration::from_millis(2500));
    assert!(f.signals().is_empty(), "the Lead is working: pi runs on");
    assert_eq!(f.env.calls("pi").len(), 1);

    f.go_idle();
    wait_until("pi restarted", || f.env.calls("pi").len() == 2);
    assert_eq!(f.signals(), "TERM\n");
    assert!(!alive(old_pi));
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

fn alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

#[test]
fn a_pi_that_ignores_sigterm_is_killed_after_pi_stop_seconds_and_the_wrapper_starts_pi_again_for_a_new_pi() {
    let f = start_with("idle", ":");
    let pid = f.pane_pid();
    let old_pi = f.pi_pids()[0];

    let changed_at = Instant::now();
    f.activate_pi("1.3.0");

    wait_until("SIGTERM to reach pi", || !f.signals().is_empty());
    std::thread::sleep(Duration::from_secs(1));
    assert!(alive(old_pi), "pi is not killed before pi_stop_seconds (3) are up");
    assert_eq!(f.env.calls("pi").len(), 1);

    wait_until("pi restarted", || f.env.calls("pi").len() == 2);
    assert!(changed_at.elapsed() >= Duration::from_secs(3), "killed after {:?}", changed_at.elapsed());
    assert!(!alive(old_pi), "the old pi is dead");
    assert_eq!(f.signals(), "TERM\n", "SIGTERM once, then SIGKILL");
    assert_eq!(f.pane_pid(), pid);
    wait_until("the new pi to record its pid", || f.pi_pids().len() == 2);
    let new_pi = f.pi_pids()[1];
    assert!(alive(new_pi) && new_pi != old_pi);
    // pi ignores SIGTERM, so the pane's hangup is what ends the new one.
    f.env.tmux(&["kill-window", "-t", &f.pane]);
    wait_until("the new pi to end", || !alive(new_pi));
}

/// The incident itself (hs-n141, hs-n148): the store is migrated by a newer cpd while the old
/// wrapper runs. The old wrapper's reads fail; it does not exit and does not touch pi. When a
/// new cpd that can open the store is installed, it re-execs and adopts pi.
#[test]
fn a_newer_store_does_not_end_a_wrapper_and_the_new_cpd_that_opens_it_adopts_pi() {
    let f = start("working");
    let pid = f.pane_pid();
    let old_pi = f.pi_pids()[0];

    f.migrate_store();
    wait_until("the wrapper to say its read failed", || f.screen().contains("Retrying at the next poll"));
    // Several polls: said once, nothing ended, nothing signalled.
    std::thread::sleep(Duration::from_millis(4500));
    let screen = f.screen();
    assert_eq!(screen.matches("Retrying at the next poll").count(), 1, "{screen}");
    assert!(alive(old_pi), "pi was ended: {screen}");
    assert!(f.signals().is_empty());
    assert_eq!(f.pane_pid(), pid, "the wrapper ended: {screen}");
    assert!(f.runs().is_empty());

    // The new cpd has migrated the store itself: here the test puts it back for the built cpd to open.
    f.unmigrate_store();
    f.install_script(OPENS);
    wait_until("the new binary", || !f.runs().is_empty());
    assert_eq!(f.runs(), [format!("{pid} session run {LEAD} --adopt {old_pi}")]);
    std::thread::sleep(Duration::from_millis(2500));
    assert!(alive(old_pi) && f.signals().is_empty(), "pi was touched");
    assert_eq!(f.env.calls("pi").len(), 1);
    assert_eq!(f.pane_pid(), pid);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

/// A store read that fails while nothing was installed: said once, retried at each poll, pi untouched.
#[test]
fn a_failing_store_read_is_said_once_and_retried_with_pi_untouched() {
    let f = start("working");
    let pid = f.pane_pid();
    let old_pi = f.pi_pids()[0];

    f.migrate_store();
    wait_until("the wrapper to say its read failed", || f.screen().contains("Retrying at the next poll"));
    std::thread::sleep(Duration::from_millis(4500));
    let screen = f.screen();
    assert_eq!(screen.matches("Retrying at the next poll").count(), 1, "{screen}");
    assert!(alive(old_pi) && f.signals().is_empty(), "pi was touched: {screen}");
    assert_eq!(f.pane_pid(), pid, "the wrapper ended: {screen}");
    assert!(f.runs().is_empty());

    // The store reads again: the wrapper is the same one and carries on (pi's death is seen and reported).
    f.unmigrate_store();
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(old_pi), nix::sys::signal::Signal::SIGKILL).unwrap();
    wait_until("the wrapper to see pi exit", || f.screen().contains("pi exited. Press Enter"));
    assert_eq!(f.pane_pid(), pid);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

/// The 00:02Z incident: a cpd that cannot open the database (its verdict, contract §4.5) is
/// installed over a running wrapper. The wrapper does not signal
/// pi, does not `exec` it and says so once; a cpd that can open the store,
/// installed next, is exec'd, and adopts pi.
#[test]
fn a_lead_wrapper_does_not_reload_into_a_cpd_older_than_the_store_and_reloads_into_one_that_can_open_it() {
    let f = start("working");
    let pid = f.pane_pid();
    let old_pi = f.pi_pids()[0];

    f.install_older();
    wait_until("the wrapper to refuse the reload", || f.screen().contains("reload skipped"));
    // Several polls: pi is not signalled, nothing is exec'd, nothing is said twice.
    std::thread::sleep(Duration::from_millis(2500));
    let screen = f.screen();
    assert_eq!(screen.matches("reload skipped").count(), 1, "{screen}");
    assert!(screen.contains(&format!("reload skipped: {} cannot open the store: The database was migrated by a newer cpd.", f.installed.display())), "{screen}");
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(alive(old_pi));
    assert!(f.runs().is_empty(), "the older cpd was exec'd: {:?}", f.runs());
    assert_eq!(f.env.calls("pi").len(), 1);
    assert_eq!(f.pane_pid(), pid);

    f.install();
    wait_until("the new binary", || !f.runs().is_empty());
    assert_eq!(f.runs(), [format!("{pid} session run {LEAD} --adopt {old_pi}")], "the same process re-execed the new file");
    std::thread::sleep(Duration::from_millis(2500));
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(alive(old_pi));
    assert_eq!(f.env.calls("pi").len(), 1);
    assert_eq!(f.pane_pid(), pid);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

#[test]
fn a_lead_wrapper_whose_pi_exits_does_not_exec_a_cpd_older_than_the_store() {
    let f = start("working");
    f.install_older();
    // The Lead is not idle: the wrapper only meets the install when pi dies on its own.
    // (An exit 0 would archive the Lead and end the wrapper.)
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(f.pi_pids()[0]), nix::sys::signal::Signal::SIGKILL).unwrap();
    wait_until("the wrapper to refuse the reload", || f.screen().contains("reload skipped"));
    wait_until("the wrapper to carry on", || f.screen().contains("pi exited. Press Enter"));
    assert!(f.runs().is_empty(), "the older cpd was exec'd: {:?}", f.runs());
    assert_eq!(f.env.calls("pi").len(), 1);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

/// The verdict is the real one, not a fake `--version`: the store has applied a breaking migration
/// the installed cpd does not know (its floor), so `cpd --version` reports `store.opens` false and
/// the wrapper keeps running its old code.
#[test]
fn a_lead_wrapper_follows_the_real_verdict_of_the_new_binary() {
    use rusqlite::{Connection, functions::FunctionFlags};
    let f = start("idle");
    let pid = f.pane_pid();
    {
        let conn = Connection::open(f.env.cpd_home().join("cpd.db")).unwrap();
        let txn: i64 = conn.query_row("SELECT max(id) FROM txns", [], |r| r.get(0)).unwrap();
        conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn)).unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
             VALUES ('29991231T2359-breaking', 'breaking', lower(hex(zeroblob(32))), (SELECT max(seq) + 1 FROM schema_migrations), ?1, 0)",
            [txn],
        )
        .unwrap();
    }

    f.install();
    wait_until("the wrapper to refuse the reload", || f.screen().contains("reload skipped"));
    let screen = f.screen();
    assert!(screen.contains(&format!("reload skipped: {} cannot open the store: ", f.installed.display())), "{screen}");
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(f.runs().is_empty(), "the new cpd was exec'd: {:?}", f.runs());
    assert_eq!(f.pane_pid(), pid);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}
