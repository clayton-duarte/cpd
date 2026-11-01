//! Red tests of the lint engine (contract §2.7): pure calls, no store, no git, no process.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::jobs::{Job, JobSpec};
use super::lint::{Finding, Input, Rule, Severity, first_error, run};
use super::outline::Outline;
use super::prs::WorkflowRow;
use super::row::Phase;
use crate::error::ErrorCode;
use crate::ids::PlanId;

const MESSAGE_TAIL: &str = "A hint, not a conflict prediction: on the ledger of 2026-10-30, 5 of 209 such pairs (2.4%) hit a merge conflict.";

fn job(key: &str, workflow: &str, kind: &str, status: &str, touches: &[&str], needs: &[&str]) -> Job {
    Job {
        seq: 1,
        key: key.into(),
        workflow: Some(workflow.into()),
        title: format!("Title {key}"),
        tier: Some("S2".into()),
        spec: Some(JobSpec {
            goal: "g".into(),
            context: "c".into(),
            touches: json!(touches),
            acceptance: json!(["b"]),
            out_of_scope: json!([]),
        }),
        status: status.into(),
        appended: false,
        kind: kind.into(),
        tests: None,
        test_files: None,
        resources: None,
        test_approved: None,
        touches_approved: None,
        urgent: false,
        fingerprint: None,
        retry_tier: None,
        cancel_reason: None,
        cancel_requested_at: None,
        failed_cause: None,
        merged_sha: None,
        summary: None,
        created_at: "t".into(),
        needs: needs.iter().map(|n| (*n).to_owned()).collect(),
    }
}

/// A draft `work` Job of Workflow `web`.
fn draft(key: &str, touches: &[&str], needs: &[&str]) -> Job {
    job(key, "web", "work", "draft", touches, needs)
}

fn stub(key: &str) -> Job {
    let mut j = draft(key, &[], &[]);
    j.tier = None;
    j.spec = None;
    j
}

fn workflow(key: &str) -> WorkflowRow {
    WorkflowRow::fixture(key, "/repo", "main", &[], "waiting")
}

fn tdd(key: &str) -> WorkflowRow {
    let mut p = workflow(key);
    p.shape = "tdd".into();
    p
}

fn outline(prs: &[&str]) -> Outline {
    Outline {
        assumptions: vec![],
        decided: vec![],
        decisions: vec![],
        done_when: prs
            .iter()
            .map(|k| ((*k).to_owned(), vec!["done".to_owned()]))
            .collect(),
        goal: "g".into(),
        out_of_scope: vec![],
        risks: vec![],
    }
}

fn lint_with(phase: Phase, workflows: &[WorkflowRow], jobs: &[Job], outline: Option<&Outline>) -> Vec<Finding> {
    let plan = PlanId::parse("pl").unwrap();
    run(&Input::new(&plan, phase, workflows, jobs, outline))
}

/// Findings of the Plan with an outline for `web` and Workflow `web`, in phase `dispatch` (no workflow_empty).
fn lint(jobs: &[Job]) -> Vec<Finding> {
    lint_with(
        Phase::Dispatch,
        &[workflow("web")],
        jobs,
        Some(&outline(&["web"])),
    )
}

fn overlaps(findings: &[Finding]) -> Vec<&Finding> {
    of(findings, Rule::ParallelWriteOverlap)
}

fn of(findings: &[Finding], rule: Rule) -> Vec<&Finding> {
    findings.iter().filter(|f| f.rule == rule).collect()
}

fn rules(findings: &[Finding]) -> Vec<Rule> {
    findings.iter().map(|f| f.rule).collect()
}

/// A positive control: the two Jobs overlap on `shared.rs` and give one finding.
fn assert_fires(jobs: &[Job]) {
    assert_eq!(overlaps(&lint(jobs)).len(), 1, "control: {jobs:?}");
}

fn assert_silent(jobs: &[Job]) {
    assert!(overlaps(&lint(jobs)).is_empty());
}

#[test]
fn parallel_write_overlap_reports_equal_files_of_parallel_jobs() {
    let jobs = [
        draft("a", &["src/z.rs", "src/a.rs"], &[]),
        draft("b", &["src/a.rs", "src/z.rs", "src/b.rs"], &[]),
    ];
    let findings = lint(&jobs);
    let hits = overlaps(&findings);
    assert_eq!(hits.len(), 1);
    let f = hits[0];
    assert_eq!(f.severity(), Severity::Warn);
    assert_eq!(f.pr.as_deref(), Some("web"));
    assert_eq!(f.job.as_deref(), Some("a"));
    assert_eq!(f.other.as_deref(), Some("b"));
    assert_eq!(f.paths, ["src/a.rs", "src/z.rs"]);
    assert!(f.message.contains("not a conflict prediction"));
    assert!(f.message.contains("5 of 209"));
    assert!(f.message.contains("2.4%"));
    assert_eq!(
        f.message,
        format!(
            "a and b have no dependency path and both name src/a.rs, src/z.rs. {MESSAGE_TAIL} Settle it with cpd job edit a --needs b, or narrow touches."
        )
    );
}

#[test]
fn parallel_write_overlap_ignores_jobs_with_a_dependency_path_either_way() {
    let base = [draft("a", &["x.rs"], &[]), draft("b", &["x.rs"], &[])];
    assert_fires(&base);
    assert_silent(&[draft("a", &["x.rs"], &["b"]), draft("b", &["x.rs"], &[])]);
    assert_silent(&[draft("a", &["x.rs"], &[]), draft("b", &["x.rs"], &["a"])]);
    assert_silent(&[
        draft("a", &["x.rs"], &["m"]),
        draft("m", &[], &["b"]),
        draft("b", &["x.rs"], &[]),
    ]);
    assert_silent(&[
        draft("a", &["x.rs"], &["m"]),
        job("m", "web", "work", "cancelled", &[], &["b"]),
        draft("b", &["x.rs"], &[]),
    ]);
}

#[test]
fn parallel_write_overlap_ignores_directory_entries() {
    assert_fires(&[
        draft("a", &["docs/contract.md"], &[]),
        draft("b", &["docs/contract.md"], &[]),
    ]);
    assert_silent(&[
        draft("a", &["docs/"], &[]),
        draft("b", &["docs/contract.md"], &[]),
    ]);
    assert_silent(&[draft("a", &["docs/"], &[]), draft("b", &["docs/"], &[])]);
}

#[test]
fn parallel_write_overlap_ignores_open_and_update_and_cancelled_jobs() {
    assert_fires(&[
        draft("a", &["x.rs"], &[]),
        job("b", "web", "work", "queued", &["x.rs"], &[]),
    ]);
    assert_silent(&[
        draft("a", &["x.rs"], &[]),
        job("b", "web", "open", "queued", &["x.rs"], &[]),
    ]);
    assert_silent(&[
        draft("a", &["x.rs"], &[]),
        job("b", "web", "update", "queued", &["x.rs"], &[]),
    ]);
    assert_silent(&[
        draft("a", &["x.rs"], &[]),
        job("b", "web", "work", "cancelled", &["x.rs"], &[]),
    ]);
    assert_silent(&[
        job("a", "web", "open", "draft", &["x.rs"], &[]),
        job("b", "web", "work", "queued", &["x.rs"], &[]),
    ]);
}

#[test]
fn parallel_write_overlap_needs_a_draft_under_review() {
    let queued = |k: &str| job(k, "web", "work", "queued", &["x.rs"], &[]);
    assert_fires(&[draft("a", &["x.rs"], &[]), queued("b")]);
    assert_silent(&[queued("a"), queued("b")]);
    assert_silent(&[
        draft("a", &["x.rs"], &[]),
        job("b", "web", "work", "merged", &["x.rs"], &[]),
    ]);
}

#[test]
fn jobs_of_different_workflows_are_not_compared() {
    let same = [draft("a", &["x.rs"], &[]), draft("b", &["x.rs"], &[])];
    assert_fires(&same);
    let other = [
        draft("a", &["x.rs"], &[]),
        job("b", "api", "work", "draft", &["x.rs"], &[]),
    ];
    let findings = lint_with(
        Phase::Dispatch,
        &[workflow("web"), workflow("api")],
        &other,
        Some(&outline(&["web", "api"])),
    );
    assert!(overlaps(&findings).is_empty());
    let findings = lint_with(
        Phase::Dispatch,
        &[workflow("web"), workflow("api")],
        &same,
        Some(&outline(&["web", "api"])),
    );
    assert_eq!(overlaps(&findings).len(), 1);
}

#[test]
fn one_finding_per_pair_with_sorted_paths() {
    let jobs = [
        draft("a", &["c.rs", "a.rs", "b.rs"], &[]),
        draft("b", &["b.rs", "c.rs", "a.rs"], &[]),
        draft("c", &["a.rs"], &[]),
    ];
    let findings = lint(&jobs);
    let hits = overlaps(&findings);
    assert_eq!(hits.len(), 3);
    let first = hits
        .iter()
        .find(|f| f.job.as_deref() == Some("a") && f.other.as_deref() == Some("b"))
        .unwrap();
    assert_eq!(first.paths, ["a.rs", "b.rs", "c.rs"]);
    let pairs: BTreeSet<(String, String)> = hits
        .iter()
        .map(|f| (f.job.clone().unwrap(), f.other.clone().unwrap()))
        .collect();
    assert_eq!(pairs.len(), 3);
}

#[test]
fn stub_drafts_compare_nothing() {
    assert_fires(&[draft("a", &["x.rs"], &[]), draft("b", &["x.rs"], &[])]);
    assert_silent(&[stub("a"), draft("b", &["x.rs"], &[])]);
    assert_silent(&[stub("a"), stub("b")]);
}

#[test]
fn lockfiles_are_exempt() {
    for name in [
        "Cargo.lock",
        "core/Cargo.lock",
        "pnpm-lock.yaml",
        "package-lock.json",
        "yarn.lock",
    ] {
        assert_fires(&[
            draft("a", &["Cargo.toml"], &[]),
            draft("b", &["Cargo.toml"], &[]),
        ]);
        assert_silent(&[draft("a", &[name], &[]), draft("b", &[name], &[])]);
    }
}

#[test]
fn run_is_deterministic_and_orders_findings() {
    let prs = [tdd("web"), tdd("api")];
    let jobs = [
        draft("b", &["x.rs"], &[]),
        draft("a", &["x.rs"], &[]),
        job("c", "api", "work", "draft", &["y.rs"], &[]),
        job("d", "api", "work", "draft", &["y.rs"], &[]),
        stub("s"),
    ];
    let go = || lint_with(Phase::Dispatch, &prs, &jobs, None);
    let findings = go();
    assert_eq!(findings, go());
    assert!(!findings.is_empty());
    let order: Vec<Rule> = rules(&findings);
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
    assert_eq!(order.first(), Some(&Rule::DraftIncomplete));
    assert_eq!(order.last(), Some(&Rule::ParallelWriteOverlap));
    let shape: Vec<Option<&str>> = of(&findings, Rule::ShapeIncomplete)
        .iter()
        .map(|f| f.pr.as_deref())
        .collect();
    assert_eq!(shape, [Some("api"), Some("web")]);
    let ov: Vec<(Option<&str>, Option<&str>)> = overlaps(&findings)
        .iter()
        .map(|f| (f.pr.as_deref(), f.job.as_deref()))
        .collect();
    assert_eq!(ov, [(Some("api"), Some("c")), (Some("web"), Some("a"))]);
}

#[test]
fn run_reports_every_error_not_only_the_first() {
    let prs = [tdd("web")];
    let jobs = [
        job("t", "web", "test", "draft", &[], &[]),
        draft("a", &[], &["b"]),
        draft("b", &[], &["a"]),
        stub("s"),
    ];
    let findings = lint_with(Phase::Dispatch, &prs, &jobs, Some(&outline(&["web"])));
    let got = rules(&findings);
    for rule in [
        Rule::DraftIncomplete,
        Rule::ShapeIncomplete,
        Rule::DependencyCycle,
    ] {
        assert!(got.contains(&rule), "{rule:?} in {got:?}");
    }
}

#[test]
fn first_error_is_the_refusal_of_the_first_error_finding() {
    let prs = [tdd("web")];
    let jobs = [job("t", "web", "test", "draft", &[], &[]), stub("s")];
    let findings = lint_with(Phase::Dispatch, &prs, &jobs, Some(&outline(&["web"])));
    assert!(rules(&findings).contains(&Rule::ShapeIncomplete));
    let err = first_error(&findings).expect("an error finding");
    assert_eq!(err.code(), ErrorCode::DraftIncomplete);
    assert_eq!(
        err.details()["jobs"],
        json!([{"key": "s", "missing": ["tier", "spec"]}])
    );
    assert!(err.message().contains('s'));
    assert!(err.message().contains("s."), "{}", err.message());
}

#[test]
fn a_waiting_stub_is_not_exempt_the_review_refuses_a_stub_whether_it_waits_or_not() {
    // The Workflow `web` waits (status `waiting`, phase Dispatch): its stub is a finding all the same.
    let prs = [tdd("web")];
    let jobs = [job("t", "web", "test", "draft", &[], &[]), stub("s")];
    let findings = lint_with(Phase::Dispatch, &prs, &jobs, Some(&outline(&["web"])));
    assert_eq!(of(&findings, Rule::DraftIncomplete).len(), 1);
    let err = first_error(&findings).expect("an error finding");
    assert_eq!(err.code(), ErrorCode::DraftIncomplete);
    assert_eq!(
        err.details()["jobs"],
        json!([{"key": "s", "missing": ["tier", "spec"]}])
    );
}

#[test]
fn first_error_ignores_warnings() {
    let overlap = [draft("a", &["x.rs"], &[]), draft("b", &["x.rs"], &[])];
    let findings = lint(&overlap);
    assert_eq!(overlaps(&findings).len(), 1, "control");
    assert!(first_error(&findings).is_none());
    let mut with_error = overlap.to_vec();
    with_error.push(stub("s"));
    let findings = lint(&with_error);
    assert_eq!(overlaps(&findings).len(), 1, "control");
    assert_eq!(
        first_error(&findings).unwrap().code(),
        ErrorCode::DraftIncomplete
    );
}

#[test]
fn pr_empty_fires_only_in_phase_plan() {
    let prs = [workflow("web"), workflow("api")];
    let jobs = [draft("a", &[], &[])];
    let o = outline(&["web"]);
    let in_plan = lint_with(Phase::Plan, &prs, &jobs, Some(&o));
    let hits = of(&in_plan, Rule::WorkflowEmpty);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].details["prs"], json!(["api"]));
    let later = lint_with(Phase::Dispatch, &prs, &jobs, Some(&o));
    assert!(of(&later, Rule::WorkflowEmpty).is_empty());
}

#[test]
fn pr_empty_with_no_live_pr_names_pr_add() {
    let findings = lint_with(Phase::Plan, &[], &[], Some(&outline(&[])));
    let hits = of(&findings, Rule::WorkflowEmpty);
    assert_eq!(hits.len(), 1);
    assert!(
        hits[0].details["next"]
            .as_str()
            .unwrap()
            .starts_with("cpd workflow add")
    );
}

#[test]
fn dependency_cycle_finding_lists_the_cycle() {
    let findings = lint(&[draft("a", &[], &["b"]), draft("b", &[], &["a"])]);
    let hits = of(&findings, Rule::DependencyCycle);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].severity(), Severity::Error);
    let cycle: Vec<&str> = hits[0].details["cycle"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(cycle.contains(&"a") && cycle.contains(&"b"), "{cycle:?}");
    assert!(
        of(
            &lint(&[draft("a", &[], &[]), draft("b", &[], &["a"])]),
            Rule::DependencyCycle
        )
        .is_empty()
    );
}

#[test]
fn outline_findings_follow_the_outline() {
    let jobs = [draft("a", &[], &[])];
    let missing = lint_with(Phase::Dispatch, &[workflow("web")], &jobs, None);
    assert_eq!(of(&missing, Rule::OutlineMissing).len(), 1);
    assert!(of(&missing, Rule::OutlineIncomplete).is_empty());
    let incomplete = lint_with(Phase::Dispatch, &[workflow("web")], &jobs, Some(&outline(&[])));
    let hits = of(&incomplete, Rule::OutlineIncomplete);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].details["workflows"], json!(["web"]));
    assert!(of(&incomplete, Rule::OutlineMissing).is_empty());
    assert!(of(&lint(&jobs), Rule::OutlineIncomplete).is_empty());
}

#[test]
fn shape_findings_are_separate_rules() {
    let prs = [tdd("web")];
    let o = outline(&["web"]);
    let run_it = |jobs: &[Job]| lint_with(Phase::Dispatch, &prs, jobs, Some(&o));
    let impl_only = run_it(&[job("i", "web", "impl", "draft", &[], &[])]);
    assert_eq!(of(&impl_only, Rule::TestJobRequired).len(), 1);
    let verify_only = run_it(&[job("v", "web", "verify", "draft", &[], &[])]);
    assert_eq!(of(&verify_only, Rule::ImplJobRequired).len(), 1);
    let test_only = run_it(&[job("t", "web", "test", "draft", &[], &[])]);
    let hits = of(&test_only, Rule::ShapeIncomplete);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].pr.as_deref(), Some("web"));
    assert_eq!(hits[0].details["missing"], json!(["impl"]));
    let whole = run_it(&[
        job("t", "web", "test", "draft", &[], &[]),
        job("i", "web", "impl", "draft", &[], &["t"]),
        job("v", "web", "verify", "draft", &[], &["i"]),
    ]);
    assert!(
        rules(&whole)
            .iter()
            .all(|r| *r == Rule::ParallelWriteOverlap)
    );
    assert!(whole.iter().all(|f| f.severity() == Severity::Warn));
}

#[test]
fn finding_json_has_every_key() {
    let findings = lint(&[
        draft("a", &["x.rs"], &[]),
        draft("b", &["x.rs"], &[]),
        stub("s"),
    ]);
    assert!(findings.len() >= 2);
    for f in &findings {
        let v: Value = f.to_json();
        let obj = v.as_object().unwrap();
        let keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "rule", "severity", "message", "pr", "job", "other", "paths", "details"
            ])
        );
        assert_eq!(obj["rule"], f.rule.as_str());
        assert_eq!(obj["severity"], f.severity().as_str());
    }
    let warn = overlaps(&findings)[0].to_json();
    assert_eq!(warn["details"], json!({}));
    assert_eq!(warn["paths"], json!(["x.rs"]));
}
