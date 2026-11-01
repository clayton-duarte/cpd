//! Test for hs-d541 (contract §6.1, §6.1.1, hs-d634, hs-d640): the status line has no Inbox pill; the right side is the
//! incident pill, then the workers pill, then the version pill.

use super::STATUS_FORMAT;

#[test]
fn the_status_line_has_no_inbox_pill() {
    let format: &str = &STATUS_FORMAT;
    assert!(
        !format.contains("@cpd_inbox"),
        "no option of the Inbox counter is read (`@cpd_inbox`, `@cpd_inbox_tier`): {format}"
    );
    let marker = "#[default]#{?@cpd_incident";
    let right =
        &format[format.find(marker).expect("the start of the right side") + "#[default]".len()..];
    let caps = right.matches('\u{e0b6}').count();
    assert_eq!(
        caps, 4,
        "the right side is the incident, the workers and the version pills (the version has two colour branches): {right}"
    );
    let incident = right.find("Incident Mode: ").expect("the incident pill");
    let workers = right.find("#{@cpd_workers}").expect("the workers pill");
    let version = right.find("#{@cpd_version}").expect("the version pill");
    let first_cap = right.find('\u{e0b6}').unwrap();
    assert!(
        first_cap < incident,
        "the incident pill opens the right side: {right}"
    );
    assert!(
        incident < workers,
        "the workers pill follows the incident pill: {right}"
    );
    assert!(
        workers < version,
        "the version pill follows the workers pill: {right}"
    );
}
