//! E2 after an OS shutdown (contract §3.7 E2, pro-d358): pi reports `stopped` while the pane stays recorded in
//! `sessions.tmux_pane`. A Lead or Manager (`runner` `pi`) whose recorded pane is dead or missing is restarted as a parked
//! window even when its `state` was `stopped`, unless it is archived, asleep, or a Lead of a `done` or archived Plan; a
//! `stopped` Session with no recorded pane is not restarted. On the fake ports: tmux is the in-process fake, no process starts.

use std::sync::Arc;

use cpd_core::clock::ManualClock;
use cpd_core::host::runner::FakeRunner;
use cpd_core::host::spawn::FakeSpawn;
use cpd_core::ports::Ports;

use super::liveness;
use super::testkit::Fx;

/// Plan `pay` (`dispatch`) with Lead `lead1` and Manager `mgr1`, both `stopped`, `pi`, with no pane; the fake tmux has a server
/// with the cockpit session `cpd`, and lists no pane `%99`.
fn fx() -> (Fx, FakeRunner) {
    let mut fx = Fx::with_plan("");
    let runner = fx.on_fake_tmux();
    unsafe { std::env::set_var("TMUX_TMPDIR", fx.engine.home.root()) };
    fx.engine.tmux = cpd_core::host::Tmux::new(fx.engine.config.tmux.socket.clone())
        .with_runner(Arc::new(runner.clone()));
    fx.tmux_server();
    fx.engine
        .tmux
        .run(&["new-session", "-d", "-s", "cpd", "sleep", "600"].map(str::to_owned))
        .unwrap();
    fx.engine.ports = Ports {
        clock: Arc::new(ManualClock::new()),
        runner: Arc::new(runner.clone()),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    fx.fixture(
        "UPDATE sessions SET state = 'stopped', runner = 'pi' WHERE id = 'lead1';
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
           VALUES ('mgr1', 'manager', 'ctx', NULL, 'ctx-manager', '/', 'O4', 'stopped', 't', 't', 'pi');",
    );
    (fx, runner)
}

fn new_windows(runner: &FakeRunner) -> usize {
    runner
        .calls()
        .iter()
        .filter(|c| c.args.iter().any(|a| a == "new-window"))
        .count()
}

/// Records the vanished pane `%99` for `id`.
fn vanished(fx: &mut Fx, id: &str) {
    fx.fixture(&format!(
        "UPDATE sessions SET tmux_pane = '%99' WHERE id = '{id}';"
    ));
}

#[test]
fn a_lead_whose_pane_vanished_while_stopped_is_restarted() {
    let (mut fx, runner) = fx();
    vanished(&mut fx, "lead1");
    liveness::run(&mut fx.engine);
    assert_eq!(new_windows(&runner), 1, "the Lead is restarted parked");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn a_manager_whose_pane_vanished_while_stopped_is_restarted() {
    let (mut fx, runner) = fx();
    vanished(&mut fx, "mgr1");
    liveness::run(&mut fx.engine);
    assert_eq!(new_windows(&runner), 1, "the Manager is restarted parked");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn a_stopped_lead_with_no_recorded_pane_is_not_restarted() {
    let (mut fx, runner) = fx();
    liveness::run(&mut fx.engine);
    assert_eq!(new_windows(&runner), 0);
}

#[test]
fn an_asleep_lead_is_not_restarted() {
    let (mut fx, runner) = fx();
    vanished(&mut fx, "lead1");
    fx.fixture("UPDATE sessions SET asleep_at = 't' WHERE id = 'lead1';");
    liveness::run(&mut fx.engine);
    assert_eq!(new_windows(&runner), 0);
}

#[test]
fn an_archived_lead_is_not_restarted() {
    let (mut fx, runner) = fx();
    vanished(&mut fx, "lead1");
    fx.fixture("UPDATE sessions SET archived_at = 't' WHERE id = 'lead1';");
    liveness::run(&mut fx.engine);
    assert_eq!(new_windows(&runner), 0);
}

#[test]
fn a_stopped_lead_of_a_done_plan_is_not_restarted() {
    let (mut fx, runner) = fx();
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner, tmux_pane)
           VALUES ('lead2', 'lead', 'ctx', 'old', 'Old', '/', 'O3', 'stopped', 't', 't', 'pi', '%99');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('old', 'ctx', 'Old', 'done', 'lead2', 't', 't', 't');",
    );
    liveness::run(&mut fx.engine);
    assert_eq!(new_windows(&runner), 0);
}
