//! Red tests of the Experimental rows of the settings view (docs/commands.md §13.2 "Experimental", §14.2b): the drawn
//! rows below the Tiers rows, a click that flips a row, Save running `cpd config experimental` for each changed row
//! only, Cancel, and the initial values read from `cpd config`. They use the doubles of `view_tests`.

use ratatui::crossterm::event::{KeyCode, MouseEventKind};
use serde_json::json;

use super::view_tests::{Act, Commands, call, first_screen, popup, reads};

/// The rows of the docs' Experimental block with the focus on `write_curation`, turned off by a click.
const AFTER_CLICK: [&str; 5] = [
    " Experimental",
    "     durable_workers      off",
    "   ▸ write_curation       off*",
    "     failure_triage       on",
    "     turn_question_guard  on",
];

fn experimental_block(rows: &[String]) -> Vec<String> {
    let at = rows
        .iter()
        .position(|r| r.contains("Experimental"))
        .expect("the screen has an Experimental header");
    rows[at..at + 5].to_vec()
}

fn scroll_to_the_buttons() -> Act {
    Act::Wheel(MouseEventKind::ScrollDown)
}

#[test]
fn settings_first_screen_shows_the_experimental_rows_below_tiers() {
    let run = popup(&Commands::new(), vec![Act::Snap, Act::Key(KeyCode::Esc)]);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    let rows = run.snaps[0].rows();
    assert_eq!(rows, first_screen());
    assert_eq!(rows[17], " Experimental");
    assert_eq!(rows[18], "     durable_workers      off");
    assert_eq!(rows[21], "     turn_question_guard  on");
}

#[test]
fn clicking_an_experimental_row_flips_it_and_marks_it_changed() {
    let run = popup(
        &Commands::new(),
        vec![
            Act::ClickEnd("write_curation"),
            Act::Snap,
            Act::Key(KeyCode::Esc),
        ],
    );
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(
        experimental_block(&run.snaps[0].rows()),
        AFTER_CLICK.map(str::to_owned),
        "flipped, focused and marked changed"
    );
    assert_eq!(run.calls, reads(), "a flip writes nothing until Save");
}

#[test]
fn save_runs_cpd_config_experimental_for_each_changed_feature_only() {
    let commands = Commands::new();
    let run = popup(
        &commands,
        vec![
            Act::ClickEnd("turn_question_guard"),
            Act::ClickEnd("durable_workers"),
            scroll_to_the_buttons(),
            Act::Click("[ Save ]", "[ Save ]"),
        ],
    );
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    let mut want = reads();
    want.extend([
        call(&["config", "experimental", "durable_workers", "on"]),
        call(&["config", "experimental", "turn_question_guard", "off"]),
    ]);
    assert_eq!(
        run.calls, want,
        "in the order of the rows, the unchanged rows not written, no retier"
    );
}

#[test]
fn cancel_discards_experimental_flips() {
    let run = popup(
        &Commands::new(),
        vec![
            Act::ClickEnd("write_curation"),
            Act::Snap,
            scroll_to_the_buttons(),
            Act::Click("[ Cancel ]", "[ Cancel ]"),
        ],
    );
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(
        experimental_block(&run.snaps[0].rows()),
        AFTER_CLICK.map(str::to_owned),
        "the flip was made before Cancel"
    );
    assert_eq!(run.calls, reads(), "Cancel runs no write command");
}

#[test]
fn experimental_rows_load_from_cpd_config_json() {
    let commands = Commands::new().with_experimental(json!({
        "durable_workers": true, "write_curation": false,
        "failure_triage": true, "turn_question_guard": false,
    }));
    let run = popup(&commands, vec![Act::Snap, Act::Key(KeyCode::Esc)]);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(
        experimental_block(&run.snaps[0].rows()),
        [
            " Experimental",
            "     durable_workers      on",
            "     write_curation       off",
            "     failure_triage       on",
            "     turn_question_guard  off",
        ]
        .map(str::to_owned),
        "the values come from the experimental object, with no change marker"
    );
}
