//! Red tests of the `model` domain's one-provider rule (contract §8.3 "Models").

use super::model::{ModelSet, Models, check_one_provider, provider_of};
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
        j: "gc/j".into(),
    }
}

/// A `[model]` table of the five keys `O`, `S`, `H`, `G`, `J` (contract §8.3 "Models").
fn models5(o: &str, s: &str, h: &str, g: &str, j: &str) -> Models {
    Models {
        o: o.into(),
        s: s.into(),
        h: h.into(),
        g: g.into(),
        j: j.into(),
    }
}

/// The five refs of a scope `[model]` table, one key each.
const FIVE_KEYS: &str =
    "O = \"p/o\"\nS = \"p/s\"\nH = \"p/h\"\nG = \"p/g\"\nJ = \"openrouter/typesafe/jev-router\"\n";

/// J is the fifth `[model]` key: a scope table holding it parses, and the compiled default names its model
/// (`github-copilot/gemini-3.8-flash`, contract §8.3 "Models").
#[test]
fn the_model_table_takes_a_fifth_key_j() {
    let parsed: Result<ModelSet, _> = toml::from_str(FIVE_KEYS);
    assert!(
        parsed.is_ok(),
        "a [model] table takes the five keys O, S, H, G, J: {:?}",
        parsed.err()
    );
    assert_eq!(
        crate::policy::default_policy().model.j,
        "github-copilot/gemini-3.8-flash",
        "the compiled default names J's model"
    );
}

/// The one-provider rule holds at the five keys, with no exemption for J (contract §8.3 "Models":
/// "the rule is unchanged at the five keys `O`, `S`, `H`, `G` and `J`").
#[test]
fn one_provider_holds_with_five_keys() {
    let m = models5("gc/a", "gc/b", "gc/c/d", "gc/e", "gc/f");
    assert!(
        check_one_provider(&m, Path::new(FILE)).is_ok(),
        "five refs of one provider are accepted"
    );
    let mixed = models5("gc/o", "gc/s", "gc/h", "gc/g", "bifrost/j");
    let result = check_one_provider(&mixed, Path::new(FILE));
    assert!(
        result.is_err(),
        "a J ref from another provider is refused, as any key's is"
    );
    let error = result.unwrap_err();
    assert_eq!(error.code(), ErrorCode::PolicyInvalid);
    assert_eq!(error.details().get("file"), Some(&json!(FILE)));
    assert_eq!(error.details().get("key"), Some(&json!("model")));
    let message = error.message();
    for word in ["J", "bifrost", "gc"] {
        assert!(message.contains(word), "{message}");
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
