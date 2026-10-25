//! The adapter watchdog of `session run` (hs-d141; contract §6.7 "Adapter
//! watchdog"): a pi that is alive while its Session stays `stopped` for more
//! than 60 s is restarted, with one `error` Event. The limit and its 2 s poll
//! are constants of the wrapper; a debug build takes shorter ones from
//! `CPD_TEST_ADAPTER_WATCHDOG` and `CPD_TEST_WATCHDOG_POLL` (contract §1.2 "Test
//! overrides"), which these tests set and wait out, in a pane of a private tmux
//! server with a fake `pi`.

use crate::common;

use std::fs;
use std::time::Duration;

use common::{CPD, SLEEP_SECONDS, TestEnv, wait_until};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

const LEAD: &str = "lead1";
const POLL: Duration = Duration::from_millis(100);
/// The wrapper's polls: the watchdog's and the wrapper's loop (`FILE_POLL`, 0.5 s).
const POLLS: [(&str, &str); 2] = [("CPD_TEST_WATCHDOG_POLL", "0.1"), ("CPD_TEST_FILE_POLL", "0.1")];
/// The finished turn the Lead's session file ends with.
const FINISHED: &str = "{\"type\":\"session\"}\n{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";

struct Fixture {
    env: TestEnv,
    pane: String,
}

impl Fixture {
    fn errors(&self) -> i64 {
        self.env.query("SELECT count(*) FROM events WHERE kind = 'error'")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.env.tmux(&["kill-window", "-t", &self.pane]);
    }
}

/// A Manager `lead1`, in `state` since long ago, with a finished session
/// file, running `session run` in a pane with the environment `vars`. `pi` is
/// the body of the fake pi; it exits at `SIGTERM`.
fn start(state: &str, pi: &str, vars: &[(&str, &str)]) -> Fixture {
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
                   VALUES ('{LEAD}', 'manager', 'ctx', NULL, NULL, 'Manager', '{cwd}', 'O3', '{state}', '{at}', '{at}');"
            ))?;
            Ok(())
        })
        .unwrap();
    let sessions = env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join(format!("2026_{LEAD}.jsonl")), FINISHED).unwrap();

    let home_dir = env.home().display().to_string();
    env.fake_pi(&format!(
        "trap 'exit 0' TERM\n\
         {pi}\n\
         while :; do\n  if [ -e \"{home_dir}/pi.exit\" ]; then rm \"{home_dir}/pi.exit\"; exit 0; fi\n  sleep 0.2\ndone"
    ));

    let ok = |args: &[&str]| {
        let out = env.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    ok(&["new-session", "-d", "-s", "cpd", "-n", "main", &format!("sleep {SLEEP_SECONDS}")]);
    let vars: String = vars.iter().map(|(k, v)| format!("{k}={v} ")).collect();
    let command = format!("exec env {vars}'{CPD}' session run {LEAD}");
    let pane = ok(&["new-window", "-d", "-t", "=cpd:", "-P", "-F", "#{pane_id}", &command]);
    ok(&["set", "-p", "-t", &pane, "@cpd_session", LEAD]);
    let fixture = Fixture { env, pane };
    wait_until("the first pi", || !fixture.env.calls("pi").is_empty());
    fixture
}

#[test]
fn a_pi_that_exits_is_handled_as_before_with_no_error_event() {
    // Status 1: a crash, not the User's quit (which archives and ends the wrapper), after a few polls of the watchdog
    // (the real limit: no restart comes first).
    let f = start("stopped", "sleep 0.3; exit 1", &POLLS);
    wait_until("the exit to be reported", || {
        f.env.tmux(&["capture-pane", "-p", "-t", &f.pane]).stdout.windows(9).any(|w| w == b"pi exited")
    });
    // time-claim: the wrapper waits for Enter, over a few of its polls; no per-poll signal from `session run`.
    std::thread::sleep(POLL * 3);
    assert_eq!(f.env.calls("pi").len(), 1, "the wrapper waits for Enter");
    assert_eq!(f.errors(), 0);
}
