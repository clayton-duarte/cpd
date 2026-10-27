//! The Worker Spec says which resources its Attempt holds a slot of (commands.md §16.1; slots: contract §8.3).

use std::path::Path;

use super::{Input, JobKind, render};
use crate::config::Tier;
use crate::ids::{AttemptId, PlanId};

fn text(resources: &[String]) -> String {
    let (plan, attempt) = (PlanId::parse("pay").unwrap(), AttemptId::parse("api-schema-a1").unwrap());
    let one = ["a".to_owned()];
    render(&Input {
        plan: &plan,
        attempt: &attempt,
        title: "API schema",
        tier: Tier::parse("S3").unwrap(),
        worktree: Path::new("/w/pay--api-schema-a1"),
        kind: JobKind::Work,
        goal: "Define the schema.",
        context: "None.",
        touches: &one,
        acceptance: &one,
        out_of_scope: &[],
        resources,
        quick_check: &[],
        protected: &[],
        previous: &[],
        answers: &[],
    })
    .unwrap()
}

#[test]
fn resource_spec_the_worker_spec_names_the_resources_it_holds() {
    let held = text(&["e2e-port".to_owned(), "db".to_owned()]);
    assert!(held.contains("`e2e-port`") && held.contains("`db`"), "the Spec names the resources:\n{held}");
    assert!(held.to_lowercase().contains("resource"), "{held}");
    let none = text(&[]);
    assert!(!none.to_lowercase().contains("resource"), "a Job with none says nothing of resources:\n{none}");
}
