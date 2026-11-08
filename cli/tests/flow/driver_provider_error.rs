//! The `provider_error` cause and its same-Tier retry rule (contract §3.4 A5, §3.2, §9; pro-d102): a pi stop whose last assistant
//! message has `stopReason` "error" and an `errorMessage` that is "Connection error.", "Request timed out." or starts with an HTTP
//! 5xx status ends the Attempt `failed` / `provider_error`, and the Job is queued again at the SAME Tier, at most 3 times since its
//! last `job retry`, apart from `retry_cap`. Every other stop error stays `crashed`. The driver runs in-process (the harness of
//! `driver.rs`); the Worker's pi stop is the session file the test writes and the Session's `idle` state.

use std::fs;

use cpd_core::attempt::ledger::{self, Cause, End, Ladder, Stage, Stored};
use cpd_core::config::{Catalog, Tier, TierModel};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::plan::{jobs, locked_fingerprint};
use serde_json::{Value, json};

use super::driver::{JOB, PLAN, fixture, store};
use super::driver_open::{Live, live, submit};

const CONNECTION: &str = "Connection error.";

/// A `live` env whose Job `api` is at Tier S1 and whose `retry_cap` is 2.
fn live_s1() -> Live {
    let live = live("retry_cap = 2");
    set_job_tier(&live, "S1");
    live
}

fn set_job_tier(live: &Live, tier: &str) {
    // A Locked Job's Tier cannot change (the `jobs_locked` trigger): the trigger is dropped for the update and created again as it was.
    let trigger: String =
        live.value("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'jobs_locked'");
    fixture(
        &live.fx.env,
        &format!(
            "DROP TRIGGER jobs_locked; UPDATE jobs SET tier = '{tier}' WHERE plan = 'pay' AND key = 'api'; {trigger};"
        ),
    );
    let plan = PlanId::parse(PLAN).unwrap();
    let all = store(&live.fx.env)
        .read(|conn| jobs::load(conn, &plan))
        .unwrap();
    let job = all.iter().find(|j| j.key == JOB).unwrap();
    fixture(
        &live.fx.env,
        &format!(
            "UPDATE jobs SET fingerprint = '{}' WHERE plan = 'pay' AND key = 'api';",
            locked_fingerprint(job)
        ),
    );
}

/// pi stops: the session file of Attempt `id`'s Worker ends with an assistant message with `stop_reason` (and `error`, when given as
/// the `errorMessage`), and the Worker's Session is `idle`.
fn stop(live: &Live, id: &str, stop_reason: &str, error: Option<&str>) {
    let sid: String = live.value(&format!(
        "SELECT worker_session FROM attempts WHERE plan = 'pay' AND id = '{id}'"
    ));
    let mut message = json!({
        "role": "assistant",
        "usage": {"input": 1, "output": 1, "cacheRead": 0, "cacheWrite": 0, "cost": {"total": 0}},
        "stopReason": stop_reason,
    });
    if let Some(error) = error {
        message["errorMessage"] = json!(error);
    }
    let line =
        json!({"type": "message", "timestamp": "2026-10-27T14:03:54.000Z", "message": message});
    let dir = live.fx.env.home().join(".pi/agent/sessions/w");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("2026_{sid}.jsonl")),
        format!(
            "{}\n{line}\n",
            r#"{"type":"session","timestamp":"2026-10-27T14:03:50.000Z"}"#
        ),
    )
    .unwrap();
    live.fx
        .cpd(&["session", "state", "idle", "--session", &sid])
        .ok();
}

/// The Attempt the driver ran for the next dispatch of Job `api`, whose Worker's pi stops with `error` at its first nap.
fn drive_stop(live: &Live, stop_reason: &'static str, error: Option<&'static str>) -> String {
    live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            stop(live, id, stop_reason, error);
        }
    })
    .0
}

fn provider_stop(live: &Live) -> String {
    drive_stop(live, "error", Some(CONNECTION))
}

fn events_of(live: &Live, id: &str) -> Vec<Value> {
    live.events(id)
}

fn event_of(live: &Live, id: &str, kind: &str) -> Value {
    events_of(live, id)
        .into_iter()
        .find(|e| e["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} for {id}"))
}

fn count_of(live: &Live, kind: &str) -> usize {
    live.value::<i64>(&format!(
        "SELECT count(*) FROM events WHERE plan = 'pay' AND job = 'api' AND kind = '{kind}'"
    )) as usize
}

fn assert_provider_error(live: &Live, id: &str) {
    let attempt = live.attempt(id);
    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("failed"), &json!("provider_error")),
        "{attempt}"
    );
}

fn attempt_count(live: &Live) -> i64 {
    live.value("SELECT count(*) FROM attempts WHERE plan = 'pay' AND job = 'api'")
}

fn failed_cause(live: &Live) -> Option<String> {
    live.value("SELECT failed_cause FROM jobs WHERE plan = 'pay' AND key = 'api'")
}

#[test]
fn three_provider_error_stops_then_success_keep_one_job_at_the_same_tier() {
    let live = live_s1();

    for n in 1..=3 {
        let id = provider_stop(&live);
        assert_eq!(id, format!("api-a{n}"));
        assert_provider_error(&live, &id);
        assert_eq!(live.job_status(JOB), "queued");
        let retry = event_of(&live, &id, "attempt-retry");
        assert_eq!(retry["data"]["next_tier"], "S1", "{retry}");
    }
    let (attempt, _) = live.run_job(JOB, |live, id| {
        live.commit(&live.worktree(id), "api.txt", "api\n", "api");
        submit(live, id)
    });

    assert_eq!(attempt["outcome"], "merged", "{attempt}");
    assert_eq!(live.job_status(JOB), "merged");
    assert_eq!(count_of(&live, "job-failed"), 0);
    assert_eq!(attempt_count(&live), 4);
    let tiers: Vec<String> = (1..=4)
        .map(|n| {
            live.value(&format!(
                "SELECT tier FROM attempts WHERE plan = 'pay' AND id = 'api-a{n}'"
            ))
        })
        .collect();
    assert_eq!(tiers, ["S1"; 4]);
}

#[test]
fn a_fourth_provider_error_fails_the_job_with_cause_provider_error() {
    let live = live_s1();

    let ids: Vec<String> = (0..4).map(|_| provider_stop(&live)).collect();

    assert_eq!(ids, ["api-a1", "api-a2", "api-a3", "api-a4"]);
    assert_provider_error(&live, "api-a4");
    assert_eq!(live.job_status(JOB), "failed");
    assert_eq!(failed_cause(&live).as_deref(), Some("provider_error"));
    assert_eq!(count_of(&live, "job-failed"), 1);
    assert_eq!(attempt_count(&live), 4);
}

#[test]
fn provider_errors_use_no_try_of_the_retry_cap_and_a_later_crash_retries_the_same_tier() {
    let live = live_s1();
    for _ in 0..3 {
        provider_stop(&live);
    }

    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            let window = format!("{PLAN}--{id}");
            let pane = live
                .fx
                .panes()
                .into_iter()
                .find(|p| p.window_name == window)
                .expect("the Worker's pane");
            assert!(
                live.fx
                    .tmux(&["__ctl", "pane-exit", &pane.pane, "--status", "4"])
                    .status
                    .success()
            );
        }
    });

    assert_eq!(live.attempt(&id)["cause"], "crashed");
    assert_eq!(live.job_status(JOB), "queued");
    assert_eq!(
        event_of(&live, &id, "attempt-retry")["data"]["next_tier"],
        "S1"
    );
    assert_eq!(failed_cause(&live), None);
}

#[test]
fn provider_error_is_told_apart_from_other_stop_errors() {
    // (stopReason, errorMessage, a provider error?)
    let table: [(&'static str, Option<&'static str>, bool); 13] = [
        ("error", Some("Connection error."), true),
        ("error", Some("Request timed out."), true),
        ("error", Some("502 Bad Gateway"), true),
        ("error", Some("503 no available server\n"), true),
        ("error", Some("529 {\"type\":\"error\"}"), true),
        (
            "error",
            Some("ENOSPC: no space left on device, write"),
            false,
        ),
        ("error", Some("This operation was aborted"), false),
        ("error", Some("404 404 page not found\n"), false),
        ("error", Some("400 {\"type\":\"error\"}"), false),
        (
            "error",
            Some("Anthropic stream ended before message_stop"),
            false,
        ),
        ("error", Some(""), false),
        ("aborted", None, false),
        ("error", None, false),
    ];
    for (reason, message, provider) in table {
        let live = live_s1();
        let id = drive_stop(&live, reason, message);
        let (cause, next) = if provider {
            ("provider_error", "S1")
        } else {
            ("crashed", "S1")
        };
        let what = format!("stopReason {reason:?}, errorMessage {message:?}");
        assert_eq!(live.attempt(&id)["cause"], cause, "{what}");
        assert_eq!(
            event_of(&live, &id, "attempt-retry")["data"]["next_tier"],
            next,
            "{what}"
        );
    }
}

#[test]
fn a_job_retry_gives_a_job_three_more_provider_error_retries() {
    let live = live_s1();
    for _ in 0..4 {
        provider_stop(&live);
    }
    assert_eq!(live.job_status(JOB), "failed");

    live.fx
        .cpd(&["job", "retry", JOB, "--tier", "S1", "--plan", PLAN])
        .ok();
    for _ in 0..3 {
        let id = provider_stop(&live);
        assert_provider_error(&live, &id);
        assert_eq!(live.job_status(JOB), "queued", "{id}");
    }
    let last = provider_stop(&live);

    assert_provider_error(&live, &last);
    assert_eq!(live.job_status(JOB), "failed");
    assert_eq!(failed_cause(&live).as_deref(), Some("provider_error"));
    assert_eq!(attempt_count(&live), 8);
    assert_eq!(count_of(&live, "job-failed"), 2);
}

#[test]
fn a_provider_error_retry_event_names_the_cause_the_count_and_the_same_tier() {
    let live = live_s1();

    let first = provider_stop(&live);
    let text = event_of(&live, &first, "attempt-retry")["text"]
        .as_str()
        .unwrap()
        .to_owned();
    for needle in [
        "failed (provider_error)",
        CONNECTION,
        "Retry 1 of 3",
        "queued again at Tier S1",
    ] {
        assert!(text.contains(needle), "{needle:?} not in {text}");
    }
    for _ in 0..2 {
        provider_stop(&live);
    }
    let fourth = provider_stop(&live);

    let text = event_of(&live, &fourth, "job-failed")["text"]
        .as_str()
        .unwrap()
        .to_owned();
    for needle in ["failed (provider_error)", "The 3 retries are used up"] {
        assert!(text.contains(needle), "{needle:?} not in {text}");
    }
}

#[test]
fn provider_error_is_a_cause_the_ledger_stores_and_reads_back() {
    let cause = Cause::parse("provider_error").expect("provider_error is a Cause");
    assert!(Cause::ALL.contains(&cause));
    assert!(!cause.is_ladder());
    assert_eq!(Stored::Failed(cause).outcome(), "failed");
    assert_eq!(Stored::parse("provider_error"), Some(Stored::Failed(cause)));

    // The `attempts` CHECK accepts it: a running Attempt is ended with it through `ledger::end`.
    let live = live_s1();
    fixture(
        &live.fx.env,
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S1', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 't');",
    );
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![0, 1, 2, 3, 4, 5],
    };
    let catalog = Catalog {
        j: cpd_core::config::TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.0.0".to_owned(),
        o: model("p/opus"),
        s: model("p/sonnet"),
        h: model("p/haiku"),
        g: TierModel {
            model: "p/gemini".to_owned(),
            efforts: vec![],
        },
    };
    let ladder = Ladder {
        catalog: &catalog,
        retry_cap: 2,
        roles: cpd_core::config::RoleTiers {
            decide: cpd_core::config::Tier::x(cpd_core::config::Letter::O),
            auxiliar: Tier::parse("GX").unwrap(),
            worker: Tier::parse("H0").unwrap(),
            lead: Tier::parse("S4").unwrap(),
            manager: Tier::parse("O4").unwrap(),
        },
    };
    let plan = PlanId::parse(PLAN).unwrap();
    let mut store = store(&live.fx.env);
    let attempt = store
        .read(|conn| ledger::load(conn, &plan, &AttemptId::parse("api-a1").unwrap()))
        .unwrap();
    store
        .write("end provider_error", |tx| {
            ledger::end(
                tx,
                &attempt,
                &End::Failed {
                    cause,
                    detail: CONNECTION.to_owned(),
                },
                None,
                &ladder,
                Stage::Driver,
            )
        })
        .unwrap();

    assert_provider_error(&live, "api-a1");
}
