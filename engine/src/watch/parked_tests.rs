//! E20's message (contract §3.7 E20, hs-d731): the keys are sorted and the text begins with the record [`prefix`] names.

use super::parked::text;
use cpd_core::ids::PlanId;
use cpd_core::plan::parked_wake::prefix;

#[test]
fn the_message_lists_the_keys_sorted_after_the_prefix() {
    let plan = PlanId::parse("pay").unwrap();
    let message = text(&plan, &["zeta".to_owned(), "alpha".to_owned()]);
    assert!(
        message.starts_with(&format!(
            "{}alpha, zeta (no pending Plan review, no unfinished need).",
            prefix(&plan)
        )),
        "{message}"
    );
    assert!(message.ends_with("Nothing runs it otherwise."), "{message}");
}
