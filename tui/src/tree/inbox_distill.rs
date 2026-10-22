//! The distill-tier Inbox item's click opens the Digest dialog that dismisses it (hs-d547).

use super::actions::Action;
use super::ask::Question;
use super::fixture::sample;
use super::inbox::{Jump, item, jump};
use cpd_core::ids::ContextId;
use cpd_core::inbox::model::{Kind, Urgency};
use cpd_core::inbox::rules::TierNotice;

#[test]
fn a_distill_tier_click_is_a_notice_of_its_context() {
    let tree = sample();
    let alpha = ContextId::parse("alpha").unwrap();
    let mut tier = item(4, Kind::DistillTier, Urgency::Magenta, "alpha digest needed S1 ×3", "2026-01-01T00:00:00.000Z");
    assert_eq!(jump(&tree, &tier), None, "no Context: nowhere to go");
    tier.context = Some(alpha.clone());
    assert_eq!(jump(&tree, &tier), Some(Jump::Notice(4, alpha)));
}

#[test]
fn the_notice_dialog_names_the_context_steps_top_and_count_and_dismisses() {
    let alpha = ContextId::parse("alpha").unwrap();
    let notice = TierNotice { upto: 9, top: "S2".into(), steps: ["H0", "H1", "S1", "S2"].map(String::from).to_vec(), runs: 3 };
    assert_eq!(
        Question::distill_tier(4, &alpha, &notice),
        Question::Confirm {
            title: "Digest",
            text: "The distill run of alpha (records up to 9) climbed H0 → H1 → S1 → S2 and reached S2. Runs on an S Tier since you last dismissed this: 3.".into(),
            yes_label: "Dismiss",
            then: Action::Dismiss(4),
        }
    );
}
