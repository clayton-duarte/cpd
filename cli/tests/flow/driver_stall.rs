//! The durable stall rule (contract §3.4 A4 "Liveness and restart, `durable`", hs-d798; §8.2 `timeouts.stall_minutes`), through the
//! driver in this process: while a `durable` Attempt runs, the driver watches the size of its transcript
//! `run/<plan>/durable/<attempt>.sqlite` (§6.12); a transcript that has not grown for `stall_minutes` appends ONE `error` Event —
//! source `driver`, code `worker_stalled`, `data.details = {attempt, minutes}` — and E8 opens ONE `engine-error` Inbox item (the
//! existing kind, §2.2) whose label names the remedy. One Event and one item per **stall episode**: a transcript that grows again
//! re-arms the watch, so a Worker that stalls twice appends twice, and a transcript touched every tick appends nothing.
//!
//! The harness is the durable-driver one of `worker_runner_durable.rs`: the runner is a process of the `FakeSpawn` (nothing runs),
//! the transcript is a file the test writes (the bytes the Worker's host would append), and time is the `ManualClock` — the driver's
//! poll naps on it, the hook acts between two polls, no sleep. The one Inbox item is E8's work, so the test runs one tick of the
//! engine ([`Fx::engine_tick`]) on the same fake ports and reads the `inbox_items` rows.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};

use super::driver::{JOB, PLAN, store};
use super::driver_open::{Live, submit};
use super::worker_runner_durable::durable_live;

/// The `timeouts.stall_minutes` these tests set: the lowest valid value (§8.2, an int of at least 1), so one stall episode is one
/// quiet minute of the `ManualClock` — 120 polls of the driver's 500 ms.
const STALL_MINUTES: u64 = 1;

/// `run/<plan>/durable/<attempt>.sqlite`: the transcript a durable Worker's host appends every turn (§6.12 `storePath`).
fn transcript(live: &Live, id: &str) -> PathBuf {
    live.fx.env.cpd_home().join("run").join(PLAN).join("durable").join(format!("{id}.sqlite"))
}

/// Appends a byte to the transcript, as the Worker's host does at every turn it writes: the file grows.
fn grow(live: &Live, id: &str) {
    let path = transcript(live, id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::OpenOptions::new().create(true).append(true).open(&path).unwrap().write_all(b"x").unwrap();
}

/// The `error` Events of Plan `pay` whose `data` carries the stall code: their `data`, in order.
fn stalled(live: &Live) -> Vec<Value> {
    store(&live.fx.env)
        .read(|c| {
            let mut stmt = c
                .prepare("SELECT data FROM events WHERE plan = 'pay' AND kind = 'error' AND json_extract(data, '$.code') = 'worker_stalled' ORDER BY id")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.iter().map(|d| serde_json::from_str(d).unwrap()).collect())
        })
        .unwrap()
}

/// The `engine-error` Inbox items of Plan `pay`: `(subject, label, closed_at)`, in order.
fn stall_items(live: &Live) -> Vec<(String, String, Option<String>)> {
    store(&live.fx.env)
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT subject, label, closed_at FROM inbox_items WHERE kind = 'engine-error' AND plan = 'pay' ORDER BY id")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap()
}

/// The one `error` Event of a stall episode: source `driver`, the details the contract names, and the Attempt's own.
fn assert_one_event(live: &Live, id: &str) {
    let events = stalled(live);
    assert_eq!(events.len(), 1, "one worker_stalled Event: {events:?}");
    let e = &events[0];
    assert_eq!(e["source"], "driver", "{e}");
    assert_eq!(e["details"], json!({"attempt": id, "minutes": STALL_MINUTES}), "{e}");
    let listed = live.events(id).into_iter().filter(|e| e["kind"] == "error" && e["data"]["code"] == "worker_stalled").count();
    assert_eq!(listed, 1, "the Event is the Attempt's: {:?}", live.events(id));
}

/// A Worker that commits `api.txt` and submits, as pi does at its end.
fn work(live: &Live, id: &str) -> Value {
    live.commit(&live.worktree(id), "api.txt", "api\n", "api");
    submit(live, id)
}

#[test]
fn a_quiet_durable_transcript_appends_one_worker_stalled_and_one_inbox_item() {
    let live = durable_live(&format!("[timeouts]\nstall_minutes = {STALL_MINUTES}\n"));
    // 0: the watch has not fired yet. 1: re-armed after the first episode. 2: both episodes seen, the Worker has submitted.
    let mut stage = 0;

    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            // The runner's host creates its transcript at its start; from here it is quiet.
            grow(live, id);
            return;
        }
        match stage {
            // Quiet, until the first stall episode: one Event, then E8's tick opens the one open item.
            0 => {
                if stalled(live).is_empty() {
                    return;
                }
                assert_one_event(live, id);
                live.fx.engine_tick();
                let items = stall_items(live);
                assert_eq!(items.len(), 1, "one engine-error item: {items:?}");
                let (subject, label, closed_at) = &items[0];
                assert_eq!(subject, &format!("error:driver/worker_stalled/{id}"), "{items:?}");
                assert!(label.contains(&format!("cpd job cancel {JOB}")), "the label names the remedy: {items:?}");
                assert_eq!(closed_at, &None, "the item is open while the Attempt runs: {items:?}");
                stage = 1;
                // The transcript grows again: the watch re-arms, and the second quiet minute is a second episode.
                grow(live, id);
            }
            // The second episode: a second Event, still the one open item (one per stalled Attempt, not per Event).
            1 => {
                if stalled(live).len() < 2 {
                    return;
                }
                assert_eq!(stalled(live).len(), 2, "one Event per stall episode: {:?}", stalled(live));
                live.fx.engine_tick();
                let items = stall_items(live);
                assert_eq!(items.len(), 1, "one item per stalled Attempt: {items:?}");
                assert_eq!(items[0].2, None, "the Attempt still runs: {items:?}");
                stage = 2;
                // The Worker submits: the Attempt ends, and the item closes with it.
                work(live, id);
            }
            _ => {}
        }
    });

    assert_eq!(stage, 2, "the drive ended at stage {stage}");
    let attempt = live.attempt(&id);
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(stalled(&live).len(), 2);
    // The engine's next reconcile closes the item: the Attempt has ended.
    live.fx.engine_tick();
    let items = stall_items(&live);
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].2.is_some(), "the item closed when the Attempt ended: {items:?}");
}

#[test]
fn a_growing_transcript_appends_nothing() {
    let live = durable_live(&format!("[timeouts]\nstall_minutes = {STALL_MINUTES}\n"));
    let mut submitted = false;

    // The transcript grows at every tick, as a Worker's does while it works; past a full stall window, the Worker submits.
    let (id, _) = live.drive(JOB, |live, id, nap| {
        grow(live, id);
        if nap >= 150 && !submitted {
            submitted = true;
            work(live, id);
        }
    });

    assert!(submitted, "the Worker never submitted");
    let attempt = live.attempt(&id);
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    // 150 polls of the driver's 500 ms are 75 s of clock: the drive covered the 60 s window, all of it growing.
    assert!(
        live.fx.fakes.clock.elapsed() >= Duration::from_secs(STALL_MINUTES * 60),
        "the drive lasted {:?}, not a full stall window",
        live.fx.fakes.clock.elapsed()
    );
    assert_eq!(stalled(&live), Vec::<Value>::new(), "a growing transcript appends no Event");
    live.fx.engine_tick();
    assert_eq!(stall_items(&live), Vec::<(String, String, Option<String>)>::new(), "and opens no item");
}
