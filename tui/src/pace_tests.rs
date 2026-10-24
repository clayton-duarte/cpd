//! Red tests of the loader fix (commands.md §17 and §13.2, rules R1-R4).
//!
//! Manual check, which the implementation Job and the remeasure Job rely on: run the plan view with `CPD_TUI_TRACE=1`
//! against a Lead that works and no running Job. At every frame change the trace shows `gap_us` about 100000 and
//! `layout_us` 0 (before the fix: one frame in 35 s, and `layout_us` of 13-18 ms at p50 when a Job runs).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::pace::{Reads, wait_for_next_frame};
use crate::palette;
use crate::plan::boxes::{self, LAYOUTS};
use crate::plan::{CHART_MARGIN, Centre};
use crate::read::{JobNode, JobStatus, NeedsUser, PlanBoxes, PrNode, PrState, PrStatus, Pull, ChatSlot};
use cpd_core::config::Tier;
use cpd_core::ids::{ContextId, JobKey, PlanId, PrKey, SessionId};

fn phase(ms: u64) -> usize {
    palette::frame(UNIX_EPOCH + Duration::from_millis(ms))
}

#[test]
fn the_wait_runs_to_the_next_100_ms_boundary_of_the_clock() {
    let ms = Duration::from_millis;
    assert_eq!(wait_for_next_frame(1_000), ms(100));
    assert_eq!(wait_for_next_frame(1_001), ms(99));
    assert_eq!(wait_for_next_frame(1_099), ms(1));
    for now in [0u64, 1, 50, 99, 100, 777, 123_456_789] {
        let wait = wait_for_next_frame(now);
        assert_eq!(phase(now + wait.as_millis() as u64), (phase(now) + 1) % 8, "now {now}");
    }
}

#[test]
fn a_draw_under_a_frame_long_skips_no_phase() {
    let mut t: u64 = 1_700_000_000_037;
    let mut seen = Vec::new();
    for _ in 0..80 {
        t += wait_for_next_frame(t).as_millis() as u64;
        seen.push(phase(t));
        t += 15; // the draw's work
    }
    for pair in seen.windows(2) {
        assert_eq!(pair[1], (pair[0] + 1) % 8, "phases {seen:?}");
    }
}

#[test]
fn a_draw_over_a_frame_long_is_followed_by_one_draw_per_frame_not_a_burst() {
    let mut t: u64 = 1_700_000_000_037;
    let mut ticks = Vec::new();
    for _ in 0..40 {
        t += wait_for_next_frame(t).as_millis() as u64;
        ticks.push(t / 100);
        t += 250; // the draw's work
    }
    for pair in ticks.windows(2) {
        assert_ne!(pair[0], pair[1], "two draws in one frame: {ticks:?}");
        assert!(pair[1] - pair[0] <= 3, "more than 3 phases apart: {ticks:?}");
    }
}

#[test]
fn a_slow_reader_never_delays_the_draw_cadence() {
    let mut reads: Reads<u32> = Reads::new();
    let began = Instant::now();
    reads.request(|| {
        std::thread::sleep(Duration::from_millis(1200));
        7
    });
    let mut last = Instant::now();
    let mut widest = Duration::ZERO;
    while began.elapsed() < Duration::from_millis(1500) {
        let now_ms = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        std::thread::sleep(wait_for_next_frame(now_ms));
        let got = reads.take();
        if began.elapsed() < Duration::from_millis(1190) {
            assert!(got.is_none(), "a result before the job ended");
        }
        widest = widest.max(last.elapsed());
        last = Instant::now();
    }
    assert!(widest < Duration::from_millis(250), "the widest gap between draws was {widest:?}");
}

#[test]
fn one_read_runs_at_a_time_and_the_newest_waiting_request_wins() {
    let mut reads: Reads<u32> = Reads::new();
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
    assert!(!second_ran.load(Ordering::SeqCst), "the older waiting request ran");
    assert!(!overlapped.load(Ordering::SeqCst), "two jobs ran at once");
    assert_eq!(reads.take(), None);
}

fn pr_node(pull: Option<Pull>) -> PrNode {
    PrNode {
        key: PrKey::parse("web").unwrap(),
        repo_name: "repo".into(),
        base: "main".into(),
        status: PrStatus::Dispatch,
        needs: Vec::new(),
        pull,
        waiting_on: Vec::new(),
        superseded_by: None,
    }
}

fn pull(lead_working: bool) -> Pull {
    Pull { number: 12, state: PrState::Open, url: "https://github.com/acme/repo/pull/12".into(), conflicting: false, clean: false, lead_working, update: None }
}

fn job_node(key: &str, status: JobStatus) -> JobNode {
    JobNode {
        key: JobKey::parse(key).unwrap(),
        pr: PrKey::parse("web").unwrap(),
        seq: 0,
        title: String::new(),
        status,
        tier: Some(Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        appended: false,
        retry_tier: None,
        failed_cause: None,
        cancel_reason: None,
        merged_sha: None,
        waiting_on: Vec::new(),
        attempts: Vec::new(),
    }
}

/// A centre built the way a snapshot builds it: one layout.
fn centre(pull: Option<Pull>, jobs: Vec<JobNode>) -> Centre {
    let plan = PlanBoxes {
        plan: PlanId::parse("plan-a").unwrap(),
        context: ContextId::parse("ctx").unwrap(),
        prs: vec![pr_node(pull)],
        jobs,
        needs_user: NeedsUser::default(),
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

#[test]
fn the_plan_view_spins_for_a_lead_dot_with_no_running_job() {
    let queued = || vec![job_node("job-1", JobStatus::Queued)];
    assert!(centre(Some(pull(true)), queued()).spins(), "a working Lead's dot spins");
    assert!(!centre(Some(pull(false)), queued()).spins(), "a Lead not working, no running Job");
    assert!(centre(Some(pull(false)), vec![job_node("job-1", JobStatus::Running)]).spins(), "a running Job spins");
}

#[test]
fn the_plan_view_lays_out_once_per_snapshot_and_a_frame_change_draws_the_same_cells() {
    let before = LAYOUTS.with(|n| n.get());
    let mut c = centre(Some(pull(true)), vec![job_node("job-1", JobStatus::Running), job_node("job-2", JobStatus::Queued)]);
    assert_eq!(LAYOUTS.with(|n| n.get()), before + 1, "one layout for the snapshot");

    let mut cells = Vec::new();
    for frame in 1..=16 {
        c.turn(frame % 8);
        cells.push((frame % 8, c.boxes.chart.buf.clone()));
    }
    assert_eq!(LAYOUTS.with(|n| n.get()), before + 1, "a frame change lays nothing out");

    for (frame, buf) in cells.iter().take(8) {
        let full = boxes::layout(&c.plan.prs, &c.plan.jobs, &c.plan.needs_user, *frame).with_margin(CHART_MARGIN);
        assert_eq!(&full.chart.buf, buf, "frame {frame}");
    }
}
