//! `session run` reloads itself after an install (hs-d33, hs-d38, hs-d49,
//! hs-d151; contract §6.7 "Reload on install"; incident hs-n141), in-process on
//! virtual time (`common::wrapper`): the wrapper runs on a thread of the `Pane`,
//! stepped one poll at a time, its pi a pid of the `FakeSpawn` and its `exec` a
//! recorded `Spawn::exec` that ends that wrapper (`Wrapper::exec`). An install is
//! a new file renamed over the installed path; `<installed> --version` is a route
//! of the `FakeRunner` that answers what the built cpd answers on the env's store,
//! or what the test says. The `exec` is followed by the new process: `Pane::resume`
//! runs the arguments of the recorded `Launch` again, which adopts pi (`--adopt`);
//! only a new pi restarts pi, at the Lead's idle.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use common::fx::Fx;
use common::wrapper::{Ended, Pane, Wrapper};
use cpd_core::clock::Clock;
use cpd_core::host::spawn::{Launch, Pid, Signal};

const LEAD: &str = "lead1";
/// A `user_version` the built cpd's verdict refuses (`store_too_new`, contract §4.5):
/// neither legacy nor its own.
const NEWER_SCHEMA: i64 = cpd_core::store::VERSION + 1;
/// The `--version` of a cpd whose verdict opens the store (commands.md §1.2).
const OPENS: &str =
    r#"{"ok":true,"version":"x","store":{"opens":true,"refusal":null,"unknown":[],"pending":[]}}"#;
/// The `--version` of a cpd that cannot open the store: a refusing verdict (`store.opens` false).
const OLDER: &str = r#"{"ok":true,"version":"x","store":{"opens":false,"refusal":{"code":"store_too_new","message":"The database was migrated by a newer cpd."},"unknown":[],"pending":[]}}"#;

/// The finished turn the Lead's session file ends with (§6.7 step 1).
const FINISHED: &str = "{\"type\":\"session\"}\n{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";

/// The wrapper, the pane, the env and the files; they drop in this order.
struct Fixture {
    /// The process of the pane now: the one started, or the one the last `exec` made.
    w: Wrapper,
    pane: Pane,
    fx: Fx,
    /// The path the wrapper runs from.
    installed: PathBuf,
    session_file: PathBuf,
}

impl Fixture {
    /// The pid of the first pi.
    fn pi(&self) -> Pid {
        self.pane.spawn.pids()[0]
    }

    fn signals(&self) -> Vec<(Pid, Signal)> {
        self.pane.spawn.signals()
    }

    fn pis(&self) -> usize {
        self.pane.spawn.pis().len()
    }

    /// What the wrapper in the pane asked the `Spawn` to `exec`, once it has ended in it: it is the program
    /// the installed path holds, with `args` (`[session, run, id, ...]`).
    fn execed(&mut self) -> Launch {
        self.w.finish();
        let launch = self.w.exec().clone();
        assert_eq!(
            launch.program,
            self.installed.display().to_string(),
            "the installed cpd is what is exec'd"
        );
        launch
    }

    /// The pane's process is replaced: the new one runs the arguments of `launch`.
    fn resume(&mut self, launch: &Launch) {
        self.w = self.pane.resume(launch);
    }

    /// The arguments of an `exec` that adopts `pi`: `session run <id> --adopt <pid>`, `--pi-stale` when its pi changed.
    fn adopting(pi: Pid, stale: bool) -> Vec<String> {
        let mut args = vec![
            "session".to_owned(),
            "run".to_owned(),
            LEAD.to_owned(),
            "--adopt".to_owned(),
            pi.0.to_string(),
        ];
        if stale {
            args.push("--pi-stale".to_owned());
        }
        args
    }

    /// A new pi: `install/current-version` rewritten by rename, as pi's own update does.
    fn activate_pi(&self, version: &str) {
        let current = self.fx.env.home().join(".pi/agent/install/current-version");
        let new = current.with_extension("tmp");
        fs::write(&new, format!("{version}\n")).unwrap();
        fs::rename(&new, &current).unwrap();
    }

    /// A new install: a new file renamed over the installed path. It answers `--version` as the built `cpd` does
    /// (the wrapper asks before it reloads).
    fn install(&self) {
        self.pane.version_is_real();
        self.replace_installed();
    }

    /// An install whose `--version` says `json` (a verdict, whatever the store is).
    fn install_says(&self, json: &str) {
        self.pane.version_says(json);
        self.replace_installed();
    }

    fn replace_installed(&self) {
        let new = self.installed.with_extension("new");
        fs::write(&new, "").unwrap();
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
        self.fx.env.sql(&format!("UPDATE sessions SET state = 'idle', state_at = '2026-01-01T00:00:00Z' WHERE id = '{LEAD}'"));
    }

    fn store_sql(&self, sql: &str) {
        let conn = rusqlite::Connection::open(self.fx.env.cpd_home().join("cpd.db")).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        conn.execute_batch(sql).unwrap();
    }

    /// What the pane shows: the wrapper's stdout and stderr is its terminal.
    fn screen(&self) -> String {
        self.pane.screen()
    }

    /// Steps the wrapper until the pane shows `text`.
    fn until_screen(&mut self, what: &str, text: &str) {
        let pane = &self.pane;
        self.w.until(what, || pane.screen().contains(text));
    }

    /// Steps the wrapper until `n` pis have started.
    fn until_pis(&mut self, what: &str, n: usize) {
        let pane = &self.pane;
        self.w.until(what, || pane.spawn.pis().len() >= n);
    }

    fn state(&self) -> String {
        self.fx
            .env
            .query(&format!("SELECT state FROM sessions WHERE id = '{LEAD}'"))
    }
}

/// A grounded Lead (`lead1`) in `state`, with a finished session file, whose `session run` has started its first pi.
/// Its pi exits at `SIGTERM`.
fn start(state: &str) -> Fixture {
    let mut fixture = prepare(state, 5000);
    fixture.until_pis("the first pi", 1);
    fixture
}

/// As [`start`], up to the pane: the wrapper is made but not started, so a test can set the store up first.
/// `store_busy_ms` is the config's.
fn prepare(state: &str, store_busy_ms: u32) -> Fixture {
    let fx = Fx::with_ports_over(common::fx::bare_with_store());
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[timeouts]\nfork_quiet_seconds = 1\nstore_busy_ms = {store_busy_ms}\npi_stop_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
            fx.env.tmux_socket()
        ),
    )
    .unwrap();
    let at = "2026-01-01T00:00:00Z";
    let cwd = fx.env.root().display().to_string();
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('{LEAD}', 'lead', 'ctx', NULL, NULL, 'Grounded', '{cwd}', 'O3', '{state}', '{at}', '{at}');"
    ));
    let sessions = fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let session_file = sessions.join(format!("2026_{LEAD}.jsonl"));
    fs::write(&session_file, FINISHED).unwrap();
    // Quiet for `fork_quiet_seconds` already, on the wrapper's clock: a restart at idle need not wait for it.
    let hour_ago = SystemTime::from(fx.fakes.clock.utc()) - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&session_file)
        .unwrap()
        .set_modified(hour_ago)
        .unwrap();
    let current_version = fx.env.home().join(".pi/agent/install/current-version");
    fs::create_dir_all(current_version.parent().unwrap()).unwrap();
    fs::write(&current_version, "1.2.0\n").unwrap();

    let installed = fx.env.root().join("installed-bin/cpd");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    fs::write(&installed, "").unwrap();

    let pane = Pane::new(&fx, LEAD, &installed);
    let w = pane.run(&["session", "run", LEAD]);
    Fixture {
        w,
        pane,
        fx,
        installed,
        session_file,
    }
}

#[test]
fn an_install_re_execs_an_idle_wrapper_at_once_keeping_pi_and_a_new_pi_then_restarts_it_at_idle() {
    let mut f = prepare("idle", 5000);
    let session_file = f.session_file.display().to_string();
    // The session file is recorded already, so the first wrapper writes nothing (see below).
    f.fx.env.sql(&format!(
        "UPDATE sessions SET file = '{session_file}' WHERE id = '{LEAD}'"
    ));
    f.until_pis("the first pi", 1);
    let old_pi = f.pi();
    assert!(
        f.pane.spawn.pis()[0]
            .args
            .windows(2)
            .any(|w| w == ["--session", &session_file]),
        "the first start resumes"
    );
    assert!(f.signals().is_empty());

    // Cleared, the file is recorded again by the next wrapper to read the Session: the new one, in its loop, once it
    // has read pi's install. Only then is a new pi a change to it.
    f.fx.env.sql(&format!(
        "UPDATE sessions SET file = NULL WHERE id = '{LEAD}'"
    ));
    f.install();

    let exec = f.execed();
    assert_eq!(
        exec.args,
        Fixture::adopting(old_pi, false),
        "the wrapper re-execed the new file, adopting pi"
    );
    assert!(
        f.signals().is_empty(),
        "pi was signalled by the old wrapper"
    );
    f.resume(&exec);
    let (fx, w) = (&f.fx, &mut f.w);
    w.until("the new wrapper to record the session file", || {
        fx.env
            .query::<Option<String>>(&format!("SELECT file FROM sessions WHERE id = '{LEAD}'"))
            .is_some()
    });
    // Several polls: pi runs on, untouched, under the new wrapper.
    f.w.steps(5);
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(f.pane.spawn.alive(old_pi));
    assert_eq!(f.pis(), 1, "pi was not restarted");
    assert_eq!(f.pane.spawn.execs().len(), 1);

    // The new wrapper still reacts: a new pi restarts the adopted one (it can signal and wait for it).
    f.activate_pi("1.3.0");
    let restart = f.execed();
    assert_eq!(
        restart.args,
        ["session", "run", LEAD],
        "no pi runs for the new process to adopt"
    );
    assert_eq!(
        f.signals(),
        [(old_pi, Signal::Term)],
        "the adopted pi got SIGTERM once"
    );
    assert!(!f.pane.spawn.alive(old_pi), "the adopted pi is dead");
    f.resume(&restart);
    f.until_pis("pi restarted", 2);
    assert!(
        f.pane.spawn.pis()[1]
            .args
            .windows(2)
            .any(|w| w == ["--session", &session_file]),
        "{:?}",
        f.pane.spawn.pis()[1]
    );
    assert_eq!(f.signals(), [(old_pi, Signal::Term)]);
}

#[test]
fn a_parked_idle_lead_restarts_its_pi_on_its_session_file_after_pi_changes() {
    let mut f = start("idle");
    let session_file = f.session_file.display().to_string();
    let old_pi = f.pi();
    assert!(f.signals().is_empty());
    // With nothing installed pi is not restarted over three polls of the wrapper.
    f.w.steps(3);
    assert_eq!(f.pis(), 1, "nothing changed, pi runs on");
    assert!(f.signals().is_empty());

    f.activate_pi("1.3.0");

    let restart = f.execed();
    assert_eq!(restart.args, ["session", "run", LEAD]);
    assert_eq!(
        f.signals(),
        [(old_pi, Signal::Term)],
        "the old pi got SIGTERM once"
    );
    f.resume(&restart);
    f.until_pis("pi restarted", 2);
    assert!(
        f.pane.spawn.pis()[1]
            .args
            .windows(2)
            .any(|w| w == ["--session", &session_file]),
        "{:?}",
        f.pane.spawn.pis()[1]
    );
    assert_eq!(f.pane.spawn.execs().len(), 1, "cpd was not reinstalled");
}

/// The incident of hs-n141 in its simplest form: a Lead that is working while cpd is installed.
#[test]
fn a_working_lead_survives_an_install_the_wrapper_re_execs_and_pi_is_never_signalled() {
    let mut f = start("working");
    let old_pi = f.pi();

    f.install();

    let exec = f.execed();
    assert_eq!(exec.args, Fixture::adopting(old_pi, false));
    assert!(!f.screen().contains("new cpd installed"), "{}", f.screen());
    f.resume(&exec);
    // Several polls: the Lead is working, pi runs on and nothing else happens.
    f.w.steps(5);
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(f.pane.spawn.alive(old_pi));
    assert_eq!(f.pane.spawn.pids(), [old_pi], "no other pi");
    assert_eq!(f.pis(), 1);
    assert_eq!(f.state(), "working");

    // The new wrapper owns pi as its child: it sees it die and reports it.
    // (An exit 0 would archive the Lead and end the wrapper.)
    f.pane.spawn.killed(old_pi);
    f.until_screen("the new wrapper to see pi exit", "pi exited. Press Enter");
    assert_eq!(f.state(), "stopped");
    assert_eq!(f.pane.spawn.execs().len(), 1);
}

/// A pi that changed before the install is still restarted at the Lead's idle, by the new wrapper.
#[test]
fn a_pi_that_changed_before_the_install_is_restarted_at_idle_by_the_new_wrapper() {
    let mut f = start("working");
    let old_pi = f.pi();

    f.activate_pi("1.3.0");
    f.install();

    let exec = f.execed();
    assert_eq!(exec.args, Fixture::adopting(old_pi, true));
    f.resume(&exec);
    // The Lead is working, so the new wrapper does not restart the stale pi over five polls.
    f.w.steps(5);
    assert!(f.signals().is_empty(), "the Lead is working: pi runs on");
    assert_eq!(f.pis(), 1);

    f.go_idle();
    let restart = f.execed();
    assert_eq!(restart.args, ["session", "run", LEAD]);
    assert_eq!(f.signals(), [(old_pi, Signal::Term)]);
    assert!(!f.pane.spawn.alive(old_pi));
    f.resume(&restart);
    f.until_pis("pi restarted", 2);
}

#[test]
fn a_pi_that_ignores_sigterm_is_killed_after_pi_stop_seconds_and_the_wrapper_starts_pi_again_for_a_new_pi()
 {
    let mut f = start("idle");
    f.pane.spawn.ignore_term();
    let old_pi = f.pi();

    f.activate_pi("1.3.0");
    let changed_at = f.fx.fakes.clock.elapsed();

    let pane = &f.pane;
    f.w.until("SIGTERM to reach pi", || !pane.spawn.signals().is_empty());
    // pi ignores SIGTERM: it is not killed with it.
    assert!(f.pane.spawn.alive(old_pi), "pi is killed with SIGTERM");
    assert_eq!(f.pis(), 1);
    assert_eq!(f.signals(), [(old_pi, Signal::Term)]);
    let signalled_at = f.fx.fakes.clock.elapsed();

    // `pi_stop_seconds` (1) of the clock after the SIGTERM, the SIGKILL: not a step before.
    f.fx.advance(Duration::from_millis(900));
    f.w.steps(1);
    assert_eq!(
        f.signals(),
        [(old_pi, Signal::Term)],
        "killed {:?} after SIGTERM",
        f.fx.fakes.clock.elapsed() - signalled_at
    );
    f.fx.advance(Duration::from_millis(100));
    let restart = f.execed();
    assert!(f.fx.fakes.clock.elapsed() - changed_at >= Duration::from_secs(1));
    assert_eq!(
        f.signals(),
        [(old_pi, Signal::Term), (old_pi, Signal::Kill)],
        "SIGTERM once, then SIGKILL"
    );
    assert!(!f.pane.spawn.alive(old_pi), "the old pi is dead");
    assert_eq!(restart.args, ["session", "run", LEAD]);
    f.resume(&restart);
    f.until_pis("pi restarted", 2);
    let new_pi = f.pane.spawn.pids()[1];
    assert!(f.pane.spawn.alive(new_pi) && new_pi != old_pi);
}

/// The incident itself (hs-n141, hs-n148): the store is migrated by a newer cpd while the old
/// wrapper runs. The old wrapper's reads fail; it does not exit and does not touch pi. When a
/// new cpd that can open the store is installed, it re-execs and adopts pi.
#[test]
fn a_newer_store_does_not_end_a_wrapper_and_the_new_cpd_that_opens_it_adopts_pi() {
    let mut f = start("working");
    let old_pi = f.pi();

    f.migrate_store();
    f.until_screen(
        "the wrapper to say its read failed",
        "Retrying at the next poll",
    );
    // Several polls: said once, nothing ended, nothing signalled.
    f.w.steps(9);
    let screen = f.screen();
    assert_eq!(
        screen.matches("Retrying at the next poll").count(),
        1,
        "{screen}"
    );
    assert!(f.pane.spawn.alive(old_pi), "pi was ended: {screen}");
    assert!(f.signals().is_empty());
    assert!(f.w.ended().is_none(), "the wrapper ended: {screen}");
    assert!(f.pane.spawn.execs().is_empty());

    // The new cpd has migrated the store itself: here the test puts it back for the built cpd to open.
    f.unmigrate_store();
    f.install_says(OPENS);
    let exec = f.execed();
    assert_eq!(exec.args, Fixture::adopting(old_pi, false));
    f.resume(&exec);
    f.w.steps(5);
    assert!(
        f.pane.spawn.alive(old_pi) && f.signals().is_empty(),
        "pi was touched"
    );
    assert_eq!(f.pis(), 1);
    assert_eq!(f.pane.spawn.execs().len(), 1);
}

/// A store read that fails while nothing was installed: said once, retried at each poll, pi untouched.
#[test]
fn a_failing_store_read_is_said_once_and_retried_with_pi_untouched() {
    let mut f = start("working");
    let old_pi = f.pi();

    f.migrate_store();
    f.until_screen(
        "the wrapper to say its read failed",
        "Retrying at the next poll",
    );
    f.w.steps(9);
    let screen = f.screen();
    assert_eq!(
        screen.matches("Retrying at the next poll").count(),
        1,
        "{screen}"
    );
    assert!(
        f.pane.spawn.alive(old_pi) && f.signals().is_empty(),
        "pi was touched: {screen}"
    );
    assert!(f.w.ended().is_none(), "the wrapper ended: {screen}");
    assert!(f.pane.spawn.execs().is_empty());

    // The store reads again: the wrapper is the same one and carries on (pi's death is seen and reported).
    f.unmigrate_store();
    f.pane.spawn.killed(old_pi);
    f.until_screen("the wrapper to see pi exit", "pi exited. Press Enter");
    assert!(f.pane.spawn.execs().is_empty());
}

/// The 00:02Z incident: a cpd that cannot open the database (its verdict, contract §4.5) is
/// installed over a running wrapper. The wrapper does not signal
/// pi, does not `exec` it and says so once; a cpd that can open the store,
/// installed next, is exec'd, and adopts pi.
#[test]
fn a_lead_wrapper_does_not_reload_into_a_cpd_older_than_the_store_and_reloads_into_one_that_can_open_it()
 {
    let mut f = start("working");
    let old_pi = f.pi();

    f.install_says(OLDER);
    f.until_screen("the wrapper to refuse the reload", "reload skipped");
    // Pi is not signalled, nothing is exec'd and nothing is said twice over three polls of the wrapper.
    f.w.steps(3);
    let screen = f.screen();
    assert_eq!(screen.matches("reload skipped").count(), 1, "{screen}");
    assert!(
        screen.contains(&format!(
            "reload skipped: {} cannot open the store: The database was migrated by a newer cpd.",
            f.installed.display()
        )),
        "{screen}"
    );
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(f.pane.spawn.alive(old_pi));
    assert!(
        f.pane.spawn.execs().is_empty(),
        "the older cpd was exec'd: {:?}",
        f.pane.spawn.execs()
    );
    assert_eq!(f.pis(), 1);

    f.install();
    let exec = f.execed();
    assert_eq!(
        exec.args,
        Fixture::adopting(old_pi, false),
        "the wrapper re-execed the new file"
    );
    f.resume(&exec);
    f.w.steps(5);
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(f.pane.spawn.alive(old_pi));
    assert_eq!(f.pis(), 1);
}

#[test]
fn a_lead_wrapper_whose_pi_exits_does_not_exec_a_cpd_older_than_the_store() {
    let mut f = start("working");
    f.install_says(OLDER);
    // The Lead is not idle: the wrapper only meets the install when pi dies on its own.
    // (An exit 0 would archive the Lead and end the wrapper.)
    f.pane.spawn.killed(f.pi());
    f.until_screen("the wrapper to refuse the reload", "reload skipped");
    f.until_screen("the wrapper to carry on", "pi exited. Press Enter");
    assert!(
        f.pane.spawn.execs().is_empty(),
        "the older cpd was exec'd: {:?}",
        f.pane.spawn.execs()
    );
    assert_eq!(f.pis(), 1);
}

/// The root of the `no_cockpit` flake: a Lead wrapper whose first store write waits longer than
/// `store_busy_ms` for the write lock used to end with `store_busy`, and its pane with it. It
/// says what it waits for and starts pi once the lock is free.
#[test]
fn a_wrapper_whose_first_write_meets_a_held_write_lock_waits_and_starts_pi_when_it_is_released() {
    // The shortest wait SQLite is given: the lock is held for much longer than `store_busy_ms`.
    let mut f = prepare("idle", 1);
    let holder = rusqlite::Connection::open(f.fx.env.cpd_home().join("cpd.db")).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();

    f.until_screen(
        "the wrapper to say it waits for the store",
        "Waiting for the store",
    );
    // Over several tries of the wrapper (a 0.5 s, 1 s, 2 s wait on its clock), it neither ended nor started pi.
    f.w.steps(8);
    let screen = f.screen();
    assert_eq!(
        screen.matches("Waiting for the store").count(),
        1,
        "said once: {screen}"
    );
    assert!(
        screen.contains("recording the session file") && screen.contains("store_busy"),
        "{screen}"
    );
    assert!(f.w.ended().is_none(), "the wrapper ended: {screen}");
    assert_eq!(f.pis(), 0, "pi started under the lock");

    holder.execute_batch("COMMIT").unwrap();
    f.until_pis("pi", 1);
    assert!(f.w.ended().is_none());
    let recorded: String =
        f.fx.env
            .query("SELECT file FROM sessions WHERE id = 'lead1'");
    assert_eq!(
        recorded,
        f.session_file.display().to_string(),
        "the write that waited was made"
    );
}

/// What does not heal still ends a wrapper, as before: a Session that does not exist.
#[test]
fn a_wrapper_whose_session_does_not_exist_still_ends_with_that_error() {
    let mut f = prepare("idle", 100);

    f.w = f.pane.run(&["session", "run", "nosuch"]);
    let Ended::Returned { code, .. } = f.w.finish() else {
        panic!("the wrapper did not end with an error")
    };
    assert_ne!(*code, 0);
    let screen = f.screen();
    assert!(screen.contains("session_not_found"), "{screen}");
    assert!(!screen.contains("Waiting for the store"), "{screen}");
    assert_eq!(f.pis(), 0);
}

/// The verdict is the real one, not a fake `--version`: the store has applied a breaking migration
/// the installed cpd does not know (its floor), so `cpd --version` reports `store.opens` false and
/// the wrapper keeps running its old code.
#[test]
fn a_lead_wrapper_follows_the_real_verdict_of_the_new_binary() {
    use rusqlite::{Connection, functions::FunctionFlags};
    let mut f = start("idle");
    {
        let conn = Connection::open(f.fx.env.cpd_home().join("cpd.db")).unwrap();
        let txn: i64 = conn
            .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
            .unwrap();
        conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn))
            .unwrap();
        conn.execute(
            "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
             VALUES ('29991231T2359-breaking', 'breaking', lower(hex(zeroblob(32))), (SELECT max(seq) + 1 FROM schema_migrations), ?1, 0)",
            [txn],
        )
        .unwrap();
    }

    f.install();
    f.until_screen("the wrapper to refuse the reload", "reload skipped");
    let screen = f.screen();
    assert!(
        screen.contains(&format!(
            "reload skipped: {} cannot open the store: ",
            f.installed.display()
        )),
        "{screen}"
    );
    assert!(f.signals().is_empty(), "pi was signalled");
    assert!(
        f.pane.spawn.execs().is_empty(),
        "the new cpd was exec'd: {:?}",
        f.pane.spawn.execs()
    );
    assert!(f.w.ended().is_none());
}
