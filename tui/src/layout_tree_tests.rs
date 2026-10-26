//! The layout rows of the tree pane (another Job's: the Inbox and the Context fold).

use crate::layout_tests::{TreeCase, check};

const WIDTHS: [u16; 5] = [43, 24, 12, 6, 3];

#[test]
fn layout_inbox() {
    let cases = [(TreeCase::InboxNone, 12), (TreeCase::Inbox3, 12), (TreeCase::Inbox5Folded, 12), (TreeCase::Inbox5Expanded, 12), (TreeCase::InboxCues, 8)];
    for width in WIDTHS {
        for (case, height) in cases {
            let drawn = crate::widgets::inbox::layout_sample(width, height, case);
            check(&format!("inbox, {case:?}, {width}x{height}"), width, &drawn);
        }
    }
}

#[test]
fn layout_context_fold() {
    for width in WIDTHS {
        let drawn = crate::tree::layout_fold_sample(width, 40, TreeCase::ContextFold);
        check(&format!("context fold, {width}x40"), width, &drawn);
    }
}
