//! The plan markdown the User reviews in the chat (commands.md §4.5, kept
//! from cpd 1 §11 `plan render`, grouped by PR): title, Context and phase; a
//! table of the PRs not `cancelled` and, from two, a Mermaid graph of their
//! needs; then a section per PR with its Jobs' table, their Dependencies graph
//! and one subsection per Job with its Spec; between them the `## Overlaps`
//! section. GitHub-flavoured Markdown only:
//! tables, code and Mermaid, no footnotes, no raw HTML (t4).

use std::collections::BTreeMap;

use serde_json::Value;

use super::display::Display;
use super::jobs::{Job, JobSpec};
use super::overlap::Overlap;
use super::prs::PrRow;

/// What the render shows for what a stub draft lacks (commands.md §4.5, hs-d241).
const MISSING_TIER: &str = "—";
const MISSING_SPEC: &str = "Spec: not written yet";

/// What the render says in place of the Spec of an `open` Job's placeholder (commands.md §4.5, hs-d497).
const OPEN_PLACEHOLDER: &str = "Added by cpd when the PR starts: merges the base only on a conflict, runs the Gate, pushes and opens the pull request ready.";
/// The placeholder's `Needs` (the Job's Ready rule, contract §3.2): not Dependencies, so no row and no graph edge.
const OPEN_NEEDS: &str = "every other Job of the PR";

/// The `open` Job a PR that has not started will get (hs-d497): shown, never stored. `tier` is `open_tier`.
/// Only for a PR whose Plan holds no `open` Job (an older PR): a template's opener stub is a real Job and is shown as its own row.
struct OpenPlaceholder<'a> {
    pr: &'a PrRow,
    tier: &'a str,
}

impl OpenPlaceholder<'_> {
    fn key(&self) -> String {
        format!("open-{}", self.pr.key)
    }

    fn title(&self) -> String {
        format!("Open the pull request: {}", self.pr.title.as_deref().unwrap_or(self.pr.key.as_str()))
    }
}

/// Inline text made safe for GFM: no HTML tags, no footnote syntax. Code
/// spans (`` `…` `` on one line) are left alone.
pub(super) fn inline(text: &str) -> String {
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

pub(super) fn cell(text: &str) -> String {
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
fn ordered<'a>(prs: &[&'a PrRow], display: &Display) -> Vec<&'a PrRow> {
    let mut rest: Vec<&PrRow> = prs.to_vec();
    rest.sort_by(|a, b| a.key.cmp(&b.key));
    let mut out: Vec<&PrRow> = Vec::new();
    while !rest.is_empty() {
        let next = rest
            .iter()
            .position(|p| display.pr_needs(p).iter().all(|n| out.iter().any(|o| o.key.as_str() == n) || !rest.iter().any(|r| r.key.as_str() == n)))
            .unwrap_or(0);
        out.push(rest.remove(next));
    }
    out
}

/// The anchor GitHub gives a heading: its text lowercased, spaces as `-`, everything but letters, digits, `-` and `_` dropped.
fn anchor(heading: &str) -> String {
    heading.to_lowercase().chars().filter_map(|c| if c == ' ' { Some('-') } else { (c.is_alphanumeric() || c == '-' || c == '_').then_some(c) }).collect()
}

/// Where each blocker is shown: a Job's section, a PR's section. A reference of another Plan has none.
struct Anchors {
    jobs: BTreeMap<String, String>,
}

impl Anchors {
    fn new(jobs: &[Job]) -> Anchors {
        Anchors { jobs: jobs.iter().map(|j| (j.key.clone(), anchor(&format!("{}: {}", j.key, inline(&j.title))))).collect() }
    }

    /// `key` as a link to its section, or as it is when it has none (a PR of another Plan).
    fn link(&self, key: &str) -> String {
        let target = match self.jobs.get(key) {
            Some(a) => Some(a.clone()),
            None if !key.contains('/') => Some(anchor(&format!("PR {key}"))),
            None => None,
        };
        target.map_or_else(|| format!("`{key}`"), |a| format!("[`{key}`](#{a})"))
    }

    fn waiting_on(&self, blockers: &[String]) -> String {
        format!("waiting on {}", blockers.iter().map(|b| self.link(b)).collect::<Vec<_>>().join(", "))
    }
}

/// A Job's Status cell: `waiting on <links>` for a `queued` Job that cannot start yet, `cancelled: <reason>` for a cancelled one.
fn job_status(j: &Job, blockers: &[String], anchors: &Anchors) -> String {
    match (j.status.as_str(), &j.cancel_reason) {
        _ if !blockers.is_empty() => anchors.waiting_on(blockers),
        ("cancelled", Some(reason)) => format!("cancelled: {}", cell(reason)),
        _ => j.status.clone(),
    }
}

/// What a Job is in a review of a phase after `plan` (§4.5): a `draft` is
/// under review, every other Job is Locked context.
fn mark(job: &Job) -> &'static str {
    if job.is_under_review() { "**under review**" } else { "Locked" }
}

/// What the sections of a render are drawn from: the Plan's Jobs, what the views show of waiting and cancelled work, and the
/// anchors of the sections.
struct Shown<'a> {
    all_jobs: &'a [Job],
    display: &'a Display,
    anchors: Anchors,
}

/// One PR's section: repo, base, needs, its Jobs' table and graph, its Jobs'
/// Specs; `later` is a phase after `plan`, where the drafts are marked. `jobs` include the cancelled ones: they are
/// in the table, in no graph and with no Spec.
fn pr_section(out: &mut Vec<String>, pr: &PrRow, jobs: &[&Job], later: bool, open_tier: &str, shown: &Shown) {
    // A PR that has not started, and whose Plan holds no `open` Job (an older PR), has no opener stub: its placeholder is the last row and the last section.
    let placeholder = (pr.status == "waiting" && !jobs.iter().any(|j| j.kind == "open")).then_some(OpenPlaceholder { pr, tier: open_tier });
    out.extend([
        format!("## PR {}", pr.key),
        String::new(),
        format!("Repo: {} · Base: `{}` · Needs: {}", code(&pr.repo), pr.base, joined(&shown.display.pr_needs(pr), code)),
        String::new(),
    ]);
    if jobs.is_empty() && placeholder.is_none() {
        out.extend(["No Jobs yet.".to_owned(), String::new()]);
        return;
    }
    // In a phase after `plan` the table has a last column, Review: what each Job is in this review.
    let (head, rule) = if later {
        ("| Key | Title | Kind | Tier | Status | Needs | Touches | Review |", "|---|---|---|---|---|---|---|---|")
    } else {
        ("| Key | Title | Kind | Tier | Status | Needs | Touches |", "|---|---|---|---|---|---|---|")
    };
    out.extend(["### Jobs".to_owned(), String::new(), head.to_owned(), rule.to_owned()]);
    let blockers: Vec<Vec<String>> = jobs.iter().map(|j| shown.display.job_blockers(shown.all_jobs, j)).collect();
    for (j, blockers) in jobs.iter().zip(&blockers) {
        let review = if later { format!(" {} |", mark(j)) } else { String::new() };
        out.push(format!(
            "| {} | {} | {} | {} | {} | {} | {} |{review}",
            tcode(&j.key),
            cell(&j.title),
            j.kind,
            shown.display.job_tier(j).as_deref().unwrap_or(MISSING_TIER),
            job_status(j, blockers, &shown.anchors),
            joined(&j.needs, tcode),
            j.spec.as_ref().map_or_else(|| MISSING_TIER.to_owned(), |s| joined(&strings(&s.touches), tcode)),
        ));
    }
    if let Some(open) = &placeholder {
        let review = if later { format!(" {MISSING_TIER} |") } else { String::new() };
        out.push(format!(
            "| {} | {} | open | {} | {MISSING_TIER} | {OPEN_NEEDS} | {MISSING_TIER} |{review}",
            tcode(&open.key()),
            cell(&open.title()),
            open.tier,
        ));
    }
    let live: Vec<(&&Job, &Vec<String>)> = jobs.iter().zip(&blockers).filter(|(j, _)| j.status != "cancelled").collect();
    if !live.is_empty() {
        let nodes: Vec<(&str, &[String])> = live.iter().map(|(j, _)| (j.key.as_str(), j.needs.as_slice())).collect();
        out.extend([String::new(), "### Dependencies".to_owned(), String::new(), graph("TD", "j", &nodes), String::new()]);
    } else {
        out.push(String::new());
    }
    for (j, blockers) in live {
        let waiting = if blockers.is_empty() { String::new() } else { format!(" · {}", shown.anchors.waiting_on(blockers)) };
        out.extend([
            format!("### {}: {}", code(&j.key), inline(&j.title)),
            String::new(),
            format!(
                "Status: `{}`{waiting}{}{} · Tier: {} · Needs: {}",
                if blockers.is_empty() { j.status.as_str() } else { "waiting" },
                if j.kind == "work" { String::new() } else { format!(" · Kind: `{}`", j.kind) },
                if later { format!(" · {}", mark(j)) } else { String::new() },
                shown.display.job_tier(j).map_or_else(|| MISSING_TIER.to_owned(), |t| format!("`{t}`")),
                joined(&j.needs, code)
            ),
            String::new(),
        ]);
        match &j.spec {
            Some(spec) => spec_section(out, spec),
            None => out.extend([MISSING_SPEC.to_owned(), String::new()]),
        }
        if let (Some(tests), Some(files)) = (&j.tests, &j.test_files) {
            out.extend(["#### Tests".to_owned(), String::new(), strings(tests).iter().map(|t| format!("- {}", code(t))).collect::<Vec<_>>().join("\n"), String::new()]);
            out.extend(["#### Test files".to_owned(), String::new(), strings(files).iter().map(|t| format!("- {}", code(t))).collect::<Vec<_>>().join("\n"), String::new()]);
        }
        if let Some(resources) = &j.resources {
            out.extend(["#### Resources".to_owned(), String::new(), strings(resources).iter().map(|t| format!("- {}", code(t))).collect::<Vec<_>>().join("\n"), String::new()]);
        }
    }
    if let Some(open) = &placeholder {
        out.extend([
            format!("### {}: {}", code(&open.key()), inline(&open.title())),
            String::new(),
            format!("Status: {MISSING_TIER} · Kind: `open` · Tier: `{}` · Needs: {OPEN_NEEDS}", open.tier),
            String::new(),
            OPEN_PLACEHOLDER.to_owned(),
            String::new(),
        ]);
    }
}

/// A Job's Spec: goal, context, the files it touches, acceptance, out of scope.
fn spec_section(out: &mut Vec<String>, spec: &JobSpec) {
    let out_of_scope = strings(&spec.out_of_scope);
    out.extend([
        "#### Goal".to_owned(),
        String::new(),
        block(&spec.goal),
        String::new(),
        "#### Context".to_owned(),
        String::new(),
        if spec.context.is_empty() { "None.".to_owned() } else { block(&spec.context) },
        String::new(),
        "#### Files it touches".to_owned(),
        String::new(),
        strings(&spec.touches).iter().map(|t| format!("- {}", code(t))).collect::<Vec<_>>().join("\n"),
        String::new(),
        "#### Acceptance".to_owned(),
        String::new(),
        list(&strings(&spec.acceptance)),
        String::new(),
        "#### Out of scope".to_owned(),
        String::new(),
        if out_of_scope.is_empty() { "None.".to_owned() } else { list(&out_of_scope) },
        String::new(),
    ]);
}

/// The most paths a row of `## Overlaps` shows before it only counts the rest.
const PATHS_SHOWN: usize = 5;

/// The `Paths` cell: the paths in code spans, at most [`PATHS_SHOWN`], then
/// `+N` for the N more.
fn paths_cell(paths: &[String]) -> String {
    let mut cell: Vec<String> = paths.iter().take(PATHS_SHOWN).map(|p| tcode(p)).collect();
    if paths.len() > PATHS_SHOWN {
        cell.push(format!("+{}", paths.len() - PATHS_SHOWN));
    }
    cell.join(", ")
}

/// The `## Overlaps` section (commands.md §4.5): a table, or that no other
/// active Plan touches these files.
fn overlaps_section(out: &mut Vec<String>, overlaps: &[Overlap]) {
    out.extend(["## Overlaps".to_owned(), String::new()]);
    if overlaps.is_empty() {
        out.push("No other active Plan touches these files.".to_owned());
    } else {
        out.extend(["| Job | Other Plan | PR | Its Jobs (status) | Paths |".to_owned(), "|---|---|---|---|---|".to_owned()]);
        for o in overlaps {
            let other_jobs: Vec<String> = o.other_jobs.iter().map(|j| format!("{} ({})", tcode(&j.key), j.status)).collect();
            out.push(format!("| {} | {} | {} | {} | {} |", tcode(&o.job), tcode(&o.plan), tcode(&o.pr), other_jobs.join(", "), paths_cell(&o.paths)));
        }
    }
    out.push(String::new());
}

/// What the document opens with (commands.md §4.5, §4.6; hs-d291): `plan.md`
/// is headed by the Plan.
pub enum Heading<'a> {
    Plan { title: &'a str, plan: &'a str },
}

/// The plan markdown for `prs` and `jobs` (every one of the Plan); `cancelled`
/// PRs and Jobs are shown, after the live ones, and counted nowhere. `display` says what waits and what was superseded.
/// `overlaps` are those of [`super::overlap::find`]; `open_tier` is the config key of that name, the Tier of the
/// placeholder `open` Job of a PR that has not started (§4.5, hs-d497). Pure: the same PRs, Jobs
/// and overlaps give the same text.
pub fn markdown(heading: &Heading, context: &str, phase: &str, prs: &[PrRow], jobs: &[Job], overlaps: &[Overlap], open_tier: &str, display: &Display) -> String {
    let shown = Shown { all_jobs: jobs, display, anchors: Anchors::new(jobs) };
    let (cancelled, prs): (Vec<&PrRow>, Vec<&PrRow>) = prs.iter().partition(|p| p.status == "cancelled");
    let mut out = match heading {
        Heading::Plan { title, .. } => vec![format!("# Plan: {}", inline(title)), String::new()],
    };
    out.extend([format!("Context: `{context}` · Phase: `{phase}`"), String::new()]);
    let later = !matches!(phase, "grounding" | "plan");
    if later {
        out.extend([
            "This review covers the Jobs marked **under review**, the draft Jobs only; the Jobs marked Locked are shown as context and are not for approval.".to_owned(),
            String::new(),
        ]);
    }
    if prs.is_empty() && cancelled.is_empty() {
        out.extend(["No PRs yet.".to_owned(), String::new()]);
        return out.join("\n");
    }
    let order = ordered(&prs, display);
    out.extend([
        "## PRs".to_owned(),
        String::new(),
        "| Key | Repo | Base | Needs | Status |".to_owned(),
        "|---|---|---|---|---|".to_owned(),
    ]);
    for p in prs.iter().chain(&cancelled) {
        let status = match (p.status.as_str(), display.pr_blockers(p)) {
            ("cancelled", _) => match display.superseded_by(p.key.as_str()) {
                Some(by) => format!("cancelled · superseded by {by}"),
                None => "cancelled".to_owned(),
            },
            (_, blockers) if !blockers.is_empty() => shown.anchors.waiting_on(&blockers),
            ("waiting", _) => "queued".to_owned(),
            (status, _) => status.to_owned(),
        };
        out.push(format!("| {} | {} | {} | {} | {status} |", tcode(p.key.as_str()), tcode(&p.repo), tcode(&p.base), joined(&display.pr_needs(p), tcode)));
    }
    out.push(String::new());
    if prs.len() >= 2 {
        let needs: Vec<Vec<String>> = order.iter().map(|p| display.pr_needs(p)).collect();
        let nodes: Vec<(&str, &[String])> = order.iter().zip(&needs).map(|(p, n)| (p.key.as_str(), n.as_slice())).collect();
        out.extend([graph("LR", "p", &nodes), String::new()]);
    }
    overlaps_section(&mut out, overlaps);
    let mut cancelled = cancelled;
    cancelled.sort_by(|a, b| a.key.cmp(&b.key));
    for pr in order.into_iter().chain(cancelled) {
        let jobs: Vec<&Job> = jobs.iter().filter(|j| j.pr_str() == pr.key.as_str()).collect();
        pr_section(&mut out, pr, &jobs, later, open_tier, &shown);
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
    fn the_paths_cell_shows_five_paths_and_counts_the_rest() {
        let paths = |n: usize| (1..=n).map(|i| format!("a/{i}.rs")).collect::<Vec<_>>();
        assert_eq!(paths_cell(&paths(1)), "`a/1.rs`");
        assert_eq!(paths_cell(&paths(5)), "`a/1.rs`, `a/2.rs`, `a/3.rs`, `a/4.rs`, `a/5.rs`");
        assert_eq!(paths_cell(&paths(6)), "`a/1.rs`, `a/2.rs`, `a/3.rs`, `a/4.rs`, `a/5.rs`, +1");
        assert_eq!(paths_cell(&paths(40)), "`a/1.rs`, `a/2.rs`, `a/3.rs`, `a/4.rs`, `a/5.rs`, +35");
    }

    #[test]
    fn a_plan_without_prs_says_so() {
        assert_eq!(markdown(&Heading::Plan { title: "T", plan: "p1" }, "ctx", "plan", &[], &[], &[], "S2", &Display::default()), "# Plan: T\n\nContext: `ctx` · Phase: `plan`\n\nNo PRs yet.\n");
    }

    fn pr(key: &str, needs: &[&str]) -> PrRow {
        PrRow::fixture(key, &format!("/r/{key}"), "main", needs, "waiting")
    }

    #[test]
    fn prs_are_ordered_by_needs_then_key() {
        let all = [pr("a-integration", &["b"]), pr("b", &["a-contract"]), pr("a-contract", &[]), pr("z", &[]), pr("c", &["gone"])];
        let refs: Vec<&PrRow> = all.iter().collect();
        let keys: Vec<&str> = ordered(&refs, &Display::of(&all)).iter().map(|p| p.key.as_str()).collect();
        assert_eq!(keys, ["a-contract", "b", "a-integration", "c", "z"]);
    }

    fn job(key: &str, pr: &str, kind: &str) -> Job {
        Job {
            seq: 1,
            key: key.into(),
            pr: Some(pr.into()),
            title: format!("Title {key}"),
            tier: Some("S2".into()),
            spec: Some(JobSpec { goal: "g".into(), context: "c".into(), touches: serde_json::json!(["a"]), acceptance: serde_json::json!(["b"]), out_of_scope: serde_json::json!([]) }),
            status: "draft".into(),
            appended: false,
            kind: kind.into(),
            tests: None,
            test_files: None,
            test_approved: None,
            resources: None,
            urgent: false,
            fingerprint: None,
            retry_tier: None,
            cancel_reason: None,
            cancel_requested_at: None,
            failed_cause: None,
            merged_sha: None,
            summary: None,
            created_at: "t".into(),
            needs: Vec::new(),
        }
    }

    /// The Plan `T` as `markdown` renders it for `prs` and `jobs` alone: no needs of other Plans, nothing superseded.
    fn plain(phase: &str, prs: &[PrRow], jobs: &[Job], open_tier: &str) -> String {
        markdown(&Heading::Plan { title: "T", plan: "p1" }, "ctx", phase, prs, jobs, &[], open_tier, &Display::of(prs))
    }

    fn titled_pr(key: &str, status: &str) -> PrRow {
        let mut pr = PrRow::fixture(key, "/r", "main", &[], status);
        pr.title = Some(format!("Change {key}"));
        pr
    }

    #[test]
    fn a_pr_that_has_not_started_shows_the_open_job_as_its_last_row_and_section_at_open_tier() {
        let mut test = job("web-tests", "web", "test");
        test.tests = Some(serde_json::json!(["t::one"]));
        test.test_files = Some(serde_json::json!(["tests/t.rs"]));
        let md = plain("plan", &[titled_pr("web", "waiting")], &[job("web-docs", "web", "docs"), test], "S3");
        assert!(md.contains("| Key | Title | Kind | Tier | Status | Needs | Touches |"), "{md}");
        assert!(md.contains("| `web-docs` | Title web-docs | docs | S2 | draft | none | `a` |"), "{md}");
        // The last row of the table: key, title, kind, `open_tier`, no status, the Ready rule in place of needs.
        let rows: Vec<&str> = md.lines().filter(|l| l.starts_with("| `")).collect();
        assert_eq!(rows.last().unwrap(), &"| `open-web` | Open the pull request: Change web | open | S3 | — | every other Job of the PR | — |");
        // The last section, with the text in place of a Spec; a `test` Job lists its tests and files.
        assert!(md.trim_end().ends_with(&format!("### `open-web`: Open the pull request: Change web\n\nStatus: — · Kind: `open` · Tier: `S3` · Needs: every other Job of the PR\n\n{OPEN_PLACEHOLDER}")), "{md}");
        assert!(md.contains("Status: `draft` · Kind: `test` · Tier: `S2` · Needs: none"), "{md}");
        assert!(md.contains("#### Tests\n\n- `t::one`\n\n#### Test files\n\n- `tests/t.rs`"), "{md}");
        // It is no Job: no node in the graph.
        assert!(!md.contains("open-web[") && !md.contains("[\"open-web\"]"), "{md}");
    }

    #[test]
    fn the_open_job_placeholder_is_for_a_pr_that_has_not_started_only_and_in_every_phase_until_then() {
        let started = plain("dispatch", &[titled_pr("web", "dispatch")], &[job("web-docs", "web", "docs")], "S2");
        assert!(!started.contains("open-web"), "{started}");
        let waiting = plain("dispatch", &[titled_pr("web", "waiting")], &[], "S2");
        assert!(waiting.contains("| `open-web` | Open the pull request: Change web | open | S2 | — | every other Job of the PR | — | — |"), "{waiting}");
        // A PR with no Jobs yet is not "No Jobs yet": it has the Job cpd adds.
        assert!(!waiting.contains("No Jobs yet."), "{waiting}");
        let cancelled = plain("plan", &[titled_pr("web", "cancelled")], &[], "S2");
        assert!(!cancelled.contains("open-web"), "{cancelled}");
    }

    #[test]
    fn cancelled_work_is_shown_after_the_live_prs_with_its_status_and_reason_and_in_no_graph() {
        let mut gone = job("gone", "web", "docs");
        (gone.status, gone.cancel_reason) = ("cancelled".into(), Some("not needed".into()));
        let mut queued = job("later", "web", "docs");
        (queued.status, queued.needs) = ("queued".into(), vec!["gone".into(), "web-docs".into()]);
        let prs = [titled_pr("aaa", "cancelled"), titled_pr("web", "dispatch")];
        let md = plain("dispatch", &prs, &[gone, queued, job("web-docs", "web", "docs")], "S2");
        assert!(md.contains("| `gone` | Title gone | docs | S2 | cancelled: not needed | none | `a` |"), "{md}");
        // a need on a cancelled Job blocks nothing: only the draft `web-docs` is waited for
        assert!(md.contains("waiting on [`web-docs`](#web-docs-title-web-docs) |"), "{md}");
        assert!(md.find("| `web` |").unwrap() < md.find("| `aaa` |").unwrap(), "{md}");
        assert!(md.contains("| `aaa` | `/r` | `main` | none | cancelled |"), "{md}");
        assert!(!md.contains("[\"gone\"]") && !md.contains("### `gone`"), "{md}");
    }
}
