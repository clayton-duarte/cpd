//! `config tiers set` (commands.md §14.2a): the pure rewrite of the four Tier keys of `config.toml`.

/// The text of a `config.toml` with each `(key, tier)` of `set` written as
/// `<key> = "<tier>" # set by the Agent Tiers dialog, <date>` (`date` is `YYYY-MM-DD`).
/// A key whose line holds `tier` already is not touched; a present key's line is replaced whole; an absent key is
/// inserted after the last top-level `key = value` line before the first table, in the order of `set`. Every other
/// byte is unchanged.
pub fn set_tiers_in_text(text: &str, set: &[(&str, &str)], date: &str) -> String {
    let _ = (text, set, date);
    todo!("stub: the implementation fills it in")
}
