//! The StatusBar's settings button (the gear, U+F013): the fifth glyph button of the left group, after costs and before the
//! runner pill, in its own click range `settings`. Written from the docs; the format text is what cpd writes, tmux is not run.

use super::pill_lines_tests::{display_width, range_of};
use super::{SETTINGS_GLYPH, STATUS_FORMAT};

#[test]
fn the_settings_glyph_is_u_f013() {
    assert_eq!(SETTINGS_GLYPH, "\u{F013}");
    assert!(
        STATUS_FORMAT.contains(SETTINGS_GLYPH),
        "the glyph is in the status format: {}",
        *STATUS_FORMAT
    );
}

#[test]
fn the_settings_pill_follows_costs_and_precedes_the_runner_pill_in_its_own_range() {
    let format = STATUS_FORMAT.as_str();
    assert_eq!(
        format.matches("range=user|settings").count(),
        1,
        "one settings range: {format}"
    );
    let ranges: Vec<&str> = format
        .match_indices("#[range=user|")
        .map(|(i, _)| &format[i + "#[range=user|".len()..])
        .map(|r| &r[..r.find(']').expect("a closed range")])
        .collect();
    assert_eq!(
        ranges[..6],
        ["reset", "legend", "logs", "costs", "settings", "runner"],
        "{ranges:?}"
    );
    // the glyph and one space between the caps, the bright colour of the other buttons
    let pill = range_of("settings");
    assert_eq!(
        pill,
        format!(
            "#[range=user|settings]#[fg=colour15]\u{e0b6}#[reverse]{SETTINGS_GLYPH} #[noreverse]\u{e0b4}#[default]#[norange]"
        )
    );
    // one grey join after the costs pill, and the runner conditional right after the settings pill
    let costs = range_of("costs");
    let costs_end = format.find(costs).unwrap() + costs.len();
    assert_eq!(
        &format[costs_end..format.find(pill).unwrap()],
        "#[fg=colour8]\u{2501}#[default]"
    );
    let after = &format[format.find(pill).unwrap() + pill.len()..];
    assert!(after.starts_with("#{?@cpd_runner,"), "{after}");
    assert!(
        !format.contains("@cpd_settings"),
        "no conditional on the button: {format}"
    );
}

#[test]
fn the_settings_click_range_owns_columns_21_to_25() {
    let format = STATUS_FORMAT.as_str();
    let pill = range_of("settings");
    let at = format.find(pill).unwrap();
    assert_eq!(display_width(&format[..at]), 21, "it starts at column 21");
    assert_eq!(display_width(pill), 4, "it covers columns 21-24");
    // the left group is 25 cells and ends with the pill; tmux's range also owns the join after it, column 25
    assert_eq!(
        display_width(&format[..at + pill.len()]),
        25,
        "the left group is 25 cells and ends with the settings pill; the range also owns the join after it, column 25"
    );
}
