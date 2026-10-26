use super::*;

fn pr(working: bool) -> read::OpenPr {
    read::OpenPr {
        repo: "acme/repo".to_owned(),
        number: 1,
        url: String::new(),
        title: String::new(),
        draft: false,
        checks: read::Checks::Passed,
        review_decision: read::ReviewDecision::None,
        conflicting: false,
        update: None,
        clean: false,
        active: false,
        lead: Some(SessionId::parse("l-a").unwrap()),
        lead_working: working,
        plan_failed: false,
        plan_phase: None,
        merging: false,
    }
}

#[test]
fn no_spinning_pr_needs_only_the_sample_time() {
    let t = Instant::now();
    for prs in [vec![], vec![pr(false), pr(false)]] {
        let n = needs(&prs, &[], &BTreeSet::new(), t);
        assert!(!n.frame);
        assert!(!n.second);
        assert!(n.at.is_some());
    }
}

#[test]
fn a_working_lead_needs_the_frame_clock_and_the_sample_time() {
    let n = needs(&[pr(false), pr(true)], &[], &BTreeSet::new(), Instant::now());
    assert!(n.frame);
    assert!(!n.second);
    assert!(n.at.is_some());
}

#[test]
fn a_clean_pr_whose_merge_runs_needs_the_frame_clock() {
    let merging = read::OpenPr { clean: true, merging: true, ..pr(false) };
    assert!(needs(&[merging], &[], &BTreeSet::new(), Instant::now()).frame);
}

#[test]
fn a_merge_that_runs_on_a_pr_that_is_not_clean_needs_no_frame_clock() {
    let merging = read::OpenPr { clean: false, merging: true, ..pr(false) };
    assert!(!needs(&[merging], &[], &BTreeSet::new(), Instant::now()).frame);
}

#[test]
fn the_sample_time_is_two_seconds_after_the_last_sample() {
    let t = Instant::now();
    assert_eq!(needs(&[], &[], &BTreeSet::new(), t).at, Some(t + Duration::from_secs(2)));
}
