//! The record ids a text names after `supersedes`, `overrules` or `replaces`.
//!
//! Pure: no store access, so other writes can reuse it as an id scanner.

use crate::ids::RecordId;

const VERBS: [&str; 3] = ["supersedes", "overrules", "replaces"];

/// A token: a run of letters, digits, `-` and `_`.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// The token at the start of `s`, and the rest.
fn token(s: &str) -> (&str, &str) {
    let end = s.find(|c| !is_word(c)).unwrap_or(s.len());
    s.split_at(end)
}

/// The id at the start of `s` (after blanks), and the rest.
fn id(s: &str) -> Option<(RecordId, &str)> {
    let (word, rest) = token(s.trim_start());
    RecordId::parse(word).ok().map(|id| (id, rest))
}

/// The separator at the start of `s` (`,`, `&` or the word `and`) removed, `None` when there is none.
fn separator(s: &str) -> Option<&str> {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix(',').or_else(|| s.strip_prefix('&')) {
        let rest = rest.trim_start();
        let (word, after) = token(rest);
        return Some(if word.eq_ignore_ascii_case("and") {
            after
        } else {
            rest
        });
    }
    let (word, rest) = token(s);
    word.eq_ignore_ascii_case("and").then_some(rest)
}

/// The ids named after a supersession verb, in text order, each once.
///
/// After a verb (any case) come one or more ids joined by `,`, `and` or `&`; a list ends at the first token that is
/// not an id, so `supersedes A1 of pro-d342` names none.
pub fn prose_supersession_targets(text: &str) -> Vec<RecordId> {
    let mut found: Vec<RecordId> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(at) = rest.find(is_word) else { break };
        let tail = &rest[at..];
        let (word, after) = token(tail);
        rest = after;
        if !VERBS.iter().any(|v| word.eq_ignore_ascii_case(v)) {
            continue;
        }
        let mut cursor = after;
        while let Some((id, next)) = id(cursor) {
            if !found.contains(&id) {
                found.push(id);
            }
            rest = next;
            match separator(next) {
                Some(more) => cursor = more,
                None => break,
            }
        }
    }
    found
}
