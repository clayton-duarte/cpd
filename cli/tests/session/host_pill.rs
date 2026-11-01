//! Red test for the host alert's version pill (contract §6.1, §6.1.1; hs-d618): with
//! `@cpd_host_alert` set the version pill is the bright red block (index 9) inside the `user|host`
//! click range; otherwise it stays the grey block (index 8) it always was. The exact text of
//! `STATUS_FORMAT` is pinned for both branches.

use cpd_core::host::tmux::STATUS_FORMAT;

/// One grey heavy `\u{2501}` join, outside a click range (contract §6.1.1, hs-d684).
const JOIN: &str = "#[fg=colour8]\u{2501}#[default]";

#[test]
fn the_alert_draws_a_bright_red_clickable_version_pill_and_the_plain_pill_stays_as_it_was() {
    // The red branch: the bright red block (index 9) with its caps, the version text inverted, inside
    // the `user|host` click range a click on it runs.
    let red = "#[fg=colour8]\u{2501}#[range=user|host]#[fg=colour9]\u{e0b6}#[reverse]#{@cpd_version}#[noreverse]\u{e0b4}#[default]#[norange]";
    // The plain branch, exactly as before the alert: the grey block (index 8), only while
    // `@cpd_version` is set.
    let plain = "#{?#{@cpd_version},#[fg=colour8]\u{2501}#[fg=colour8]\u{e0b6}#[reverse]#{@cpd_version}#[noreverse]\u{e0b4}#[default],}";
    let want = format!("#{{?#{{@cpd_host_alert}},{red},{plain}}}");

    let at = STATUS_FORMAT
        .rfind("#{?#{@cpd_host_alert},")
        .expect("the host alert branch of the version pill");
    let got = STATUS_FORMAT[at..]
        .strip_suffix(JOIN)
        .expect("the closing join follows the version pill");
    assert_eq!(
        got, want,
        "the version pill's two branches: {}",
        *STATUS_FORMAT
    );
    assert_eq!(
        STATUS_FORMAT.matches("#{@cpd_host_alert}").count(),
        1,
        "the alert is read once"
    );
    assert_eq!(
        STATUS_FORMAT.matches("#[range=user|host]").count(),
        1,
        "one click range for the red pill"
    );
    assert_eq!(
        STATUS_FORMAT
            .matches("#[reverse]#{@cpd_version}#[noreverse]")
            .count(),
        2,
        "the version text is in both branches"
    );
}
