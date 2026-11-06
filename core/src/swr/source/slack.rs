//! The `slack` source (docs/architecture/swr-slack.md §3, §6): `fetch` runs the helper `pi/lib/slack-channel.ts` through the
//! Runner port with the stdin of §3.2, stores its `posts` in `swr_slack_posts` and confirms the User's own posts (E11's rule).
//! Time is passed in; the only state is the in-memory fresh-until of the channel query (D5).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::clock::timestamp;
use crate::error::{CpdError, ErrorCode};
use crate::host::deadline::Failed;
use crate::ids::{OwnerLogin, RepoName};
use crate::policy::{PolicyKey, resolve};
use crate::pr::jira_sync;
use crate::swr::outbox::{self, Coalesce, Mutation, Op};
use crate::swr::{cache, endpoint};

/// The helper's deadline: above its 45 s channel read and its thread reads of 30 s, eight at a time.
const DEADLINE: Duration = Duration::from_secs(120);

/// A failed read is retried this long after it (the query row keeps its old `stale_after`, so memory holds the retry time).
const RETRY: Duration = Duration::from_secs(60);

/// Until when each channel query is fresh, in this process only (D5): after a restart every query is stale once.
static FRESH_UNTIL: Mutex<Option<HashMap<String, OffsetDateTime>>> = Mutex::new(None);

fn set_fresh(key: &str, until: OffsetDateTime) {
    FRESH_UNTIL
        .lock()
        .expect("the fresh map lock")
        .get_or_insert_with(HashMap::new)
        .insert(key.to_owned(), until);
}

pub struct Slack;

/// The query key of the team channel.
pub fn channel_key(channel: &str) -> String {
    format!("slack:channel:{channel}")
}

impl Source for Slack {
    fn system(&self) -> &'static str {
        "slack"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["slack"]
    }
    fn due(&self, cx: &mut Cx<'_>, now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError> {
        let channel = &cx.config.inbox.team_channel;
        if channel.is_empty() {
            return Ok(Vec::new());
        }
        let key = channel_key(channel);
        let invalidated = cache::get(cx.store, &key)?.is_some_and(|q| q.invalidated);
        let until = FRESH_UNTIL
            .lock()
            .expect("the fresh map lock")
            .as_ref()
            .and_then(|m| m.get(&key).copied());
        Ok(if invalidated || until.is_none_or(|u| now >= u) {
            vec![key]
        } else {
            Vec::new()
        })
    }

    fn fetch(
        &self,
        cx: &mut Cx<'_>,
        keys: &[QueryKey],
        poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError> {
        let channel = cx.config.inbox.team_channel.clone();
        let key = channel_key(&channel);
        if channel.is_empty() || !keys.contains(&key) {
            return Ok(());
        }
        let now = cx.ports.clock.utc();
        let period = Duration::from_secs(u64::from(cx.config.watch.team_review_seconds));
        match read(cx, &channel) {
            Ok(answer) => {
                let posts = answer["posts"].as_array().cloned().ok_or_else(|| {
                    CpdError::new(
                        ErrorCode::SlackHelperFailed,
                        "The team review helper answered ok with no `posts`.",
                    )
                });
                let posts = match posts {
                    Ok(p) => p,
                    Err(e) => return failed(cx, &key, now, e),
                };
                set_fresh(&key, now + period);
                let hash = hash_of(&posts);
                cache::commit(cx.store, &key, &hash, now, poll_started_at, |tx| {
                    tx.execute(
                        "UPDATE swr_slack_posts SET inactive_at = ?2 WHERE channel = ?1 AND inactive_at IS NULL",
                        params![channel, timestamp(now)],
                    )?;
                    for p in &posts {
                        upsert(tx, &channel, p, now)?;
                    }
                    Ok(())
                })?;
                if endpoint::get(cx.store, "slack")?
                    .is_some_and(|e| e.status.as_deref() == Some("error"))
                {
                    endpoint::record_ok(cx.store, "slack", now)?;
                }
                confirm_own_posts(cx, &channel, &posts, now)?;
                Ok(())
            }
            Err(e) => failed(cx, &key, now, e),
        }
    }

    fn execute(&self, _cx: &mut Cx<'_>, _m: &Mutation) -> ExecOutcome {
        // Reactions, posts and reposts are the outbox Job's (swr-slack.md §4); nothing enqueues them yet.
        ExecOutcome::Failed {
            error: "not implemented".into(),
        }
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        Coalesce::LatestWins
    }
}

/// A failed read: the query and the endpoint record it, no row is written or deleted (swr.md §16).
fn failed(cx: &mut Cx<'_>, key: &str, now: OffsetDateTime, e: CpdError) -> Result<(), CpdError> {
    set_fresh(key, now + RETRY);
    let text = format!("{}: {}", e.code().as_str(), e.message());
    cache::fail(cx.store, key, now, &text)?;
    let kind = match e.code() {
        ErrorCode::SlackAuth
        | ErrorCode::SlackNotConfigured
        | ErrorCode::SlackHelperFailed
        | ErrorCode::PiNotFound
        | ErrorCode::SpawnFailed => endpoint::Kind::Permanent,
        _ => endpoint::Kind::Transient,
    };
    endpoint::record_failure(cx.store, "slack", now, &text, kind)
}

/// The stdin of §3.2: the stored state of every active row and the terminal pull requests the rows name.
fn stdin(cx: &mut Cx<'_>, channel: &str) -> Result<Value, CpdError> {
    cx.store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT ts, repo, number, author_is_me, reply_count, latest_reply, reactions_json, eyes_me, thread_tags_me
               FROM swr_slack_posts WHERE channel = ?1 AND inactive_at IS NULL ORDER BY ts DESC, repo, number",
        )?;
        let known = stmt
            .query_map([channel], |r| {
                let reactions: String = r.get(6)?;
                Ok(json!({
                    "ts": r.get::<_, String>(0)?, "repo": r.get::<_, String>(1)?, "number": r.get::<_, i64>(2)?,
                    "author_is_me": r.get::<_, i64>(3)? == 1,
                    "reply_count": r.get::<_, Option<i64>>(4)?, "latest_reply": r.get::<_, Option<String>>(5)?,
                    "reactions": serde_json::from_str::<Value>(&reactions).unwrap_or(Value::Null),
                    "eyes_me": r.get::<_, Option<i64>>(7)?.map(|v| v == 1),
                    "thread_tags_me": r.get::<_, Option<i64>>(8)?.map(|v| v == 1),
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt = c.prepare(
            "SELECT DISTINCT s.repo || '#' || s.number FROM swr_slack_posts s
               JOIN pull_requests p ON p.repo = s.repo AND p.number = s.number
              WHERE s.channel = ?1 AND s.inactive_at IS NULL AND p.state IN ('merged', 'closed')
              ORDER BY 1",
        )?;
        let terminal = stmt
            .query_map([channel], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"version": 1, "known": known, "terminal": terminal}))
    })
}

/// Runs the helper and returns its `ok` answer; a helper that failed is the error it printed.
fn read(cx: &mut Cx<'_>, channel: &str) -> Result<Value, CpdError> {
    let input = stdin(cx, channel)?;
    let call = crate::slack::call(
        cx.config,
        "slack-channel.ts",
        &[("channel", channel), ("me", &cx.config.inbox.slack_user_id)],
        Some(input.to_string().into_bytes()),
        DEADLINE,
    )?;
    let out = cx.ports.runner.run(&call).map_err(|f| match f {
        Failed::Io(e) => CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot run {}: {e}.", call.args.join(" ")),
        ),
        Failed::Timeout(e) => e,
    })?;
    crate::slack::answer(&out, &call.args)
}

/// The data hash (§3.5): the canonical rows, newest first, without the helper's `read` count.
fn hash_of(posts: &[Value]) -> String {
    let mut rows: Vec<&Value> = posts.iter().collect();
    rows.sort_by(|a, b| {
        b["ts_raw"]
            .as_str()
            .cmp(&a["ts_raw"].as_str())
            .then_with(|| a["repo"].as_str().cmp(&b["repo"].as_str()))
            .then_with(|| a["number"].as_i64().cmp(&b["number"].as_i64()))
    });
    // `serde_json` maps are sorted by key: the text is canonical.
    let text = Value::Array(rows.into_iter().cloned().collect()).to_string();
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn upsert(
    tx: &crate::store::WriteTxn<'_>,
    channel: &str,
    p: &Value,
    now: OffsetDateTime,
) -> Result<(), CpdError> {
    let bad = |what: &str| {
        CpdError::new(
            ErrorCode::SlackHelperFailed,
            format!("A post of the helper's answer has no {what}."),
        )
    };
    let ts = p["ts_raw"].as_str().ok_or_else(|| bad("ts_raw"))?;
    let repo = p["repo"].as_str().ok_or_else(|| bad("repo"))?;
    let number = p["number"].as_i64().ok_or_else(|| bad("number"))?;
    let flag = |v: &Value| v.as_bool().map(i64::from);
    let claim = (!p["claim"].is_null()).then(|| p["claim"].to_string());
    let reactions = if p["reactions"].is_object() {
        p["reactions"].to_string()
    } else {
        "{}".to_owned()
    };
    tx.execute(
        "INSERT INTO swr_slack_posts (channel, ts, repo, number, primary_pr, author_id, author, author_is_me, reply_count,
           latest_reply, reactions_json, claim_json, eyes_me, thread_tags_me, fetched_at, inactive_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL)
         ON CONFLICT (channel, ts, repo, number) DO UPDATE SET primary_pr = excluded.primary_pr, author_id = excluded.author_id,
           author = excluded.author, author_is_me = excluded.author_is_me, reply_count = excluded.reply_count,
           latest_reply = excluded.latest_reply, reactions_json = excluded.reactions_json, claim_json = excluded.claim_json,
           eyes_me = excluded.eyes_me, thread_tags_me = excluded.thread_tags_me, fetched_at = excluded.fetched_at,
           inactive_at = NULL",
        params![
            channel,
            ts,
            repo,
            number,
            i64::from(p["primary"].as_bool().unwrap_or(false)),
            p["author_id"].as_str(),
            p["author"].as_str(),
            i64::from(p["author_is_me"].as_bool().unwrap_or(false)),
            p["reply_count"].as_i64(),
            p["latest_reply"].as_str(),
            reactions,
            claim,
            flag(&p["eyes_me"]),
            flag(&p["thread_tags_me"]),
            timestamp(now),
        ],
    )?;
    Ok(())
}

/// The workspace host of a permalink: that of the newest existing `pr_posts` link, else `app.slack.com`.
fn link_host(conn: &rusqlite::Connection) -> Result<String, CpdError> {
    let link: Option<String> = conn
        .query_row(
            "SELECT link FROM pr_posts ORDER BY posted_at DESC, plan, workflow LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(link
        .as_deref()
        .and_then(|l| l.strip_prefix("https://"))
        .and_then(|l| l.split('/').next())
        .filter(|h| h.ends_with(".slack.com"))
        .unwrap_or("app.slack.com")
        .to_owned())
}

/// Confirms the User's own post of a pull request (swr-slack.md §6): for each open Workflow with an open pull request, no
/// `pr_posts` row and `pr.post` enabled, the earliest message of the User that carries the pull request and is not older
/// than its `created_at` (D1) is its post. The row holds the message's time (D2) and the policy's merged reaction (D3);
/// a confirmed post also queues the `auto_merge` mutation (§6.2). Nothing is queued for a post that was never sent.
fn confirm_own_posts(
    cx: &mut Cx<'_>,
    channel: &str,
    posts: &[Value],
    now: OffsetDateTime,
) -> Result<(), CpdError> {
    let me = cx.config.inbox.slack_user_id.clone();
    if me.is_empty() {
        return Ok(());
    }
    let open = cx.store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT w.plan, w.key, q.repo, q.number, q.created_at FROM workflows w JOIN pull_requests q ON q.id = w.pull_request
              WHERE w.status = 'open' AND q.state = 'open'
                AND NOT EXISTS (SELECT 1 FROM pr_posts p WHERE p.plan = w.plan AND p.workflow = w.key)
              ORDER BY w.plan, w.key",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    for (plan, key, repo, number, created_at) in open {
        let Ok(created) = OffsetDateTime::parse(&created_at, &Rfc3339) else {
            continue;
        };
        let earliest = posts
            .iter()
            .filter(|p| {
                p["author_id"].as_str() == Some(me.as_str())
                    && p["repo"]
                        .as_str()
                        .is_some_and(|r| r.eq_ignore_ascii_case(&repo))
                    && p["number"].as_i64() == Some(number)
            })
            .filter_map(|p| {
                let raw = p["ts_raw"].as_str()?;
                let at = OffsetDateTime::from_unix_timestamp(raw.split('.').next()?.parse().ok()?)
                    .ok()?;
                (at >= created).then_some((p["ts"].as_f64()?, raw, at))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, ts_raw, at)) = earliest else {
            continue;
        };
        let Some((owner, name)) = repo.split_once('/') else {
            continue;
        };
        let (Ok(owner), Ok(name)) = (OwnerLogin::parse(owner), RepoName::parse(name)) else {
            continue;
        };
        // A policy that does not resolve confirms nothing; that is reported where the policy is used.
        let Ok(resolved) = resolve(cx.home, &PolicyKey::Repo(owner, name)) else {
            continue;
        };
        let post = &resolved.policy.pr.post;
        if !post.enabled {
            continue;
        }
        let reaction = (!post.merged_reaction.is_empty()).then(|| post.merged_reaction.clone());
        let posted = timestamp(at);
        let new = cx.store.write("swr slack confirm", |tx| {
            let host = link_host(tx)?;
            let rows = tx.execute(
                "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, reaction, auto_merge)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, 'pending') ON CONFLICT (plan, workflow) DO NOTHING",
                params![
                    plan,
                    key,
                    channel,
                    ts_raw,
                    format!("https://{host}/archives/{channel}/p{}", ts_raw.replace('.', "")),
                    posted,
                    reaction,
                ],
            )?;
            if rows == 1 {
                jira_sync::queue(tx, &plan, &key, jira_sync::Target::InReview)?;
            }
            Ok(rows == 1)
        })?;
        if new {
            let vars = json!({"number": number, "plan": plan, "repo": repo, "workflow": key});
            outbox::enqueue(
                cx.store,
                now,
                Op {
                    system: "github".into(),
                    target_key: format!("github:pr:{repo}#{number}"),
                    op_kind: "auto_merge".into(),
                    vars: vars.clone(),
                    patch: None,
                    context: None,
                },
                Coalesce::Dedupe(vars.to_string()),
            )?;
        }
    }
    Ok(())
}
