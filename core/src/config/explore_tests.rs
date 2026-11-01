//! Red tests of the config key `timeouts.explore_minutes` (Plan explore-roles, R7): the active-time limit of an explore Attempt.

use std::path::Path;

use serde_json::json;

use super::{Config, Source};
use crate::error::ErrorCode;

fn parse(extra: &str) -> Result<(Config, super::file::Sources), crate::error::CpdError> {
    Config::parse(
        &format!("version = 1\nmachine = \"hs\"\n{extra}"),
        Path::new("config.toml"),
    )
}

#[test]
fn explore_minutes_defaults_to_20_and_rejects_zero() {
    let (config, sources) = parse("").unwrap();
    assert_eq!(config.timeouts.explore_minutes, 20, "unset: 20");
    assert_eq!(sources["timeouts.explore_minutes"], Source::Default);
    assert_eq!(
        json!(config)["timeouts"]["explore_minutes"],
        json!(20),
        "`cpd config` prints the effective value"
    );

    for minutes in [1, 2, 90] {
        let (config, sources) =
            parse(&format!("[timeouts]\nexplore_minutes = {minutes}\n")).unwrap();
        assert_eq!(config.timeouts.explore_minutes, minutes);
        assert_eq!(sources["timeouts.explore_minutes"], Source::File);
        assert_eq!(
            config.timeouts.attempt_minutes, 30,
            "attempt_minutes is a key of its own"
        );
    }

    for bad in ["0", "-1", "1.5", "\"20\"", "true"] {
        let err = parse(&format!("[timeouts]\nexplore_minutes = {bad}\n")).expect_err(bad);
        assert_eq!(
            err.code(),
            ErrorCode::ConfigInvalid,
            "explore_minutes = {bad}"
        );
    }
}
