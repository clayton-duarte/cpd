//! E9's curation tick (contract §3.7 E9): the engine starts `cpd context curate --context <ctx>` per Context with a due
//! record, backs off after a failed run, and defers to a held distill lock. Time is the `ManualClock`, the spawn the
//! `FakeSpawn`; no process runs.

use std::time::Duration;

use super::ports_tests::{CURATION, EXE, World, fx_on, tick_e9, with_a_record, world};
use super::testkit::Fx;
use cpd_core::context::curate_vectors::VectorStore;
use cpd_core::context::distill::Lock;
use cpd_core::host::spawn::{Launch, Pid};
use cpd_core::ids::{ContextId, RecordId};

const SECS: fn(u64) -> Duration = Duration::from_secs;

fn ctx() -> ContextId {
    "ctx".parse().unwrap()
}

/// A world and an engine whose Context `ctx` holds a record written now: due, and too young for the distiller.
fn due() -> (World, Fx) {
    let w = world();
    let mut fx = fx_on(&w, CURATION);
    with_a_record(&mut fx, &w.stamp(0));
    (w, fx)
}

/// The launches of `cpd context curate` so far (the distiller's are not these).
fn curates(w: &World) -> Vec<Launch> {
    w.launches()
        .into_iter()
        .filter(|l| l.args.get(1).map(String::as_str) == Some("curate"))
        .collect()
}

/// The only curate launch.
fn only(w: &World) -> Launch {
    let launches = curates(w);
    assert_eq!(launches.len(), 1, "one curate launch: {launches:?}");
    launches.into_iter().next().unwrap()
}

fn curate_args(context: &str) -> Vec<String> {
    ["context", "curate", "--context", context]
        .map(str::to_owned)
        .to_vec()
}

#[test]
fn curate_starts_cpd_context_curate_for_a_context_with_a_due_record() {
    let (w, mut fx) = due();
    let home = fx.engine.home.clone();

    tick_e9(&mut fx);

    let launch = only(&w);
    assert_eq!(launch.program, EXE);
    assert_eq!(launch.args, curate_args("ctx"));
    assert!(launch.setsid);
    assert_eq!(launch.cwd.as_deref(), Some(home.root()));
    assert!(
        launch.env_set.contains(&(
            "CPD_HOME".to_owned(),
            home.root().to_string_lossy().into_owned()
        )),
        "{:?}",
        launch.env_set
    );
    for removed in [
        "CPD_PLAN",
        "CPD_SESSION",
        "CPD_ATTEMPT",
        "TMUX",
        "TMUX_PANE",
    ] {
        assert!(
            launch.env_remove.iter().any(|r| r == removed),
            "{removed}: {:?}",
            launch.env_remove
        );
    }
}

#[test]
fn curate_records_the_pid_in_curate_pid_and_logs_to_curate_log() {
    let (w, mut fx) = due();
    let dir = fx.engine.home.root().join("run/context-digests/ctx");

    tick_e9(&mut fx);

    assert_eq!(
        std::fs::read_to_string(dir.join("curate.pid")).unwrap(),
        "1000\n"
    );
    assert_eq!(only(&w).stdout_to, Some(dir.join("curate.log")));
}

#[test]
fn curate_does_nothing_with_the_flag_off() {
    let w = world();
    let mut fx = fx_on(
        &w,
        &format!("{CURATION}[experimental]\nwrite_curation = false\n"),
    );
    with_a_record(&mut fx, &w.stamp(0));
    let home = fx.engine.home.clone();

    tick_e9(&mut fx);

    assert!(curates(&w).is_empty(), "{:?}", curates(&w));
    assert!(!home.vectors_db().exists());

    // Control: with the flag on the same Context is started.
    let config = std::fs::read_to_string(home.config()).unwrap();
    std::fs::write(home.config(), config.replace("false", "true")).unwrap();
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 1, "the flag on starts it");
}

#[test]
fn curate_starts_nothing_when_no_record_is_due() {
    // The only record is checked.
    let (w, mut fx) = due();
    let home = fx.engine.home.clone();
    VectorStore::at(&home)
        .mark_checked(&ctx(), &RecordId::parse("m1-n1").unwrap(), "t")
        .unwrap();
    tick_e9(&mut fx);
    assert!(curates(&w).is_empty(), "checked: {:?}", curates(&w));
    // Control: a new unchecked record is due.
    fx.fixture(&format!("INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n2', 'note', '{}', 'm1', 'New');", w.stamp(0)));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 1, "an unchecked record is due");

    // The only record is older than 24 hours.
    let w = world();
    let mut fx = fx_on(&w, CURATION);
    with_a_record(&mut fx, &w.stamp(-25 * 3600 * 1000));
    tick_e9(&mut fx);
    assert!(curates(&w).is_empty(), "old: {:?}", curates(&w));
}

#[test]
fn curate_starts_no_second_run_while_the_first_has_not_exited() {
    let (w, mut fx) = due();

    tick_e9(&mut fx);
    tick_e9(&mut fx);
    tick_e9(&mut fx);

    assert_eq!(curates(&w).len(), 1);
}

#[test]
fn a_failed_run_backs_off_one_minute_then_two() {
    let (w, mut fx) = due();
    tick_e9(&mut fx);
    w.spawn.exit(Pid(1000), 1);

    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 1, "no launch right after the failure");
    w.advance(SECS(59));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 1, "59 s is too early");
    w.advance(SECS(1));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 2, "60 s is due");

    w.spawn.exit(Pid(1001), 1);
    tick_e9(&mut fx);
    w.advance(SECS(119));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 2, "119 s is too early");
    w.advance(SECS(1));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 3, "120 s is due");
}

#[test]
fn a_successful_run_resets_the_backoff() {
    let (w, mut fx) = due();
    tick_e9(&mut fx);
    w.spawn.exit(Pid(1000), 1);
    tick_e9(&mut fx);
    w.advance(SECS(60));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 2);

    w.spawn.exit(Pid(1001), 0);
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 3, "a success owes no wait");

    w.spawn.exit(Pid(1002), 1);
    tick_e9(&mut fx);
    w.advance(SECS(59));
    tick_e9(&mut fx);
    assert_eq!(
        curates(&w).len(),
        3,
        "the next failure waits 60 s again: 59 s"
    );
    w.advance(SECS(1));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 4, "60 s, not 120 s");
}

#[test]
fn a_held_distill_lock_defers_the_run_without_a_backoff() {
    let (w, mut fx) = due();
    let lock = Lock::take(&fx.engine.home, &ctx()).unwrap();

    tick_e9(&mut fx);
    assert!(curates(&w).is_empty(), "{:?}", curates(&w));
    assert!(fx.engine.curate_backoff.is_empty());

    drop(lock);
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 1, "the next tick starts it");
}

#[test]
fn the_backoff_is_per_context() {
    let (w, mut fx) = due();
    fx.fixture("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx2', 'Ctx2', 'acme', 't', 'm1');");
    fx.fixture(&format!("INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx2', 'm1-n1', 'note', '{}', 'm1', 'A note');", w.stamp(0)));
    tick_e9(&mut fx);
    let launches = curates(&w);
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert_eq!(launches[0].args, curate_args("ctx"));
    assert_eq!(launches[1].args, curate_args("ctx2"));

    w.spawn.exit(Pid(1000), 1);
    w.spawn.exit(Pid(1001), 0);
    tick_e9(&mut fx);

    let launches = curates(&w);
    assert_eq!(launches.len(), 3, "{launches:?}");
    assert_eq!(launches[2].args, curate_args("ctx2"));
}

#[test]
fn a_context_under_the_floor_gets_no_curate_run() {
    let w = world();
    let mut fx = fx_on(&w, "context_curation_min_records = 2\n");
    with_a_record(&mut fx, &w.stamp(0));

    tick_e9(&mut fx);
    assert!(curates(&w).is_empty(), "{:?}", curates(&w));

    // Control: a second record reaches the floor.
    fx.fixture(&format!("INSERT INTO context_records (context, id, kind, at, machine, text) VALUES ('ctx', 'm1-n2', 'note', '{}', 'm1', 'Another');", w.stamp(0)));
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 1, "over the floor it starts");
}

#[test]
fn curate_starts_nothing_while_incident_mode_holds() {
    let (w, mut fx) = due();
    fx.fixture("INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn());");

    tick_e9(&mut fx);

    assert!(curates(&w).is_empty(), "{:?}", curates(&w));

    // Control: the incident ended.
    fx.fixture("DELETE FROM incident;");
    tick_e9(&mut fx);
    assert_eq!(curates(&w).len(), 1, "without the incident it starts");
}
