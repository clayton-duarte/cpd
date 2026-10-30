//! Red tests of the explore box of the Plan view (Plan `explore-roles`; commands.md §13.2 "The explore box"): one box titled
//! `explore` below the PR boxes, drawn like a PR box (pills with the shown Tier, the key and the status, the same connectors for
//! needs) but with no `open` Job row and no pull-request or branch footer; absent when the Plan has no explore Job.
//!
//! An explore Job is a [`JobNode`] with no PR (`pr` is `None`). Assumption the tests pin: the box is the level below the last PR
//! box, [`boxes::GAP`] (two) blank rows under it, and a need is never drawn between a PR box and the explore box.

use super::flow::Live;
use super::{boxes, sample_job, sample_pr};
use crate::read::{JobNode, JobStatus, NeedsUser, PrNode, Pull};
use crate::test_text::rows;
use cpd_core::config::Tier;
use cpd_core::ids::JobKey;

const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";
const TO: &str = "\u{F19B0}";

fn rep(s: &str, n: usize) -> String {
    s.repeat(n)
}

fn sp(n: usize) -> String {
    " ".repeat(n)
}

/// A pill as drawn: caps, icon, key, the bar and the tier.
fn pill(live: Live, key: &str, tier: &str) -> String {
    format!("{CAP_L}{} {key}│{tier}{CAP_R}", live.icon(0).0)
}

/// An explore Job: no PR.
fn explore(key: &str, status: JobStatus, tier: &str, needs: &[&str]) -> JobNode {
    JobNode {
        pr: None,
        status,
        tier: Some(Tier::parse(tier).unwrap()),
        shown_tier: Some(Tier::parse(tier).unwrap()),
        needs: needs.iter().map(|n| JobKey::parse(n).unwrap()).collect(),
        ..sample_job("api", key)
    }
}

/// The merged scout `scout-a` (H2) and the queued research `res-b` (S1) that needs it.
fn explore_jobs() -> Vec<JobNode> {
    vec![explore("scout-a", JobStatus::Merged, "H2", &[]), explore("res-b", JobStatus::Queued, "S1", &["scout-a"])]
}

fn canvas(prs: &[PrNode], jobs: &[JobNode]) -> Vec<String> {
    rows(&boxes::layout(prs, jobs, &NeedsUser::default(), 0).chart.buf)
}

/// The rows of the PR box `api` holding the Job `a`: 38 cells wide (its title is 34 and the walls 4).
fn api_box() -> Vec<String> {
    vec![
        format!("╭─ api · api-repo {TO} main · dispatch ─╮"),
        format!("│ {}{}│", pill(Live::Queued, "a", "S1"), sp(27)),
        format!("╰{}╯", rep("─", 36)),
    ]
}

#[test]
fn plan_view_draws_an_explore_box_below_the_pr_boxes() {
    let got = canvas(&[sample_pr("api")], &[vec![sample_job("api", "a")], explore_jobs()].concat());
    // the explore box: its chart is scout-a's pill (14), five dashes and res-b's (12), so 31 and the box 35 wide, in a canvas of 38
    let want: Vec<String> = [
        api_box(),
        vec![String::new(), String::new()],
        vec![
            format!("╭─ explore {}╮", rep("─", 23)),
            format!("│ {}{}{} │", pill(Live::Merged, "scout-a", "H2"), rep("─", 5), pill(Live::Queued, "res-b", "S1")),
            format!("╰{}╯", rep("─", 33)),
        ],
    ]
    .concat();
    assert_eq!(got, want, "\ngot:\n{}\nwant:\n{}", got.join("\n"), want.join("\n"));
}

#[test]
fn plan_view_omits_the_explore_box_without_explore_jobs() {
    let got = canvas(&[sample_pr("api")], &[sample_job("api", "a")]);
    assert_eq!(got, api_box(), "\n{}", got.join("\n"));
    assert!(!got.join("\n").contains("explore"));
}

#[test]
fn explore_box_has_no_open_job_row_or_pull_request_footer() {
    // a PR with a pull request and a footer (elapsed time and cost), above an explore box
    let pr = PrNode {
        pull: Some(Pull {
            number: 12,
            url: "https://github.com/acme/repo/pull/12".into(),
            state: crate::read::PrState::Open,
            draft: false,
            clean: true,
            conflicting: false,
            lead_working: false,
            update: None,
        }),
        elapsed_seconds: Some(125),
        cost_usd: Some(1.5),
        ..sample_pr("api")
    };
    let got = canvas(&[pr], &[vec![sample_job("api", "a")], explore_jobs()].concat());
    let at = got.iter().position(|r| r.starts_with("╭─ explore"));
    assert!(at.is_some(), "no explore box:\n{}", got.join("\n"));
    let at = at.unwrap();
    let explore_rows = &got[at..];
    // top border, one row of pills, bottom border: no `open` row, no footer on the bottom border
    assert_eq!(explore_rows.len(), 3, "\n{}", explore_rows.join("\n"));
    assert_eq!(explore_rows[2], format!("╰{}╯", rep("─", 33)), "no footer on the bottom border");
    let text = explore_rows.join("\n");
    for absent in ["open", "#12", "main", "·", TO, "$"] {
        assert!(!text.contains(absent), "the explore box shows {absent:?}:\n{text}");
    }
}
