//! The visible φ mark on Session labels (contract §3.6, §3.9; hs-d694): the loop guard's label
//! is exactly `φ: loop guard`, not the old cpd label.

use super::LOOP_GUARD_LABEL;

#[test]
fn loop_guard_label_is_phi() {
    assert_eq!(LOOP_GUARD_LABEL, "φ: loop guard");
}
