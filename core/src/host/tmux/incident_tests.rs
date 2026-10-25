//! Red tests of the incident status pill (contract §6.1/§6.1.1, hs-d634): one
//! `Incident Mode` pill, guarded by `@cpd_incident`, red (colour 9) and italic,
//! left of the version pill, whose width the fill and the long-form test subtract.

use super::STATUS_FORMAT;

/// The width `@cpd_incident` adds to the right group while set: one join, two caps
/// and `Incident Mode` = 16 cells (contract §6.1, the right group).
const INCIDENT_WIDTH: &str = "#{?@cpd_incident,16,0}";

/// The incident pill is guarded by `@cpd_incident`, reads `Incident Mode`, is
/// colour 9 and its text is reversed and italic, and it stands left of the
/// conditional version pill.
#[test]
fn the_incident_pill_is_red_italic_and_left_of_the_version_pill() {
    let f: &str = &STATUS_FORMAT;
    assert_eq!(f.matches("Incident Mode").count(), 1, "one incident pill: {f}");
    let label = f.find("Incident Mode").expect("the incident pill's label");
    // The branch of the pill: the nearest `@cpd_incident` before the label.
    let guard = f[..label].rfind("@cpd_incident").expect("the pill is guarded by @cpd_incident");
    assert!(f[guard..label].contains("#{?"), "the label sits in the @cpd_incident branch: {}", &f[guard..label]);
    // The whole pill: the guard, the join, the caps and the text.
    let close = label + f[label..].find('\u{e0b4}').expect("the pill's right cap") + 1;
    let pill = &f[guard..close];
    assert!(pill.contains("colour9"), "the block and caps are colour 9: {pill}");
    assert!(pill.replace("noreverse", "").contains("reverse"), "the text is reversed: {pill}");
    assert!(pill.replace("noitalics", "").contains("italics"), "the text is italic: {pill}");
    // The conditional version pill's text: the incident pill stands to its left.
    let version = f.find("#[reverse]#{@cpd_version}#[noreverse]").expect("the version pill");
    assert!(label < version, "the incident pill is left of the version pill: {f}");
}

/// The fill count and `long_fits!()` subtract the incident pill's width while
/// `@cpd_incident` is set, exactly as they subtract the version pill's.
#[test]
fn the_incident_pill_shortens_the_status_fill() {
    let fill = fill_count!();
    assert!(fill.contains(INCIDENT_WIDTH), "the fill count subtracts the incident width: {fill}");
    assert!(long_fits!().contains(INCIDENT_WIDTH), "long_fits subtracts the incident width: {}", long_fits!());
    assert!(STATUS_FORMAT.contains(fill), "the format uses the fill count: {}", *STATUS_FORMAT);
    assert!(STATUS_FORMAT.contains(long_fits!()), "the format uses long_fits: {}", *STATUS_FORMAT);
}
