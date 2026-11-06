//! The swapped tiers of authored pull request rows (pro-d428): draft and stacked cyan; checks running, in review,
//! waiting, updating and fixing white; yellow, red and the rank order unchanged.

use super::model::Urgency;
use super::pr::Status;
use super::pr_tests::{fixture, is};

#[test]
fn a_draft_and_a_stacked_pr_are_cyan() {
    assert_eq!(Status::Draft.urgency(), Urgency::Cyan);
    assert_eq!(Status::Stacked.urgency(), Urgency::Cyan);
}

#[test]
fn a_checks_running_an_in_review_and_a_waiting_pr_are_white() {
    assert_eq!(Status::ChecksRunning.urgency(), Urgency::White);
    assert_eq!(Status::InReview.urgency(), Urgency::White);
    assert_eq!(Status::Waiting.urgency(), Urgency::White);
}

#[test]
fn an_updating_and_a_fixing_pr_are_white() {
    assert_eq!(Status::Updating.urgency(), Urgency::White);
    assert_eq!(Status::Fixing.urgency(), Urgency::White);
}

#[test]
fn yellow_and_red_pr_statuses_and_the_tier_rank_order_are_unchanged() {
    for s in [Status::ReadyForReview, Status::ReadyToMerge] {
        assert_eq!(s.urgency(), Urgency::Yellow, "{s:?}");
    }
    for s in [Status::Failing, Status::Conflict, Status::ChangesRequested] {
        assert_eq!(s.urgency(), Urgency::Red, "{s:?}");
    }
    assert!(Urgency::White < Urgency::Cyan);
    assert!(Urgency::Cyan < Urgency::Blue);
    assert!(Urgency::Blue < Urgency::Magenta);
    assert!(Urgency::Magenta < Urgency::Yellow);
    assert!(Urgency::Yellow < Urgency::Red);
}

#[test]
fn a_classified_draft_pr_opens_a_cyan_row_and_a_checks_running_one_a_white_row() {
    let mut f = fixture();
    f.workflow(1, "draft = 1, merge_state = 'draft'");
    f.workflow(2, "checks = 'pending'");
    is(&mut f, 1, Urgency::Cyan, "draft");
    is(&mut f, 2, Urgency::White, "checks running");
}
