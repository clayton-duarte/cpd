//! An engine whose store outgrew its binary stands down (contract §3.7 "An engine whose store outgrew its binary stands
//! down"; hs-d745): when the store verdict no longer opens for this binary (a breaking migration it lacks), the engine runs
//! no further step, writes nothing (no `error`, no `watch-stopped`), logs one `run/watch.log` line and exits 0. A store
//! with only additive migrations it lacks is inside the window: the engine keeps running.
//!
//! The engine runs in-process on the `ManualClock` ([`Fx::watch`]); the migration is applied by hand, between two ticks
//! (the hook runs before each nap) or inside one (the tmux route of E2). The step that would visibly write is E8: a
//! `run/models-bifrost.json` with a fresh change opens a `models-changed` Inbox item at the next tick that runs E8.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::fx::Fx;
use common::newer_migration::apply_newer;
use cpd_core::clock::Clock;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Reply};
use cpd_core::lock;
use cpd_core::output::Output;
use serde_json::json;
use time::format_description::well_known::Rfc3339;

const BREAKING: &str = "29991231T2400-rebuild";
const ADDITIVE: &str = "29991231T2358-newer-table";

/// The test's own stop bound, in naps of virtual time: far more than a stood-down engine takes.
const BOUND: usize = 30;

/// The nap before which the migration lands (the engine has run one tick).
const MIGRATES_AT: usize = 2;

fn breaking(cpd_home: &Path) {
    apply_newer(
        cpd_home,
        BREAKING,
        "breaking",
        "CREATE TABLE rebuilt (id INTEGER PRIMARY KEY) STRICT;",
    );
}

fn additive(cpd_home: &Path) {
    apply_newer(
        cpd_home,
        ADDITIVE,
        "additive",
        "CREATE TABLE newer_things (id INTEGER PRIMARY KEY, label TEXT NOT NULL) STRICT;",
    );
}

/// The engine's home: config, Context `ctx`, a tmux with no server.
fn engine() -> Fx {
    let fx = Fx::with_ports();
    fx.engine_config("");
    fx.env.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');");
    fx.engine_routes();
    fx
}

/// `run/models-bifrost.json` as E15 wrote it a minute ago with a change made a minute ago: the next E8 opens a
/// `models-changed` item for it, a write only a step that runs can make.
fn announce_a_change(cpd_home: &Path, clock: &impl Clock) {
    let at = (clock.utc() - time::Duration::seconds(60))
        .replace_nanosecond(0)
        .unwrap()
        .format(&Rfc3339)
        .unwrap();
    let file = json!({
        "version": 1, "provider": "bifrost", "polled_at": at, "pi_version": "1.3.0",
        "source": {"kind": "openai-models", "auth": "custom", "base": null},
        "ids": ["anthropic/claude-sonnet-5"],
        "changes": {"at": at, "appeared": ["openai/gpt-5.6"], "went_away": []}
    });
    fs::create_dir_all(cpd_home.join("run")).unwrap();
    fs::write(cpd_home.join("run/models-bifrost.json"), file.to_string()).unwrap();
}

fn items(fx: &Fx) -> i64 {
    fx.env
        .query("SELECT count(*) FROM inbox_items WHERE kind = 'models-changed'")
}

/// The newest `txns` id the migration `name` stands on (`apply_newer` records it): what is newer than it was written after.
fn migration_txn(fx: &Fx, name: &str) -> i64 {
    fx.env.query(&format!(
        "SELECT txn FROM schema_migrations WHERE name = '{name}'"
    ))
}

fn log(fx: &Fx) -> String {
    fs::read_to_string(
        CpdHome::resolve(Some(fx.env.cpd_home().into()), None)
            .unwrap()
            .watch_log(),
    )
    .unwrap_or_default()
}

fn standing_down_lines(fx: &Fx) -> Vec<String> {
    log(fx)
        .lines()
        .filter(|l| l.contains("standing down"))
        .map(str::to_owned)
        .collect()
}

/// Runs the engine until the migration `land` lands in the hook before nap [`MIGRATES_AT`], with the announced change
/// written right after it. Returns what the engine returned and how many naps it took.
fn run_with_migration_between_ticks(fx: &Fx, land: fn(&Path)) -> (Output, usize) {
    let home: PathBuf = fx.env.cpd_home();
    let clock = (*fx.fakes.clock).clone();
    let out = fx
        .watch(BOUND, move |nap| {
            if nap == MIGRATES_AT {
                land(&home);
                announce_a_change(&home, &clock);
            }
        })
        .unwrap();
    (out, fx.fakes.clock.naps().len())
}

#[test]
fn stand_down_between_ticks_runs_no_step_and_writes_nothing() {
    let fx = engine();

    let (out, naps) = run_with_migration_between_ticks(&fx, breaking);

    let txn = migration_txn(&fx, BREAKING);
    assert_eq!(out, Output::success(json!({"stopped": true})), "{out:?}");
    assert!(
        naps < BOUND,
        "the engine went on ticking on a store it cannot open: {naps} naps, the test's bound is {BOUND}"
    );
    assert_eq!(
        fx.env.query::<i64>(&format!(
            "SELECT count(*) FROM txns WHERE command = 'watch' AND id > {txn}"
        )),
        0,
        "a command `watch` wrote after the migration"
    );
    assert_eq!(
        items(&fx),
        0,
        "E8 ran after the migration and opened an Inbox item"
    );
    let after = fx.env.query::<i64>(&format!(
        "SELECT count(*) FROM events WHERE txn > {txn} AND kind IN ('error', 'watch-stopped')"
    ));
    assert_eq!(
        after, 0,
        "an `error` or `watch-stopped` Event was appended after the migration"
    );
}

#[test]
fn stand_down_during_a_tick_runs_no_later_step_and_reports_no_error() {
    let fx = Fx::with_ports();
    fx.engine_config("");
    fx.env.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');");
    // A later step (E8) would open an item at this tick.
    announce_a_change(&fx.env.cpd_home(), &*fx.fakes.clock);
    // The first rule that matches answers, so this one is put before `engine_routes`' tmux with no server: E2's listing is
    // where the migration lands (the matcher runs on the engine's thread, in the step) and the step fails (`tmux_failed`).
    let landed = Arc::new(AtomicBool::new(false));
    let (home, flag) = (fx.env.cpd_home(), landed.clone());
    let lists = move |call: &Call| {
        let lists = call.args.iter().any(|a| a == "list-panes");
        if lists && !flag.swap(true, Ordering::SeqCst) {
            breaking(&home);
        }
        lists
    };
    // What a tmux prints that the listing cannot parse.
    let _ = (*fx.fakes.runner)
        .clone()
        .on("tmux", lists, Reply::ok("%1_cpd_main_0_1234_x\n"));
    fx.engine_routes();

    let out = fx.watch(BOUND, |_| {}).unwrap();

    assert!(
        landed.load(Ordering::SeqCst),
        "E2 never listed the panes: the fixture does not reach the step it fails"
    );
    let naps = fx.fakes.clock.naps().len();
    assert_eq!(out, Output::success(json!({"stopped": true})), "{out:?}");
    assert!(
        naps < BOUND,
        "the engine went on ticking on a store it cannot open: {naps} naps, the test's bound is {BOUND}"
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM events WHERE kind = 'error'"),
        0,
        "the failure after the migration was reported"
    );
    assert_eq!(
        items(&fx),
        0,
        "a step after the failing one ran and opened an Inbox item"
    );
    assert_eq!(standing_down_lines(&fx).len(), 1, "{}", log(&fx));
}

#[test]
fn stand_down_leaves_the_lock_free_and_logs_one_line() {
    let fx = engine();

    run_with_migration_between_ticks(&fx, breaking);

    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.watch_lock())
        .unwrap();
    assert!(
        lock::try_lock(&file).unwrap(),
        "another process cannot take locks/watch.lock after the stand-down"
    );
    let lines = standing_down_lines(&fx);
    assert_eq!(lines.len(), 1, "{}", log(&fx));
    assert!(lines[0].contains("store_too_new"), "{}", lines[0]);
}

#[test]
fn stand_down_does_not_happen_for_an_additive_migration() {
    let fx = engine();

    let (out, naps) = run_with_migration_between_ticks(&fx, additive);

    assert_eq!(out, Output::success(json!({"stopped": true})), "{out:?}");
    assert_eq!(naps, BOUND, "the engine stopped before the test's bound");
    assert_eq!(items(&fx), 1, "a later tick did not run E8's write");
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-stopped'"),
        1
    );
    assert!(standing_down_lines(&fx).is_empty(), "{}", log(&fx));
}
