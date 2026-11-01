//! The adapter watchdog of `session run` (hs-d141; contract §6.7 "Adapter
//! watchdog"): a pi that is alive while its Session stays `stopped` for more
//! than 60 s is restarted, with one `error` Event. The limit and its 2 s poll
//! are the wrapper's constants, and these tests wait them out on the virtual
//! clock (`Fx::advance` between two polls of the wrapper), in the process: the
//! wrapper runs on a thread of the `Pane` of `common::wrapper`, its pi is a pid
//! of the `FakeSpawn`, and no process, tmux or time is real.

use crate::common;

use std::fs;
use std::time::Duration;

use common::fx::Fx;
use common::wrapper::{Pane, Wrapper};
use cpd_core::host::spawn::{Pid, Signal};

const LEAD: &str = "lead1";
/// The wrapper's own poll of the loop (`FILE_POLL`, 0.5 s): a poll of the watchdog is every fourth.
const POLL: Duration = Duration::from_millis(500);
/// The limit of the watchdog: more than this long `stopped`, with pi running (contract §6.7).
const LIMIT: Duration = Duration::from_secs(60);
/// The finished turn the Lead's session file ends with.
const FINISHED: &str = "{\"type\":\"session\"}\n{\"type\":\"message\",\"id\":\"a\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";

/// The wrapper, its pane and its env; they drop in this order.
struct Fixture {
    wrapper: Wrapper,
    pane: Pane,
    fx: Fx,
}

impl Fixture {
    fn errors(&self) -> i64 {
        self.fx
            .env
            .query("SELECT count(*) FROM events WHERE kind = 'error'")
    }

    /// The `Launch`es of pi so far.
    fn pis(&self) -> usize {
        self.pane.spawn.pis().len()
    }

    /// Virtual time since the first pi started.
    fn elapsed(&self) -> Duration {
        self.fx.fakes.clock.elapsed()
    }
}

/// A Manager `lead1`, in `state` since long ago (the clock's epoch), with a finished session file, whose `session run`
/// has started its first pi.
fn start(state: &str) -> Fixture {
    let fx = Fx::with_ports_over(common::fx::bare_with_store());
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n",
            fx.env.tmux_socket()
        ),
    )
    .unwrap();
    let at = "2026-01-01T00:00:00Z";
    let cwd = fx.env.root().display().to_string();
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('{LEAD}', 'manager', 'ctx', NULL, NULL, 'Manager', '{cwd}', 'O3', '{state}', '{at}', '{at}');"
    ));
    let sessions = fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join(format!("2026_{LEAD}.jsonl")), FINISHED).unwrap();
    let installed = fx.env.root().join("installed-bin/cpd");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    fs::write(&installed, "").unwrap();

    let pane = Pane::new(&fx, LEAD, &installed);
    let wrapper = pane.run(&["session", "run", LEAD]);
    let mut f = Fixture { wrapper, pane, fx };
    let pane = &f.pane;
    f.wrapper
        .until("the first pi", || pane.spawn.pis().len() == 1);
    f
}

#[test]
fn a_pi_alive_with_its_state_stopped_for_sixty_seconds_is_restarted_with_one_error_event() {
    let mut f = start("stopped");
    let first_pi = f.pane.spawn.pids()[0];

    // Not before the limit: the polls of the watchdog up to 59.5 s of virtual time, and the wrapper has done nothing.
    f.fx.advance(LIMIT - Duration::from_secs(3));
    f.wrapper.steps(4);
    // time-claim: the watchdog's limit is the subject, on the ManualClock's virtual time; no per-poll signal from the watchdog.
    assert!(f.elapsed() < LIMIT);
    assert_eq!(
        f.pis(),
        1,
        "restarted at {:?}, before the limit",
        f.elapsed()
    );
    assert!(f.pane.spawn.signals().is_empty());
    assert_eq!(f.errors(), 0);

    // Past it, at the next poll of the watchdog: pi gets SIGTERM and is started again, on its session file.
    let pane = &f.pane;
    f.wrapper
        .until("pi restarted", || pane.spawn.pis().len() >= 2);
    let restarted = f.elapsed();
    assert!(
        restarted > LIMIT,
        "restarted after {restarted:?}, before the limit"
    );
    assert_eq!(
        f.pane.spawn.signals(),
        [(first_pi, Signal::Term)],
        "SIGTERM once, to the first pi"
    );
    let session_file =
        f.fx.env
            .home()
            .join(format!(".pi/agent/sessions/g/2026_{LEAD}.jsonl"))
            .display()
            .to_string();
    assert!(
        f.pane.spawn.pis()[1]
            .args
            .windows(2)
            .any(|w| w == ["--session", &session_file]),
        "it resumes the session file"
    );
    assert_eq!(f.errors(), 1, "one Event for the restart");

    // The restarted pi has no adapter either, so it is restarted in turn: the limit starts again at the restart, and
    // each restart has its one Event.
    let (pane, fx) = (&f.pane, &f.fx);
    f.wrapper.until("a second restart", || {
        pane.spawn.pis().len() >= 3
            && fx
                .env
                .query::<i64>("SELECT count(*) FROM events WHERE kind = 'error'")
                >= 2
    });
    let apart = f.elapsed() - restarted;
    assert!(
        apart > LIMIT,
        "the second restart came {apart:?} after the first, within the limit"
    );
    let first = |sql: &str| {
        f.fx.env.query::<String>(&format!(
            "{sql} FROM events WHERE kind = 'error' ORDER BY id LIMIT 1"
        ))
    };
    assert_eq!(
        first("SELECT json_extract(data, '$.code')"),
        "adapter_missing"
    );
    assert_eq!(
        first("SELECT json_extract(data, '$.source')"),
        "session run"
    );
    assert_eq!(
        first("SELECT json_extract(data, '$.details.session')"),
        LEAD
    );
    assert_eq!(first("SELECT session"), LEAD);
    // How long pi had run with the adapter stopped, as each Event says it: over the limit, from each start.
    for seconds in f.fx.env.read(|conn| {
        conn.prepare("SELECT json_extract(data, '$.details.seconds') FROM events WHERE kind = 'error' ORDER BY id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()
    }) {
        assert!(seconds as u64 > LIMIT.as_secs() && seconds as u64 <= LIMIT.as_secs() + 3, "the Event says {seconds} s");
    }
    assert_eq!(
        f.pane
            .spawn
            .signals()
            .iter()
            .filter(|(_, signal)| *signal == Signal::Term)
            .count(),
        2,
        "one SIGTERM per restart"
    );
}

#[test]
fn a_pi_whose_adapter_reports_idle_within_the_limit_is_left_alone() {
    let mut f = start("stopped");
    // Up to the last poll of the watchdog within the limit (59.5 s of virtual time): nothing yet.
    f.fx.advance(LIMIT - Duration::from_secs(3));
    f.wrapper.steps(4);
    // time-claim: the watchdog's limit is the subject, on the ManualClock's virtual time; no per-poll signal from the watchdog.
    assert!(f.elapsed() < LIMIT);
    // The adapter reports `idle` now; the limit passes at the next polls of the watchdog, which see `idle`, not `stopped`.
    f.fx.env.sql(&format!(
        "UPDATE sessions SET state = 'idle' WHERE id = '{LEAD}'"
    ));
    f.fx.advance(Duration::from_secs(5));
    f.wrapper.steps(12);
    assert!(f.elapsed() > LIMIT + Duration::from_secs(5));
    // The claim is a count of what the wrapper did to pi (nothing signalled, nothing started), not a wait for nothing.
    assert_eq!(f.pis(), 1);
    assert!(f.pane.spawn.signals().is_empty(), "pi was signalled");
    assert_eq!(f.errors(), 0);
}

#[test]
fn a_pi_that_exits_is_handled_as_before_with_no_error_event() {
    // Status 1: a crash, not the User's quit (which archives and ends the wrapper), after a few polls of the watchdog
    // (the real limit: no restart comes first).
    let mut f = start("stopped");
    let pi: Pid = f.pane.spawn.pids()[0];
    f.wrapper.steps(4);
    f.fx.fakes.spawn.exit(pi, 1);
    f.wrapper.finish();
    // The wrapper reported the exit and waited for Enter (its input ends here: it gives up); a few polls later it has
    // not started pi again.
    assert!(f.pane.screen().contains("pi exited"), "{}", f.pane.screen());
    f.fx.advance(POLL * 3);
    assert_eq!(f.pis(), 1, "the wrapper waits for Enter");
    assert_eq!(f.errors(), 0);
}
