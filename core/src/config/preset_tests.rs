//! `[presets.<name>]` tables of `config.toml` (contract §8.2 "`[presets.<name>]`", commands.md §14.6; D1 of the
//! tier-preset switch): the typed key walk of [`Config::parse`] accepts a preset of the four refs `O`, `S`, `H`, `G`
//! and refuses a bad name, a missing letter, an extra key or a malformed ref with `config_invalid` (`details.key`),
//! `version` still 1.
//!
//! These tests are red before the tier-preset switch: today a `[presets]` table is an unknown key, so the "parses"
//! test fails (`Config::parse` returns `config_invalid`) and each refusal reports the key `presets`, not the preset's
//! own key path (`presets.<name>`, `presets.<name>.<letter>`).

use super::*;
use crate::error::ErrorCode;
use std::path::Path;

/// The machine and a `[presets.fast]` table of the four refs the docs name.
const FAST: &str = "[presets.fast]\nO = \"github-copilot/claude-haiku-4.5\"\nS = \"github-copilot/claude-sonnet-5.5\"\nH = \"github-copilot/claude-haiku-4.5\"\nG = \"github-copilot/gemini-3.8-flash\"\n";

fn parsed(text: &str) -> Config {
    Config::parse(text, Path::new("config.toml")).unwrap().0
}

/// The `config_invalid` key the typed walk reports for a refusal (`details.key`).
fn refused_key(text: &str) -> (ErrorCode, String) {
    let err = Config::parse(text, Path::new("config.toml")).unwrap_err();
    (
        err.code(),
        err.details()["key"].as_str().unwrap_or("").to_owned(),
    )
}

#[test]
fn a_presets_table_with_all_four_letters_parses() {
    let config = parsed(&format!("version = 1\nmachine = \"m1\"\n{FAST}"));
    let value = serde_json::to_value(&config).unwrap();
    let fast = &value["presets"]["fast"];
    assert!(
        fast.is_object(),
        "a [presets.fast] table is parsed as a map by name: {value}"
    );
    assert_eq!(fast["O"], "github-copilot/claude-haiku-4.5");
    assert_eq!(fast["S"], "github-copilot/claude-sonnet-5.5");
    assert_eq!(fast["H"], "github-copilot/claude-haiku-4.5");
    assert_eq!(fast["G"], "github-copilot/gemini-3.8-flash");
}

#[test]
fn a_bad_preset_name_or_an_extra_key_or_a_missing_letter_is_refused() {
    // A name that does not match `^[a-z][a-z0-9-]{0,31}$`.
    let (code, key) = refused_key(
        "version = 1\nmachine = \"m1\"\n[presets.Fast]\nO = \"p/a\"\nS = \"p/b\"\nH = \"p/c\"\nG = \"p/d\"\n",
    );
    assert_eq!(code, ErrorCode::ConfigInvalid);
    assert_eq!(
        key, "presets.Fast",
        "a bad name is refused with `presets.<name>`"
    );

    // An extra key beyond the four letters.
    let (code, key) = refused_key(&format!(
        "version = 1\nmachine = \"m1\"\n{FAST}X = \"p/x\"\n"
    ));
    assert_eq!(code, ErrorCode::ConfigInvalid);
    assert_eq!(
        key, "presets.fast.X",
        "an extra key is refused with its own path"
    );

    // A missing letter (only three of the four).
    let (code, key) = refused_key(
        "version = 1\nmachine = \"m1\"\n[presets.fast]\nO = \"p/a\"\nS = \"p/b\"\nH = \"p/c\"\n",
    );
    assert_eq!(code, ErrorCode::ConfigInvalid);
    assert!(
        key.starts_with("presets.fast"),
        "a missing letter is refused at the preset: {key}"
    );
}

#[test]
fn a_malformed_model_ref_is_refused() {
    // A ref with no `/` (contract §8.3 `[model]` shape) is refused at its own letter.
    let (code, key) = refused_key(
        "version = 1\nmachine = \"m1\"\n[presets.fast]\nO = \"noprovider\"\nS = \"p/b\"\nH = \"p/c\"\nG = \"p/d\"\n",
    );
    assert_eq!(code, ErrorCode::ConfigInvalid);
    assert_eq!(
        key, "presets.fast.O",
        "a malformed ref is refused with `presets.<name>.<letter>`"
    );
}
