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
