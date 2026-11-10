//! E16, the janitor (contract §3.7, §8.2 `watch.janitor_minutes`): starts `cpd janitor` detached on the first tick of an
//! engine's life and then every `watch.janitor_minutes` by the engine's clock. The spawn is a `FakeSpawn` and the clock a
//! `ManualClock`: no process starts and no test sleeps.

use std::fs::{self, File};
use std::time::Duration;

use cpd_core::host::spawn::Launch;
use cpd_core::ports::Fakes;
use serde_json::json;

use super::janitor;
use super::testkit::Fx;

/// An engine whose `watch.janitor_minutes` is 10, on the fake clock and the fake spawn (whose `cpd` is `/fake/cpd`).
fn fx() -> (Fx, Fakes) {
    let mut fx = Fx::new("[watch]\njanitor_minutes = 10\n");
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe("/fake/cpd");
    fx.engine.ports = fakes.ports.clone();
    (fx, fakes)
}

fn tick(fx: &mut Fx) {
    janitor::run(&mut fx.engine);
}

fn started(fakes: &Fakes) -> Vec<Launch> {
    fakes.spawn.launches()
}

fn watch_log(fx: &Fx) -> String {
    fs::read_to_string(fx.engine.home.watch_log()).unwrap()
}

#[test]
fn e16_starts_the_janitor_on_the_first_tick_and_then_every_janitor_minutes() {
    let (mut fx, fakes) = fx();

    tick(&mut fx);
    let launches = started(&fakes);
    assert_eq!(
        launches.len(),
        1,
        "the first tick of an engine's life starts a pass: {launches:?}"
    );
    assert_eq!(launches[0].program, "/fake/cpd");
    assert_eq!(launches[0].args, ["janitor"]);
    assert!(launches[0].setsid, "detached");
    assert_eq!(
        launches[0].stdout_to,
        Some(cpd_core::janitor::log_path(&fx.engine.home)),
        "its output is in the janitor log"
    );

    fakes.clock.advance(Duration::from_secs(9 * 60 + 59));
    tick(&mut fx);
    assert_eq!(
        started(&fakes).len(),
        1,
        "a tick before janitor_minutes later starts none"
    );

    fakes.clock.advance(Duration::from_secs(1));
    tick(&mut fx);
    assert_eq!(
        started(&fakes).len(),
        2,
        "a tick at janitor_minutes starts one"
    );

    fakes.clock.advance(Duration::from_secs(9 * 60));
    tick(&mut fx);
    assert_eq!(
        started(&fakes).len(),
        2,
        "the window counts from the last start"
    );

    fakes.clock.advance(Duration::from_secs(5 * 60));
    tick(&mut fx);
    assert_eq!(
        started(&fakes).len(),
        3,
        "a tick after janitor_minutes starts one"
    );
}

#[test]
fn e16_starts_nothing_while_the_janitor_lock_is_held() {
    let (mut fx, fakes) = fx();
    let locks = fx.engine.home.locks_dir();
    fs::create_dir_all(&locks).unwrap();
    let held = File::create(locks.join("janitor.lock")).unwrap();
    assert!(
        cpd_core::lock::try_lock(&held).unwrap(),
        "the test takes the lock"
    );

    tick(&mut fx);
    fakes.clock.advance(Duration::from_secs(3600));
    tick(&mut fx);

    assert!(started(&fakes).is_empty(), "{:?}", started(&fakes));
    assert_eq!(fx.count("error"), 0);

    // The same engine, the lock let go: the pass is due (none was started in this engine's life), so one starts.
    drop(held);
    tick(&mut fx);
    assert_eq!(started(&fakes).len(), 1, "{:?}", started(&fakes));
}

#[test]
fn e16_holds_while_incident_mode_is_on() {
    let (mut fx, fakes) = fx();
    fx.fixture("INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn());");

    tick(&mut fx);

    assert!(
        watch_log(&fx).contains("cpd watch: janitor: holding: incident mode is on"),
        "{}",
        watch_log(&fx)
    );
    assert!(started(&fakes).is_empty(), "{:?}", started(&fakes));
}

#[test]
fn e16_reports_a_failed_spawn_as_spawn_failed() {
    let mut fx = Fx::new("");
    // A FakeSpawn with no `self_exe` cannot say what to start.
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();

    tick(&mut fx);

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(
        (&errors[0].3["source"], &errors[0].3["code"]),
        (&json!("watch"), &json!("spawn_failed"))
    );
    assert!(started(&fakes).is_empty());
}
