//! The layout rows of the tree pane (another Job's: the Inbox and the Context fold).

use crate::layout_tests::{TreeCase, check};

const WIDTHS: [u16; 5] = [43, 24, 12, 6, 3];

#[test]
fn layout_context_fold() {
    for width in WIDTHS {
        let drawn = crate::tree::layout_fold_sample(width, 40, TreeCase::ContextFold);
        check(&format!("context fold, {width}x40"), width, &drawn);
    }
}
