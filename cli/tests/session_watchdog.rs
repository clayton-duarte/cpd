//! The adapter watchdog of `session run` (hs-d141; contract §6.7 "Adapter
//! watchdog"): a pi that is alive while its Session stays `stopped` for more
//! than 60 s is restarted, with one `error` Event. The limit is a constant of
//! the wrapper, so these tests wait it out, in a pane of a private tmux server
//! with a fake `pi`.

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::{CPD, SLEEP_SECONDS, TestEnv, wait_until, wait_until_for};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

const LEAD: &str = "lead1";
/// The watchdog's limit (the wrapper's constant), and a margin for its 2 s poll.
const LIMIT: Duration = Duration::from_secs(60);
const MARGIN: Duration = Duration::from_secs(8);

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000).unwrap().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// The finished turn the Lead's session file ends with.
const FINISHED: &str = "{\"type\":\"session\"}\n{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";

struct Fixture {
    env: TestEnv,
    pane: String,
    started: Instant,
}

impl Fixture {
    fn errors(&self) -> i64 {
        query(&self.env, "SELECT count(*) FROM events WHERE kind = 'error'")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.env.tmux(&["kill-window", "-t", &self.pane]);
    }
}

/// A Manager `lead1`, `stopped` since long ago, with a finished session
/// file, running `session run` in a pane. `pi` is the body of the fake pi after
/// it records its pid in `pi.pids`; it exits at `SIGTERM`.
fn start(pi: &str) -> Fixture {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()),
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
                   VALUES ('{LEAD}', 'manager', 'ctx', NULL, NULL, 'Manager', '{cwd}', 'O3', 'stopped', '{at}', '{at}');"
            ))?;
            Ok(())
        })
        .unwrap();
    let sessions = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join(format!("2026_{LEAD}.jsonl")), FINISHED).unwrap();

    let home_dir = env.home().display().to_string();
    let cpd_home = env.cpd_home().display().to_string();
    env.fake_pi(&format!(
        "trap 'exit 0' TERM\n\
         report() {{ CPD_HOME='{cpd_home}' '{CPD}' session state \"$1\" --session {LEAD} >/dev/null; }}\n\
         {pi}\n\
         while :; do\n  if [ -e \"{home_dir}/pi.exit\" ]; then rm \"{home_dir}/pi.exit\"; exit 0; fi\n  sleep 0.2\ndone"
    ));

    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    ok(&["new-session", "-d", "-s", "cpd", "-n", "main", &format!("sleep {SLEEP_SECONDS}")]);
    let command = format!("exec '{CPD}' session run {LEAD}");
    let pane = ok(&["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", &command]);
    ok(&["set", "-p", "-t", &pane, "@cpd_session", LEAD]);
    let started = Instant::now();
    let fixture = Fixture { env, pane, started };
    wait_until("the first pi", || fixture.env.calls("pi").len() == 1);
    fixture
}

#[test]
fn a_pi_alive_with_its_state_stopped_for_sixty_seconds_is_restarted_with_one_error_event() {
    let f = start(":");
    assert_eq!(f.env.calls("pi").len(), 1, "not restarted at once");
    assert_eq!(f.errors(), 0);

    // The lower bound is the claim: not restarted before the limit. How late it comes is no claim, only a deadline.
    wait_until_for("pi restarted", LIMIT + MARGIN, || f.env.calls("pi").len() == 2);
    let after = f.started.elapsed();
    assert!(after >= LIMIT, "restarted after {after:?}, before the limit");
    let session_file = f.env.home().join(format!(".pi/agent/sessions/g/2026_{LEAD}.jsonl")).display().to_string();
    assert!(f.env.calls("pi")[1].windows(2).any(|w| w == ["--session", &session_file]), "it resumes the session file");

    assert_eq!(f.errors(), 1);
    assert_eq!(query::<String>(&f.env, "SELECT json_extract(data, '$.code') FROM events WHERE kind = 'error'"), "adapter_missing");
    assert_eq!(query::<String>(&f.env, "SELECT json_extract(data, '$.source') FROM events WHERE kind = 'error'"), "session run");
    assert_eq!(query::<String>(&f.env, "SELECT json_extract(data, '$.details.session') FROM events WHERE kind = 'error'"), LEAD);
    assert_eq!(query::<String>(&f.env, "SELECT session FROM events WHERE kind = 'error'"), LEAD);

    // The 60 s start again after the restart.
    // time-claim: no second restart and no second Event over two 2 s polls of the watchdog; no per-poll signal from
    // `session run`.
    std::thread::sleep(Duration::from_millis(4500));
    assert_eq!(f.env.calls("pi").len(), 2);
    assert_eq!(f.errors(), 1);
}

#[test]
fn a_pi_whose_adapter_reports_idle_within_the_limit_is_left_alone() {
    let f = start("sleep 1; report idle");
    wait_until("idle", || query::<String>(&f.env, "SELECT state FROM sessions WHERE id = 'lead1'") == "idle");
    // time-claim: an idle pi is left alone past the 60 s limit and a poll of the watchdog; no per-poll signal from
    // `session run`.
    std::thread::sleep((LIMIT + MARGIN).saturating_sub(f.started.elapsed()));
    assert_eq!(f.env.calls("pi").len(), 1);
    assert_eq!(f.errors(), 0);
}

#[test]
fn a_pi_that_exits_is_handled_as_before_with_no_error_event() {
    // Status 1: a crash, not the User's quit (which archives and ends the wrapper).
    let f = start("sleep 1; exit 1");
    wait_until("the exit to be reported", || {
        f.env.tmux(&["capture-pane", "-p", "-t", &f.pane]).stdout.windows(9).any(|w| w == b"pi exited")
    });
    // time-claim: the wrapper waits for Enter, over a few of its 0.5 s polls; no per-poll signal from `session run`.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(f.env.calls("pi").len(), 1, "the wrapper waits for Enter");
    assert_eq!(f.errors(), 0);
}
