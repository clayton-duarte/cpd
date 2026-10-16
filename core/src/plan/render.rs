//! The plan markdown the User reviews in plannotator (commands.md §4.5, kept
//! from cpd 1 §11 `plan render`, grouped by PR): title, Context and phase; a
//! table of the PRs not `cancelled` and, from two, a Mermaid graph of their
//! needs; then a section per PR with its Jobs' table, their Dependencies graph
//! and one subsection per Job with its Spec. GitHub-flavoured Markdown only:
//! tables, code and Mermaid, no footnotes, no raw HTML (t4).

use std::collections::BTreeMap;

use serde_json::Value;

use super::fingerprint::PrSpec;
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

/// A Mermaid graph of `nodes` (key, keys it needs), `direction` `TD` or `LR`.
/// Node ids are positional (`n1`, `n2`, …) because keys may be reserved
/// words; the label is the key. A need that is not a node is left out.
fn graph(direction: &str, prefix: &str, nodes: &[(&str, &[String])]) -> String {
    let ids: BTreeMap<&str, String> = nodes.iter().enumerate().map(|(i, (k, _))| (*k, format!("{prefix}{}", i + 1))).collect();
    let mut lines = vec!["```mermaid".to_owned(), format!("graph {direction}")];
    lines.extend(nodes.iter().map(|(k, _)| format!("  {}[\"{k}\"]", ids[k])));
    for (key, needs) in nodes {
        for need in *needs {
            if let Some(from) = ids.get(need.as_str()) {
                lines.push(format!("  {from} --> {}", ids[key]));
            }
        }
    }
    lines.push("```".to_owned());
    lines.join("\n")
}

/// The PRs in the order of the needs graph: a PR comes after every PR it
/// needs, ties by key. Needs outside `prs` are ignored; a cycle (the schema
/// forbids none, `pr add` does) ends the order with the rest by key.
fn ordered<'a>(prs: &[&'a PrSpec]) -> Vec<&'a PrSpec> {
    let mut rest: Vec<&PrSpec> = prs.to_vec();
    rest.sort_by(|a, b| a.key.cmp(&b.key));
    let mut out: Vec<&PrSpec> = Vec::new();
    while !rest.is_empty() {
        let next = rest
            .iter()
            .position(|p| p.needs.iter().all(|n| out.iter().any(|o| o.key == *n) || !rest.iter().any(|r| r.key == *n)))
            .unwrap_or(0);
        out.push(rest.remove(next));
    }
    out
}

/// One PR's section: repo, base, needs, its Jobs' table and graph, its Jobs' Specs.
fn pr_section(out: &mut Vec<String>, pr: &PrSpec, jobs: &[&Job]) {
    out.extend([
        format!("## PR {}", pr.key),
        String::new(),
        format!("Repo: {} · Base: `{}` · Needs: {}", code(&pr.repo), pr.base, joined(&pr.needs, code)),
        String::new(),
    ]);
    if jobs.is_empty() {
        out.extend(["No Jobs yet.".to_owned(), String::new()]);
        return;
    }
    out.extend([
        "### Jobs".to_owned(),
        String::new(),
        "| Key | Title | Tier | Status | Needs | Touches |".to_owned(),
        "|---|---|---|---|---|---|".to_owned(),
    ]);
    for j in jobs {
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
    let nodes: Vec<(&str, &[String])> = jobs.iter().map(|j| (j.key.as_str(), j.needs.as_slice())).collect();
    out.extend([String::new(), "### Dependencies".to_owned(), String::new(), graph("TD", "j", &nodes), String::new()]);
    for j in jobs {
        let out_of_scope = strings(&j.out_of_scope);
        out.extend([
            format!("### {}: {}", code(&j.key), inline(&j.title)),
            String::new(),
            format!("Status: `{}` · Tier: `{}` · Needs: {}", j.status, j.tier, joined(&j.needs, code)),
            String::new(),
            "#### Goal".to_owned(),
            String::new(),
            block(&j.goal),
            String::new(),
            "#### Context".to_owned(),
            String::new(),
            if j.context.is_empty() { "None.".to_owned() } else { block(&j.context) },
            String::new(),
            "#### Files it touches".to_owned(),
            String::new(),
            strings(&j.touches).iter().map(|t| format!("- {}", code(t))).collect::<Vec<_>>().join("\n"),
            String::new(),
            "#### Acceptance".to_owned(),
            String::new(),
            list(&strings(&j.acceptance)),
            String::new(),
            "#### Out of scope".to_owned(),
            String::new(),
            if out_of_scope.is_empty() { "None.".to_owned() } else { list(&out_of_scope) },
            String::new(),
        ]);
    }
}

/// The plan markdown for `prs` and `jobs` (every one of the Plan); `cancelled`
/// PRs and Jobs are left out, and so are the Jobs of a `cancelled` PR. Pure:
/// the same PRs and Jobs give the same text.
pub fn markdown(title: &str, context: &str, phase: &str, prs: &[PrSpec], jobs: &[Job]) -> String {
    let prs: Vec<&PrSpec> = prs.iter().filter(|p| p.status != "cancelled").collect();
    let mut out = vec![format!("# Plan: {}", inline(title)), String::new(), format!("Context: `{context}` · Phase: `{phase}`"), String::new()];
    if prs.is_empty() {
        out.extend(["No PRs yet.".to_owned(), String::new()]);
        return out.join("\n");
    }
    let order = ordered(&prs);
    out.extend([
        "## PRs".to_owned(),
        String::new(),
        "| Key | Repo | Base | Needs |".to_owned(),
        "|---|---|---|---|".to_owned(),
    ]);
    for p in &prs {
        out.push(format!("| {} | {} | {} | {} |", tcode(&p.key), tcode(&p.repo), tcode(&p.base), joined(&p.needs, tcode)));
    }
    out.push(String::new());
    if prs.len() >= 2 {
        let nodes: Vec<(&str, &[String])> = order.iter().map(|p| (p.key.as_str(), p.needs.as_slice())).collect();
        out.extend([graph("LR", "p", &nodes), String::new()]);
    }
    for pr in order {
        let jobs: Vec<&Job> = jobs.iter().filter(|j| j.pr == pr.key && j.status != "cancelled").collect();
        pr_section(&mut out, pr, &jobs);
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
    fn a_plan_without_prs_says_so() {
        assert_eq!(markdown("T", "ctx", "plan", &[], &[]), "# Plan: T\n\nContext: `ctx` · Phase: `plan`\n\nNo PRs yet.\n");
    }

    fn pr(key: &str, needs: &[&str]) -> PrSpec {
        PrSpec { key: key.into(), repo: format!("/r/{key}"), base: "main".into(), needs: needs.iter().map(|n| (*n).to_owned()).collect(), status: "waiting".into() }
    }

    #[test]
    fn prs_are_ordered_by_needs_then_key() {
        let all = [pr("a-integration", &["b"]), pr("b", &["a-contract"]), pr("a-contract", &[]), pr("z", &[]), pr("c", &["gone"])];
        let refs: Vec<&PrSpec> = all.iter().collect();
        let keys: Vec<&str> = ordered(&refs).iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["a-contract", "b", "a-integration", "c", "z"]);
    }
}
