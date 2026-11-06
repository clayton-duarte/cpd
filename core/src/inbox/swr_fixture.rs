//! Test support: the rows the single pass (`pull_requests`, `swr_pr_detail`, `swr_viewer`) and E11 (`swr_slack_posts`) write, so a
//! review-requested test states its Slack posts and its GitHub answer as data, not through `run/team-review.json` and `gh`.

use std::collections::BTreeMap;

use rusqlite::params;
use serde_json::{Value, json};

use super::team::{GhPr, GhState, Lookup};
use crate::error::CpdError;
use crate::store::Store;

/// The team channel the posts of these tests were read from.
pub const CHANNEL: &str = "C02JU6C2Z1D";

/// One Slack post, as the old cache's JSON had it (`repo`, `number`, `ts`, `primary`, `reactions`, `claim`, `eyes_me`,
/// `thread_tags_me`); a post that is not primary is not stored.
pub fn posts(store: &mut Store, posts: &[Value]) -> Result<(), CpdError> {
    store.write("fixture posts", |tx| {
        tx.execute("DELETE FROM swr_slack_posts", [])?;
        for p in posts.iter().filter(|p| p["primary"].as_bool() == Some(true)) {
            let ts = format!("{:.6}", p["ts"].as_f64().unwrap_or_default());
            let claim = (!p["claim"].is_null()).then(|| p["claim"].to_string());
            tx.execute(
                "INSERT INTO swr_slack_posts (channel, ts, repo, number, author_is_me, reply_count, reactions_json, eyes_me,
                                              claim, thread_tags_me, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?6, ?7, ?8, 't')",
                params![
                    CHANNEL,
                    ts,
                    p["repo"].as_str(),
                    p["number"].as_i64(),
                    p["reactions"].to_string(),
                    p["eyes_me"].as_bool(),
                    claim,
                    p["thread_tags_me"].as_bool().unwrap_or(false)
                ],
            )?;
        }
        Ok::<(), CpdError>(())
    })
}

/// The viewer and each pull request of `lookup`, as the pass would have stored them: an open or merged row, review requested of
/// the viewer, its reviews and head checks in the detail. A `None` pull request has no row.
pub fn lookup(store: &mut Store, lookup: &Lookup) -> Result<(), CpdError> {
    store.write("fixture lookup", |tx| {
        tx.execute(
            "INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, ?1, 't')
             ON CONFLICT (id) DO UPDATE SET login = excluded.login",
            [&lookup.me],
        )?;
        for ((repo, number), pr) in &lookup.workflows {
            tx.execute(
                "DELETE FROM pull_requests WHERE repo = ?1 AND number = ?2",
                params![repo, *number as i64],
            )?;
            let Some(pr) = pr else { continue };
            let state = match pr.state {
                GhState::Open => "open",
                GhState::Merged => "merged",
                GhState::Closed => "closed",
            };
            let decision = match pr.decision.as_deref() {
                Some("APPROVED") => "approved",
                Some("CHANGES_REQUESTED") => "changes_requested",
                _ => "none",
            };
            let url = format!("https://github.com/{repo}/pull/{number}");
            tx.execute(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                            authored, review_requested, head_oid)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'none', 't', 't', ?8, 1, ?9)",
                params![
                    repo,
                    *number as i64,
                    url,
                    pr.title,
                    pr.draft,
                    state,
                    decision,
                    pr.author == lookup.me,
                    pr.head_oid
                ],
            )?;
            let reviews: Vec<Value> = pr
                .reviews
                .iter()
                .map(|r| {
                    json!({"state": r.state, "author": {"login": r.author}, "commit": {"oid": r.commit_oid}})
                })
                .collect();
            let detail = json!({
                "latestReviews": {"nodes": reviews},
                "commits": {"nodes": [{"commit": {"oid": pr.head_oid, "statusCheckRollup": {"state": pr.checks_state, "contexts": {"nodes": []}}}}]},
                "reviewThreads": {"nodes": []}, "comments": {"nodes": []}
            });
            tx.execute(
                "INSERT OR REPLACE INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
                 VALUES (?1, ?2, ?3, 'U', 't', ?4, 'h')",
                params![repo, *number as i64, pr.head_oid, detail.to_string()],
            )?;
        }
        Ok::<(), CpdError>(())
    })
}

/// A lookup of `workflows` for the viewer `me`.
#[must_use]
pub fn lookup_of(me: &str, workflows: BTreeMap<(String, u64), Option<GhPr>>) -> Lookup {
    Lookup {
        me: me.to_owned(),
        workflows,
    }
}
