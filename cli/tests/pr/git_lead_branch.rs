//! The worktree and branch-clash rules of the Lead Branch `<plan>/<key>` (pro-d112, pro-d113 A4), against the fake git.

use crate::common::fx::Fx;

use std::time::Duration;

use cpd_core::error::ErrorCode;
use cpd_core::git::{Cut, Setup, attempt_branch, create_attempt_worktree, pr_lead_worktree_path, prepare_pr_lead_worktree};
use cpd_core::ids::{AttemptId, PlanId, PrKey};
use cpd_core::git::Pipeline;

fn plan() -> PlanId {
    PlanId::parse("pay-4521").unwrap()
}

fn key() -> PrKey {
    PrKey::parse("api").unwrap()
}

fn setup_in(fx: &Fx) -> Setup {
    Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("logs").join("start.log") }
}

/// Starts the Lead worktree of `branch`; the error is the start's.
fn start(fx: &Fx, repo: &std::path::Path, branch: &str) -> Result<cpd_core::git::Prepared, cpd_core::git::StartFailure> {
    let path = fx.scoped(|| pr_lead_worktree_path(repo, &plan(), &key(), branch)).unwrap();
    fx.scoped(|| prepare_pr_lead_worktree(repo, &plan(), &path, branch, "main", Cut::Base, &setup_in(fx), &Pipeline::default()))
}

#[test]
fn lead_worktree_start_refuses_when_the_plan_branch_exists() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    fx.git(&repo, &["branch", "pay-4521", "main"]);

    let failure = start(&fx, &repo, "pay-4521/api").expect_err("a branch `pay-4521` makes `pay-4521/api` impossible");

    assert_eq!(failure.error.code(), ErrorCode::BranchExists);
    assert_eq!(failure.error.details()["branch"], "pay-4521");
}

#[test]
fn attempt_worktree_is_cut_from_a_lead_branch_named_plan_slash_key() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let lead = fx.env.root().join("lead");
    fx.git(&repo, &["worktree", "add", "-q", "--no-track", "-b", "pay-4521/api", lead.to_str().unwrap(), "main"]);
    let attempt = AttemptId::parse("api-a1").unwrap();

    let made = fx.scoped(|| create_attempt_worktree(&repo, &lead, &plan(), &attempt, &setup_in(&fx))).unwrap();

    assert_eq!(made.branch, "cpd/pay-4521--api-a1");
    assert_eq!(made.branch, attempt_branch(&plan(), &attempt));
}

#[test]
fn guard_a_cpd_plan_slash_key_start_is_refused_when_cpd_plan_exists() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    fx.git(&repo, &["branch", "cpd/pay-4521", "main"]);

    let failure = start(&fx, &repo, "cpd/pay-4521/api").expect_err("refused");

    assert_eq!(failure.error.code(), ErrorCode::BranchExists);
    assert_eq!(failure.error.details()["branch"], "cpd/pay-4521");
}

#[test]
fn guard_a_legacy_start_has_no_parent_check() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");

    let made = start(&fx, &repo, "cpd/pay-4521").expect("started");

    assert_eq!(made.branch, "cpd/pay-4521");
}

#[test]
fn guard_the_lead_worktree_path_of_a_pr() {
    let fx = Fx::with_ports();
    let repo = fx.git_repo("pay");
    let parent = repo.parent().unwrap();
    let path = |branch: &str| fx.scoped(|| pr_lead_worktree_path(&repo, &plan(), &key(), branch)).unwrap();

    assert_eq!(path("pay-4521/api"), parent.join("pay.pay-4521.api"));
    assert_eq!(path("cpd/pay-4521"), parent.join("pay.pay-4521"));
}
