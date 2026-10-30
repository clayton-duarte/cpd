//! hs-d738, hs-d739: the Worker's excluded tool list names only the tools that remain. The constant is reachable
//! from outside the crate (`cpd_core::host::pi::WORKER_EXCLUDED_TOOLS`).

use cpd_core::host::pi::WORKER_EXCLUDED_TOOLS;

#[test]
fn drop_plannotator_worker_exclusion_names_only_ask_and_plan_review() {
    assert_eq!(WORKER_EXCLUDED_TOOLS, "ask_user_question,cpd_plan_review");
}
