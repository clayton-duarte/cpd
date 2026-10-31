//! The tmux restart's drain (contract §6.4) and E13, Sleep (contract §3.7), with a `durable` Lead (contract §3.6): a durable
//! Lead has no pane, so a working one does not hold the restart, and a finished one is put to sleep without a tmux call; a
//! `pi` Lead is still waited for and its pane still closed. On the fake ports: no process starts.

use std::sync::Arc;

use cpd_core::clock::ManualClock;
use cpd_core::host::runner::FakeRunner;
use cpd_core::host::spawn::FakeSpawn;
use cpd_core::ports::Ports;

use super::testkit::{Fx, ago};
use super::{restart, sleep};

/// Plan `pay`'s Lead `lead1` has the stored `runner`, the `state` and (for the drain) a restart pending.
fn drain_fx(runner: &str, state: &str) -> (Fx, Arc<FakeSpawn>) {
    let mut fx = Fx::with_plan("");
    let spawn = Arc::new(FakeSpawn::new());
    spawn.set_self_exe("/opt/cpd/bin/cpd");
    fx.engine.ports = Ports { clock: Arc::new(ManualClock::new()), runner: Arc::new(FakeRunner::new(ManualClock::new())), spawn: spawn.clone() };
    fx.fixture(&format!("UPDATE sessions SET runner = '{runner}', state = '{state}' WHERE id = 'lead1';"));
    restart::mark(&fx.engine.home, "3.4", "3.8").unwrap();
    (fx, spawn)
}

#[test]
fn tmux_restart_drain_ignores_a_working_durable_lead() {
    let (mut fx, spawn) = drain_fx("durable", "working");
    restart::drain(&mut fx.engine);
    assert_eq!(spawn.launches().len(), 1, "nothing is in the way: the restart starts");
    assert!(restart::busy(&mut fx.engine.store).unwrap().is_empty());
}

#[test]
fn tmux_restart_drain_still_waits_for_a_working_pi_lead() {
    let (mut fx, spawn) = drain_fx("pi", "working");
    restart::drain(&mut fx.engine);
    assert!(spawn.launches().is_empty(), "a working pi Lead holds the restart");
    assert_eq!(restart::busy(&mut fx.engine.store).unwrap().len(), 1);
}

/// Plan `pay` is done and finished long ago, with no PR; Lead `lead1` is idle with the stored `runner`. The tmux is the
/// in-process fake with a server; the returned runner records what E13 sent.
fn sleep_fx(runner: &str) -> (Fx, FakeRunner) {
    let mut fx = Fx::new("");
    let fake = fx.on_fake_tmux();
    fx.tmux_server();
    let long_ago = ago(24 * 60);
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{long_ago}', 't', '{runner}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'lead1', 't', 't', '{long_ago}');"
    ));
    (fx, fake)
}

fn asleep(fx: &mut Fx) -> bool {
    fx.query::<i64>("SELECT count(*) FROM sessions WHERE id = 'lead1' AND asleep_at IS NOT NULL") == 1
}

#[test]
fn sleep_marks_a_finished_durable_lead_asleep_and_touches_no_pane() {
    let (mut fx, fake) = sleep_fx("durable");
    let before = fake.calls().len();
    sleep::run(&mut fx.engine);
    assert!(asleep(&mut fx), "asleep_at is written");
    assert_eq!(fx.count("session-asleep"), 1);
    let calls: Vec<_> = fake.calls().iter().skip(before).map(|c| c.args.join(" ")).collect();
    assert!(calls.is_empty(), "no tmux call: {calls:?}");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn sleep_still_closes_the_pane_of_a_finished_pi_lead() {
    let (mut fx, fake) = sleep_fx("pi");
    // A live pane in a parked window, recorded as the Lead's.
    let pane = fx
        .engine
        .tmux
        .run(&["new-window", "-d", "-P", "-F", "#{pane_id}", "-t", "keepalive:", "sleep", "600"].map(str::to_owned))
        .unwrap();
    let pane = pane.trim().to_owned();
    fx.fixture(&format!("UPDATE sessions SET tmux_pane = '{pane}' WHERE id = 'lead1';"));
    sleep::run(&mut fx.engine);
    assert!(asleep(&mut fx), "asleep_at is written");
    assert!(fake.calls().iter().any(|c| c.args.iter().any(|a| a == "kill-pane")), "the pane was killed");
}
