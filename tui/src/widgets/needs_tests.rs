use super::*;

fn item(kind: model::Kind, subject: &str) -> Item {
    Item { kind, subject: subject.to_owned(), ..inbox::item(1, kind, model::Urgency::Yellow, "repo#1 ready to merge", "2026-01-01T00:00:00.000Z") }
}

#[test]
fn nothing_spinning_needs_only_the_sample_time() {
    let t = Instant::now();
    let n = needs(&[], &BTreeSet::new(), t);
    assert!(!n.frame);
    assert!(!n.second);
    assert!(n.at.is_some());
}

#[test]
fn an_item_is_an_age_text_and_needs_the_second_clock() {
    let n = needs(&[item(model::Kind::PrReady, "pr:acme/repo#1")], &BTreeSet::new(), Instant::now());
    assert!(!n.frame);
    assert!(n.second);
}

#[test]
fn a_pr_ready_item_whose_merge_runs_needs_the_frame_clock() {
    let merging = BTreeSet::from(["acme/repo#1".to_owned()]);
    assert!(needs(&[item(model::Kind::PrReady, "pr:acme/repo#1")], &merging, Instant::now()).frame);
}

#[test]
fn a_merge_that_runs_on_another_pull_request_needs_no_frame_clock() {
    let merging = BTreeSet::from(["acme/repo#2".to_owned()]);
    assert!(!needs(&[item(model::Kind::PrReady, "pr:acme/repo#1")], &merging, Instant::now()).frame);
}

#[test]
fn the_sample_time_is_two_seconds_after_the_last_sample() {
    let t = Instant::now();
    assert_eq!(needs(&[], &BTreeSet::new(), t).at, Some(t + Duration::from_secs(2)));
}
