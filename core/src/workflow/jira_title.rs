//! The Jira title of a Workflow (contract §7.6 "The Jira title", pro-d399): `<type>[!]: <KEY>/<summary>`.

use super::super::args::WorkflowTitle;

const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// The length of a leading Jira key and its slash (`[A-Z][A-Z0-9]*-[0-9]+/`) in `text`, if it starts with one.
fn key_prefix_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !bytes.first()?.is_ascii_uppercase() {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() && (bytes[i].is_ascii_uppercase() || bytes[i].is_ascii_digit()) {
        i += 1;
    }
    if bytes.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let digits = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    (i > digits && bytes.get(i) == Some(&b'/')).then_some(i + 1)
}

/// The title with `key` composed in after `<type>[!]: `, or unchanged when it already holds `key`; `None` when it cannot conform.
pub fn conform(title: &str, key: &str) -> Option<String> {
    let (head, summary) = title.split_once(": ")?;
    let ty = head.strip_suffix('!').unwrap_or(head);
    if !TYPES.contains(&ty) {
        return None;
    }
    let composed = match key_prefix_len(summary) {
        Some(len) => {
            if &summary[..len - 1] != key || summary.len() == len {
                return None;
            }
            title.to_owned()
        }
        None if summary.is_empty() => return None,
        None => format!("{head}: {key}/{summary}"),
    };
    (composed.chars().count() <= WorkflowTitle::MAX_CHARS).then_some(composed)
}
