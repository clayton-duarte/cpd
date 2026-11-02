//! `config tiers set` (commands.md §14.2a): the pure rewrite of the four Tier keys of `config.toml`.

use super::migrate::top_level_line;
use super::runner::insert_top_level;

/// The text of a `config.toml` with each `(key, tier)` of `set` written as
/// `<key> = "<tier>" # set by the Agent Tiers dialog, <date>` (`date` is `YYYY-MM-DD`).
/// A key whose line holds `tier` already is not touched; a present key's line is replaced whole; an absent key is
/// inserted after the last top-level `key = value` line before the first table, in the order of `set`. Every other
/// byte is unchanged.
pub fn set_tiers_in_text(text: &str, set: &[(&str, &str)], date: &str) -> String {
    let mut out = text.to_owned();
    for (key, tier) in set {
        let held = out
            .parse::<toml::Table>()
            .ok()
            .and_then(|t| t.get(*key).and_then(|v| v.as_str().map(str::to_owned)));
        if held.as_deref() == Some(tier) {
            continue;
        }
        let line = format!("{key} = \"{tier}\" # set by the Agent Tiers dialog, {date}");
        out = match top_level_line(&out, key) {
            Some((start, body_end, _, _)) => {
                format!("{}{line}{}", &out[..start], &out[body_end..])
            }
            None => insert_top_level(&out, &line),
        };
    }
    out
}
