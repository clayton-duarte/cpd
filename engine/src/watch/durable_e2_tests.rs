//! E2 and the stored runner of a Session (contract §3.6, §3.7 E2): a paneless `durable` Lead or Manager is live by design,
//! so E2 does not stop, restart or touch it; a paneless `pi` one is stopped and restarted as before. On the fake ports: time
//! moves only on the `ManualClock`, tmux is the in-process fake, no process starts.

use std::sync::Arc;
use std::time::Duration;

use cpd_core::clock::ManualClock;
use cpd_core::host::spawn::FakeSpawn;
use cpd_core::ports::Ports;

use super::liveness;
use super::testkit::Fx;
use cpd_core::host::runner::FakeRunner;

/// A Plan `pay` whose Lead `lead1` is `state` with no pane and the stored `runner`, and a Manager `mgr1` likewise; the fake
/// tmux has a server with the cockpit session `cpd`. Returns the fixture, the clock and the tmux runner (what E2 sent).
fn fx(lead_runner: &str, manager_runner: &str, state: &str) -> (Fx, Arc<ManualClock>, FakeRunner) {
    let mut fx = Fx::with_plan("");
    let runner = fx.on_fake_tmux();
    // E2's restart builds its own `Tmux` (the config socket, `$TMUX_TMPDIR`): the engine's `Tmux` is the same one, so both reach
    // one fake server (nextest runs one test per process).
    unsafe { std::env::set_var("TMUX_TMPDIR", fx.engine.home.root()) };
    fx.engine.tmux = cpd_core::host::Tmux::new(fx.engine.config.tmux.socket.clone())
        .with_runner(Arc::new(runner.clone()));
    fx.tmux_server();
    fx.engine
        .tmux
        .run(&["new-session", "-d", "-s", "cpd", "sleep", "600"].map(str::to_owned))
        .unwrap();
    let clock = Arc::new(ManualClock::new());
    fx.engine.ports = Ports {
        clock: clock.clone(),
        runner: Arc::new(runner.clone()),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    fx.fixture(&format!(
        "UPDATE sessions SET state = '{state}', runner = '{lead_runner}' WHERE id = 'lead1';
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
           VALUES ('mgr1', 'manager', 'ctx', NULL, 'ctx-manager', '/', 'O4', '{state}', 't', 't', '{manager_runner}');"
    ));
    (fx, clock, runner)
}

fn state_and_pane(fx: &mut Fx, id: &str) -> (String, Option<String>) {
    fx.engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, tmux_pane FROM sessions WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

fn new_windows(runner: &FakeRunner) -> usize {
    runner
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a == "new-window"))
        .count()
}

#[test]
fn e2_leaves_a_paneless_durable_lead_alone() {
    let (mut fx, _clock, runner) = fx("durable", "pi", "idle");
    // The pi Manager beside it is stopped with no pane, so E2 does not read it: its restart would be a window of its own.
    fx.fixture("UPDATE sessions SET state = 'stopped' WHERE id = 'mgr1';");
    liveness::run(&mut fx.engine);
    assert_eq!(
        state_and_pane(&mut fx, "lead1"),
        ("idle".to_owned(), None),
        "not stopped, no pane written"
    );
    assert_eq!(new_windows(&runner), 0, "not restarted");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn e2_leaves_a_paneless_durable_manager_alone() {
    let (mut fx, _clock, _runner) = fx("pi", "durable", "idle");
    liveness::run(&mut fx.engine);
    assert_eq!(
        state_and_pane(&mut fx, "mgr1"),
        ("idle".to_owned(), None),
        "not stopped, no pane written"
    );
    assert_ne!(
        state_and_pane(&mut fx, "lead1").0,
        "idle",
        "the pi Lead beside it is still stopped by E2"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn e2_never_stops_or_restarts_a_working_durable_session_with_no_pane() {
    let (mut fx, clock, runner) = fx("durable", "durable", "working");
    for _ in 0..3 {
        liveness::run(&mut fx.engine);
        clock.advance(Duration::from_secs(11 * 60));
    }
    for id in ["lead1", "mgr1"] {
        assert_eq!(
            state_and_pane(&mut fx, id),
            ("working".to_owned(), None),
            "{id}"
        );
    }
    assert_eq!(
        new_windows(&runner),
        0,
        "no window, however long the Session has run"
    );
    assert!(
        !fx.engine
            .home
            .run_dir()
            .join("session-restarts.json")
            .exists(),
        "no restart was recorded"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn e2_still_stops_and_restarts_a_paneless_pi_lead() {
    let (mut fx, _clock, runner) = fx("pi", "pi", "idle");
    // The pi Manager beside it is stopped with no pane, so E2 does not read it: its restart would be a second window.
    fx.fixture("UPDATE sessions SET state = 'stopped' WHERE id = 'mgr1';");
    liveness::run(&mut fx.engine);
    assert_eq!(
        new_windows(&runner),
        1,
        "the pi Lead is restarted in a parked window"
    );
    let (state, pane) = state_and_pane(&mut fx, "lead1");
    assert_eq!(state, "stopped", "E2 stopped it");
    assert!(pane.is_some(), "and the restart recorded its new pane");
    assert!(
        fx.engine
            .home
            .run_dir()
            .join("session-restarts.json")
            .exists(),
        "the restart is recorded"
    );
}
