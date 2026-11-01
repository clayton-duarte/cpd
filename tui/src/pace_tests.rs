//! Red tests of the loader fix (commands.md §17 and §13.2, rules R1-R4).
//!
//! Manual check, which the implementation Job and the remeasure Job rely on: run the plan view with `CPD_TUI_TRACE=1`
//! against a Lead that works and no running Job. At every frame change the trace shows `gap_us` about 100000 and
//! `layout_us` 0 (before the fix: one frame in 35 s, and `layout_us` of 13-18 ms at p50 when a Job runs).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::pace::Reads;
use crate::plan::boxes::{self, LAYOUTS};
use crate::plan::{CHART_MARGIN, Centre};
use crate::read::{Dot, JobNode, JobStatus, NeedsUser, PlanBoxes, WorkflowNode, PrState, WorkflowStatus, Pull, ChatSlot};
use cpd_core::config::Tier;
use cpd_core::ids::{ContextId, JobKey, PlanId, WorkflowKey, SessionId};

#[test]
fn one_read_runs_at_a_time_and_the_newest_waiting_request_wins() {
    let (_tx, input) = mpsc::channel::<ratatui::crossterm::event::Event>();
    let pump = Pump::new(move || input.recv().map_err(std::io::Error::other));
    let mut reads: Reads<u32> = Reads::with_waker(pump.waker());
    let running = Arc::new(AtomicUsize::new(0));
    let overlapped = Arc::new(AtomicBool::new(false));
    let second_ran = Arc::new(AtomicBool::new(false));
    let (started_tx, started) = mpsc::channel::<()>();
    let (release, released) = mpsc::channel::<()>();

    let (r, o) = (running.clone(), overlapped.clone());
    reads.request(move || {
        r.fetch_add(1, Ordering::SeqCst);
        started_tx.send(()).unwrap();
        released.recv().unwrap();
        if r.fetch_sub(1, Ordering::SeqCst) != 1 {
            o.store(true, Ordering::SeqCst);
        }
        1
    });
    started.recv().unwrap();
    let s = second_ran.clone();
    reads.request(move || {
        s.store(true, Ordering::SeqCst);
        2
    });
    let (r, o) = (running.clone(), overlapped.clone());
    reads.request(move || {
        if r.fetch_add(1, Ordering::SeqCst) != 0 {
            o.store(true, Ordering::SeqCst);
        }
        r.fetch_sub(1, Ordering::SeqCst);
        3
    });
    release.send(()).unwrap();

    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while out.len() < 2 && Instant::now() < deadline {
        match reads.take() {
            Some(v) => out.push(v),
            None => std::thread::yield_now(),
        }
    }
    assert_eq!(out, [1, 3]);
    assert!(
        !second_ran.load(Ordering::SeqCst),
        "the older waiting request ran"
    );
    assert!(!overlapped.load(Ordering::SeqCst), "two jobs ran at once");
    assert_eq!(reads.take(), None);
}

fn workflow_node(pull: Option<Pull>, lead_dot: Option<Dot>) -> WorkflowNode {
    WorkflowNode {
        key: WorkflowKey::parse("web").unwrap(),
        repo_name: "repo".into(),
        base: "main".into(),
        status: WorkflowStatus::Dispatch,
        needs: Vec::new(),
        pull,
        waiting_on: Vec::new(),
        superseded_by: None,
        elapsed_seconds: None,
        cost_usd: None,
        lead_dot,
    }
}

fn pull() -> Pull {
    Pull {
        number: 12,
        state: PrState::Open,
        url: "https://github.com/acme/repo/pull/12".into(),
        conflicting: false,
        clean: false,
        update: None,
        draft: false,
    }
}

fn job_node(key: &str, status: JobStatus) -> JobNode {
    JobNode {
        kind: "work".into(),
        key: JobKey::parse(key).unwrap(),
        workflow: Some(WorkflowKey::parse("web").unwrap()),
        seq: 0,
        title: String::new(),
        status,
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
    }
}

/// A centre built the way a snapshot builds it: one layout.
fn centre(lead_dot: Option<Dot>, jobs: Vec<JobNode>) -> Centre {
    let plan = PlanBoxes {
        plan: PlanId::parse("plan-a").unwrap(),
        context: ContextId::parse("ctx").unwrap(),
        workflows: vec![workflow_node(Some(pull()), lead_dot)],
        jobs,
        needs_user: NeedsUser::default(),
        events_cursor: 0,
    };
    let boxes = boxes::layout(&plan.workflows, &plan.jobs, &plan.needs_user, 0).with_margin(CHART_MARGIN);
    let slot = ChatSlot {
        session: SessionId::parse("00000000-0000-4000-8000-000000000000").unwrap(),
        name: "lead".into(),
        plan: plan.plan.clone(),
        moved_event: 0,
    };
    Centre {
        slot,
        plan,
        boxes,
        frame: 0,
    }
}

#[test]
fn the_plan_view_spins_for_a_lead_dot_with_no_running_job() {
    let queued = || vec![job_node("job-1", JobStatus::Queued)];
    assert!(
        centre(Some(Dot::working(false, None)), queued()).spins(),
        "a working Lead's dot spins"
    );
    assert!(
        !centre(None, queued()).spins(),
        "a Lead not working, no running Job"
    );
    assert!(
        centre(None, vec![job_node("job-1", JobStatus::Running)]).spins(),
        "a running Job spins"
    );
}

#[test]
fn the_plan_view_lays_out_once_per_snapshot_and_a_frame_change_draws_the_same_cells() {
    let before = LAYOUTS.with(|n| n.get());
    let mut c = centre(
        Some(Dot::working(false, None)),
        vec![
            job_node("job-1", JobStatus::Running),
            job_node("job-2", JobStatus::Queued),
        ],
    );
    assert_eq!(
        LAYOUTS.with(|n| n.get()),
        before + 1,
        "one layout for the snapshot"
    );

    let mut cells = Vec::new();
    for frame in 1..=16 {
        c.turn(frame % 8);
        cells.push((frame % 8, c.boxes.chart.buf.clone()));
    }
    assert_eq!(
        LAYOUTS.with(|n| n.get()),
        before + 1,
        "a frame change lays nothing out"
    );

    for (frame, buf) in cells.iter().take(8) {
        let full = boxes::layout(&c.plan.workflows, &c.plan.jobs, &c.plan.needs_user, *frame).with_margin(CHART_MARGIN);
        assert_eq!(&full.chart.buf, buf, "frame {frame}");
    }
}
