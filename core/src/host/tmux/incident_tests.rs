//! Red tests of the incident status pill (contract §6.1/§6.1.1, hs-d634, hs-d640): one
//! `Incident Mode: <elapsed>` pill, guarded by `@cpd_incident`, red (colour 9) and italic,
//! first in the right group before the workers pill, whose width the fill and the long-form test
//! subtract.

use super::STATUS_FORMAT;

/// The incident pill's label; `Incident Mode: ` is 15 cells (contract §6.1, hs-d640).
const LABEL: &str = "Incident Mode: ";

/// The width `@cpd_incident` adds to the right group while set: one join, two caps and
/// `Incident Mode: ` plus the value's cells (contract §6.1, the right group).
const INCIDENT_WIDTH: &str = "#{?@cpd_incident,#{e|+:#{n:@cpd_incident},18},0}";

/// The incident pill's left cap sits directly after the fill: no `#[align=right]` builds a
/// separate right screen between the fill's reset and the incident branch, so the branch opens
/// with its click range and its left cap `\u{e0b6}` immediately after the fill's `#[default]`
/// (contract §6.1.1, hs-d640, hs-d678).
#[test]
fn the_incident_pill_is_first_after_the_fill() {
    let f: &str = &STATUS_FORMAT;
    assert!(
        !f.contains("#[align=right]"),
        "no right-aligned side clips the incident pill's left cap: {f}"
    );
    assert!(
        f.contains("#[default]#{?@cpd_incident,#[range=user|incident]#[fg=colour9]\u{e0b6}"),
        "the incident branch opens with its click range and left cap immediately after the fill's reset: {f}"
    );
}

/// The incident pill carries the click range `#[range=user|incident]` around the whole pill, caps
/// included (contract §6.1, hs-d678): the range opens before the `#[fg=colour9]` left cap, closes
/// with `#[norange]` before the join that follows the pill, and the pill keeps its colour 9,
/// reverse and italics look and its label.
#[test]
fn the_incident_pill_carries_the_incident_click_range() {
    let f: &str = &STATUS_FORMAT;
    let label = f
        .find("Incident Mode: #{@cpd_incident}")
        .expect("the incident pill's label");
    let open = f[..label]
        .rfind("#[range=user|incident]")
        .expect("the incident pill's click range");
    let close = label
        + f[label..]
            .find("#[norange]")
            .expect("the pill's range close")
        + "#[norange]".len();
    assert!(
        f[open..].starts_with("#[range=user|incident]#[fg=colour9]\u{e0b6}"),
        "the range opens immediately before the left cap: {}",
        &f[open..close]
    );
    assert!(
        f[close..].starts_with("#[fg=colour8]\u{2501}"),
        "the range closes before the following join: {}",
        &f[close..]
    );
    let pill = &f[open..close];
    assert!(
        pill.contains("colour9"),
        "the block and caps are colour 9: {pill}"
    );
    assert!(
        pill.replace("noreverse", "").contains("reverse"),
        "the text is reversed: {pill}"
    );
    assert!(
        pill.replace("noitalics", "").contains("italics"),
        "the text is italic: {pill}"
    );
    assert!(
        pill.contains("Incident Mode: #{@cpd_incident}"),
        "the label reads the option: {pill}"
    );
}

/// The incident pill is guarded by `@cpd_incident`, reads `Incident Mode: ` immediately followed
/// by the option's value (15 cells, no leading space), is colour 9 and its text is reversed and
/// italic, and it stands first in the right group, before the workers pill.
#[test]
fn the_incident_pill_shows_the_elapsed_time_first_in_the_right_group() {
    let f: &str = &STATUS_FORMAT;
    assert_eq!(
        f.matches("Incident Mode").count(),
        1,
        "one incident pill: {f}"
    );
    assert!(
        f.contains("Incident Mode: #{@cpd_incident}"),
        "the label reads the option: {f}"
    );
    assert!(
        !f.contains(" Incident Mode: "),
        "the label is 15 cells with no leading space: {f}"
    );
    let label = f
        .find("Incident Mode: ")
        .expect("the incident pill's label");
    // The branch of the pill: the plain `#{?@cpd_incident` conditional that opens it.
    let guard = f[..label]
        .rfind("#{?@cpd_incident")
        .expect("the pill is guarded by @cpd_incident");
    assert!(
        f[guard..label].contains("#{?@cpd_incident"),
        "the label sits in the @cpd_incident branch: {}",
        &f[guard..label]
    );
    // The whole pill: the guard, the join, the caps and the text.
    let close =
        label + f[label..].find('\u{e0b4}').expect("the pill's right cap") + '\u{e0b4}'.len_utf8();
    let pill = &f[guard..close];
    assert!(
        pill.contains("colour9"),
        "the block and caps are colour 9: {pill}"
    );
    assert!(
        pill.replace("noreverse", "").contains("reverse"),
        "the text is reversed: {pill}"
    );
    assert!(
        pill.replace("noitalics", "").contains("italics"),
        "the text is italic: {pill}"
    );
    // The incident pill is first in the right group, before the workers pill.
    let workers = f.find("#{@cpd_workers}").expect("the workers pill");
    assert!(
        guard < workers,
        "the incident branch comes before the workers pill: {f}"
    );
}

/// The fill count and `long_fits!()` subtract the incident pill's width while
/// `@cpd_incident` is set, exactly as they subtract the version pill's. That width follows the
/// duration-based text: one join, two caps and the 15-cell label (`18`), plus the value's cells.
#[test]
fn the_incident_pill_width_follows_the_duration() {
    let f: &str = &STATUS_FORMAT;
    assert_eq!(LABEL.chars().count(), 15, "the label is 15 cells");
    assert!(
        f.contains("Incident Mode: #{@cpd_incident}"),
        "the pill's text carries the label and the duration: {f}"
    );
    assert!(
        !f.contains(" Incident Mode: "),
        "the label has no leading space: {f}"
    );
    let cells = LABEL.chars().count() + 3;
    assert!(
        INCIDENT_WIDTH.contains(&format!(",{cells}}}")),
        "the width is one join, two caps and the 15-cell label: {INCIDENT_WIDTH}"
    );
    let fill = fill_count!();
    assert!(
        fill.contains(INCIDENT_WIDTH),
        "the fill count subtracts the incident width: {fill}"
    );
    assert!(
        long_fits!().contains(INCIDENT_WIDTH),
        "long_fits subtracts the incident width: {}",
        long_fits!()
    );
    assert!(
        STATUS_FORMAT.contains(fill),
        "the format uses the fill count: {}",
        *STATUS_FORMAT
    );
    assert!(
        STATUS_FORMAT.contains(long_fits!()),
        "the format uses long_fits: {}",
        *STATUS_FORMAT
    );
}
