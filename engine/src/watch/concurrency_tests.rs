//! Red test: the Worker cap (`concurrency`) follows the config file on each tick (contract §8.2, §3.7 E3).
//!
//! Today the E3 step takes `engine.config.concurrency`, loaded once when the engine starts
//! (engine/src/watch/engine.rs), while `durable_workers` is already re-read from the file on every tick. This pins
//! the same live read for `concurrency`: the cap a tick computes is the one the file holds at that tick, not the one
//! loaded at start-up. Fakes only: no process, no tmux, no network, no sleep.

use cpd_core::ports::Fakes;

use super::schedule;
use super::testkit::Fx;

/// An explore Job `key` of Plan `pay` (no Workflow), `queued` with its fingerprint, then moved by `then` (SQL): the
/// fixture of `explore_tests`. Explore Jobs take Worker slots and have no linear/parallel rule, so the only thing
/// gating them is `concurrency`.
fn add_explore(fx: &mut Fx, kind: &str, tier: &str, key: &str, needs: &[&str], then: &str) {
    let mut sql = format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, created_txn, created_at)
         VALUES ('pay', NULL, '{key}', 'Title of {key}', '{tier}', 'Find {key}.', 'Because.', '[\"{key}.md\"]', '[\"it is answered\"]', '[]', '{kind}', 'draft', 0, cpd_txn(), 't');"
    );
    for need in needs {
        sql += &format!(
            "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');"
        );
    }
    fx.fixture(&sql);
    fx.lock_job(key, then);
}

/// The Attempts the store holds: one per Worker the tick started.
fn started(fx: &mut Fx) -> i64 {
    fx.query("SELECT count(*) FROM attempts")
}

#[test]
fn the_worker_cap_follows_the_config_file_each_tick() {
    let mut fx = Fx::with_plan("dispatch", "concurrency = 1\n");
    add_explore(&mut fx, "scout", "H1", "one", &[], "");
    add_explore(&mut fx, "scout", "H2", "two", &[], "");
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    fx.engine.ports = fakes.ports.clone();

    schedule::run(&mut fx.engine);
    assert_eq!(started(&mut fx), 1, "the start value 1 admits one Worker");

    // The User sets `concurrency = 2`; the next tick must read it, not the value loaded at start-up.
    let path = fx.engine.home.config();
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("concurrency = 1", "concurrency = 2")).unwrap();

    schedule::run(&mut fx.engine);
    assert_eq!(
        started(&mut fx),
        2,
        "the cap the tick computes is 2, not the start value 1"
    );
}
