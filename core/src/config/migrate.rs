//! Config migrations (contract §8.1 "Config migrations", commands.md §14.2, hs-d722):
//! an ordered, additive list of idempotent transforms over `config.toml`, applied by
//! `cpd config --migrate` — and so by `cpd install` before the binary is replaced.

use toml::{Table, Value};

/// One transform of `config.toml`. `run` returns the migrated text only when the stale
/// state is present and its text changed, else `None`; it never fails.
pub struct ConfigMigration {
    /// The stable name `cpd config --migrate` reports and `cpd install` prints.
    pub name: &'static str,
    /// The transform: `Some(new_text)` when it changed the text, else `None`.
    pub run: fn(&str) -> Option<String>,
}

/// The migrations in the order they are applied (contract §8.1): `distill-tier-gx` then
/// `drop-test-concurrency`.
pub const MIGRATIONS: &[ConfigMigration] = &[
    ConfigMigration {
        name: "distill-tier-gx",
        run: distill_tier_gx,
    },
    ConfigMigration {
        name: "drop-test-concurrency",
        run: drop_test_concurrency,
    },
];

/// Folds the migrations in order over `text`, returning the final text and the names of
/// the migrations that changed it, in list order (contract §8.1).
pub fn apply(text: &str) -> (String, Vec<&'static str>) {
    let mut current = text.to_owned();
    let mut applied = Vec::new();
    for migration in MIGRATIONS {
        if let Some(next) = (migration.run)(&current) {
            current = next;
            applied.push(migration.name);
        }
    }
    (current, applied)
}

/// The byte span of the top-level `key = value` line in `text`: `(start, body_end, line_end,
/// eq)`, where `body_end` is past the line's text (before its line ending), `line_end` past the
/// whole line, and `eq` the offset of its `=`. `None` when there is no such line before the
/// first table header, or the value spans lines (the line alone does not parse to the key).
pub(super) fn top_level_line(text: &str, key: &str) -> Option<(usize, usize, usize, usize)> {
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let body_end = start + body.len();
        let line_end = start + line.len();
        if body.trim_start().starts_with('[') {
            // A top-level key cannot follow a table header.
            return None;
        }
        if let Some(eq) = body.find('=') {
            let name = body[..eq].trim().trim_matches(['"', '\'']);
            if name == key {
                // Only when the value is complete on this line: parsing the line alone yields the key.
                if body.parse::<Table>().ok()?.contains_key(key) {
                    return Some((start, body_end, line_end, start + eq));
                }
                return None;
            }
        }
        start = line_end;
    }
    None
}

/// `distill-tier-gx` (hs-d722): a top-level `context_distill_tier` whose value is exactly
/// `"H2"` becomes `"GX"`; every other line, comment and the formatting survive.
fn distill_tier_gx(text: &str) -> Option<String> {
    let table: Table = text.parse().ok()?;
    match table.get("context_distill_tier") {
        Some(Value::String(value)) if value == "H2" => {}
        _ => return None,
    }
    let (_, body_end, _, eq) = top_level_line(text, "context_distill_tier")?;
    let after = &text[eq + 1..body_end];
    let ws = after.len() - after.trim_start().len();
    let value = &after[ws..];
    let quote = value.chars().next().filter(|q| *q == '"' || *q == '\'')?;
    let close = value[quote.len_utf8()..].find(quote)? + quote.len_utf8();
    let token_len = close + quote.len_utf8();
    // The value stands alone: only whitespace or a comment follows it on the line.
    let rest = value[token_len..].trim_start();
    if !rest.is_empty() && !rest.starts_with('#') {
        return None;
    }
    // Splice the whole quoted token, preserving its quote style.
    let token_start = eq + 1 + ws;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..token_start]);
    out.push(quote);
    out.push_str("GX");
    out.push(quote);
    out.push_str(&text[token_start + token_len..]);
    Some(out)
}

/// `drop-test-concurrency` (hs-d679): the top-level `test_concurrency` line is removed; every
/// other line, comment and the formatting survive.
fn drop_test_concurrency(text: &str) -> Option<String> {
    let table: Table = text.parse().ok()?;
    if !table.contains_key("test_concurrency") {
        return None;
    }
    let (start, _, line_end, _) = top_level_line(text, "test_concurrency")?;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(&text[line_end..]);
    Some(out)
}
