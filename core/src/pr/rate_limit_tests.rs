//! A GitHub rate-limit reply to the poll's search (decisions pro-d392, pro-d394, assumption A2): it stays `gh_failed`, never
//! transient and never retried inside the poll, and it is marked `rate_limited` in `details`, so E5 pauses until the stored reset
//! (plus its grace) instead of raising `gh_failed`. Any other failure carries no such mark. On a `FakeRunner`; no process.

use std::path::Path;

use super::gh::search_open;
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::host::runner::{FakeRunner, Reply};

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
