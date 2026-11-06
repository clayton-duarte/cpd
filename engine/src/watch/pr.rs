//! E5, pull request poll (contract §3.7, §7.6; hs-d96, hs-d97): `cpd pr poll` as a
//! subprocess (a process boundary, cpd 1 §11 step 5), `pr_poll_seconds` after the
//! end of the previous poll, at its due time (the engine wakes for it between ticks),
//! whether or not any Plan is in `deploy`: one poll covers every open pull request of the User. `cpd` is the executable the engine started
//! from (`Spawn::self_exe`), as for the drivers of E3 (hs-d151): not `PATH`.
//!
//! The rate guard: when a poll leaves fewer than [`RATE_FLOOR`] requests of
//! GitHub's rate limit, no poll runs until the limit resets, and one `error`
//! `gh_rate_limited` tells the User; it clears when polling resumes.
//!
//! A transient `gh` failure (an HTTP 5xx or a timeout, `details.transient`, hs-d354) is logged and retried at the next
//! poll; E5 raises the `error` only after `watch.pr_poll_transient_minutes` of the same call failing, keeping when it
//! first failed in `run/pr-poll-failing.json` ([`transient`]).

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::engine::Engine;
use super::poll_run::{Finished, PollRun};
use super::report::{About, Failure, Step};
use cpd_core::clock::Clock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::git::{TAIL_LINES, tail};
use cpd_core::host::deadline::Failed;
use cpd_core::host::limit::limit;
use cpd_core::host::runner::Runner;
use cpd_core::ports::Ports;

/// Below this many requests remaining, E5 stops polling until the limit resets (D9).
const RATE_FLOOR: u64 = 100;
/// The wait after GitHub's reset before E5 polls again (decision pro-d392): the first poll at the reset instant can still be refused.
const RATE_GRACE: Duration = Duration::from_secs(3);
/// The pause after a rate-limited reply when no reset is known (decision of the Lead, core-impl-engine-a1-q1).
const RATE_FALLBACK: Duration = Duration::from_secs(60);
/// The `cpd pr poll` child's deadline (contract §3.7): above `pr poll`'s own 15 s
/// per `gh` call (§7.6), so a hung child cannot stop the one-thread tick.
const POLL_DEADLINE: Duration = Duration::from_secs(120);

/// When the next poll is due: `pr_poll_seconds` after the last one ended (tests: `CPD_TEST_PR_POLL`, [`limit`]), and
/// not before the rate limit resets; `now` when none has run; `None` while a poll runs. The engine wakes at this time
/// instead of at the next tick (contract §3.7).
pub(super) fn due(engine: &Engine, now: Instant) -> Option<Instant> {
    if engine.poll.is_some() {
        return None;
    }
    let every = limit(
        "PR_POLL",
        Duration::from_secs(u64::from(engine.config.watch.pr_poll_seconds)),
    );
    // A cpd action changed a pull request on GitHub: poll at once (no floor), but never inside a rate-limit pause.
    let asked = engine.home.run_dir().join(cpd_core::pr::POLL_NOW).exists();
    let after_last = if asked {
        now
    } else {
        engine.last_poll_end.map_or(now, |end| end + every)
    };
    Some(
        engine
            .paused_until
            .map_or(after_last, |until| until.max(after_last)),
    )
}

/// E5, first step: starts `cpd pr poll` on its own thread when it is due and none runs.
pub(super) fn start(engine: &mut Engine) {
    let now = engine.ports.clock.now();
    if due(engine, now).is_none_or(|due| now < due) {
        return;
    }
    engine.paused_until = None;
    // A request written during this poll is served by the next one (contract §3.7).
    cpd_core::pr::clear_poll_now_in(&engine.home);
    let root = engine.home.root().to_path_buf();
    let ports = engine.ports.clone();
    engine.poll = Some(PollRun::start_on(ports.clock.clone(), move || {
        poll(&ports, &root)
    }));
}

/// E5, second step: applies the poll that ended, if one did.
pub(super) fn reap(engine: &mut Engine) {
    if engine.outgrown() {
        return;
    }
    // The outbox of GitHub writes drains here, in E5's step (swr-github.md §9).
    super::swr::drain_github(engine);
    let Some(finished) = engine.poll.as_mut().and_then(PollRun::try_take) else {
        return;
    };
    engine.poll = None;
    apply(engine, finished);
}

/// At stop: waits for a running poll and applies it.
pub(super) fn finish(engine: &mut Engine) {
    let Some(finished) = engine.poll.as_mut().and_then(PollRun::wait) else {
        return;
    };
    engine.poll = None;
    apply(engine, finished);
}

/// At a stand-down: waits for a running poll and drops its result, applying nothing.
pub(super) fn discard(engine: &mut Engine) {
    if let Some(mut poll) = engine.poll.take() {
        let _ = poll.wait();
    }
}

fn apply(engine: &mut Engine, finished: Finished) {
    let out = match finished.result {
        // A busy store is not a failure (contract §3.7): the poll did not happen, so it is not counted as one; it runs again at the next tick.
        Err(failure) if is_store_busy(&failure) => {
            engine.log.line(&format!(
                "E5: the store is busy, the poll is tried again at the next tick: {}: {}",
                failure.code, failure.message
            ));
            return;
        }
        Ok(out) => {
            engine.last_poll_end = Some(finished.ended);
            failing::delete(engine);
            out
        }
        Err(failure) => {
            engine.last_poll_end = Some(finished.ended);
            if failure.details["rate_limited"] == true {
                // A rate-limit reply is a pause until the stored reset plus the grace, not a machine error (A2).
                let clock = &*engine.ports.clock;
                let wait = engine.rate_reset.map_or(RATE_FALLBACK, |reset| {
                    Duration::try_from(reset - clock.utc()).unwrap_or_default() + RATE_GRACE
                });
                engine.paused_until = Some(clock.now() + wait);
                engine.log.line(&format!(
                    "E5: GitHub's rate limit is spent: polling pauses {} s.",
                    wait.as_secs()
                ));
                return;
            }
            if let Some(failure) = transient(engine, failure) {
                engine.fail(Step::E5, About::Machine, failure);
            }
            return;
        }
    };
    let listed = |key: &str| out[key].as_array().map_or(0, Vec::len);
    let (changed, feedback, merged, done) = (
        listed("changed"),
        listed("feedback"),
        listed("merged"),
        listed("done"),
    );
    if changed + feedback + merged + done > 0 {
        engine.log.line(&format!(
            "E5: polled {} pull request(s): {changed} changed, {feedback} feedback, {merged} merged, {done} plan(s) done",
            out["polled"]
        ));
    }
    engine.rate_reset = out["rate_limit"]["reset_at"]
        .as_str()
        .and_then(|at| OffsetDateTime::parse(at, &Rfc3339).ok());
    match rate_guard(&*engine.ports.clock, &out, &engine.ports.clock.zone()) {
        Ok(None) => engine.clear(Step::E5, &About::Machine),
        Ok(Some((until, failure))) => {
            engine.paused_until = Some(until);
            engine.fail(Step::E5, About::Machine, failure);
        }
        Err(failure) => engine.fail(Step::E5, About::Machine, failure),
    }
}

/// What a failed poll becomes (contract §3.7 E5, hs-d354): the failure to report now, or `None` when it is transient and
/// has not lasted `watch.pr_poll_transient_minutes` yet (it is logged; the next poll tries again). A failure that is not
/// transient deletes the record of the failing call and is reported at once.
fn transient(engine: &mut Engine, failure: Failure) -> Option<Failure> {
    let details = &failure.details;
    let call = details["call"]
        .as_str()
        .filter(|_| failure.code == ErrorCode::GhFailed.as_str() && details["transient"] == true);
    let (Some(call), Some(reason)) = (
        call.map(str::to_owned),
        details["reason"].as_str().map(str::to_owned),
    ) else {
        failing::delete(engine);
        return Some(failure);
    };
    let now = OffsetDateTime::now_utc();
    let (since, started) = match failing::read(engine) {
        Some(record) if record.call == call => (record.since, record.at),
        _ => {
            let since = now.format(&Rfc3339).expect("a time formats as RFC 3339");
            failing::write(engine, &call, &since);
            (since, now)
        }
    };
    let seconds = (now - started).whole_seconds().max(0);
    engine.log.line(&format!(
        "pr poll: gh {call} failed transiently ({reason}), failing since {since} ({seconds} s): retrying at the next poll"
    ));
    let minutes = seconds / 60;
    if minutes < i64::from(engine.config.watch.pr_poll_transient_minutes) {
        return None;
    }
    let last = details["stderr"]
        .as_str()
        .and_then(|s| s.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or(&failure.message)
        .trim()
        .to_owned();
    Some(Failure {
        code: failure.code,
        message: format!("pr poll: gh {call} has failed for {minutes} minutes ({reason}): {last}"),
        details: json!({
            "call": call,
            "since": since,
            "minutes": minutes,
            "reason": reason,
            "argv": details["argv"],
            "exit": details["exit"],
            "stderr": details["stderr"],
        }),
    })
}

/// `run/pr-poll-failing.json`, `{call, since}`: when E5's `gh` call first failed transiently (contract §2.1, §3.7 E5).
/// A file, so an engine restart does not lose the first failure; safe to delete (the 10 minutes then start again).
mod failing {
    use super::*;

    pub(super) struct Record {
        pub call: String,
        pub since: String,
        pub at: OffsetDateTime,
    }

    fn path(engine: &Engine) -> std::path::PathBuf {
        engine.home.run_dir().join("pr-poll-failing.json")
    }

    /// The record, or `None` when there is none; a file that is not `{call, since}` is deleted, with a log line.
    pub(super) fn read(engine: &mut Engine) -> Option<Record> {
        let path = path(engine);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                engine
                    .log
                    .line(&format!("pr poll: cannot read {}: {e}", path.display()));
                return None;
            }
        };
        let parsed = serde_json::from_str::<Value>(&text).ok().and_then(|v| {
            let (call, since) = (
                v["call"].as_str()?.to_owned(),
                v["since"].as_str()?.to_owned(),
            );
            let at = OffsetDateTime::parse(&since, &Rfc3339).ok()?;
            Some(Record { call, since, at })
        });
        if parsed.is_none() {
            engine.log.line(&format!(
                "pr poll: {} is not {{call, since}}: deleted, written again",
                path.display()
            ));
            delete(engine);
        }
        parsed
    }

    pub(super) fn write(engine: &mut Engine, call: &str, since: &str) {
        let path = path(engine);
        let written = cpd_core::atomic::write_atomic(
            &path,
            json!({"call": call, "since": since}).to_string(),
        );
        if let Err(e) = written {
            engine
                .log
                .line(&format!("pr poll: cannot write {}: {e}", path.display()));
        }
    }

    pub(super) fn delete(engine: &mut Engine) {
        let path = path(engine);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => engine
                .log
                .line(&format!("pr poll: cannot delete {}: {e}", path.display())),
        }
    }
}

/// Whether `cpd pr poll` failed on the store being busy: a transient store error, or the wait for the store's setup lock
/// (`lock_timeout` naming `store.lock`, not `pr-poll.lock`), the same errors every other step waits out (§3.7).
fn is_store_busy(failure: &Failure) -> bool {
    let sqlite_busy = failure.details["sqlite_code"]
        .as_i64()
        .is_some_and(|c| matches!(c & 0xff, 5 | 6));
    match failure.code.as_str() {
        c if c == ErrorCode::StoreBusy.as_str() => true,
        c if c == ErrorCode::StoreFailed.as_str() => sqlite_busy,
        c if c == ErrorCode::LockTimeout.as_str() => failure.details["lock"]
            .as_str()
            .is_some_and(|l| l.ends_with("store.lock")),
        _ => false,
    }
}

/// D9: `None` when the rate limit leaves room; else when to poll again and the
/// failure to report. A `rate_limit` that is not `{remaining, reset_at}` is a failure. The reset is read against `clock`,
/// the same clock the loop waits on, so a test's handover between the poll answering and the guard reading cannot skew it.
pub(super) fn rate_guard(
    clock: &dyn Clock,
    out: &Value,
    zone: &cpd_core::clock::TimeZone,
) -> Result<Option<(Instant, Failure)>, Failure> {
    let unusable = |why: String| {
        Failure::from(
            &CpdError::new(
                ErrorCode::SpawnFailed,
                format!("cpd pr poll printed an unusable rate_limit: {why}."),
            )
            .with("stdout", out.to_string()),
        )
    };
    let rate = &out["rate_limit"];
    let (Some(remaining), Some(reset_at)) = (rate["remaining"].as_u64(), rate["reset_at"].as_str())
    else {
        return Err(unusable("it has no remaining and reset_at".to_owned()));
    };
    if remaining >= RATE_FLOOR {
        return Ok(None);
    }
    let reset = OffsetDateTime::parse(reset_at, &Rfc3339)
        .map_err(|e| unusable(format!("reset_at {reset_at:?} is not RFC 3339: {e}")))?;
    let wait = (reset + RATE_GRACE - clock.utc()).max(time::Duration::ZERO);
    let until = clock.now() + Duration::try_from(wait).unwrap_or_default();
    let failure = Failure {
        code: ErrorCode::GhRateLimited.as_str().to_owned(),
        message: format!(
            "GitHub's rate limit has {remaining} requests left: pull request polling pauses until {}.",
            cpd_core::clock::prose(reset, zone)
        ),
        details: json!({"remaining": remaining, "reset_at": reset_at}),
    };
    Ok(Some((until, failure)))
}

/// Runs `cpd pr poll` and returns its success value, or the failure it printed.
fn poll(ports: &Ports, root: &Path) -> Result<Value, Failure> {
    let exe = ports.spawn.self_exe().map_err(|e| Failure::from(&e))?;
    poll_with(&*ports.runner, &exe, root)
}

/// [`poll`] on `runner`, with `exe` the recorded path of cpd's own executable.
pub(super) fn poll_with(runner: &dyn Runner, exe: &Path, root: &Path) -> Result<Value, Failure> {
    let argv = [&*exe.to_string_lossy(), "pr", "poll"];
    let mut cmd = Command::new(exe);
    cmd.args(&argv[1..])
        .env("CPD_HOME", root)
        // The engine's own selection is not the subprocess's.
        .env_remove("CPD_PLAN")
        .env_remove("CPD_SESSION")
        .env_remove("CPD_ATTEMPT")
        .current_dir(root)
        .stdin(Stdio::null());
    let out = runner
        .output(&mut cmd, "cpd", POLL_DEADLINE)
        .map_err(|e| match e {
            Failed::Io(e) => Failure::from(
                &CpdError::new(
                    ErrorCode::SpawnFailed,
                    format!("Cannot run {}: {e}.", argv.join(" ")),
                )
                .with("argv", json!(argv)),
            ),
            Failed::Timeout(e) => Failure::from(&e),
        })?;
    printed(&out, &argv)
}

/// What a child printed: its success value (`ok` true, exit 0), or the failure it printed (`ok` false, with an
/// `error` code), or a `spawn_failed` failure when it printed no result (named apart when it exited 0 with no output). Shared with E11's helper.
pub(super) fn printed(out: &Output, argv: &[&str]) -> Result<Value, Failure> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let printed: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    match printed {
        Some(value) if out.status.success() && value["ok"] == true => Ok(value),
        Some(value) if value["ok"] == false && value["error"]["code"].is_string() => {
            let error = &value["error"];
            Err(Failure {
                code: error["code"].as_str().unwrap_or_default().to_owned(),
                message: error["message"].as_str().unwrap_or_default().to_owned(),
                details: error["details"].clone(),
            })
        }
        _ => {
            let message = if out.status.success() && stdout.trim().is_empty() {
                format!("{} exited 0 but printed no result.", argv.join(" "))
            } else {
                format!("{} printed no result.", argv.join(" "))
            };
            Err(Failure::from(
                &CpdError::new(ErrorCode::SpawnFailed, message)
                    .with("argv", json!(argv))
                    .with("exit", out.status.code())
                    .with("stdout", tail(&stdout, TAIL_LINES))
                    .with(
                        "stderr",
                        tail(&String::from_utf8_lossy(&out.stderr), TAIL_LINES),
                    ),
            ))
        }
    }
}

#[cfg(test)]
#[path = "pr_failing_tests.rs"]
mod failing_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(code: ErrorCode, details: Value) -> Failure {
        Failure {
            code: code.as_str().to_owned(),
            message: String::new(),
            details,
        }
    }

    #[test]
    fn a_busy_store_is_not_a_failed_poll() {
        assert!(is_store_busy(&failure(ErrorCode::StoreBusy, json!({}))));
        assert!(is_store_busy(&failure(
            ErrorCode::StoreFailed,
            json!({"sqlite_code": 5})
        )));
        assert!(is_store_busy(&failure(
            ErrorCode::LockTimeout,
            json!({"lock": "/h/locks/store.lock"})
        )));
    }

    #[test]
    fn other_failures_stay_failures() {
        assert!(!is_store_busy(&failure(
            ErrorCode::LockTimeout,
            json!({"lock": "/h/locks/pr-poll.lock"})
        )));
        assert!(!is_store_busy(&failure(
            ErrorCode::StoreFailed,
            json!({"sqlite_code": 1})
        )));
        assert!(!is_store_busy(&failure(ErrorCode::SpawnFailed, json!({}))));
    }
}
