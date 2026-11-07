//! Red tests of `context distill`'s re-ask, non-silent clamp and empty-reply smaller-batch retry
//! (Plan distill-fix, commands.md §3.12).
//!
//! They drive `Run::run` end to end on a `FakeRunner` and a `ManualClock`: no real pi, no process,
//! no sleep. `distill::run_pi_with` builds the `pi` call and hands it to the runner, so scripting the
//! runner's replies is the whole seam.

use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use super::{Run, State};
use crate::clock::ManualClock;
use crate::config::Config;
use crate::context::{By, Draft, Record};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};
use crate::ids::{ContextId, RecordId};
use crate::ports::Ports;
use crate::store::Store;

/// A Context with a few in-force records, its `State`, and the temp dir that holds it.
struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
    context: ContextId,
    state: State,
}

fn record(id: &str, text: &str) -> Record {
    Record {
        id: RecordId::parse(id).unwrap(),
        at: "2026-10-01T10:00:00.000Z".into(),
        by: By {
            session: None,
            plan: None,
        },
        text: text.into(),
        draft: Draft::Note,
    }
}

fn fixture(records: &[(&str, &str)]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let context = ContextId::parse("ctx").unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'C', '2026-11-01T00:00:00.000Z', 'hs');",
            )?;
            for (id, text) in records {
                record(id, text).insert(tx, &context)?;
            }
            Ok(())
        })
        .unwrap();
    let state = store.read(|conn| State::read(conn, &context)).unwrap();
    Fixture {
        _dir: dir,
        home,
        store,
        context,
        state,
    }
}

fn config() -> Config {
    Config::parse(
        "version = 1\nmachine = \"test\"\n",
        Path::new("config.toml"),
    )
    .unwrap()
    .0
}

/// The `Ports` over `runner` on `clock`: the pi seam, no process.
fn ports(runner: Arc<FakeRunner>, clock: ManualClock) -> Ports {
    let mut ports = Ports::real();
    ports.clock = Arc::new(clock);
    ports.runner = runner;
    ports
}

/// The whole run through its private `Run`, with `ports` ambient so the grep seam sees the fake too.
fn run(f: &mut Fixture, ports: &Ports, batch: usize) -> Result<Value, CpdError> {
    let config = config();
    Ports::scope(ports, || {
        let run = Run {
            home: &f.home,
            config: &config,
            store: &mut f.store,
            context: &f.context,
            ports,
        };
        run.run(&f.state, batch)
    })
}

/// A pi `--mode json` stream whose final assistant message holds `answer`.
fn pi_stream(answer: &str) -> String {
    serde_json::json!({
        "type": "message_end",
        "message": {
            "role": "assistant",
            "content": [{"type": "text", "text": answer}],
            "usage": {"input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0, "cost": {"total": 0.0}},
            "stopReason": "stop",
        }
    })
    .to_string()
        + "\n"
}

/// One curation line of the distiller's answer.
fn line(id: &str, topic: &str, short: &str) -> String {
    serde_json::json!({"id": id, "topic": topic, "short": short, "retire": null}).to_string()
}

/// A stream whose answer is one curation line for `id`.
fn answer_line(id: &str, short: &str) -> String {
    pi_stream(&format!("{}\n", line(id, "topic", short)))
}

fn id(text: &str) -> RecordId {
    RecordId::parse(text).unwrap()
}

#[test]
fn distill_retry_reasks_a_failing_short() {
    // A first answer whose `short` is over the 120-byte limit is rejected, so the model is asked again
    // for that record; the second answer is valid and the run ends with the record curated.
    let mut f = fixture(&[("hs-n1", "Do the thing.")]);
    let clock = ManualClock::new();
    let over = answer_line("hs-n1", &"word ".repeat(30));
    let good = answer_line("hs-n1", "do the thing");
    let runner = Arc::new(
        FakeRunner::new(clock.clone())
            .once("pi", |_| true, Reply::ok(&over))
            .on("pi", |_| true, Reply::ok(&good)),
    );
    let ports = ports(runner.clone(), clock);
    let result = run(&mut f, &ports, 1).expect("the run ends valid after the re-ask");
    assert_eq!(
        runner.calls().len(),
        2,
        "pi is asked again for the record whose short was rejected"
    );
    assert_eq!(result["curated"], 1);
    let after = f.store.read(|conn| State::read(conn, &f.context)).unwrap();
    assert!(
        after.curated.contains_key(&id("hs-n1")),
        "the re-asked record is curated: {:?}",
        after.curated
    );
}

#[test]
fn distill_retry_clamps_an_over_limit_short() {
    // Every answer is over the limit: after at most two re-asks the short is clamped, never silent, and
    // the run ends valid with a one-line short of at most 120 bytes.
    let mut f = fixture(&[("hs-n1", "Do the thing.")]);
    let clock = ManualClock::new();
    let over = answer_line("hs-n1", &"word ".repeat(30));
    let runner = Arc::new(FakeRunner::new(clock.clone()).on("pi", |_| true, Reply::ok(&over)));
    let ports = ports(runner.clone(), clock);
    let result = run(&mut f, &ports, 1).expect("the run ends valid once the short is clamped");
    assert_eq!(
        runner.calls().len(),
        3,
        "one call, then at most two re-asks, then the clamp"
    );
    assert_eq!(result["curated"], 1);
    let clamped = result["clamped"].as_array().expect("a clamped array");
    assert_eq!(clamped.len(), 1, "one clamp: {clamped:?}");
    assert_eq!(clamped[0]["id"], "hs-n1");
    let bytes = clamped[0]["bytes"].as_u64().unwrap();
    assert!((1..=120).contains(&bytes), "clamped to {bytes} bytes");
    let after = f.store.read(|conn| State::read(conn, &f.context)).unwrap();
    let short = &after.curated[&id("hs-n1")].1;
    assert!(
        short.len() <= 120 && !short.contains('\n'),
        "the curated short is {short:?}"
    );
}

#[test]
fn distill_retry_retries_an_empty_reply_with_a_smaller_batch() {
    // An empty final assistant message gets exactly one retry with a smaller batch; the valid answer
    // ends the run.
    let mut f = fixture(&[("hs-n1", "Do the thing."), ("hs-n2", "Do another thing.")]);
    let clock = ManualClock::new();
    let empty = pi_stream("");
    let both = pi_stream(&format!(
        "{}\n{}\n",
        line("hs-n1", "topic", "do the thing"),
        line("hs-n2", "topic", "do another thing")
    ));
    let runner = Arc::new(
        FakeRunner::new(clock.clone())
            .once("pi", |_| true, Reply::ok(&empty))
            .on("pi", |_| true, Reply::ok(&both)),
    );
    let ports = ports(runner.clone(), clock);
    let result = run(&mut f, &ports, 2).expect("the smaller-batch retry ends valid");
    assert_eq!(runner.calls().len(), 2, "exactly one smaller-batch retry");
    assert!(
        result["curated"].as_u64().unwrap() >= 1,
        "the smaller batch is curated: {}",
        result["curated"]
    );
}

#[test]
fn distill_retry_reports_one_failure_after_two_empty_replies() {
    // Two empty replies: the one retry is used up, the run fails once, and no more pi is called.
    let mut f = fixture(&[("hs-n1", "Do the thing."), ("hs-n2", "Do another thing.")]);
    let clock = ManualClock::new();
    let empty = pi_stream("");
    let runner = Arc::new(FakeRunner::new(clock.clone()).on("pi", |_| true, Reply::ok(&empty)));
    let ports = ports(runner.clone(), clock);
    let error = run(&mut f, &ports, 2).expect_err("two empty replies fail the run");
    assert_eq!(
        runner.calls().len(),
        2,
        "one retry, then the failure: no third call"
    );
    assert_eq!(error.code(), ErrorCode::DistillFailed);
    let reported: i64 = f
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'distill_failed'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(reported, 1, "exactly one distill_failed event");
}
