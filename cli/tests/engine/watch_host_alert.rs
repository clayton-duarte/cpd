//! Red tests for E6, the host alert (contract §3.7, §1.4; hs-d618): the install state file and the
//! `@cpd_host_alert` tmux call the step builds from it.
//!
//! The engine step is a stub and is not in the tick yet (impl lands later), so these test the pure
//! functions the step calls — the tmux argv it builds from the old and the new state — rather than
//! starting an engine and polling for a tick, which the Spec forbids.

use cpd_core::error::ErrorCode;
use cpd_core::home::CpdHome;
use cpd_core::host::install_state::InstallState;
use cpd_engine::watch::host_alert;

/// The option the version pill reads (contract §6.1).
const OPTION: &str = "@cpd_host_alert";

const OK: &str = r#"{"status":"ok","since":"2026-10-24T12:00:00Z"}"#;
const SKIPPED_DIRTY: &str =
    r#"{"status":"skipped","reason":"dirty","since":"2026-10-24T12:00:00Z"}"#;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn a_failed_state_sets_the_alert_option_to_its_reason() {
    let raw = r#"{"status":"failed","reason":"installer failed (exit 1)","since":"2026-10-24T12:00:00Z"}"#;
    let state = InstallState::parse(raw).expect("a failed state");
    assert_eq!(
        state.alert(),
        Some("installer failed (exit 1)"),
        "the pill's colour and text come from the failure's reason"
    );
    assert_eq!(
        host_alert::argv(None, state.alert()),
        Some(args(&["set", "-g", OPTION, "installer failed (exit 1)"])),
        "a failed install sets {OPTION} to its reason"
    );
}

#[test]
fn the_core_parses_every_status_the_dotfiles_writer_can_write() {
    // Dotfiles scripts/cpd-install.sh writes:
    // - ok: {"status":"ok","since":"<T>"}
    // - skipped: {"status":"skipped","reason":"<dirty|diverged|branch>","since":"<T>"}
    // - failed: {"status":"failed","reason":"installer failed (exit <err>)","since":"<T>"}
    let ok = r#"{"status":"ok","since":"2026-10-24T12:00:00Z"}"#;
    let skipped_dirty = r#"{"status":"skipped","reason":"dirty","since":"2026-10-24T12:00:00Z"}"#;
    let skipped_diverged =
        r#"{"status":"skipped","reason":"diverged","since":"2026-10-24T12:00:00Z"}"#;
    let skipped_branch = r#"{"status":"skipped","reason":"branch","since":"2026-10-24T12:00:00Z"}"#;
    let failed = r#"{"status":"failed","reason":"installer failed (exit 1)","since":"2026-10-24T12:00:00Z"}"#;

    let s_ok = InstallState::parse(ok).expect("ok status");
    assert_eq!(s_ok.alert(), None);

    let s_dirty = InstallState::parse(skipped_dirty).expect("skipped dirty");
    assert_eq!(s_dirty.alert(), Some("dirty"));

    let s_diverged = InstallState::parse(skipped_diverged).expect("skipped diverged");
    assert_eq!(s_diverged.alert(), Some("diverged"));

    let s_branch = InstallState::parse(skipped_branch).expect("skipped branch");
    assert_eq!(s_branch.alert(), Some("branch"));

    let s_failed = InstallState::parse(failed).expect("failed status");
    assert_eq!(s_failed.alert(), Some("installer failed (exit 1)"));

    // And invalid variations of failed status:
    // status failed without reason
    let failed_no_reason = r#"{"status":"failed","since":"2026-10-24T12:00:00Z"}"#;
    assert_eq!(
        InstallState::parse(failed_no_reason)
            .expect_err("failed without reason must fail")
            .code(),
        ErrorCode::InstallStateInvalid
    );
}

#[test]
fn a_skipped_state_sets_the_alert_option_to_its_reason() {
    let state = InstallState::parse(SKIPPED_DIRTY).expect("a skipped state");
    assert_eq!(
        state.alert(),
        Some("dirty"),
        "the pill's colour comes from the reason"
    );
    assert_eq!(
        host_alert::argv(None, state.alert()),
        Some(args(&["set", "-g", OPTION, "dirty"])),
        "a skipped install sets {OPTION} to its reason"
    );
}

#[test]
fn an_ok_state_unsets_the_alert_option() {
    let state = InstallState::parse(OK).expect("an ok state");
    assert_eq!(state.alert(), None, "an ok install does not alert");
    assert_eq!(
        host_alert::argv(Some("dirty"), state.alert()),
        Some(args(&["set", "-gu", OPTION])),
        "clearing a red pill unsets {OPTION}"
    );
    assert_eq!(
        host_alert::argv(None, None),
        None,
        "an option already unset is not written again"
    );
}

#[test]
fn a_missing_state_file_unsets_the_alert_option() {
    let dir = tempfile::tempdir().expect("a temporary home");
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).expect("a home");
    assert_eq!(
        InstallState::read(&home).expect("a readable home"),
        None,
        "no file means no state"
    );
    assert_eq!(
        host_alert::argv(Some("dirty"), None),
        Some(args(&["set", "-gu", OPTION])),
        "a missing file unsets {OPTION}"
    );
}

#[test]
fn an_invalid_state_file_is_one_error_and_leaves_the_option_alone() {
    let read: Result<Option<InstallState>, _> =
        InstallState::parse("not the install state").map(Some);
    let error = read.as_ref().expect_err("an invalid file");
    assert_eq!(error.code(), ErrorCode::InstallStateInvalid, "{error:?}");
    assert_eq!(
        host_alert::call(Some("dirty"), &read),
        None,
        "an invalid file is reported and {OPTION} is left as it is"
    );
}

#[test]
fn a_state_needs_a_known_reason_a_since_and_no_other_field() {
    for text in [
        OK,
        SKIPPED_DIRTY,
        r#"{"status":"skipped","reason":"diverged","since":"2026-10-24T12:00:00Z"}"#,
        r#"{"status":"skipped","reason":"branch","since":"2026-10-24T12:00:00Z"}"#,
    ] {
        assert!(InstallState::parse(text).is_ok(), "valid: {text}");
    }

    for text in [
        // not the JSON object §1.4 gives
        "not json",
        "[]",
        // an unknown field is refused
        r#"{"status":"ok","since":"2026-10-24T12:00:00Z","extra":1}"#,
        // status is `ok` or `skipped`
        r#"{"status":"done","since":"2026-10-24T12:00:00Z"}"#,
        r#"{"since":"2026-10-24T12:00:00Z"}"#,
        // `reason` is present exactly when the status is `skipped`
        r#"{"status":"ok","reason":"dirty","since":"2026-10-24T12:00:00Z"}"#,
        r#"{"status":"skipped","since":"2026-10-24T12:00:00Z"}"#,
        // the reason is `dirty`, `diverged` or `branch` (assumption A1)
        r#"{"status":"skipped","reason":"behind","since":"2026-10-24T12:00:00Z"}"#,
        // `since` is required, RFC 3339 and UTC (`Z`)
        r#"{"status":"ok"}"#,
        r#"{"status":"ok","since":"yesterday"}"#,
        r#"{"status":"ok","since":"2026-10-24T12:00:00+02:00"}"#,
        r#"{"status":"ok","since":7}"#,
    ] {
        let error = InstallState::parse(text).expect_err(&format!("invalid: {text}"));
        assert_eq!(
            error.code(),
            ErrorCode::InstallStateInvalid,
            "invalid: {text}"
        );
    }
}
