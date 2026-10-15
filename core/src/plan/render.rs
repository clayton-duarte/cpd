//! The plan markdown the User reviews in plannotator (commands.md §4.5, kept
//! from cpd 1 §11 `plan render`): title, Context and phase; a table of every
//! Job not `cancelled`; a Mermaid graph of the Dependencies; one section per
//! Job with its Spec. GitHub-flavoured Markdown only: tables, code and
//! Mermaid, no footnotes, no raw HTML (t4).

use std::collections::BTreeMap;

use serde_json::Value;

use super::jobs::Job;

/// Inline text made safe for GFM: no HTML tags, no footnote syntax. Code
/// spans (`` `…` `` on one line) are left alone.
fn inline(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '`' {
            let close = chars[i + 1..].iter().position(|x| *x == '`' || *x == '\n').map(|n| i + 1 + n);
            if let Some(close) = close.filter(|&n| chars[n] == '`') {
                out.extend(&chars[i..=close]);
                i = close + 1;
                continue;
            }
        }
        match (c, next) {
            ('<', Some(n)) if n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?') => out.push_str("&lt;"),
            ('[', Some('^')) => out.push_str("\\["),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// Block text (goal, context) made safe the same way, leaving fenced code
/// blocks untouched.
fn block(text: &str) -> String {
    let mut fence: Option<String> = None;
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            let marker = fence_marker(line);
            match (&fence, marker) {
                (None, Some(m)) => {
                    fence = Some(m);
                    line.to_owned()
                }
                (Some(open), Some(m)) if m.starts_with(&open[..1]) && m.len() >= open.len() => {
                    fence = None;
                    line.to_owned()
                }
                (None, None) => inline(line),
                (Some(_), _) => line.to_owned(),
            }
        })
        .collect();
    lines.join("\n")
}

/// The fence a line opens or closes (```` ``` ```` or `~~~`, three or more,
/// indented at most three spaces).
fn fence_marker(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = rest.chars().take_while(|x| *x == c).count();
    (run >= 3).then(|| c.to_string().repeat(run))
}

fn cell(text: &str) -> String {
    let lines: Vec<String> = inline(text).split('\n').map(|l| l.trim().to_owned()).collect();
    lines.join(" ").replace('|', "\\|")
}

fn code(text: &str) -> String {
    if text.contains('`') { format!("`` {text} ``") } else { format!("`{text}`") }
}

/// A code span inside a table cell: `|` must be escaped even there.
fn tcode(text: &str) -> String {
    code(text).replace('|', "\\|")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect())
        .unwrap_or_default()
}

fn list(items: &[String]) -> String {
    items.iter().map(|i| format!("- {}", inline(i).replace('\n', "\n  "))).collect::<Vec<_>>().join("\n")
}

fn joined(items: &[String], f: fn(&str) -> String) -> String {
    if items.is_empty() { "none".to_owned() } else { items.iter().map(|i| f(i)).collect::<Vec<_>>().join(", ") }
}

/// Mermaid node ids are positional (`j1`, `j2`, …) because Job keys may be
/// reserved words; the label is the key.
fn graph(jobs: &[&Job]) -> String {
    let ids: BTreeMap<&str, String> = jobs.iter().enumerate().map(|(i, j)| (j.key.as_str(), format!("j{}", i + 1))).collect();
    let mut lines = vec!["```mermaid".to_owned(), "graph TD".to_owned()];
    lines.extend(jobs.iter().map(|j| format!("  {}[\"{}\"]", ids[j.key.as_str()], j.key)));
    for j in jobs {
        for need in &j.needs {
            if let Some(from) = ids.get(need.as_str()) {
                lines.push(format!("  {from} --> {}", ids[j.key.as_str()]));
            }
        }
    }
    lines.push("```".to_owned());
    lines.join("\n")
}

/// The plan markdown for `jobs` (every Job of the Plan, in creation order);
/// `cancelled` Jobs are left out. Pure: the same Jobs give the same text.
pub fn markdown(title: &str, context: &str, phase: &str, jobs: &[Job]) -> String {
    let jobs: Vec<&Job> = jobs.iter().filter(|j| j.status != "cancelled").collect();
    let mut out = vec![format!("# Plan: {}", inline(title)), String::new(), format!("Context: `{context}` · Phase: `{phase}`"), String::new()];
    if jobs.is_empty() {
        out.extend(["No Jobs yet.".to_owned(), String::new()]);
        return out.join("\n");
    }
    out.extend([
        "## Jobs".to_owned(),
        String::new(),
        "| Key | Title | Tier | Status | Needs | Touches |".to_owned(),
        "|---|---|---|---|---|---|".to_owned(),
    ]);
    for j in &jobs {
        out.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            tcode(&j.key),
            cell(&j.title),
            j.tier,
            j.status,
            joined(&j.needs, tcode),
            joined(&strings(&j.touches), tcode),
        ));
    }
    out.extend([String::new(), "## Dependencies".to_owned(), String::new(), graph(&jobs), String::new()]);
    for j in &jobs {
        let out_of_scope = strings(&j.out_of_scope);
        out.extend([
            format!("## {}: {}", code(&j.key), inline(&j.title)),
            String::new(),
            format!("Status: `{}` · Tier: `{}` · Needs: {}", j.status, j.tier, joined(&j.needs, code)),
            String::new(),
            "### Goal".to_owned(),
            String::new(),
            block(&j.goal),
            String::new(),
            "### Context".to_owned(),
            String::new(),
            if j.context.is_empty() { "None.".to_owned() } else { block(&j.context) },
            String::new(),
            "### Files it touches".to_owned(),
            String::new(),
            strings(&j.touches).iter().map(|t| format!("- {}", code(t))).collect::<Vec<_>>().join("\n"),
            String::new(),
            "### Acceptance".to_owned(),
            String::new(),
            list(&strings(&j.acceptance)),
            String::new(),
            "### Out of scope".to_owned(),
            String::new(),
            if out_of_scope.is_empty() { "None.".to_owned() } else { list(&out_of_scope) },
            String::new(),
        ]);
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_text_has_no_html_or_footnotes_outside_code_spans() {
        assert_eq!(inline("a <b> and </i> but 1 < 2"), "a &lt;b> and &lt;/i> but 1 < 2");
        assert_eq!(inline("see [^1]"), "see \\[^1]");
        assert_eq!(inline("keep `<b>` and `[^1]`"), "keep `<b>` and `[^1]`");
        assert_eq!(inline("a lone ` then <b>"), "a lone ` then &lt;b>");
    }

    #[test]
    fn blocks_leave_fenced_code_alone() {
        let text = "Use <b>.\n```html\n<b>bold</b>\n```\nThen <i>.";
        assert_eq!(block(text), "Use &lt;b>.\n```html\n<b>bold</b>\n```\nThen &lt;i>.");
    }

    #[test]
    fn table_cells_escape_pipes_and_newlines() {
        assert_eq!(cell("a | b\n  c"), "a \\| b c");
        assert_eq!(tcode("a|b"), "`a\\|b`");
        assert_eq!(code("a`b"), "`` a`b ``");
    }

    #[test]
    fn a_plan_without_jobs_says_so() {
        assert_eq!(markdown("T", "ctx", "plan", &[]), "# Plan: T\n\nContext: `ctx` · Phase: `plan`\n\nNo Jobs yet.\n");
    }
}
