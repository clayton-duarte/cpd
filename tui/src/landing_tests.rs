//! The landing state (hs-d604, hs-d605; contract 6.1.1, 7.7; commands.md 13.2): a running Attempt in an open check span is
//! `checking`; checking and merging Attempts are landing, drawn yellow, and the legend calls the state "landing".

use cpd_core::config::Tier;
use cpd_core::ids::{AttemptId, JobKey, PrKey};

use crate::palette::{self, Meaning};
use crate::plan::flow::Live;
use crate::read::{AttemptNode, AttemptStatus, JobNode, JobStatus, Origin};

#[test]
fn the_legend_calls_the_yellow_job_state_landing() {
    let names: Vec<&str> = palette::JOB_LEGEND.iter().map(|(name, _, _)| *name).collect();
    assert!(names.contains(&"landing"), "{names:?}");
    assert!(!names.contains(&"merging"), "{names:?}");
    let landing = palette::JOB_LEGEND.iter().find(|(name, _, _)| *name == "landing");
    assert_eq!(landing.map(|(_, status, merging)| (*status, *merging)), Some((JobStatus::Running, true)));
}

#[test]
fn only_checking_and_merging_attempts_are_landing() {
    assert_eq!(AttemptStatus::Checking.as_str(), "checking");
    let landing: Vec<bool> = [
        AttemptStatus::Starting,
        AttemptStatus::Running,
        AttemptStatus::Checking,
        AttemptStatus::Merging,
        AttemptStatus::Ended,
    ]
    .iter()
    .map(|s| s.is_landing())
    .collect();
    assert_eq!(landing, [false, false, true, true, false]);
}

#[test]
fn a_checking_attempt_has_the_review_meaning_like_a_merging_one() {
    assert_eq!(palette::attempt_status(AttemptStatus::Checking), Meaning::Review);
    assert_eq!(palette::attempt_status(AttemptStatus::Checking), palette::attempt_status(AttemptStatus::Merging));
}

fn job(status: AttemptStatus) -> JobNode {
    JobNode {
        kind: "work".into(),
        key: JobKey::parse("a").unwrap(),
        pr: Some(PrKey::parse("api").unwrap()),
        seq: 0,
        title: String::new(),
        status: JobStatus::Running,
        tier: Some(Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        appended: false,
        retry_tier: None,
        shown_tier: Some(Tier::parse("S1").unwrap()),
        failed_cause: None,
        cancel_reason: None,
        merged_sha: None,
        waiting_on: Vec::new(),
        attempts: vec![AttemptNode {
            id: AttemptId::parse("a-a1").unwrap(),
            number: 1,
            tier: Tier::parse("S1").unwrap(),
            origin: Origin::Ladder,
            status,
            outcome: None,
            cause: None,
            worker: None,
            started_at: "2026-01-03T00:01:00.000Z".into(),
            submitted_at: None,
            ended_at: None,
        }],
    }
}

#[test]
fn a_job_whose_attempt_is_checking_is_drawn_with_the_landing_pill() {
    assert_eq!(Live::of(&job(AttemptStatus::Checking), false), Live::Merging);
    assert_eq!(Live::of(&job(AttemptStatus::Merging), false), Live::Merging);
    assert_eq!(Live::of(&job(AttemptStatus::Running), false), Live::Running);
    assert_eq!(Live::of(&job(AttemptStatus::Checking), false).pill(), palette::job_pill(JobStatus::Running, true, false));
}
