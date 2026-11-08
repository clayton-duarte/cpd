//! Red tests for the merge's classification of an exhausted GitHub rate limit (contract §7.4 step 8b, pro-d102): a
//! `gh_failed` marked `rate_limited` is a transient provider failure, so `crash_end` ends it `Failure::ProviderError`
//! (same Tier, ≤3 tries, 30 s hold, excluded from `automatic_attempts`) instead of a `crashed` End that burns the
//! ladder; and the end's detail carries the `gh` stderr tail, the only durable record of the failure (merge/mod.rs:911-928).
//!
//! No process, no clock, no sleep: `crash_end` is called directly on an in-process [`CpdError`].

use super::{End, Failure, crash_end};
use crate::error::{CpdError, ErrorCode};

#[test]
fn a_rate_limited_gh_failure_ends_as_provider_error_with_stderr() {
    let e = CpdError::new(ErrorCode::GhFailed, "gh pr create exited 1.")
        .with("rate_limited", true)
        .with(
            "stderr",
            "gh: API rate limit already exceeded for user ID 21690061.\n",
        );
    let stepped = crash_end(e, None).expect("a gh_failed ends the Attempt");
    match stepped.end {
        End::Failed { cause, detail } => {
            assert_eq!(
                cause,
                Failure::ProviderError,
                "an exhausted rate limit is a transient provider failure, not a crash: {detail}"
            );
            assert!(
                detail.contains("API rate limit already exceeded for user ID 21690061"),
                "the Event detail must carry the gh stderr tail: {detail}"
            );
        }
        other => panic!("a rate-limited gh_failed ends Failure::ProviderError, not {other:?}"),
    }
}
