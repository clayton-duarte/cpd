//! The `dialog` view (commands.md §13.2, "The dialog kit"; hs-d295, hs-d578): each kind draws its own frame and takes
//! its keys, clicks and wheel in-process (`tui/src/dialog/pane_tests.rs`, `checklist_tests.rs`, on a `TestBackend`);
//! what stays here is the command line's contract, which needs the binary.

use crate::common::TestEnv;

#[test]
fn the_view_needs_a_terminal_and_its_arguments() {
    let env = TestEnv::new();
    let run = env.cpd(&["tui", "view", "dialog", "confirm", "--title=T", "--out=/tmp/x", "--text=Q"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
    for bad in [
        &["tui", "view", "dialog"][..],
        &["tui", "view", "dialog", "confirm", "--title=T", "--text=Q"],
        &["tui", "view", "dialog", "input", "--title=T", "--out=/tmp/x", "--prompt=P", "--validate=anything"],
        &["tui", "view", "dialog", "input", "--title=T", "--out=/tmp/x", "--prompt=P"],
        &["tui", "view", "dialog", "text", "--out=/tmp/x", "--body=B"],
    ] {
        assert_eq!(env.cpd(bad).exit, 2, "{bad:?}");
    }
}
