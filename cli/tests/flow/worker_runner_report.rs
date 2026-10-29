//! The report half of the Worker runner seam for `durable` (contract §3.4 "The Worker runner and its seam": The end report and A5 from
//! the report; §6.12 `EndReport`), through the driver in this process. For a durable Attempt the driver reads the end report JSON at
//! `run/<plan>/attempts/<attempt>.report.json`, never pi's session file: A5 (the Worker is `idle`: judge the report, nudge once, then
//! `no_submit`, `provider_error`, `crashed`) and A9's `metrics` (`duration_s` the wall seconds since the Attempt's `started_at`,
//! `thinking_level` null). A report that is absent while the Worker is idle is waited for; one that exists and does not parse is a
//! contract violation of the runner, which ends the Attempt `crashed` naming the file and the parse error.
//!
//! The runner is a process of the `FakeSpawn` (pid 1001: the driver is 1000) and the report is the file the test writes, as the real
//! runner would after a settle; the Worker's Session state is set with `cpd session state`, as the runner's adapter does.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use cpd_core::host::spawn::Pid;
use serde_json::{Value, json};

use super::driver::{JOB, PLAN};
use super::driver_open::{Live, submit};
use super::worker_runner_durable::{durable_live, report, write_report};

/// The runner the driver started, as its stop reads it: the command line a guarded signal compares (from the OS in production), so the
/// stop before a merge or after a failure reaches it as it would a real runner. That the driver starts the runner at all is the start's
/// tests' concern (`worker_runner_durable`), not this file's: with no runner launched there is nothing to register.
fn register_runner(live: &Live) {
    if let Some(runner) = live.fx.fakes.spawn.launches().get(1) {
        let line = std::iter::once(runner.program.as_str()).chain(runner.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
        live.fx.fakes.spawn.set_cmdline(Pid(1001), &line);
    }
}

/// The Worker's Session of Attempt `id`.
fn session_of(live: &Live, id: &str) -> String {
    live.value(&format!("SELECT worker_session FROM attempts WHERE plan = 'pay' AND id = '{id}'"))
}

/// What the runner's adapter does: `cpd session state <state>` with the Worker's Session.
fn state(live: &Live, id: &str, state: &str) {
    live.fx.cpd(&["session", "state", state, "--session", &session_of(live, id)]).ok();
}

/// The report file of Attempt `id`.
fn report_file(live: &Live, id: &str) -> PathBuf {
    live.fx.env.cpd_home().join("run").join(PLAN).join("attempts").join(format!("{id}.report.json"))
}

/// The nudges the driver sent for Job `api`.
fn nudges(live: &Live) -> i64 {
    live.value("SELECT count(*) FROM events WHERE plan = 'pay' AND job = 'api' AND kind = 'attempt-nudge'")
}

fn assert_ended(live: &Live, id: &str, outcome: &str, cause: &str) {
    let attempt = live.attempt(id);
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!(outcome), &json!(cause)), "{attempt}\nevents: {:#?}", live.events(id));
}

/// Every Event about Attempt `id`, as text: where the detail of an end is, whichever Event carries it.
fn events_text(live: &Live, id: &str) -> String {
    Value::Array(live.events(id)).to_string()
}

#[test]
fn a_durable_worker_that_submits_ends_merged_with_metrics_from_its_report() {
    let live = durable_live("");
    let before = live.fx.fakes.clock.elapsed();
    let long = Duration::from_secs(600);

    // The runner settled: its report has the turns, the tokens and the cost. The Worker commits and submits, 10 minutes into the Attempt.
    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap != 1 {
            return;
        }
        register_runner(live);
        write_report(
            live,
            id,
            &json!({
                "stop_reason": "stop", "error_message": null, "turns": 3, "tokens_in": 1200, "tokens_out": 340,
                "cache_read": 5000, "cache_write": 210, "cost_usd": 0.0421, "model": "p/sonnet"
            }),
        );
        live.fx.fakes.clock.advance(long);
        live.commit(&live.worktree(id), "api.txt", "api\n", "api");
        submit(live, id);
    });
    let waited = live.fx.fakes.clock.elapsed() - before;

    assert_ended(&live, &id, "merged", "merged");
    let metrics = live.attempt(&id)["metrics"].clone();
    assert!(metrics.is_object(), "A9 records the metrics of the report: {metrics}");
    let duration = metrics["duration_s"].as_f64().unwrap_or_else(|| panic!("no duration_s in {metrics}"));
    // The wall seconds of the Attempt on the ManualClock: at least the 10 minutes the test let pass, at most all the clock moved.
    assert!(duration >= long.as_secs_f64() && duration <= waited.as_secs_f64(), "duration_s {duration} is not within [{}, {}]: {metrics}", long.as_secs(), waited.as_secs());
    let mut rest = metrics.clone();
    rest.as_object_mut().unwrap().remove("duration_s");
    assert_eq!(
        rest,
        json!({
            "turns": 3, "tokens_in": 1200, "tokens_out": 340, "cache_read": 5000, "cache_write": 210, "cost_usd": 0.0421,
            "model": "p/sonnet", "thinking_level": null, "stop_reason": "stop"
        })
    );
}

#[test]
fn an_idle_durable_worker_is_nudged_once_by_its_report_then_ends_no_submit() {
    let live = durable_live("");

    let (id, _) = live.drive(JOB, |live, id, nap| match nap {
        // The runner is idle and has left no report yet: the driver waits for it, and judges nothing.
        1 => {
            register_runner(live);
            state(live, id, "idle");
            assert!(!report_file(live, id).exists());
        }
        // Still no report: no nudge, the Attempt runs on. Then the turn's report lands (one turn, `stop`, no submit).
        2 => {
            assert_eq!(nudges(live), 0, "an absent report while idle is waited for, not judged");
            assert_eq!(live.attempt(id)["status"], "running", "{}", live.attempt(id));
            write_report(live, id, &report("stop", 1));
        }
        // The nudge came; the Worker answers it with a second turn and is idle again.
        3 => {
            assert_eq!(nudges(live), 1, "events: {:#?}", live.events(id));
            state(live, id, "working");
            write_report(live, id, &report("stop", 2));
            state(live, id, "idle");
        }
        _ => {}
    });

    assert_ended(&live, &id, "failed", "no_submit");
    let attempt = live.attempt(&id);
    assert_eq!(attempt["nudged"], true, "{attempt}");
    let nudge = live.events(&id).into_iter().find(|e| e["kind"] == "attempt-nudge").expect("an attempt-nudge Event");
    assert_eq!(nudge["session"], attempt["worker_session"], "{nudge}");
    assert!(nudge["data"]["prompt"].as_str().unwrap().starts_with("You ended your turn without running cpd attempt submit api-a1."), "{nudge}");
    assert_eq!(nudges(&live), 1, "nudged once");
    let retry = live.events(&id).into_iter().find(|e| e["kind"] == "attempt-retry").expect("an attempt-retry Event");
    assert_eq!(retry["data"]["cause"], "no_submit", "{retry}");
}

#[test]
fn a_provider_error_in_the_end_report_ends_the_attempt_provider_error() {
    let live = durable_live("");

    // The last turn stopped on a provider's connection error: the same words as pi's `errorMessage`, in the report's `error_message`.
    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            register_runner(live);
            let mut stopped = report("error", 1);
            stopped["error_message"] = json!("Connection error.");
            write_report(live, id, &stopped);
            state(live, id, "idle");
        }
    });

    assert_ended(&live, &id, "failed", "provider_error");
    // Not counted as a try, not nudged, and the Job is queued again at the SAME Tier.
    assert_eq!(nudges(&live), 0, "a provider error is not nudged");
    assert_eq!(live.job_status(JOB), "queued");
    let tier = live.attempt(&id)["tier"].clone();
    let retry = live.events(&id).into_iter().find(|e| e["kind"] == "attempt-retry").expect("an attempt-retry Event");
    assert_eq!(retry["data"]["cause"], "provider_error", "{retry}");
    assert_eq!(retry["data"]["next_tier"], tier, "{retry}");
}

#[test]
fn a_malformed_end_report_ends_the_attempt_crashed_and_names_the_file() {
    let live = durable_live("");
    // The runner writes atomically: a file that does not parse is not a half-written one, it is the runner's fault. Cut off after a key.
    let text = r#"{"stop_reason": "stop", "turns":"#;
    let parse_error = serde_json::from_str::<Value>(text).unwrap_err().to_string();

    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            register_runner(live);
            let file = report_file(live, id);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
            state(live, id, "idle");
        }
    });

    assert_ended(&live, &id, "failed", "crashed");
    let file = report_file(&live, &id).display().to_string();
    let detail = events_text(&live, &id);
    assert!(detail.contains(&file), "the detail names the report file {file}: {detail}");
    assert!(detail.contains(&parse_error), "the detail holds the parse error `{parse_error}`: {detail}");
    assert_eq!(nudges(&live), 0, "a malformed report is not judged, so nothing is nudged");
}
