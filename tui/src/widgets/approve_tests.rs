//! Red tests (pro-d167): the `Approve` entry of a `deploy` Inbox row's menu (commands.md §13.2).

use super::inbox::{Choice, Entry, item};
use crate::menu::Entry as _;
use cpd_core::inbox::model::{Kind, Urgency};

const OPENED: &str = "2026-01-01T00:00:00.000Z";
const REST: [Choice; 4] = [
    Choice::Snooze15,
    Choice::Snooze1h,
    Choice::SnoozeTomorrow,
    Choice::Dismiss,
];

#[test]
fn a_deploy_rows_choices_are_approve_then_the_snoozes_and_dismiss() {
    assert_eq!(
        Choice::entries(Kind::Deploy),
        [
            Choice::Approve,
            Choice::Snooze15,
            Choice::Snooze1h,
            Choice::SnoozeTomorrow,
            Choice::Dismiss
        ]
    );
}

#[test]
fn no_other_kind_has_the_approve_choice() {
    assert_eq!(
        Choice::entries(Kind::Deploy).first(),
        Some(&Choice::Approve),
        "Deploy has it"
    );
    assert_eq!(
        Choice::entries(Kind::ReviewRequested),
        [
            Choice::Claim,
            Choice::Snooze15,
            Choice::Snooze1h,
            Choice::SnoozeTomorrow,
            Choice::Dismiss
        ]
    );
    for kind in [Kind::Dialog, Kind::PrReady] {
        assert_eq!(Choice::entries(kind), REST, "{kind:?}");
    }
}

#[test]
fn the_approve_choice_runs_inbox_approve_with_the_id() {
    assert_eq!(Choice::argv(Choice::Approve, 7), ["inbox", "approve", "7"]);
}

#[test]
fn the_approve_choice_is_labelled_approve() {
    assert_eq!(Choice::Approve.label(), "Approve");
}

#[test]
fn a_deploy_items_menu_entries_are_approve_first_and_the_four_choices() {
    let deploy = item(
        1,
        Kind::Deploy,
        Urgency::Yellow,
        "acme/pay deploy production: Release 1.2",
        OPENED,
    );
    let expected: Vec<Entry> = [Choice::Approve]
        .into_iter()
        .chain(REST)
        .map(Entry::Choice)
        .collect();
    assert_eq!(Entry::entries(&deploy, false), expected);
    let ready = item(
        2,
        Kind::PrReady,
        Urgency::Yellow,
        "acme/pay#1 ready",
        OPENED,
    );
    assert_eq!(Entry::entries(&ready, false), REST.map(Entry::Choice));
}
