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
        generated: &[],
        approved_touches: &[],
        previous: &[],
        answers: &[],
        addenda: &[],
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

/// A Spec file's text whose `resources` is `resources`.
fn spec_file(resources: &str) -> String {
    format!(r#"{{"goal": "Do it.", "context": "", "touches": ["a"], "acceptance": ["b"], "out_of_scope": [], "resources": {resources}}}"#)
}

#[test]
fn workers_in_a_spec_is_spec_invalid() {
    // The starter takes `workers` itself: a Spec never lists it (contract §8.3), alone or with others, at `$.resources`.
    for listed in [r#"["workers"]"#, r#"["e2e-port", "workers"]"#] {
        let err = crate::job::spec::parse(&spec_file(listed)).expect_err(&format!("{listed} is spec_invalid"));
        assert_eq!(err.code(), crate::error::ErrorCode::SpecInvalid, "{listed}");
        let path = err.details()["path"].as_str().unwrap_or_default();
        assert!(path.starts_with("$.resources"), "{listed}: {path}");
    }
    crate::job::spec::parse(&spec_file(r#"["e2e-port"]"#)).expect("another name stays valid");
}
