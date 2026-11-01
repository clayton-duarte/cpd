//! The logs popup's tmux arguments (`popup_args`), pure: the pane tests of `cli/tests/tui/tui_logs.rs` that only called it.

use super::popup_args;
use crate::widgets::legend;

#[test]
fn the_popup_is_sized_like_the_legends_and_at_most_80_percent_of_the_client() {
    let (w, h) = legend::size();
    let big = popup_args((1000, 1000), None);
    assert_eq!(
        big,
        [
            "display-popup",
            "-E",
            "-T",
            " logs ",
            "-w",
            &(w + 2).to_string(),
            "-h",
            &(h + 2).to_string(),
            "cpd tui view logs"
        ]
    );
    let small = popup_args((80, 40), None);
    assert_eq!((small[5].as_str(), small[7].as_str()), ("64", "32"));
    assert_eq!(small[3], " logs ");
}

#[test]
fn the_popup_runs_the_view_at_the_event_of_the_item() {
    let at = popup_args((1000, 1000), Some(42));
    assert_eq!(at.last().unwrap(), "cpd tui view logs --event 42");
    assert_eq!(
        at[..at.len() - 1],
        popup_args((1000, 1000), None)[..at.len() - 1],
        "sized as the button's"
    );
}
