//! The tree passes `--by-user` to `plan new` and `context manager` (hs-d542).

use super::*;

#[test]
fn new_plan_passes_by_user() {
    let action = Action::NewPlan {
        context: ContextId::parse("alpha").unwrap(),
        id: PlanId::parse("pay-4521").unwrap(),
        title: "Pay 4521".into(),
    };
    assert_eq!(
        action.argv(),
        [
            "plan",
            "new",
            "pay-4521",
            "--context=alpha",
            "--title=Pay 4521",
            "--by-user"
        ]
    );
}

#[test]
fn open_manager_passes_by_user() {
    let action = Action::OpenManager(ContextId::parse("alpha").unwrap());
    assert_eq!(
        action.argv(),
        ["context", "manager", "--context=alpha", "--by-user"]
    );
}
