//! The search-query sanitiser: rewrites a typed query into the FTS5 `MATCH` string cpd sends.
//! The grammar (rules R1-R10) is `docs/research/context-holographic.md`, `## Search contract`.

/// Rewrite `query` per the search contract: words are quoted, deliberate operators are kept.
pub(super) fn sanitise(query: &str) -> String {
    let query = query.trim();
    let mut out = String::with_capacity(query.len() + 8);
    // Open groups; `true` marks a NEAR group.
    let mut groups: Vec<bool> = Vec::new();
    let mut rest = query;
    while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        } else if c == '"' {
            let Some(end) = phrase_end(rest) else {
                out.push_str(rest);
                break;
            };
            let end = if rest[end..].starts_with('*') {
                end + 1
            } else {
                end
            };
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        } else {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '"')
                .unwrap_or(rest.len());
            chunk(&rest[..end], &mut groups, &mut out);
            rest = &rest[end..];
        }
    }
    out
}

/// Byte length of the phrase at the start of `s` (opening `"` to closing `"`), if closed.
fn phrase_end(s: &str) -> Option<usize> {
    let mut i = 1;
    while let Some(p) = s[i..].find('"') {
        i += p + 1;
        if s[i..].starts_with('"') {
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}

/// Rewrite one run of non-whitespace, non-`"` characters.
fn chunk(word: &str, groups: &mut Vec<bool>, out: &mut String) {
    let mut body = word;
    loop {
        if body.starts_with("NEAR(") {
            groups.push(true);
            out.push_str("NEAR(");
            body = &body[5..];
        } else if body.starts_with('(') {
            groups.push(false);
            out.push('(');
            body = &body[1..];
        } else {
            break;
        }
    }
    let closers = body.len() - body.trim_end_matches(')').len();
    let closers = closers.min(groups.len());
    body = &body[..body.len() - closers];
    if groups.last() == Some(&true) {
        let parts: Vec<String> = body.split(',').map(text).collect();
        out.push_str(&parts.join(","));
    } else {
        out.push_str(&text(body));
    }
    for _ in 0..closers {
        groups.pop();
        out.push(')');
    }
}

/// Classify one word: operator and bare words stay, anything else is quoted.
fn text(word: &str) -> String {
    if word.is_empty() || matches!(word, "AND" | "OR" | "NOT") {
        return word.to_owned();
    }
    let (core, star) = match word.strip_suffix('*') {
        Some(core) => (core, "*"),
        None => (word, ""),
    };
    if core.is_empty() {
        return format!("\"{word}\"");
    }
    if core
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii())
    {
        return word.to_owned();
    }
    format!("\"{core}\"{star}")
}
