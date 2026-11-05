//! Characterization (pro-d408): a refused `inbox snooze` writes nothing and leaves its row as it was (scout-actions §5).

use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::{Value, json};

use super::cmd::{Command, run_with};
use super::model::{Condition, Kind, Urgency, reconcile};
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};
use crate::host::spawn::FakeSpawn;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// A temp home holding item 1 (`kind`, `subject`) and the ports of `runner`.
/// `Env::open_with` reads `$CPD_HOME`: the one variable these tests set, to their own temp dir.
fn fixture(
    kind: Kind,
    subject: &str,
    runner: FakeRunner,
) -> (tempfile::TempDir, CpdHome, Store, Ports) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let condition = Condition {
        kind,
        urgency: Urgency::Yellow,
        subject: subject.into(),
        label: "acme/pay#5 wants a review".into(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    };
    reconcile(&mut store, &[condition], "2026-01-01T00:00:00.000Z").unwrap();
    let ports = Ports {
        clock: Arc::new(ManualClock::new()),
        runner: Arc::new(runner),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    unsafe { std::env::set_var("CPD_HOME", home.root()) };
    (dir, home, store, ports)
}

fn run(ports: &Ports, argv: &[&str]) -> Result<Output, CpdError> {
    let command = Cli::try_parse_from(std::iter::once("cpd").chain(argv.iter().copied()))
        .unwrap()
        .command;
    run_with(ports, command)
}

fn refused(out: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    let e = out.expect_err("an error");
    assert_eq!(e.code(), code, "{e}");
    e
}

/// Every row of `inbox_items` and the counts of Events and transactions: what a refusal must leave as it was.
fn snapshot(store: &mut Store) -> Value {
    store
        .read(|c| {
            let mut s = c.prepare(
                "SELECT id, kind, subject, closed_at, snoozed_until, dismissed_at, claimed_at FROM inbox_items ORDER BY id",
            )?;
            let rows = s
                .query_map([], |r| {
                    Ok(json!([
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?
                    ]))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let events: i64 = c.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
            let txns: i64 = c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?;
            Ok(json!({"rows": rows, "events": events, "txns": txns}))
        })
        .unwrap()
}

const OFFSET_FAILS: fn() -> FakeRunner = || {
    FakeRunner::new(ManualClock::new()).on("date", |_| true, Reply::exit(1, "", "date: broken\n"))
};

#[test]
fn a_refused_snooze_of_an_unknown_item_is_item_not_found_and_changes_no_row() {
    let (_d, _h, mut store, ports) = fixture(
        Kind::Dialog,
        "core-docs",
        FakeRunner::new(ManualClock::new()),
    );
    let before = snapshot(&mut store);
    let e = refused(
        run(&ports, &["snooze", "99", "1h"]),
        ErrorCode::ItemNotFound,
    );
    assert_eq!(e.exit().code(), 4);
    assert_eq!(snapshot(&mut store), before);
}

#[test]
fn a_refused_snooze_of_a_closed_item_is_item_closed_and_leaves_the_row_unsnoozed() {
    let (_d, _h, mut store, ports) = fixture(
        Kind::Dialog,
        "core-docs",
        FakeRunner::new(ManualClock::new()),
    );
    store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE inbox_items SET closed_at = '2026-01-01T01:00:00.000Z' WHERE id = 1",
                [],
            )?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    let before = snapshot(&mut store);
    let e = refused(run(&ports, &["snooze", "1", "15m"]), ErrorCode::ItemClosed);
    assert_eq!(e.exit().code(), 3);
    assert_eq!(snapshot(&mut store), before);
}

#[test]
fn a_refused_snooze_until_tomorrow_whose_offset_fails_is_spawn_failed_and_changes_no_row() {
    let (_d, _h, mut store, ports) = fixture(Kind::Dialog, "core-docs", OFFSET_FAILS());
    let before = snapshot(&mut store);
    let e = refused(
        run(&ports, &["snooze", "1", "tomorrow"]),
        ErrorCode::SpawnFailed,
    );
    assert_eq!(e.exit().code(), 1);
    assert_eq!(snapshot(&mut store), before);
}
