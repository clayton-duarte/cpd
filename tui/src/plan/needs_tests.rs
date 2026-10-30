//! The clocks the plan view's screen needs ([`needs`]): pure, `taken` is a plain `Instant`.

use super::*;
use crate::read::{JobStatus, Pull, PrState};

fn snap(centre: Option<Centre>, pulse: Option<Pulse>) -> Snapshot {
    Snapshot { cursor: 0, centre, pulse, layout: Duration::ZERO }
}

fn none_of(n: &crate::pump::Needs) {
    assert!(!n.frame && !n.second && n.at.is_none(), "frame {} second {} at {:?}", n.frame, n.second, n.at);
}

fn pull() -> Pull {
    Pull {
        number: 1,
        state: PrState::Open,
        url: String::new(),
        conflicting: false,
        clean: false,
        draft: false,
        update: None,
    }
}

fn pulse() -> Pulse {
    Pulse { context: cpd_core::ids::ContextId::parse("ctx").unwrap(), rows: Vec::new(), max: 0, events_cursor: 0 }
}

#[test]
fn a_chart_with_no_running_job_needs_no_clock() {
    let s = snap(Some(tests_centre(1)), None);
    none_of(&needs(&s, Instant::now()));
}

#[test]
fn a_running_job_needs_the_frame_clock() {
    let mut c = tests_centre(1);
    c.plan.jobs[0].status = JobStatus::Running;
    let n = needs(&snap(Some(c), None), Instant::now());
    assert!(n.frame && !n.second && n.at.is_none());
}

#[test]
fn a_lead_working_dot_on_a_pr_box_title_needs_the_frame_clock() {
    let mut c = tests_centre(1);
    c.plan.prs[0].pull = Some(pull());
    c.plan.prs[0].lead_dot = Some(crate::read::Dot::working(false, None));
    let n = needs(&snap(Some(c), None), Instant::now());
    assert!(n.frame && !n.second && n.at.is_none());
}

#[test]
fn pulse_needs_its_refresh_sixty_seconds_after_the_snapshot_and_no_frame_clock() {
    let taken = Instant::now();
    let n = needs(&snap(None, Some(pulse())), taken);
    assert!(!n.frame && !n.second);
    assert_eq!(n.at, Some(taken + Duration::from_secs(60)));
}

#[test]
fn no_chat_slot_needs_no_clock() {
    none_of(&needs(&snap(None, None), Instant::now()));
}

/// A Plan of one PR with `jobs` queued Jobs (the pattern of the tests in `mod.rs`).
fn tests_centre(jobs: usize) -> Centre {
    use cpd_core::config::Tier;
    use cpd_core::ids::{ContextId, JobKey, PrKey};
    use crate::read::{JobNode, PrNode, PrStatus};
    let pr = PrKey::parse("web").unwrap();
    let node = PrNode {
        key: pr.clone(),
        repo_name: "repo".into(),
        base: "main".into(),
        status: PrStatus::Dispatch,
        needs: Vec::new(),
        pull: None,
        waiting_on: Vec::new(),
        superseded_by: None,
        elapsed_seconds: None,
        cost_usd: None,
        lead_dot: None,
    };
    let job = |n: usize| JobNode {
        key: JobKey::parse(&format!("job-{n}")).unwrap(),
        pr: pr.clone(),
        seq: 0,
        title: String::new(),
        status: JobStatus::Queued,
        tier: Some(Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        appended: false,
        retry_tier: None,
        shown_tier: Some(Tier::parse("S1").unwrap()),
        failed_cause: None,
        cancel_reason: None,
        merged_sha: None,
        waiting_on: Vec::new(),
        attempts: Vec::new(),
    };
    let plan = PlanBoxes {
        plan: PlanId::parse("plan-a").unwrap(),
        context: ContextId::parse("ctx").unwrap(),
        prs: vec![node],
        jobs: (0..jobs).map(job).collect(),
        needs_user: read::NeedsUser::default(),
        events_cursor: 0,
    };
    let boxes = boxes::layout(&plan.prs, &plan.jobs, &plan.needs_user, 0).with_margin(CHART_MARGIN);
    let slot = ChatSlot {
        session: SessionId::parse("00000000-0000-4000-8000-000000000000").unwrap(),
        name: "lead".into(),
        plan: plan.plan.clone(),
        moved_event: 0,
    };
    Centre { slot, plan, boxes, frame: 0 }
}
