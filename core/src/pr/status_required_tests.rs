//! `ChecksSummary::of` with the policy's `pr.required_checks` (contract §7.6 step 3b, hs-d743): a required name absent from
//! the head's rollup or not final is `pending`; a failure still wins; an empty list changes nothing.

use crate::pr::status::ChecksSummary;
use crate::pr::gh::Check;

fn run(name: &str, status: &str, conclusion: &str) -> Check {
    let conclusion = if status == "COMPLETED" { Some(conclusion.to_owned()) } else { None };
    Check { name: Some(name.to_owned()), context: None, conclusion, state: None, url: None, started_at: None, workflow: None }
}

fn context(name: &str, state: &str) -> Check {
    Check { name: None, context: Some(name.to_owned()), conclusion: None, state: Some(state.to_owned()), url: None, started_at: None, workflow: None }
}

fn required(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

fn guard() -> Check {
    context("GitGuardian Security Checks", "SUCCESS")
}

#[test]
fn required_check_absent_from_the_rollup_is_pending() {
    assert_eq!(ChecksSummary::of(&[guard()], &[], &required(&["gate"])), ChecksSummary::Pending);
}

#[test]
fn no_check_reported_yet_with_a_required_check_is_pending() {
    assert_eq!(ChecksSummary::of(&[], &[], &required(&["gate"])), ChecksSummary::Pending);
}

#[test]
fn required_check_is_matched_by_its_exact_name_only() {
    for name in ["gate-extra", "Gate", "gat", "my gate"] {
        let checks = [run(name, "COMPLETED", "SUCCESS")];
        assert_eq!(ChecksSummary::of(&checks, &[], &required(&["gate"])), ChecksSummary::Pending, "{name}");
    }
}

#[test]
fn an_empty_required_list_changes_nothing() {
    assert_eq!(ChecksSummary::of(&[guard()], &[], &[]), ChecksSummary::Passed);
    assert_eq!(ChecksSummary::of(&[], &[], &[]), ChecksSummary::None);
}

#[test]
fn a_passed_required_check_is_fine() {
    let checks = [guard(), run("gate", "COMPLETED", "SUCCESS")];
    assert_eq!(ChecksSummary::of(&checks, &[], &required(&["gate"])), ChecksSummary::Passed);
    let checks = [run("gate", "COMPLETED", "SKIPPED")];
    assert_eq!(ChecksSummary::of(&checks, &[], &required(&["gate"])), ChecksSummary::Passed);
}

#[test]
fn a_failure_wins_over_a_missing_required_check() {
    let checks = [run("lint", "COMPLETED", "FAILURE")];
    assert_eq!(ChecksSummary::of(&checks, &[], &required(&["gate"])), ChecksSummary::Failed);
}

#[test]
fn a_required_check_that_is_running_is_pending_and_one_that_failed_is_failed() {
    assert_eq!(ChecksSummary::of(&[run("gate", "IN_PROGRESS", "")], &[], &required(&["gate"])), ChecksSummary::Pending);
    assert_eq!(ChecksSummary::of(&[run("gate", "COMPLETED", "FAILURE")], &[], &required(&["gate"])), ChecksSummary::Failed);
}
