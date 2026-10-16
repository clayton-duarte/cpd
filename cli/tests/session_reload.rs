//! `session run` reloads itself after an install (hs-d33, hs-d38, hs-d49;
//! contract §6.7 "Reload on install"), run as a copy of the built binary in a
//! pane of a private tmux server, with a fake `pi`. An install is a new file
//! renamed over the copy; the new file records that it ran (its pid and
//! arguments) and then becomes the built `cpd`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{CPD, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

const LEAD: &str = "lead1";

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

    /// The pane's process: what the wrapper is, before and after the reload.
    fn pane_pid(&self) -> String {
        let out = self.env.tmux(&["display-message", "-p", "-t", &self.pane, "#{pane_pid}"]);
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// What the installed file recorded: `<pid> <arguments>` per run.
    fn runs(&self) -> Vec<String> {
        fs::read_to_string(self.path("installed.runs")).map(|t| t.lines().map(str::to_owned).collect()).unwrap_or_default()
    }

    /// A new install: a new file renamed over the copy.
    fn install(&self) {
        let new = self.installed.with_extension("new");
        fs::write(
            &new,
            format!("#!/bin/sh\necho \"$$ $*\" >> '{}'\nexec '{CPD}' \"$@\"\n", self.path("installed.runs").display()),
        )
        .unwrap();
        fs::set_permissions(&new, fs::Permissions::from_mode(0o755)).unwrap();
        fs::rename(&new, &self.installed).unwrap();
    }
}

/// A grounded Lead (`lead1`) in `state`, with a finished session file, running
/// `session run` from a copy of `cpd` in its own pane, with its fake pi started once.
/// The fake pi records `SIGTERM` in `pi.signals` and exits at it, or when `pi.exit` appears.
fn start(state: &str) -> Fixture {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[timeouts]\nfork_quiet_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
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
                "INSERT INTO contexts (id, title, created_at, created_on, lead_session, grounded_seq)
                   VALUES ('ctx', 'Ctx', 't', 'm1', '{LEAD}', 0);
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

    let home_dir = env.home().display().to_string();
    env.fake_pi(&format!(
        "trap 'echo TERM >> \"{home_dir}/pi.signals\"; exit 0' TERM\n\
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
    ok(&["new-session", "-d", "-s", "cpd", "-n", "main", "sleep 600"]);
    let command = format!("exec '{}' session run {LEAD}", installed.display());
    let pane = ok(&["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", &command]);
    ok(&["set", "-p", "-t", &pane, "@cpd_session", LEAD]);
    let fixture = Fixture { env, installed, session_file, pane };
    wait_until("the first pi", || fixture.env.calls("pi").len() == 1);
    fixture
}

#[test]
fn a_parked_idle_lead_restarts_its_pi_on_its_session_file_after_an_install() {
    let f = start("idle");
    let pid = f.pane_pid();
    let session_file = f.session_file.display().to_string();
    assert!(f.env.calls("pi")[0].windows(2).any(|w| w == ["--session", &session_file]), "the first start resumes");
    assert!(f.signals().is_empty());

    f.install();

    wait_until("the new binary", || !f.runs().is_empty());
    wait_until("pi restarted", || f.env.calls("pi").len() == 2);
    assert_eq!(f.signals(), "TERM\n", "the old pi got SIGTERM once");
    let calls = f.env.calls("pi");
    assert!(calls[1].windows(2).any(|w| w == ["--session", &session_file]), "{:?}", calls[1]);
    assert_eq!(f.runs(), [format!("{pid} session run {LEAD}")], "the same process re-execed the new file");
    assert_eq!(f.pane_pid(), pid);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}

#[test]
fn a_working_lead_is_not_restarted_and_pi_exiting_on_its_own_execs_before_any_store_write() {
    let f = start("working");
    let pid = f.pane_pid();

    f.install();
    // Several polls: the Lead is not idle, so pi runs on.
    std::thread::sleep(Duration::from_millis(2500));
    assert!(f.runs().is_empty(), "no reload while working");
    assert!(f.signals().is_empty());
    assert_eq!(f.env.calls("pi").len(), 1);

    // pi exits on its own: the wrapper execs before it reports `stopped`.
    fs::write(f.path("pi.exit"), "").unwrap();
    wait_until("the new binary", || !f.runs().is_empty());
    wait_until("pi restarted", || f.env.calls("pi").len() == 2);
    assert_eq!(f.runs(), [format!("{pid} session run {LEAD}")]);
    assert_eq!(query::<String>(&f.env, "SELECT state FROM sessions WHERE id = 'lead1'"), "working");
    assert_eq!(query::<i64>(&f.env, "SELECT count(*) FROM events WHERE kind = 'session-state'"), 0);
    f.env.tmux(&["kill-window", "-t", &f.pane]);
}
