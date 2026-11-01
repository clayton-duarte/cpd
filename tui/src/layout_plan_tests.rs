//! The layout rows of the Plan view's PR boxes: an empty PR, and a PR box next to another, at the canvas's corner and
//! scrolled to its middle. A box the pane clips is checked for nothing outside the container only.

use crate::layout_tests::{PlanCase, Scroll, check_at};

const WIDTHS: [u16; 4] = [40, 20, 8, 4];
const HEIGHT: u16 = 8;

#[test]
fn layout_pr_box() {
    for width in WIDTHS {
        for scroll in [Scroll::Corner, Scroll::Middle] {
            for case in [PlanCase::Empty(scroll), PlanCase::Three(scroll)] {
                let drawn = crate::plan::layout_sample(width, HEIGHT, case);
                check_at(
                    &format!("pr box, {case:?}, {width}x{HEIGHT}"),
                    width,
                    &drawn,
                    scroll,
                );
            }
        }
    }
}
