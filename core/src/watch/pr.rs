//! E5, PR poll (contract §3.7, §7.6; hs-d96, hs-d97): `cpd pr poll` as a
//! subprocess (a process boundary, cpd 1 §11 step 5), `pr_poll_seconds` after the
//! end of the previous poll, at its due time (the engine wakes for it between ticks),
//! whether or not any Plan is in `deploy`: one poll covers every open pull request of the User. `cpd` is the executable the engine started
//! from (`host::exe::started_as`), as for the drivers of E3 (hs-d151): not `PATH`.
//!
//! The rate guard: when a poll leaves fewer than [`RATE_FLOOR`] requests of
//! GitHub's rate limit, no poll runs until the limit resets, and one `error`
//! `gh_rate_limited` tells the User; it clears when polling resumes.

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::engine::Engine;
use super::report::{About, Failure, Step};
use crate::error::{CpdError, ErrorCode};
use crate::git::{TAIL_LINES, tail};
use crate::host::deadline::{self, Failed};
use crate::host::exe::started_as;

/// Below this many requests remaining, E5 stops polling until the limit resets (D9).
const RATE_FLOOR: u64 = 100;
/// The `cpd pr poll` child's deadline (contract §3.7): above `pr poll`'s own 15 s
/// per `gh` call (§7.6), so a hung child cannot stop the one-thread tick.
const POLL_DEADLINE: Duration = Duration::from_secs(120);

/// When the next poll is due: `pr_poll_seconds` after the last one ended, and not before the rate limit resets;
/// `now` when none has run. The engine wakes at this time instead of at the next tick (contract §3.7).
pub(super) fn due(engine: &Engine, now: Instant) -> Instant {
    let every = Duration::from_secs(u64::from(engine.config.watch.pr_poll_seconds));
    let after_last = engine.last_poll_end.map_or(now, |end| end + every);
    engine.paused_until.map_or(after_last, |until| until.max(after_last))
}

pub(super) fn run(engine: &mut Engine) {
    let now = Instant::now();
    if now < due(engine, now) {
        return;
    }
    engine.paused_until = None;
    let polled = poll(engine);
    engine.last_poll_end = Some(Instant::now());
    let out = match polled {
        Ok(out) => out,
        Err(failure) => return engine.fail(Step::E5, About::Machine, failure),
    };
    let listed = |key: &str| out[key].as_array().map_or(0, Vec::len);
    let (changed, feedback, merged, done) = (listed("changed"), listed("feedback"), listed("merged"), listed("done"));
    if changed + feedback + merged + done > 0 {
        engine.log.line(&format!(
            "E5: polled {} pull request(s): {changed} changed, {feedback} feedback, {merged} merged, {done} plan(s) done",
            out["polled"]
        ));
    }
    match rate_guard(&out) {
        Ok(None) => engine.clear(Step::E5, &About::Machine),
        Ok(Some((until, failure))) => {
            engine.paused_until = Some(until);
            engine.fail(Step::E5, About::Machine, failure);
        }
        Err(failure) => engine.fail(Step::E5, About::Machine, failure),
    }
}

/// D9: `None` when the rate limit leaves room; else when to poll again and the
/// failure to report. A `rate_limit` that is not `{remaining, reset_at}` is a failure.
fn rate_guard(out: &Value) -> Result<Option<(Instant, Failure)>, Failure> {
    let unusable = |why: String| {
        Failure::from(&CpdError::new(ErrorCode::SpawnFailed, format!("cpd pr poll printed an unusable rate_limit: {why}.")).with("stdout", out.to_string()))
    };
    let rate = &out["rate_limit"];
    let (Some(remaining), Some(reset_at)) = (rate["remaining"].as_u64(), rate["reset_at"].as_str()) else {
        return Err(unusable("it has no remaining and reset_at".to_owned()));
    };
    if remaining >= RATE_FLOOR {
        return Ok(None);
    }
    let reset = OffsetDateTime::parse(reset_at, &Rfc3339).map_err(|e| unusable(format!("reset_at {reset_at:?} is not RFC 3339: {e}")))?;
    let wait = (reset - OffsetDateTime::now_utc()).max(time::Duration::ZERO);
    let until = Instant::now() + Duration::try_from(wait).unwrap_or_default();
    let failure = Failure {
        code: ErrorCode::GhRateLimited.as_str().to_owned(),
        message: format!("GitHub's rate limit has {remaining} requests left: PR polling pauses until {reset_at}."),
        details: json!({"remaining": remaining, "reset_at": reset_at}),
    };
    Ok(Some((until, failure)))
}

/// Runs `cpd pr poll` and returns its success value, or the failure it printed.
fn poll(engine: &Engine) -> Result<Value, Failure> {
    let exe = started_as().map_err(|e| Failure::from(&e))?;
    let argv = [&*exe.to_string_lossy(), "pr", "poll"];
    let mut cmd = Command::new(&exe);
    cmd.args(&argv[1..])
        .env("CPD_HOME", engine.home.root())
        // The engine's own selection is not the subprocess's.
        .env_remove("CPD_PLAN")
        .env_remove("CPD_SESSION")
        .env_remove("CPD_ATTEMPT")
        .current_dir(engine.home.root())
        .stdin(Stdio::null());
    let out = deadline::output(&mut cmd, "cpd", POLL_DEADLINE).map_err(|e| match e {
        Failed::Io(e) => {
            Failure::from(&CpdError::new(ErrorCode::SpawnFailed, format!("Cannot run {}: {e}.", argv.join(" "))).with("argv", json!(argv)))
        }
        Failed::Timeout(e) => Failure::from(&e),
    })?;
    printed(&out, &argv)
}

/// What a child printed: its success value (`ok` true, exit 0), or the failure it printed (`ok` false, with an
/// `error` code), or a `spawn_failed` failure when it printed no result. Shared with E10's helper.
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
        _ => Err(Failure::from(
            &CpdError::new(ErrorCode::SpawnFailed, format!("{} printed no result.", argv.join(" ")))
                .with("argv", json!(argv))
                .with("exit", out.status.code())
                .with("stdout", tail(&stdout, TAIL_LINES))
                .with("stderr", tail(&String::from_utf8_lossy(&out.stderr), TAIL_LINES)),
        )),
    }
}
