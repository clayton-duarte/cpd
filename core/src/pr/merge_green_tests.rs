use super::merge_green::require_green;

#[test]
fn merge_green_requires_passed_checks() {
    assert!(require_green("passed").is_ok());
    for checks in ["failed", "pending", "none"] {
        assert!(require_green(checks).is_err(), "{checks}");
    }
}
