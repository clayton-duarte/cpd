//! A GitHub rate-limit reply to the poll's search (decisions pro-d392, pro-d394, assumption A2): it stays `gh_failed`, never
//! transient and never retried inside the poll, and it is marked `rate_limited` in `details`, so E5 pauses until the stored reset
//! (plus its grace) instead of raising `gh_failed`. Any other failure carries no such mark. On a `FakeRunner`; no process.

use std::path::Path;
use std::sync::Arc;

use super::gh::{head_pr, search_open};
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::host::runner::{FakeRunner, Reply};
use crate::ports::Ports;

/// The poll's search on a `gh` that answers `reply`: the error and how many `gh` calls it made.
fn searched(reply: Reply) -> (CpdError, usize) {
    let runner = FakeRunner::new(ManualClock::new()).on("gh", |_| true, reply);
    let Err(e) = search_open(&runner, Path::new("/cpd-home"), &[]) else {
        panic!("the search did not answer");
    };
    (e, runner.calls().len())
}

/// A boolean of the error's `details`, `None` when it has none.
fn flag(e: &CpdError, key: &str) -> Option<bool> {
    e.details().get(key).and_then(serde_json::Value::as_bool)
}

#[test]
fn rate_limited_reply_pauses_until_reset() {
    for (name, reply) in [
        (
            "graphql RATE_LIMITED",
            Reply::exit(
                1,
                r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#,
                "gh: API rate limit exceeded for user ID 1.\n",
            ),
        ),
        (
            "graphql RATE_LIMITED with a 200",
            Reply::ok(
                r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#,
            ),
        ),
        (
            "already exceeded",
            Reply::exit(
                1,
                "",
                "gh: API rate limit already exceeded for user ID 1.\n",
            ),
        ),
    ] {
        let (e, calls) = searched(reply);
        assert_eq!(e.code(), ErrorCode::GhFailed, "{name}: A2, still gh_failed");
        assert_eq!(
            flag(&e, "rate_limited"),
            Some(true),
            "{name}: {:?}",
            e.details()
        );
        assert_ne!(flag(&e, "transient"), Some(true), "{name}: never transient");
        assert_eq!(calls, 1, "{name}: no retry inside the poll");
    }
}

#[test]
fn other_gh_failures_still_raise_gh_failed() {
    for (name, reply) in [
        (
            "a 401",
            Reply::exit(1, "", "gh: HTTP 401: Bad credentials\n"),
        ),
        (
            "a graphql error that is not a rate limit",
            Reply::exit(1, r#"{"errors":[{"message":"Expected NAME"}]}"#, ""),
        ),
        ("an empty answer", Reply::ok("")),
    ] {
        let (e, calls) = searched(reply);
        assert_eq!(e.code(), ErrorCode::GhFailed, "{name}");
        assert_ne!(flag(&e, "rate_limited"), Some(true), "{name}");
        assert_eq!(calls, 1, "{name}");
    }
    // A 502 is still transient, and no rate limit.
    let (e, _) = searched(Reply::exit(1, "", "gh: HTTP 502: Bad Gateway\n"));
    assert_eq!(flag(&e, "transient"), Some(true));
    assert_ne!(flag(&e, "rate_limited"), Some(true));
    // The mark is explicit: a rate-limit reply has it, so `rate_limited` is present and false-or-absent elsewhere.
    let (limited, _) = searched(Reply::exit(
        1,
        "",
        "gh: API rate limit already exceeded for user ID 1.\n",
    ));
    assert_eq!(flag(&limited, "rate_limited"), Some(true));
}

/// A REST `gh` CLI failure (not a poll `Reply`) whose stderr says the rate limit is spent carries the shared `rate_limited`
/// mark, so the merge path can treat it as transient (red until `failed()` sets the mark; gh.rs:61-68).
#[test]
fn a_rate_limited_head_pr_is_marked_transient() {
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on(
        "gh",
        |_| true,
        Reply::exit(
            1,
            "",
            "gh: API rate limit already exceeded for user ID 21690061.\n",
        ),
    );
    let mut ports = Ports::real();
    ports.clock = Arc::new(clock);
    ports.runner = Arc::new(runner);
    let e = Ports::scope(&ports, || {
        head_pr(Path::new("/cpd-home"), "cpd/incident--job-a1", None)
    })
    .expect_err("a rate-limited gh pr list answers an error");
    assert_eq!(e.code(), ErrorCode::GhFailed, "{:?}", e.details());
    assert_eq!(
        flag(&e, "rate_limited"),
        Some(true),
        "the shared REST mark must be set: {:?}",
        e.details()
    );
}

// A merge's gh calls (contract §7.4 "A rate-limited `gh` call in the merge", pro-d102, A12): a RATE_LIMITED reply waits in place
// for the reset and repeats the call; the Attempt has no end and no try. Virtual time (`ManualClock`), no process, no sleep.

mod merge_gh {
    use std::path::Path;
    use std::time::Duration;

    use super::super::gh::run_in_merge;
    use crate::clock::ManualClock;
    use crate::host::runner::{FakeRunner, Reply};

    /// `ManualClock::utc()` at construction, 2026-01-01 00:00:00 UTC, in epoch seconds.
    pub const NOW: u64 = 1_767_225_600;

    pub const LIMITED: &str = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#;

    pub fn is_rate_read(args: &[String]) -> bool {
        args == ["api", "rate_limit"]
    }

    /// The `gh api rate_limit` answer: both resources reset `secs` after the clock's start.
    pub fn rate_read(secs: u64) -> Reply {
        let reset = NOW + secs;
        Reply::ok(&format!(
            r#"{{"resources":{{"core":{{"reset":{reset}}},"graphql":{{"reset":{reset}}}}}}}"#
        ))
    }

    pub fn call(
        runner: &FakeRunner,
        clock: &ManualClock,
    ) -> Result<String, crate::error::CpdError> {
        run_in_merge(
            runner,
            clock,
            Path::new("/cpd-home"),
            &["pr", "list", "--head", "lead"],
        )
    }

    pub fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }
}

#[test]
fn merge_gh_rate_limited_waits_for_reset_then_succeeds() {
    use merge_gh::*;
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone())
        .once(
            "gh",
            |c| c.args.first().map(String::as_str) == Some("pr"),
            Reply::exit(1, LIMITED, "gh: API rate limit exceeded for user ID 1.\n"),
        )
        .on("gh", |c| is_rate_read(&c.args), rate_read(120))
        .on(
            "gh",
            |c| c.args.first().map(String::as_str) == Some("pr"),
            Reply::ok("[]"),
        );
    let out = call(&runner, &clock).expect("a rate-limited call waits, then succeeds");
    assert_eq!(out, "[]");
    assert_eq!(
        clock.naps(),
        vec![secs(123)],
        "one wait: the reset (120 s) plus 3 s"
    );
    let calls = runner.calls();
    let pr: Vec<_> = calls.iter().filter(|c| !is_rate_read(&c.args)).collect();
    assert_eq!(pr.len(), 2, "the same call is repeated once: {calls:?}");
    assert_eq!(pr[0].args, pr[1].args, "with the same argv");
    assert_eq!(
        calls.len(),
        3,
        "plus one `gh api rate_limit` read: {calls:?}"
    );
}

#[test]
fn merge_gh_rate_limited_past_bound_ends_crashed_naming_rate_limited() {
    use merge_gh::*;
    let clock = ManualClock::new();
    // The reset is 4 000 s away: the wait (4 003 s) would take the total past the 3 600 s bound, so it is not taken.
    let runner = FakeRunner::new(clock.clone())
        .on("gh", |c| is_rate_read(&c.args), rate_read(4_000))
        .on(
            "gh",
            |c| c.args.first().map(String::as_str) == Some("pr"),
            Reply::exit(1, LIMITED, "gh: API rate limit exceeded for user ID 1.\n"),
        );
    let e = call(&runner, &clock).expect_err("past the bound the call fails");
    assert_eq!(e.code(), ErrorCode::GhFailed);
    assert!(
        clock.naps().is_empty(),
        "a wait past the bound is not taken: {:?}",
        clock.naps()
    );
    let detail = crate::merge::crashed_detail(e).expect(
        "past the bound the Attempt ends `crashed`, an ordinary one that counts in retry_cap",
    );
    assert!(
        detail.starts_with("gh_failed: rate_limited:"),
        "the detail names rate_limited: {detail}"
    );
    assert!(
        detail.contains("API rate limit exceeded for user ID 1."),
        "and carries the stderr tail: {detail}"
    );
}

#[test]
fn merge_gh_crash_detail_carries_stderr_tail() {
    use merge_gh::*;
    let clock = ManualClock::new();
    // One wait, then a different failure whose stderr is 70 lines: the detail carries that last failure's last 60 lines.
    let long: String = (1..=70).map(|n| format!("line {n}\n")).collect();
    let runner = FakeRunner::new(clock.clone())
        .once(
            "gh",
            |c| c.args.first().map(String::as_str) == Some("pr"),
            Reply::exit(1, LIMITED, "gh: API rate limit exceeded for user ID 1.\n"),
        )
        .on("gh", |c| is_rate_read(&c.args), rate_read(10))
        .on(
            "gh",
            |c| c.args.first().map(String::as_str) == Some("pr"),
            Reply::exit(1, "", &long),
        );
    let e = call(&runner, &clock).expect_err("the second reply is a plain failure");
    let detail =
        crate::merge::crashed_detail(e).expect("a non-rate-limited gh_failed ends `crashed`");
    assert!(detail.starts_with("gh_failed:"), "{detail}");
    assert!(
        detail.contains("line 70"),
        "the stderr tail's last line: {detail}"
    );
    assert!(
        detail.contains("line 11\n"),
        "the tail's first line (60 lines): {detail}"
    );
    assert!(
        !detail.contains("line 10\n"),
        "only the last 60 lines: {detail}"
    );
}

#[test]
fn merge_gh_other_failure_still_crashes_at_once() {
    use merge_gh::*;
    let clock = ManualClock::new();
    let runner = FakeRunner::new(clock.clone()).on(
        "gh",
        |c| c.args.first().map(String::as_str) == Some("pr"),
        Reply::exit(1, "", "gh: HTTP 404: Not Found\n"),
    );
    let e = call(&runner, &clock).expect_err("a 404 fails");
    assert_eq!(runner.calls().len(), 1, "no wait, no rate read, no repeat");
    assert!(clock.naps().is_empty());
    let detail = crate::merge::crashed_detail(e).expect("ends `crashed` at once");
    assert!(detail.starts_with("gh_failed:"), "{detail}");
    assert!(!detail.contains("rate_limited"), "{detail}");
    assert!(detail.contains("HTTP 404: Not Found"), "{detail}");
}
