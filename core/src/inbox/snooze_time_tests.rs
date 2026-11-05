//! `inbox snooze` words the time for a person in the clock's zone (EST5EDT); its fields stay UTC.

use std::fs;
use std::sync::Arc;

use clap::Parser;
use serde_json::Value;

use super::{Command, run_with};
use crate::clock::ManualClock;
use crate::home::CpdHome;
use crate::host::runner::FakeRunner;
use crate::host::spawn::FakeSpawn;
use crate::inbox::model::{Condition, Kind, Urgency, reconcile};
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Snoozes item 1 for an hour on a `ManualClock` (2026-01-01T00:00:00Z); returns the Event's text and data, and the output.
fn snooze() -> (String, Value, Value) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let condition = Condition {
        kind: Kind::Dialog,
        urgency: Urgency::Red,
        subject: "core-docs".into(),
        label: "inbox/core-docs failed".into(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
        head_oid: None,
        reviewed_oid: None,
    };
    reconcile(&mut store, &[condition], "2026-01-01T00:00:00.000Z").unwrap();
    let clock = ManualClock::new();
    let ports = Ports {
        clock: Arc::new(clock.clone()),
        runner: Arc::new(FakeRunner::new(clock)),
        spawn: Arc::new(FakeSpawn::new()),
        ..Ports::real()
    };
    // `Env::open_with` reads `$CPD_HOME`: the one variable this test sets, to its own temp dir.
    unsafe { std::env::set_var("CPD_HOME", home.root()) };
    let command = Cli::try_parse_from(["cpd", "snooze", "1", "1h"])
        .unwrap()
        .command;
    let Output::Success(fields) = run_with(&ports, command).unwrap() else {
        panic!("a success")
    };
    let (text, data): (String, String) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT text, data FROM events WHERE kind = 'inbox-snoozed'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    (
        text,
        serde_json::from_str(&data).unwrap(),
        Value::Object(fields),
    )
}

#[test]
fn snooze_text_names_the_local_time_and_zone() {
    let (text, _, _) = snooze();
    // 01:00 UTC on 2026-01-01 is 20:00 EST on 2025-12-31.
    assert_eq!(
        text,
        "Inbox item 1 snoozed until 2025-12-31 20:00 EST: inbox/core-docs failed."
    );
}

#[test]
fn snooze_field_stays_utc() {
    let (text, data, output) = snooze();
    // Red now (the fields are UTC already): the text beside the fields is in the zone, and the fields do not follow it.
    assert!(text.contains(" EST: "), "{text}");
    assert_eq!(data["until"], "2026-01-01T01:00:00.000Z");
    assert_eq!(output["item"]["snoozed_until"], "2026-01-01T01:00:00.000Z");
}
