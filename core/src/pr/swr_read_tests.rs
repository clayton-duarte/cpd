//! pro-d489: a Light pass (no heavy part) carries only GitHub's coarse rollup state, `FAILURE` while any check is CANCELLED;
//! the stored same-head detail is reduced again. In process: no `gh`, no clock, no process.

use serde_json::{Value, json};

use super::gh::{Listed, Membership, PollNode, PolledPr};
use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use super::swr_read::excuse_from_detail;

fn light_node(checks: ChecksSummary) -> PollNode {
    PollNode {
        listed: Listed {
            pr: PolledPr {
                repo: "acme/pay".into(),
                number: 801,
                url: "https://github.com/acme/pay/pull/801".into(),
                title: "t".into(),
                draft: false,
                state: PrState::Open,
                review_decision: ReviewDecision::None,
                checks,
                mergeable: Mergeable::Clean,
                merge_state: MergeState::Clean,
                created_at: "2026-10-30T00:00:00Z".into(),
                updated_at: "2026-10-30T00:00:00Z".into(),
                merged_at: None,
                base_ref: Some("main".into()),
                default_branch: Some("main".into()),
                open_threads: vec![],
                archived: false,
                head_oid: Some("a1".into()),
            },
            membership: Membership::default(),
        },
        head_oid: "a1".into(),
        author_login: None,
        reviews: json!([]),
        labels: json!([]),
        heavy: None,
    }
}

fn detail(runs: &[(&str, &str)]) -> Value {
    let nodes: Vec<Value> = runs
        .iter()
        .map(|(name, conclusion)| {
            json!({"__typename": "CheckRun", "name": name, "conclusion": conclusion,
                   "startedAt": "2026-10-30T01:00:00Z"})
        })
        .collect();
    json!({"commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": nodes}}}}]}})
}

#[test]
fn light_pass_cancelled_check_is_not_failed() {
    let mut node = light_node(ChecksSummary::Failed);
    let d = detail(&[("build", "SUCCESS"), ("deploy", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_ne!(node.listed.pr.checks, ChecksSummary::Failed);
}

#[test]
fn light_pass_truly_failed_check_stays_failed() {
    let mut node = light_node(ChecksSummary::Failed);
    let d = detail(&[("build", "SUCCESS"), ("test", "FAILURE")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.checks, ChecksSummary::Failed);
}

#[test]
fn passed_checks_normalize_unstable_to_clean() {
    let mut node = light_node(ChecksSummary::Failed);
    node.listed.pr.merge_state = MergeState::Unstable;
    let d = detail(&[("gate", "SUCCESS"), ("e2e", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.checks, ChecksSummary::Passed);
    assert_eq!(node.listed.pr.merge_state, MergeState::Clean);
}

#[test]
fn failed_check_keeps_unstable() {
    let mut node = light_node(ChecksSummary::Failed);
    node.listed.pr.merge_state = MergeState::Unstable;
    let d = detail(&[("gate", "FAILURE"), ("e2e", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.merge_state, MergeState::Unstable);
}

#[test]
fn passed_checks_keep_behind() {
    let mut node = light_node(ChecksSummary::Failed);
    node.listed.pr.merge_state = MergeState::Behind;
    let d = detail(&[("gate", "SUCCESS"), ("e2e", "CANCELLED")]);
    excuse_from_detail(&mut node, &d, &[]);
    assert_eq!(node.listed.pr.merge_state, MergeState::Behind);
}
