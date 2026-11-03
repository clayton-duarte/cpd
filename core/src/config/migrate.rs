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
/// `drop-test-concurrency`, then `drop-plannotator-ports`.
pub const MIGRATIONS: &[ConfigMigration] = &[
    ConfigMigration {
        name: "distill-tier-gx",
        run: distill_tier_gx,
    },
    ConfigMigration {
        name: "drop-test-concurrency",
        run: drop_test_concurrency,
    },
    ConfigMigration {
        name: "drop-plannotator-ports",
        run: drop_plannotator_ports,
    },
    ConfigMigration {
        name: "open-tier-worker-tier",
        run: open_tier_worker_tier,
    },
    ConfigMigration {
        name: "drop-review-lead-tier",
        run: drop_review_lead_tier,
    },
    ConfigMigration {
        name: "drop-inbox-review-context",
        run: drop_inbox_review_context,
    },
    ConfigMigration {
        name: "worker-runner-to-experimental",
        run: worker_runner_to_experimental,
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
    drop_top_level_key(text, "test_concurrency")
}

/// `drop-plannotator-ports` (hs-d739): the top-level `plannotator_ports` line is removed, as above.
fn drop_plannotator_ports(text: &str) -> Option<String> {
    drop_top_level_key(text, "plannotator_ports")
}

/// `open-tier-worker-tier`: the key token of a top-level `open_tier` line becomes `worker_tier`;
/// value and trailing comment survive. Nothing when a top-level `worker_tier` exists.
fn open_tier_worker_tier(text: &str) -> Option<String> {
    let table: Table = text.parse().ok()?;
    if table.contains_key("worker_tier") || !table.contains_key("open_tier") {
        return None;
    }
    let (start, _, _, eq) = top_level_line(text, "open_tier")?;
    let key_end = start + text[start..eq].trim_end().len();
    let key_start = start + (text[start..key_end].len() - text[start..key_end].trim_start().len());
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..key_start]);
    out.push_str("worker_tier");
    out.push_str(&text[key_end..]);
    Some(out)
}

/// `drop-review-lead-tier`: the top-level `review_lead_tier` line is removed, as above.
fn drop_review_lead_tier(text: &str) -> Option<String> {
    drop_top_level_key(text, "review_lead_tier")
}

fn drop_top_level_key(text: &str, key: &str) -> Option<String> {
    let table: Table = text.parse().ok()?;
    if !table.contains_key(key) {
        return None;
    }
    let (start, _, line_end, _) = top_level_line(text, key)?;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(&text[line_end..]);
    Some(out)
}

/// `drop-inbox-review-context`: the `review_context` line of the `[inbox]` table is removed (`inbox claim` always
/// targets `codereview`); every other line, comment and the formatting survive.
fn drop_inbox_review_context(text: &str) -> Option<String> {
    let table: Table = text.parse().ok()?;
    table.get("inbox")?.as_table()?.get("review_context")?;
    let mut in_inbox = false;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let body = line.trim();
        if body.starts_with('[') {
            in_inbox = body.split('#').next().unwrap_or("").trim() == "[inbox]";
        } else if in_inbox
            && let Some((name, _)) = body.split_once('=')
            && name.trim().trim_matches(['"', '\'']) == "review_context"
            && body.parse::<Table>().is_ok()
        {
            let end = start + line.len();
            return Some(format!("{}{}", &text[..start], &text[end..]));
        }
        start += line.len();
    }
    None
}

/// `worker-runner-to-experimental` (A8). STUB: a no-op.
fn worker_runner_to_experimental(_text: &str) -> Option<String> {
    None
}
