//! The `dialog` view (commands.md §13.2, "The dialog kit"; hs-d295, hs-d578): each kind draws its own frame and takes
//! its keys, clicks and wheel in-process (`tui/src/dialog/pane_tests.rs`, `checklist_tests.rs`, on a `TestBackend`);
//! what stays here is the command line's contract, run in-process (`run_with`, no terminal).

use crate::common::fx::Fx;

#[test]
fn the_view_needs_a_terminal_and_its_arguments() {
    let fx = Fx::with_ports();
    let run = fx.cpd_without_a_terminal(&[
        "tui",
        "view",
        "dialog",
        "confirm",
        "--title=T",
        "--out=/tmp/x",
        "--text=Q",
    ]);
    assert_eq!(
        (run.exit, run.error()["code"].as_str()),
        (3, Some("not_a_terminal")),
        "{run:?}"
    );
    for bad in [
        &["tui", "view", "dialog"][..],
        &["tui", "view", "dialog", "confirm", "--title=T", "--text=Q"],
        &[
            "tui",
            "view",
            "dialog",
            "input",
            "--title=T",
            "--out=/tmp/x",
            "--prompt=P",
            "--validate=anything",
        ],
        &[
            "tui",
            "view",
            "dialog",
            "input",
            "--title=T",
            "--out=/tmp/x",
            "--prompt=P",
        ],
        &["tui", "view", "dialog", "text", "--out=/tmp/x", "--body=B"],
    ] {
        assert_eq!(fx.cpd_without_a_terminal(bad).exit, 2, "{bad:?}");
    }
}
