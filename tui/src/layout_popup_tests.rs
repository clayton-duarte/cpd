//! The layout rows of the right-click menu (the tree's): a right click at the first cell,
//! the last cell and the middle of a pane, in a roomy pane and a tiny one. Placed is `Menu::rect`.

use crate::layout_tests::{Case, Click, Sample, check};

const PANES: [(u16, u16); 2] = [(30, 8), (8, 3)];
const CLICKS: [Click; 3] = [Click::First, Click::Last, Click::Middle];

fn rows(subject: &str, sample: fn(u16, u16, Case) -> Sample) {
    for (width, height) in PANES {
        for click in CLICKS {
            let drawn = sample(width, height, Case::Menu(click));
            check(
                &format!("{subject}, a right click {click:?} in a {width}x{height} pane"),
                width,
                &drawn,
            );
        }
    }
}

#[test]
fn layout_tree_menu() {
    rows("tree menu", crate::tree::layout_sample);
}
