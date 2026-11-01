//! Test overrides of cpd's fixed limits and poll periods (contract §1.2 "Test
//! overrides"): `CPD_TEST_<NAME>=<seconds>` (a decimal, e.g. `0.2`), read by a
//! debug build only, so tests need not wait for the real limits. A release
//! build (the installed `cpd`) ignores them: there, every limit is the
//! constant at its caller.

use std::time::Duration;

/// `default`, or in a debug build the `CPD_TEST_<name>` a test set. A value
/// that is not a decimal number of seconds ≥ 0 panics, naming the variable: a
/// test's typo must not fall back to the real limit unseen.
pub fn limit(name: &str, default: Duration) -> Duration {
    if !cfg!(debug_assertions) {
        return default;
    }
    let var = format!("CPD_TEST_{name}");
    match std::env::var(&var) {
        Ok(text) => {
            parse(&text).unwrap_or_else(|| panic!("{var}={text:?} is not a number of seconds >= 0"))
        }
        Err(std::env::VarError::NotPresent) => default,
        Err(std::env::VarError::NotUnicode(text)) => {
            panic!("{var}={text:?} is not a number of seconds >= 0")
        }
    }
}

/// Decimal seconds ≥ 0 (`2`, `0.25`).
fn parse(text: &str) -> Option<Duration> {
    let seconds: f64 = text.trim().parse().ok()?;
    Duration::try_from_secs_f64(seconds).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_seconds_parse_and_anything_else_does_not() {
        assert_eq!(parse("2"), Some(Duration::from_secs(2)));
        assert_eq!(parse("0.25"), Some(Duration::from_millis(250)));
        assert_eq!(parse("0"), Some(Duration::ZERO));
        for bad in ["", "-1", "1s", "abc", "NaN", "inf"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn an_unset_variable_is_the_default() {
        assert_eq!(
            limit("NO_SUCH_LIMIT_IN_ANY_TEST", Duration::from_secs(7)),
            Duration::from_secs(7)
        );
    }
}
