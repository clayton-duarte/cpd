//! Red tests of the `model` domain's one-provider rule (contract §8.3 "Models").

use super::{Models, check_one_provider, provider_of};
use crate::error::ErrorCode;
use serde_json::json;
use std::path::Path;

const FILE: &str = "/policy/acme/owner.toml";

fn models(o: &str, s: &str, h: &str, g: &str) -> Models {
    Models {
        o: o.into(),
        s: s.into(),
        h: h.into(),
        g: g.into(),
    }
}

#[test]
fn provider_of_a_ref_is_the_text_before_the_first_slash() {
    assert_eq!(
        provider_of("github-copilot/claude-opus-5.5"),
        "github-copilot"
    );
}

#[test]
fn provider_of_a_ref_with_more_slashes_stops_at_the_first() {
    assert_eq!(provider_of("bifrost/vertex/claude-opus-5-5"), "bifrost");
}

#[test]
fn four_equal_providers_are_accepted() {
    let m = models("gc/a", "gc/b", "gc/c/d", "gc/e");
    assert!(check_one_provider(&m, Path::new(FILE)).is_ok());
}

/// The refusal of `m`: its code, file and key checked; returns the message.
fn refusal(m: &Models) -> String {
    let error = check_one_provider(m, Path::new(FILE)).expect_err("a mixed table is refused");
    assert_eq!(error.code(), ErrorCode::PolicyInvalid);
    assert_eq!(error.details().get("file"), Some(&json!(FILE)));
    assert_eq!(error.details().get("key"), Some(&json!("model")));
    error.message().to_owned()
}

#[test]
fn a_differing_o_is_refused_naming_the_keys_and_providers() {
    let message = refusal(&models("bifrost/o", "gc/s", "gc/h", "gc/g"));
    for word in ["O", "bifrost", "gc"] {
        assert!(message.contains(word), "{message}");
    }
}

#[test]
fn a_differing_s_is_refused_naming_the_keys_and_providers() {
    let message = refusal(&models("gc/o", "bifrost/s", "gc/h", "gc/g"));
    for word in ["S", "O", "bifrost", "gc"] {
        assert!(message.contains(word), "{message}");
    }
}

#[test]
fn a_differing_h_is_refused_naming_the_keys_and_providers() {
    let message = refusal(&models("gc/o", "gc/s", "bifrost/h", "gc/g"));
    for word in ["H", "bifrost", "gc"] {
        assert!(message.contains(word), "{message}");
    }
}

#[test]
fn a_differing_g_is_refused_naming_the_keys_and_providers() {
    let message = refusal(&models("gc/o", "gc/s", "gc/h", "bifrost/g"));
    for word in ["G", "bifrost", "gc"] {
        assert!(message.contains(word), "{message}");
    }
}
