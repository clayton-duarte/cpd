//! `pr open` on a cancelled Workflow (pro-d362): when the Lead Branch's pull request is open on GitHub again, the Workflow is
//! rebound (`workflow::reopen::reopen`); when it is closed or absent, the refusal stays. In process on the janitor's `Rig`.

use crate::error::ErrorCode;
use crate::workflow::reopen_tests::{
    cancelled, events_since, gh, last_event, publish, rig, rows, run,
};

/// A cancelled Workflow of a `done` Plan whose pull request is open again: the Workflow is `open` with its pull request and Lead
/// worktree, the Plan `deploy` with `done_at` NULL, the Lead awake, and the Events are `workflow-reopened` then `plan-phase`.
#[test]
fn pr_open_rebinds_a_cancelled_workflow_whose_pull_request_is_open() {
    let rig = rig("done");
    cancelled(&rig, "pay", "api", Some(801));
    gh(&rig, &[("cpd/pay/api", 801, "OPEN")]);
    publish(&rig, "cpd/pay/api");
    let since = last_event(&rig);

    run(&rig, "pay", "api").unwrap();

    assert_eq!(
        rows(
            &rig,
            "SELECT w.status, w.pr_number, w.pr_url, w.pr_opened_at IS NOT NULL, w.pull_request = r.id, w.lead_worktree IS NOT NULL
               FROM workflows w JOIN pull_requests r ON r.number = 801 WHERE w.key = 'api'"
        ),
        ["open|801|https://github.com/acme/pay/pull/801|1|1|1"]
    );
    assert_eq!(
        rows(&rig, "SELECT phase, done_at FROM plans WHERE id = 'pay'"),
        ["deploy|NULL"]
    );
    assert_eq!(
        rows(&rig, "SELECT asleep_at FROM sessions WHERE id = 'lead1'"),
        ["NULL"]
    );
    assert_eq!(
        events_since(&rig, since),
        ["workflow-reopened", "plan-phase"]
    );
}

/// The newest pull request of the branch is CLOSED, or there is none: `illegal_transition` as before, and nothing is written.
#[test]
fn pr_open_on_a_cancelled_workflow_with_a_closed_pull_request_is_refused() {
    let rig = rig("done");
    cancelled(&rig, "pay", "api", Some(801));
    cancelled(&rig, "pay", "web", Some(802));
    gh(&rig, &[("cpd/pay/api", 801, "CLOSED")]);
    publish(&rig, "cpd/pay/api");
    publish(&rig, "cpd/pay/web");
    let state =
        "SELECT key, status, lead_worktree, pr_number, pull_request FROM workflows ORDER BY key";
    let before = rows(&rig, state);
    let since = last_event(&rig);

    for key in ["api", "web"] {
        let refused = run(&rig, "pay", key).unwrap_err();
        assert_eq!(refused.code(), ErrorCode::IllegalTransition, "{key}");
    }

    assert_eq!(rows(&rig, state), before);
    assert_eq!(
        rows(&rig, "SELECT phase, done_at FROM plans WHERE id = 'pay'"),
        ["done|t"]
    );
    assert_eq!(
        rows(&rig, "SELECT asleep_at FROM sessions WHERE id = 'lead1'"),
        ["t"]
    );
    assert!(events_since(&rig, since).is_empty());
}
