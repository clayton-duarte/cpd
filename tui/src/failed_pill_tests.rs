//! The Plan view's failed pill (hs-d586, commands.md §13.2 'Chart' and 'legend'): always red, never cyan; the 28 moon_alt loader (hs-d616)
//! in place of the close circle while the Plan's Lead works, every failed pill of the Plan together; a cancelled pill
//! stays still. Glyphs are codepoints and colours come from the palette's calls, never raw values.

use cpd_core::config::Tier;
use cpd_core::ids::{JobKey, PrKey};
use ratatui::style::Style;

use crate::palette;
use crate::plan::boxes;
use crate::read::{JobNode, JobStatus, NeedsUser, PrNode, PrStatus};
use crate::widgets::legend;

const MOONS: [&str; 28] = [
    "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}",
    "\u{E3CE}", "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}",
    "\u{E3D5}", "\u{E3D6}", "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}",
    "\u{E3DC}", "\u{E3DD}", "\u{E3DE}", "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
];
const CLOSE_CIRCLE: &str = "\u{F0159}";

fn pr() -> PrNode {
    PrNode {
        key: PrKey::parse("api").unwrap(),
        repo_name: "api-repo".into(),
        base: "main".into(),
        status: PrStatus::Dispatch,
        needs: Vec::new(),
        pull: None,
        waiting_on: Vec::new(),
        superseded_by: None,
    }
}

fn job(key: &str, status: JobStatus) -> JobNode {
    JobNode {
        key: JobKey::parse(key).unwrap(),
        pr: PrKey::parse("api").unwrap(),
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

fn working() -> NeedsUser {
    NeedsUser { lead_working: true, ..NeedsUser::default() }
}

/// The icon cell's glyph and the pill's colours, for each Job of `jobs`, at `frame`.
fn pills(jobs: &[JobNode], needs: &NeedsUser, frame: usize) -> Vec<(String, Style)> {
    let b = boxes::layout(&[pr()], jobs, needs, frame);
    b.chart
        .pills
        .iter()
        .map(|p| {
            let cell = &b.chart.buf[(p.rect.x + 1, p.rect.y)];
            (cell.symbol().to_string(), Style::new().fg(cell.fg).bg(cell.bg))
        })
        .collect()
}

fn red_pill() -> Style {
    let s = palette::job_pill(JobStatus::Failed, false, true);
    Style::new().fg(s.fg.unwrap()).bg(s.bg.unwrap())
}

fn cancelled_pill() -> Style {
    let s = palette::job_pill(JobStatus::Cancelled, false, false);
    Style::new().fg(s.fg.unwrap()).bg(s.bg.unwrap())
}

#[test]
fn a_failed_pill_is_red_with_the_close_circle_when_its_lead_is_not_working() {
    let jobs = [job("a", JobStatus::Failed)];
    let stuck = NeedsUser { stuck_jobs: vec![jobs[0].key.clone()], ..NeedsUser::default() };
    for (label, needs) in [("no job-stuck item", NeedsUser::default()), ("a job-stuck item", stuck)] {
        for frame in 0..8 {
            assert_eq!(pills(&jobs, &needs, frame), [(CLOSE_CIRCLE.to_owned(), red_pill())], "{label}, frame {frame}");
        }
    }
}

#[test]
fn a_failed_pill_turns_through_the_28_moon_alt_phases_while_its_lead_works() {
    let jobs = [job("a", JobStatus::Failed)];
    let stuck = NeedsUser { stuck_jobs: vec![jobs[0].key.clone()], lead_working: true, ..NeedsUser::default() };
    for needs in [working(), stuck] {
        for frame in 0..30 {
            assert_eq!(pills(&jobs, &needs, frame), [(MOONS[frame % 28].to_owned(), red_pill())], "frame {frame}");
        }
    }
}

#[test]
fn every_failed_pill_turns_together_and_a_cancelled_pill_stays_still() {
    let jobs = [job("a", JobStatus::Failed), job("b", JobStatus::Failed), job("c", JobStatus::Cancelled)];
    for frame in 0..28 {
        let got = pills(&jobs, &working(), frame);
        assert_eq!(got[0], (MOONS[frame].to_owned(), red_pill()), "a, frame {frame}");
        assert_eq!(got[1], got[0], "b turns with a, frame {frame}");
        assert_eq!(got[2], (CLOSE_CIRCLE.to_owned(), cancelled_pill()), "cancelled: colour 8, static, frame {frame}");
    }
}

#[test]
fn the_plan_view_redraws_every_frame_for_a_failed_pill_while_the_lead_works() {
    let failed = [job("a", JobStatus::Failed), job("b", JobStatus::Merged)];
    assert!(crate::plan::spins(&failed, &working()), "a failed Job, its Lead working, nothing running");
    assert!(!crate::plan::spins(&failed, &NeedsUser::default()), "its Lead is not working: static");
    let cancelled = [job("a", JobStatus::Cancelled)];
    assert!(!crate::plan::spins(&cancelled, &working()), "a cancelled pill never turns");
}

#[test]
fn the_legend_lists_one_failed_pill_in_red_and_its_red_loader() {
    let sections = legend::sections();
    let jobs = sections.iter().find(|s| s.title.contains("Job")).expect("a Jobs section");
    let name = |row: &Vec<ratatui::text::Line>| row[0].spans.iter().map(|s| s.content.to_string()).collect::<String>();
    let failed: Vec<_> = jobs.rows.iter().filter(|r| name(r).starts_with("failed")).collect();
    assert_eq!(failed.len(), 1, "one failed pill row, no stuck row: {:?}", failed.iter().map(|r| name(r)).collect::<Vec<_>>());
    assert!(!jobs.rows.iter().any(|r| name(r).contains("stuck")), "no stuck row");
    let body = &failed[0][1].spans[1];
    assert!(body.content.starts_with(CLOSE_CIRCLE), "the static failed glyph: {:?}", body.content);
    assert_eq!(body.style, palette::job_pill(JobStatus::Failed, false, true), "the failed pill is red");
    // the red loader: the 28 moons, each in the failed (red) colour
    let red = palette::fg_clickable(palette::Meaning::Failed);
    let loader = sections.iter().flat_map(|s| s.rows.iter().flatten()).any(|line| {
        line.spans.len() == 28 && line.spans.iter().zip(MOONS).all(|(s, m)| s.content == m && s.style == red)
    });
    assert!(loader, "a red loader sample with the 28 moon frames");
}
