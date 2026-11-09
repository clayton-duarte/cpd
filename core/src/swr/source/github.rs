//! The `github` source: the live GitHub SWR source (swr.md §14).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::{Cx, ExecOutcome, QueryKey, Source};
use crate::clock::timestamp;
use crate::error::CpdError;
use crate::pr::swr_read::{self, PollNode, PollPlan, PollRead};
use crate::swr::outbox::{Coalesce, Mutation};
use crate::swr::{cache, endpoint};

/// GitHub's rate-limit floor: below it the endpoint is paused until the limit resets (was `RATE_FLOOR` of the engine).
const RATE_FLOOR: i64 = 100;
/// The most `Heavy` selections one document carries (swr-github.md §3.2; the page's guess, unverified).
const HEAVY_CAP: usize = 25;
/// Repository metadata and the viewer are read again after this long.
const META_TTL: Duration = Duration::from_secs(24 * 3600);
/// A Slack-posted pull request is first-seen only inside this window (swr-github.md §4.2 d).
const SLACK_WINDOW: i64 = 7 * 24 * 3600;
/// How long a hard `RATE_LIMITED` answer pauses the endpoint when GitHub gave no reset.
const RATE_LIMITED_PAUSE: Duration = Duration::from_secs(15 * 60);
/// The wait added to a hard `RATE_LIMITED` reply's stored reset, so the first poll at the reset instant is not refused
/// again (the engine's `RATE_GRACE`, pro-d392).
const RATE_GRACE: Duration = Duration::from_secs(3);

/// E5's poll result when the pass ran nothing (the endpoint is paused): no effects, and the rate limit the endpoint
/// still holds, so the engine's rate guard reads a usable value and raises nothing.
fn no_effects(store: &mut crate::store::Store, _now: OffsetDateTime) -> Result<Value, CpdError> {
    let row = endpoint::get(store, "github")?;
    let remaining = row.as_ref().and_then(|r| r.rate_remaining).unwrap_or(5000);
    let reset_at = row
        .and_then(|r| r.rate_reset_at)
        .and_then(|at| {
            at.format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_else(|| "2099-01-01T00:00:00Z".to_owned());
    Ok(serde_json::json!({
        "polled": 0,
        "changed": [],
        "base_changed": [],
        "feedback": [],
        "update_jobs": [],
        "merged": [],
        "done": [],
        "rate_limit": {"cost": 0, "remaining": remaining, "reset_at": reset_at},
    }))
}

/// What the previous pass's searches listed, per store: engine memory (D5), so after a restart the first pass confirms nobody
/// and the next one confirms every flagged row the first one did not list.
type Listed = HashMap<String, BTreeSet<(String, i64)>>;
static LISTED: Mutex<Option<Listed>> = Mutex::new(None);

/// The real `github` source (swr-github.md §4).
#[derive(Default)]
pub struct GithubSource {
    /// The policy's `pr.excused_labels` (pro-d447): a PR carrying one has its failing `danger` check excused.
    pub excused_labels: Vec<String>,
}

/// One stored open pull request, as the plan reads it.
struct Stored {
    repo: String,
    number: i64,
    tracked: bool,
    hydrated: bool,
    /// The stored rollup is failed and a label is excused: the contexts are needed to excuse a Danger (pro-d447).
    held_failure: bool,
    hydrated_at: Option<OffsetDateTime>,
}

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn pr_key(repo: &str, number: i64) -> String {
    format!("github:pr:{repo}#{number}")
}

impl GithubSource {
    /// The rows chosen before the call (swr-github.md §4.1, §3.2).
    fn plan(&self, cx: &mut Cx<'_>, now: OffsetDateTime) -> Result<PollPlan, CpdError> {
        let memory = {
            let guard = LISTED.lock().expect("listed memory");
            guard
                .as_ref()
                .and_then(|m| m.get(&cx.store.path().display().to_string()))
                .cloned()
        };
        let stored: Vec<Stored> = cx.store.read(|c| {
            let mut stmt = c.prepare(
                "SELECT pr.repo, pr.number,
                        pr.authored = 1 OR EXISTS (SELECT 1 FROM workflows w WHERE w.pull_request = pr.id AND w.status = 'open'),
                        pr.review_requested = 1,
                        d.hydrated_at, d.head_oid IS pr.head_oid AND d.updated_at IS pr.gh_updated_at, d.repo IS NOT NULL,
                        pr.checks = 'failed', pr.labels_json
                 FROM pull_requests pr LEFT JOIN swr_pr_detail d ON d.repo = pr.repo AND d.number = pr.number
                 WHERE pr.state = 'open' AND pr.archived = 0 ORDER BY pr.id",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    let (own, review): (bool, bool) = (r.get(2)?, r.get(3)?);
                    let (same, has): (bool, bool) = (r.get(5)?, r.get(6)?);
                    let (failed, labels): (bool, Option<String>) = (r.get(7)?, r.get(8)?);
                    let labels: Vec<String> = labels
                        .and_then(|l| serde_json::from_str(&l).ok())
                        .unwrap_or_default();
                    Ok((
                        Stored {
                            repo: r.get(0)?,
                            number: r.get(1)?,
                            tracked: own || review,
                            hydrated: has && same,
                            held_failure: failed
                                && swr_read::policy_hold(&labels, &self.excused_labels),
                            hydrated_at: cache::parse(r.get(4)?).ok().flatten(),
                        },
                        own,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows
                .into_iter()
                .map(|(mut s, own)| {
                    // Only the User's own pull requests (authored or owned) are hydrated, and a teammate's whose failed rollup
                    // may hide an excused Danger (read again only when its head moved).
                    if !own && !s.held_failure {
                        s.hydrated = true;
                    }
                    s
                })
                .collect())
        })?;
        let mut heavy: Vec<&Stored> = stored.iter().filter(|s| !s.hydrated).collect();
        heavy.sort_by_key(|s| s.hydrated_at);
        heavy.truncate(HEAVY_CAP);
        let heavy: Vec<(String, i64)> = heavy.iter().map(|s| (s.repo.clone(), s.number)).collect();
        let mut absent: Vec<(String, i64)> = Vec::new();
        if let Some(prev) = &memory {
            for s in stored.iter().filter(|s| s.tracked) {
                let key = (s.repo.clone(), s.number);
                if !prev.contains(&key) && !heavy.contains(&key) {
                    absent.push(key);
                }
            }
        }
        let floor = now.unix_timestamp() - SLACK_WINDOW;
        // Every active Slack-posted pull request not yet merged or closed is read each pass (pro-d453): one with no row is
        // first-seen, one whose row is open and not archived stays live (a teammate's PR is neither authored nor requested).
        let first_seen: Vec<(String, i64)> = cx.store.read(|c| {
            let mut stmt = c.prepare(
                "SELECT s.repo, s.number, s.ts FROM swr_slack_posts s
                 WHERE s.repo IS NOT NULL AND s.number IS NOT NULL AND s.inactive_at IS NULL
                   AND (NOT EXISTS (SELECT 1 FROM pull_requests p WHERE p.repo = s.repo AND p.number = s.number)
                        OR EXISTS (SELECT 1 FROM pull_requests p
                                   WHERE p.repo = s.repo AND p.number = s.number AND p.state = 'open' AND p.archived = 0))",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows
                .into_iter()
                .filter(|(_, _, ts)| {
                    ts.split('.')
                        .next()
                        .and_then(|s| s.parse::<i64>().ok())
                        .is_some_and(|t| t >= floor)
                })
                .map(|(repo, n, _)| (repo, n))
                .collect())
        })?;
        for key in first_seen {
            if !absent.contains(&key) && !heavy.contains(&key) {
                absent.push(key);
            }
        }
        let repos: BTreeSet<&str> = stored
            .iter()
            .map(|s| s.repo.as_str())
            .chain(heavy.iter().chain(&absent).map(|(r, _)| r.as_str()))
            .collect();
        let fresh: BTreeMap<String, Option<OffsetDateTime>> = cx.store.read(|c| {
            let mut stmt = c.prepare("SELECT repo, fetched_at FROM swr_repos")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        cache::parse(r.get(1)?).ok().flatten(),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows.into_iter().collect())
        })?;
        let meta = repos
            .into_iter()
            .filter(|r| {
                !fresh
                    .get(*r)
                    .copied()
                    .flatten()
                    .is_some_and(|at| now < at + META_TTL)
            })
            .map(str::to_owned)
            .collect();
        Ok(PollPlan {
            heavy,
            absent,
            meta,
        })
    }

    /// One engine pass: plan, one `gh api graphql` call through the Runner, commit key by key (swr-github.md §4).
    pub fn pass(&self, cx: &mut Cx<'_>, now: OffsetDateTime) -> Result<(), CpdError> {
        self.pass_with_effects(cx, now).map(|_| ())
    }

    /// [`GithubSource::pass`], returning E5's poll result: the legacy effects (the counts and the rate limit) from that
    /// single read. The engine's E5 polls through this (D9's target); the trait's `fetch` keeps `pass`.
    pub fn pass_with_effects(
        &self,
        cx: &mut Cx<'_>,
        now: OffsetDateTime,
    ) -> Result<Value, CpdError> {
        if endpoint::get(cx.store, "github")?.is_some_and(|row| endpoint::is_paused(&row, now)) {
            return no_effects(cx.store, now);
        }
        let plan = self.plan(cx, now)?;
        let poll_started_at = now;
        let read = swr_read::poll_once(
            &*cx.ports.runner,
            cx.home.root(),
            &plan,
            &cx.config.inbox.bot_reviewers,
            &self.excused_labels,
        );
        let read = match read {
            Ok(read) => read,
            Err(e) => return self.failed(cx, now, e),
        };
        let reset = OffsetDateTime::parse(
            &read.reset_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|e| {
            CpdError::new(
                crate::error::ErrorCode::GhFailed,
                format!("GitHub's rateLimit.resetAt is not a time: {e}."),
            )
        })?;
        endpoint::record_rate(cx.store, "github", read.remaining, reset)?;
        if read.remaining < RATE_FLOOR {
            endpoint::pause_until(cx.store, "github", reset)?;
        }
        endpoint::record_ok(cx.store, "github", now)?;
        self.commit(cx, &plan, read, now, poll_started_at)
    }

    /// A failed call is the endpoint's failure; the cached rows stay (swr.md §6). A hard rate limit pauses the endpoint
    /// until its stored reset plus the grace (or [`RATE_LIMITED_PAUSE`] when none is known) and is returned to E5, which
    /// pauses its own poll; it is not an error.
    fn failed(&self, cx: &mut Cx<'_>, now: OffsetDateTime, e: CpdError) -> Result<Value, CpdError> {
        let flag = |name: &str| {
            e.details()
                .get(name)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        if flag("rate_limited") {
            let until = endpoint::get(cx.store, "github")?
                .and_then(|row| row.rate_reset_at)
                .map(|reset| reset + RATE_GRACE)
                .unwrap_or(now + RATE_LIMITED_PAUSE);
            endpoint::pause_until(cx.store, "github", until)?;
            return Err(e);
        }
        let kind = if flag("transient") {
            endpoint::Kind::Transient
        } else {
            endpoint::Kind::Permanent
        };
        endpoint::record_failure(cx.store, "github", now, e.message(), kind)?;
        Err(e)
    }

    fn commit(
        &self,
        cx: &mut Cx<'_>,
        plan: &PollPlan,
        read: PollRead,
        now: OffsetDateTime,
        started: OffsetDateTime,
    ) -> Result<Value, CpdError> {
        // E5's poll IS this pass (D9's target): the legacy effects (the pr-feedback/pr-merged/pr-closed Events, the
        // Workflow merges and unstack, the counts) are applied from this same read, before the header writes below, so
        // the write phase sees the pre-pass rows and the transitions. Only when an open Workflow points at a pull
        // request; nothing else is affected.
        let effects = if crate::pr::poll::any_owned(cx.store)? {
            crate::pr::poll::apply_pass_effects(cx.ports, cx.home, &read)?
        } else {
            no_effects(cx.store, now)?
        };
        // The viewer. `cache::commit` rejects a key with one colon (`system()` wants `<system>:<kind>:<id>`), and the key is
        // `github:viewer` (swr.md §4), so its row is written here, with the same rule: unchanged data opens no write.
        let login = read.viewer.clone();
        let hash = sha(&login);
        let known = cache::get(cx.store, "github:viewer")?;
        if known.as_ref().and_then(|r| r.data_hash.as_deref()) != Some(hash.as_str()) {
            cx.store.write("swr viewer", |tx| {
                tx.execute(
                    "INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, ?1, ?2)
                     ON CONFLICT (id) DO UPDATE SET login = excluded.login, fetched_at = excluded.fetched_at",
                    params![login, timestamp(now)],
                )?;
                tx.execute(
                    "INSERT INTO swr_queries (key, system, status, data_hash, fetched_at, stale_after)
                     VALUES ('github:viewer', 'github', 'success', ?1, ?2, ?3)
                     ON CONFLICT (key) DO UPDATE SET status = 'success', data_hash = excluded.data_hash,
                       fetched_at = excluded.fetched_at, stale_after = excluded.stale_after, last_error = NULL",
                    params![hash, timestamp(now), timestamp(now + META_TTL)],
                )?;
                Ok::<(), CpdError>(())
            })?;
        }
        // The repositories.
        for (repo, meta) in plan.meta.iter().zip(&read.metas) {
            let Some(meta) = meta else { continue };
            let key = format!("github:repo:{repo}");
            let hash = sha(&format!("{meta:?}"));
            let changed = cache::commit(cx.store, &key, &hash, now, started, |tx| {
                super::github_write::write_repo_settings(tx, repo, meta, now)?;
                if meta.is_archived {
                    tx.execute(
                        "UPDATE pull_requests SET archived = 1 WHERE repo = ?1",
                        [repo],
                    )?;
                }
                Ok(())
            })?;
            if changed == cache::Committed::Unchanged {
                // Unchanged metadata still dates its read, once a day: the only write an unchanged repository makes.
                cx.store.write("swr repo fetched", |tx| {
                    tx.execute(
                        "UPDATE swr_repos SET fetched_at = ?2 WHERE repo = ?1",
                        params![repo, timestamp(now)],
                    )?;
                    Ok::<(), CpdError>(())
                })?;
            }
        }
        // The pull requests: the searches, then the aliases (which carry heavy fields and the viewer's flags).
        let mut merged: BTreeMap<(String, i64), PollNode> = BTreeMap::new();
        let mut listed: BTreeSet<(String, i64)> = BTreeSet::new();
        for (nodes, authored) in [(&read.authored, true), (&read.reviews, false)] {
            for node in nodes {
                let id = node.id();
                listed.insert(id.clone());
                let entry = merged.entry(id).or_insert_with(|| node.clone());
                entry.flag(authored, !authored);
                entry.heavy = entry.heavy.take().or_else(|| node.heavy.clone());
            }
        }
        let asked = plan.heavy.iter().chain(&plan.absent);
        let mut not_found: Vec<(String, i64)> = Vec::new();
        for (id, node) in asked.zip(read.aliases) {
            let Some(node) = node else {
                if !merged.contains_key(id) {
                    not_found.push(id.clone());
                }
                continue;
            };
            match merged.get_mut(id) {
                Some(entry) => {
                    let found = entry.flags();
                    let heavy = node.heavy.clone().or_else(|| entry.heavy.take());
                    *entry = node;
                    entry.heavy = heavy;
                    if entry.is_open() {
                        entry.flag(found.0, found.1);
                    }
                }
                None => {
                    merged.insert(id.clone(), node);
                }
            }
        }
        for (repo, number) in &not_found {
            cache::fail(
                cx.store,
                &pr_key(repo, *number),
                now,
                "GitHub cannot find the pull request",
            )?;
        }
        for ((repo, number), node) in &mut merged {
            let key = pr_key(repo, *number);
            // The stored detail of the same head, for the excused Danger a Light node cannot see.
            let stored: Option<serde_json::Value> = cx.store.read(|c| {
                Ok(c.query_row(
                    "SELECT detail_json FROM swr_pr_detail WHERE repo = ?1 AND number = ?2 AND head_oid = ?3",
                    params![repo, number, node.head_oid],
                    |r| r.get::<_, String>(0),
                )
                .ok()
                .and_then(|j| serde_json::from_str(&j).ok()))
            })?;
            if let Some(detail) = &stored {
                swr_read::excuse_from_detail(node, detail, &self.excused_labels);
            }
            let node = &*node;
            let stored_hash: Option<String> = cx.store.read(|c| {
                Ok(c.query_row(
                    "SELECT detail_hash FROM swr_pr_detail WHERE repo = ?1 AND number = ?2",
                    params![repo, number],
                    |r| r.get(0),
                )
                .ok())
            })?;
            let detail = node.heavy.as_ref().map(|h| h.to_string());
            let detail_hash = detail
                .as_deref()
                .map(sha)
                .or(stored_hash)
                .unwrap_or_default();
            let hash = sha(&format!("{}|{detail_hash}", node.fingerprint()));
            cache::commit(cx.store, &key, &hash, now, started, |tx| {
                swr_read::write_header(tx, node)?;
                if let Some(detail) = &detail {
                    tx.execute(
                        "INSERT INTO swr_pr_detail (repo, number, head_oid, updated_at, hydrated_at, detail_json, detail_hash)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT (repo, number) DO UPDATE SET head_oid = excluded.head_oid,
                           updated_at = excluded.updated_at, hydrated_at = excluded.hydrated_at,
                           detail_json = excluded.detail_json, detail_hash = excluded.detail_hash",
                        params![
                            repo,
                            number,
                            node.head_oid,
                            node.updated_at(),
                            timestamp(now),
                            detail,
                            detail_hash
                        ],
                    )?;
                }
                Ok(())
            })?;
            // Open-only: a merged or closed pull request is never stale, so nothing reads it again.
            if !node.is_open()
                && cache::get(cx.store, &key)?.is_some_and(|r| r.stale_after.is_some())
            {
                cx.store.write("swr terminal", |tx| {
                    tx.execute(
                        "UPDATE swr_queries SET stale_after = NULL WHERE key = ?1",
                        [&key],
                    )?;
                    Ok::<(), CpdError>(())
                })?;
            }
        }
        LISTED
            .lock()
            .expect("listed memory")
            .get_or_insert_with(HashMap::new)
            .insert(cx.store.path().display().to_string(), listed);
        // pro-d469: every pass fills `swr_deploys`, so E8 opens deploy items without a manual `cpd pr poll`.
        crate::pr::deploys::read_into_store(&*cx.ports.runner, cx.store, cx.home, now)?;
        Ok(effects)
    }
}

impl Source for GithubSource {
    fn system(&self) -> &'static str {
        "github"
    }
    fn endpoints(&self) -> &'static [&'static str] {
        &["github"]
    }
    /// One pseudo-key for the whole pass: the document is one call, so it is due or not as a whole.
    fn due(&self, cx: &mut Cx<'_>, now: OffsetDateTime) -> Result<Vec<QueryKey>, CpdError> {
        let paused =
            endpoint::get(cx.store, "github")?.is_some_and(|r| endpoint::is_paused(&r, now));
        Ok(if paused {
            Vec::new()
        } else {
            vec!["github:pass:all".to_owned()]
        })
    }
    fn fetch(
        &self,
        cx: &mut Cx<'_>,
        _keys: &[QueryKey],
        poll_started_at: OffsetDateTime,
    ) -> Result<(), CpdError> {
        self.pass(cx, poll_started_at).map(|_| ())
    }
    fn execute(&self, cx: &mut Cx<'_>, m: &Mutation) -> ExecOutcome {
        self.run_one(cx, m)
    }
    fn coalesce(&self, op_kind: &str) -> Coalesce {
        super::github_write::coalesce_of(op_kind)
    }
}
