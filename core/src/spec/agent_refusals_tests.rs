//! The writing-for-agents pointer (commands.md §16.1; contract §8.3 "Delivery to agents"):
//! a Spec whose declared `touches` name a skill points the Worker at the bundled reference,
//! and a Spec whose touches name no skill file carries no pointer.

use std::path::Path;

use super::{Input, JobKind, render};
use crate::config::Tier;
use crate::ids::{AttemptId, PlanId};

/// The exact line a skill-touching Spec carries (commands.md §16.1).
const POINTER: &str = "Read and follow pi/skills/writing-for-agents/SKILL.md before editing any file under pi/skills/.";

/// Renders a `work` Spec whose touches are `touches`.
fn rendered(touches: &[String]) -> String {
    let (plan, attempt) = (
        PlanId::parse("pay").unwrap(),
        AttemptId::parse("api-schema-a1").unwrap(),
    );
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
        touches,
        acceptance: &one,
        out_of_scope: &[],
        resources: &[],
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
fn injects_writing_for_agents_pointer() {
    let touches = ["pi/skills/cpd-lead/SKILL.md".to_owned()];
    let spec = rendered(&touches);
    assert!(
        spec.contains(POINTER),
        "a Job touching pi/skills/** carries the writing-for-agents pointer:\n{spec}"
    );
}

#[test]
fn omits_pointer_when_no_skill_touched() {
    let code = ["core/src/spec/mod.rs".to_owned()];
    let spec = rendered(&code);
    assert!(
        !spec.contains(POINTER),
        "a Job touching no skill file carries no pointer:\n{spec}"
    );
    // The control makes this test red until the pointer exists at all: the omission means
    // nothing while the positive case is missing.
    let skill = ["pi/skills/cpd-manager/SKILL.md".to_owned()];
    assert!(
        rendered(&skill).contains(POINTER),
        "the control: a Job touching pi/skills/** carries the pointer"
    );
}
