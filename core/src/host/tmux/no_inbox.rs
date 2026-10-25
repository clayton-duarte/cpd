//! Red test for hs-d541 (contract §6.1, §6.1.1): the status line has no Inbox pill; the right side is the workers pill,
//! then the version pill.

use super::STATUS_FORMAT;

#[test]
fn the_status_line_has_no_inbox_pill() {
    let format: &str = &STATUS_FORMAT;
    assert!(!format.contains("@cpd_inbox"), "no option of the Inbox counter is read (`@cpd_inbox`, `@cpd_inbox_tier`): {format}");
    let right = &format[format.find("#[align=right]").expect("a right-aligned side") + "#[align=right]".len()..];
    let caps = right.matches('\u{e0b6}').count();
    assert_eq!(caps, 3, "the right side is the workers pill and the version pill's two colour branches: {right}");
    let workers = right.find("#{@cpd_workers}").expect("the workers pill");
    let version = right.find("#{@cpd_version}").expect("the version pill");
    let first_cap = right.find('\u{e0b6}').unwrap();
    assert!(first_cap < workers, "the workers pill opens the right side: {right}");
    assert!(workers < version, "the version pill follows the workers pill: {right}");
}
