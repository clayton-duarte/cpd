//! `plan render` shows work that cannot start yet as `waiting`, with a link to each blocker (commands.md §4.5,
//! hs-d521, hs-d523). The stored status stays `queued`; `queued` in the render means ready to start now.

use crate::plan_render_cancelled::{fx, job, pr, render, row};

/// True when `line` links `key`: a markdown link whose text is the key, to an anchor in the document.
fn links(line: &str, key: &str) -> bool {
    [format!("[`{key}`](#"), format!("[{key}](#")].iter().any(|l| line.contains(l.as_str()))
}

#[test]
fn waiting_job_links_to_each_blocker() {
    let fx = fx();
    pr(&fx.env, "api", "dispatch", "main", &[]);
    job(&fx.env, "api", "a", "queued", &[]);
    job(&fx.env, "api", "b", "queued", &[]);
    job(&fx.env, "api", "m", "merged", &[]);
    job(&fx.env, "api", "c", "queued", &["a", "b", "m"]);
    let (_, text) = render(&fx);
    let c = row(&text, "c");
    assert!(c.contains("waiting on"), "{c}");
    assert!(!c.contains("| queued |"), "waiting, not queued: {c}");
    assert!(links(c, "a") && links(c, "b"), "a link to each blocker: {c}");
    assert!(!links(c, "m"), "a merged Job blocks nothing: {c}");
    // a Job that is ready stays queued
    let a = row(&text, "a");
    assert!(a.contains("queued") && !a.contains("waiting"), "{a}");
    // the stored status does not change
    assert_eq!(fx.env.query::<String>("SELECT status FROM jobs WHERE key = 'c'"), "queued");
}

#[test]
fn stacked_pr_waits_for_its_base_pr() {
    let fx = fx();
    pr(&fx.env, "api", "waiting", "main", &[]);
    job(&fx.env, "api", "a", "draft", &[]);
    // `web` is stacked on `api`: its base is `api`'s Lead Branch, and `api` has not started
    pr(&fx.env, "web", "waiting", "cpd/pay/api", &[]);
    job(&fx.env, "web", "w", "draft", &[]);
    let (_, text) = render(&fx);
    let web = row(&text, "web");
    assert!(web.contains("waiting on"), "{web}");
    assert!(links(web, "api"), "a link to the base PR: {web}");
    let api = row(&text, "api");
    assert!(!api.contains("waiting on"), "the base PR waits for nothing: {api}");
}

#[test]
fn stacked_pr_keeps_waiting_on_its_base_pr_while_it_is_started_and_not_open() {
    let fx = fx();
    pr(&fx.env, "api", "dispatch", "main", &[]);
    job(&fx.env, "api", "a", "queued", &[]);
    pr(&fx.env, "web", "waiting", "cpd/pay/api", &[]);
    job(&fx.env, "web", "w", "draft", &[]);
    // the base PR has started, and has not opened: the stacked PR still waits for it
    let (_, text) = render(&fx);
    let web = row(&text, "web");
    assert!(web.contains("waiting on"), "{web}");
    assert!(links(web, "api"), "a link to the base PR: {web}");
    // once the base PR is open it blocks nothing
    fx.env.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add api', 0, 'open', 'none', 'none', 't', NULL, 't');
         UPDATE workflows SET status = 'open', pr_number = 7, pr_url = 'https://github.com/acme/api/pull/7', pr_opened_at = 't',
                pull_request = (SELECT id FROM pull_requests WHERE number = 7) WHERE plan = 'pay' AND key = 'api';",
    );
    let (_, text) = render(&fx);
    let web = row(&text, "web");
    assert!(!web.contains("waiting on"), "{web}");
}

/// `alpha`, stacked on `zeta` and in `zeta`'s repo (contract §2.2: the base PR is in the same Plan and repo).
fn stacked_alpha(env: &crate::common::TestEnv) {
    env.sql(
        "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, created_txn, created_at)
         VALUES ('pay', 'alpha', '/r/zeta', 'alpha', 'cpd/pay/zeta', 'waiting', 'cpd/pay/alpha', cpd_txn(), 't');",
    );
}

/// `plan render` of `pay` with `web` (key `alpha`) after `zeta`: stacked on it, or needing it (stored); the PR table's
/// Needs cell of `alpha`, the graph's edge lines and the PR sections in order.
fn needs_cell_edges_and_order(stacked: bool) -> (String, Vec<String>, Vec<String>) {
    let fx = fx();
    pr(&fx.env, "zeta", "dispatch", "main", &[]);
    job(&fx.env, "zeta", "z", "queued", &[]);
    if stacked {
        stacked_alpha(&fx.env);
    } else {
        pr(&fx.env, "alpha", "waiting", "main", &["zeta"]);
    }
    job(&fx.env, "alpha", "x", "draft", &[]);
    let (_, text) = render(&fx);
    let header = text.lines().find(|l| l.starts_with("| Key") && l.contains("Needs") && l.contains("Base")).unwrap_or_else(|| panic!("{text}"));
    let col = header.split('|').position(|c| c.trim() == "Needs").unwrap();
    let cell = row(&text, "alpha").split('|').nth(col).unwrap().trim().to_string();
    let graph = text.split("graph LR").nth(1).unwrap_or_else(|| panic!("no PR graph:\n{text}")).split("```").next().unwrap();
    let edges = graph.lines().filter(|l| l.contains("-->")).map(|l| l.trim().to_string()).collect();
    let sections = text.lines().filter(|l| l.starts_with("## Workflow ")).map(String::from).collect();
    (cell, edges, sections)
}

#[test]
fn stacked_pr_lists_its_base_pr_among_its_needs_and_follows_it_in_the_order() {
    let (cell, edges, sections) = needs_cell_edges_and_order(false);
    assert!(cell.contains("zeta") && !edges.is_empty(), "a stored need is shown: {cell:?} {edges:?}");
    assert_eq!(sections, ["## Workflow zeta", "## Workflow alpha"], "a need comes first");
    let (stacked_cell, stacked_edges, stacked_sections) = needs_cell_edges_and_order(true);
    assert_eq!(stacked_cell, cell, "the base PR is in the Needs cell like a stored need");
    assert_eq!(stacked_edges, edges, "the base PR is an edge of the graph like a stored need");
    assert_eq!(stacked_sections, sections, "the PR follows its base PR though its key sorts first");

    // derived, not stored: `plan show` keeps the stored needs and the base PR
    let fx = fx();
    pr(&fx.env, "zeta", "dispatch", "main", &[]);
    stacked_alpha(&fx.env);
    let show = fx.cpd(&["plan", "show", "--plan", "pay"]).ok();
    let alpha = show["workflows"].as_array().unwrap().iter().find(|p| p["key"] == "alpha").unwrap_or_else(|| panic!("{show}"));
    assert_eq!(alpha["needs"], serde_json::json!([]), "{show}");
    assert_eq!(alpha["base_pr"], "pay/zeta", "{show}");
}
