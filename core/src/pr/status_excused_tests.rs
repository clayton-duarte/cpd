//! `ChecksSummary::of` under a policy hold (contract §7.6 step 3b, pro-d447): a failing check named exactly `danger` (ASCII
//! case-insensitive) counts as passed; every other check is unaffected; a pending `danger` stays pending.

use crate::pr::gh::Check;
use crate::pr::status::ChecksSummary;

fn run(name: &str, status: &str, conclusion: &str) -> Check {
    let conclusion = if status == "COMPLETED" {
        Some(conclusion.to_owned())
    } else {
        None
    };
    Check {
        name: Some(name.to_owned()),
        context: None,
        conclusion,
        state: None,
        url: None,
        started_at: None,
        workflow: None,
    }
}

fn context(name: &str, state: &str) -> Check {
    Check {
        name: None,
        context: Some(name.to_owned()),
        conclusion: None,
        state: Some(state.to_owned()),
        url: None,
        started_at: None,
        workflow: None,
    }
}

fn green() -> Check {
    run("ci/test", "COMPLETED", "SUCCESS")
}

#[test]
fn a_failing_danger_run_is_excused_under_a_policy_hold() {
    let checks = [green(), run("danger", "COMPLETED", "FAILURE")];
    assert_eq!(
        ChecksSummary::of(&checks, &[], &[], true),
        ChecksSummary::Passed
    );
}

#[test]
fn a_failing_danger_status_context_is_excused_under_a_policy_hold() {
    let checks = [green(), context("danger", "FAILURE")];
    assert_eq!(
        ChecksSummary::of(&checks, &[], &[], true),
        ChecksSummary::Passed
    );
}

#[test]
fn a_failing_danger_is_a_failure_without_a_policy_hold() {
    let checks = [green(), run("danger", "COMPLETED", "FAILURE")];
    assert_eq!(
        ChecksSummary::of(&checks, &[], &[], false),
        ChecksSummary::Failed
    );
}

#[test]
fn another_failing_check_still_fails_under_a_policy_hold() {
    assert_eq!(
        ChecksSummary::of(&[run("danger", "COMPLETED", "FAILURE")], &[], &[], true),
        ChecksSummary::Passed,
        "danger alone is excused"
    );
    let checks = [
        run("danger", "COMPLETED", "FAILURE"),
        run("ci/test", "COMPLETED", "FAILURE"),
    ];
    assert_eq!(
        ChecksSummary::of(&checks, &[], &[], true),
        ChecksSummary::Failed
    );
}

#[test]
fn a_pending_check_stays_pending_under_a_policy_hold() {
    let checks = [
        run("danger", "COMPLETED", "FAILURE"),
        run("ci/slow", "IN_PROGRESS", ""),
    ];
    assert_eq!(
        ChecksSummary::of(&checks, &[], &[], true),
        ChecksSummary::Pending
    );
    let checks = [green(), run("danger", "IN_PROGRESS", "")];
    assert_eq!(
        ChecksSummary::of(&checks, &[], &[], true),
        ChecksSummary::Pending,
        "a danger that is still running is not excused"
    );
}

#[test]
fn an_excused_danger_still_satisfies_a_required_danger_check() {
    let checks = [green(), run("danger", "COMPLETED", "FAILURE")];
    assert_eq!(
        ChecksSummary::of(&checks, &[], &["danger".to_owned()], true),
        ChecksSummary::Passed
    );
}

#[test]
fn an_excused_danger_name_match_is_exact_and_ignores_ascii_case() {
    assert_eq!(
        ChecksSummary::of(&[run("Danger", "COMPLETED", "FAILURE")], &[], &[], true),
        ChecksSummary::Passed
    );
    assert_eq!(
        ChecksSummary::of(
            &[run("danger-js-extra", "COMPLETED", "FAILURE")],
            &[],
            &[],
            true
        ),
        ChecksSummary::Failed
    );
}
