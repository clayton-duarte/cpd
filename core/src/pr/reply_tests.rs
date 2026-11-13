//! `pr reply` to a thread the last poll never saw (bug b78, pro-d593): with no stored feedback (`feedback` NULL) a
//! GitHub node id is enqueued without the lookup; a stored list still refuses an unknown id; a malformed id is refused.

use crate::error::{CpdError, ErrorCode};
use crate::output::Output;
use crate::workflow::reopen_tests::rows;

const THREAD: &str = "PRRC_kwDOVGpPbM8AAAABED_iYg";

fn rig(feedback: Option<&str>) -> crate::janitor_tests::Rig {
    let mut rig = crate::janitor_tests::Rig::new();
    rig.workflow_with("api", "open", "merged", "merged");
    let value = feedback.map_or("NULL".to_owned(), |f| format!("'{f}'"));
    rig.sql(&format!("UPDATE pull_requests SET feedback = {value};"));
    rig
}

fn reply(rig: &crate::janitor_tests::Rig, to: &str) -> Result<Output, CpdError> {
    // `Env::open_with` reads `$CPD_HOME`, and the approval guard reads `$CPD_SESSION`: this test sets the one and clears the other.
    unsafe {
        std::env::set_var("CPD_HOME", rig.home.root());
        std::env::remove_var("CPD_SESSION");
    }
    let args = super::reply::ReplyArgs::for_test("pay", "api", to, "Fixed.");
    crate::ports::Ports::scope(&rig.fakes.ports, || {
        super::reply::reply(&rig.fakes.ports, args)
    })
}

fn queued(rig: &crate::janitor_tests::Rig) -> Vec<String> {
    rows(rig, "SELECT op_kind FROM swr_mutations")
}

#[test]
fn reply_enqueues_thread_id_when_feedback_null() {
    let rig = rig(None);
    let result = reply(&rig, THREAD);
    assert!(result.is_ok(), "a node id is enqueued unlooked: {result:?}");
    assert_eq!(queued(&rig), ["reply"]);
}

#[test]
fn reply_refuses_unknown_id_when_feedback_present() {
    let rig = rig(Some("[]"));
    let err = reply(&rig, THREAD).unwrap_err();
    assert_eq!(err.code(), ErrorCode::FeedbackNotFound);
    assert!(queued(&rig).is_empty());
}

#[test]
fn reply_refuses_malformed_id_when_feedback_null() {
    let rig = rig(None);
    let err = reply(&rig, "not an id").unwrap_err();
    assert_eq!(err.code(), ErrorCode::Usage);
    assert!(queued(&rig).is_empty());
}
