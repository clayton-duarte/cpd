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
use crate::events::{NewEvent, append};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::ids::{OwnerLogin, PlanId, RepoName, WorkflowKey};
use crate::policy::{PolicyKey, policy_dir, repo_path, resolve};
use crate::pr::jira_sync;
use crate::pr::posts::{self, PostRecord};
use crate::swr::outbox::{self, Coalesce, Mutation, Op};
use crate::swr::{cache, endpoint};

/// The helper's deadline: above its 45 s channel read and its thread reads of 30 s, eight at a time.
const DEADLINE: Duration = Duration::from_secs(120);

/// A failed read is retried this long after it (the query row keeps its old `stale_after`, so memory holds the retry time).
const RETRY: Duration = Duration::from_secs(60);

/// A reaction that failed for any reason but a refusal is tried again this long after (swr-slack.md §4.3).
const REACTION_RETRY: Duration = Duration::from_secs(600);

/// The deadline of a send or a reaction: above the helper's own 30 s call.
const SEND_DEADLINE: Duration = Duration::from_secs(60);

/// The reminder's text of `pr repost`.
pub const REPOST_TEXT: &str = "Bibbidi-bobbidi-bump!";

/// The statuses of a mutation the overlay shows.
const ACTIVE: &str = "('pending', 'in_flight', 'revalidating')";

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
        requeue_parked(cx, now)?;
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

    fn execute(&self, cx: &mut Cx<'_>, m: &Mutation) -> ExecOutcome {
        let vars: Value = match serde_json::from_str(&m.vars_json) {
            Ok(v) => v,
            Err(e) => {
                return ExecOutcome::Failed {
                    error: format!("vars_json is not JSON: {e}"),
                };
            }
        };
        match m.op_kind.as_str() {
            "reaction" => execute_reaction(cx, &vars),
            "post" => execute_post(cx, &vars),
            "repost" => execute_repost(cx, &vars),
            other => ExecOutcome::Failed {
                error: format!("unknown slack op `{other}`"),
            },
        }
    }
    fn coalesce(&self, _op_kind: &str) -> Coalesce {
        Coalesce::LatestWins
    }
}

/// What an enqueueing Slack write returns (swr-slack.md §4.2, A6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub queued: bool,
    pub mutation_id: i64,
}

/// A message's reactions as the User sees them: the stored counts with the active `reaction` mutations laid over (§4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlaid {
    pub reactions: std::collections::BTreeMap<String, i64>,
    pub eyes_me: Option<bool>,
}

/// This transaction's time, the `submitted_at` of a write made inside a caller's transaction.
fn txn_at(tx: &crate::store::WriteTxn<'_>) -> Result<String, CpdError> {
    Ok(tx.query_row("SELECT at FROM txns WHERE id = cpd_txn()", [], |r| r.get(0))?)
}

/// Inserts a `pending` mutation of `op_kind` on the channel query; with `dedupe` an active one with the same `vars` is returned
/// instead. `(id, new)`.
fn insert(
    tx: &crate::store::WriteTxn<'_>,
    at: &str,
    op_kind: &str,
    channel: &str,
    vars: &Value,
    dedupe: bool,
) -> Result<(i64, bool), CpdError> {
    let key = channel_key(channel);
    let vars = vars.to_string();
    if dedupe {
        let existing = tx
            .query_row(
                &format!(
                    "SELECT id FROM swr_mutations WHERE system = 'slack' AND target_key = ?1 AND op_kind = ?2 AND vars_json = ?3
                       AND status IN {ACTIVE} ORDER BY id LIMIT 1"
                ),
                params![key, op_kind, vars],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok((id, false));
        }
    }
    tx.execute(
        "INSERT INTO swr_mutations (system, target_key, op_kind, vars_json, status, submitted_at)
         VALUES ('slack', ?1, ?2, ?3, 'pending', ?4)",
        params![key, op_kind, vars, at],
    )?;
    Ok((tx.last_insert_rowid(), true))
}

fn reaction_vars(channel: &str, ts: &str, emoji: &str, merge: Option<(&str, &str)>) -> Value {
    match merge {
        None => json!({"channel": channel, "emoji": emoji, "ts": ts}),
        Some((plan, workflow)) => {
            json!({"channel": channel, "emoji": emoji, "plan": plan, "ts": ts, "workflow": workflow})
        }
    }
}

/// Enqueues the `reaction` op of (`channel`, `ts`, `emoji`) on `slack:channel:<channel>`, deduped on those three (§4.2).
pub fn react(
    store: &mut crate::store::Store,
    now: OffsetDateTime,
    channel: &str,
    ts: &str,
    emoji: &str,
) -> Result<Queued, CpdError> {
    let at = timestamp(now);
    let vars = reaction_vars(channel, ts, emoji, None);
    let (mutation_id, _) = store.write("swr slack react", |tx| {
        insert(tx, &at, "reaction", channel, &vars, true)
    })?;
    Ok(Queued {
        queued: true,
        mutation_id,
    })
}

/// [`react`] inside the caller's transaction: `(id, new)`; `new` is false when an active mutation of the same reaction exists.
pub fn react_in(
    tx: &crate::store::WriteTxn<'_>,
    channel: &str,
    ts: &str,
    emoji: &str,
) -> Result<(i64, bool), CpdError> {
    let at = txn_at(tx)?;
    insert(
        tx,
        &at,
        "reaction",
        channel,
        &reaction_vars(channel, ts, emoji, None),
        true,
    )
}

/// Enqueues the merge reaction of Workflow `workflow` of Plan `plan` on its post when `pr_posts.reaction` is not NULL; whether a
/// mutation was newly queued. The emoji is resolved from the policy again at `execute` (§4.3).
pub fn react_merge_in(
    tx: &crate::store::WriteTxn<'_>,
    plan: &str,
    workflow: &str,
) -> Result<bool, CpdError> {
    let post: Option<(String, String, String)> = tx
        .query_row(
            "SELECT channel, ts, reaction FROM pr_posts WHERE plan = ?1 AND workflow = ?2 AND reaction IS NOT NULL",
            [plan, workflow],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((channel, ts, emoji)) = post else {
        return Ok(false);
    };
    let at = txn_at(tx)?;
    let vars = reaction_vars(&channel, &ts, &emoji, Some((plan, workflow)));
    Ok(insert(tx, &at, "reaction", &channel, &vars, true)?.1)
}

/// Enqueues the `post` op in the caller's transaction: never deduped, `pr post` guards `already_posted` itself (§4.2).
pub fn post_in(
    tx: &crate::store::WriteTxn<'_>,
    channel: &str,
    vars: &Value,
) -> Result<Queued, CpdError> {
    let at = txn_at(tx)?;
    let (mutation_id, _) = insert(tx, &at, "post", channel, vars, false)?;
    Ok(Queued {
        queued: true,
        mutation_id,
    })
}

/// Enqueues the `repost` op in the caller's transaction: never deduped, each is one reminder (§4.2).
pub fn repost_in(
    tx: &crate::store::WriteTxn<'_>,
    channel: &str,
    vars: &Value,
) -> Result<Queued, CpdError> {
    let at = txn_at(tx)?;
    let (mutation_id, _) = insert(tx, &at, "repost", channel, vars, false)?;
    Ok(Queued {
        queued: true,
        mutation_id,
    })
}

/// The id of the active `post` mutation of Workflow `workflow` of Plan `plan`, if any.
pub fn active_post(
    conn: &rusqlite::Connection,
    plan: &str,
    workflow: &str,
) -> Result<Option<i64>, CpdError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT id FROM swr_mutations WHERE system = 'slack' AND op_kind = 'post' AND status IN {ACTIVE}
                   AND json_extract(vars_json, '$.plan') = ?1 AND json_extract(vars_json, '$.workflow') = ?2
                 ORDER BY id LIMIT 1"
            ),
            [plan, workflow],
            |r| r.get(0),
        )
        .optional()?)
}

/// The message (`channel`, `ts`) with the overlay of its active mutations; `None` when the channel holds no such row.
pub fn overlaid(
    store: &mut crate::store::Store,
    channel: &str,
    ts: &str,
) -> Result<Option<Overlaid>, CpdError> {
    let key = channel_key(channel);
    store.read(|c| {
        let row: Option<(String, Option<i64>)> = c
            .query_row(
                "SELECT reactions_json, eyes_me FROM swr_slack_posts WHERE channel = ?1 AND ts = ?2 AND inactive_at IS NULL
                 ORDER BY primary_pr DESC, repo, number LIMIT 1",
                [channel, ts],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((reactions, eyes_me)) = row else {
            return Ok(None);
        };
        let mut counts: std::collections::BTreeMap<String, i64> =
            serde_json::from_str(&reactions).unwrap_or_default();
        let mut eyes_me = eyes_me.map(|v| v == 1);
        let mut stmt = c.prepare(&format!(
            "SELECT json_extract(vars_json, '$.emoji') FROM swr_mutations
              WHERE system = 'slack' AND target_key = ?1 AND op_kind = 'reaction' AND status IN {ACTIVE}
                AND json_extract(vars_json, '$.channel') = ?2 AND json_extract(vars_json, '$.ts') = ?3
              ORDER BY submitted_at, id"
        ))?;
        let emojis = stmt
            .query_map(params![key, channel, ts], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for emoji in emojis {
            if emoji == "eyes" {
                eyes_me = Some(true);
            }
            *counts.entry(emoji).or_insert(0) += 1;
        }
        Ok(Some(Overlaid {
            reactions: counts,
            eyes_me,
        }))
    })
}

/// Whether Workflow `workflow` of Plan `plan` has an active `post` mutation: the overlay shows it as posting (§5.1).
pub fn posting(
    store: &mut crate::store::Store,
    plan: &str,
    workflow: &str,
) -> Result<bool, CpdError> {
    store.read(|c| Ok(active_post(c, plan, workflow)?.is_some()))
}

/// What the Workflow's policy names as its merge reaction (`""` when none) and the policy file that sets, or should set, it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyEmoji {
    pub emoji: String,
    pub file: std::path::PathBuf,
}

/// The emoji Workflow `workflow` of `plan` names for its merge (`pr.post.merged_reaction`, `""` when none), resolved with the
/// Workflow's repo and owner policy, and the file to edit: the repo's file when it sets the key, else the owner's `owner.toml`.
pub fn policy_emoji(
    conn: &rusqlite::Connection,
    home: &CpdHome,
    plan: &str,
    workflow: &str,
) -> Result<PolicyEmoji, CpdError> {
    let plan_id = PlanId::parse(plan)?;
    let key = WorkflowKey::parse(workflow)?;
    let emoji = crate::pr::policy_of_workflow(conn, home, &plan_id, &key)?
        .pr
        .post
        .merged_reaction;
    let (owner, name): (String, Option<String>) = conn.query_row(
        "SELECT c.owner, r.name FROM workflows r JOIN plans p ON p.id = r.plan JOIN contexts c ON c.id = p.context WHERE r.plan = ?1 AND r.key = ?2",
        [plan, workflow],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let owner = OwnerLogin::parse(&owner)?;
    let owner_file = policy_dir(home).join(owner.as_str()).join("owner.toml");
    let file = match name {
        Some(name) => {
            let repo = repo_path(home, &owner, &RepoName::parse(&name)?);
            if repo_sets_reaction(&repo)? {
                repo
            } else {
                owner_file
            }
        }
        None => owner_file,
    };
    Ok(PolicyEmoji { emoji, file })
}

/// Whether the repo policy file exists and sets `pr.post.merged_reaction` (it parsed when the policy resolved).
fn repo_sets_reaction(path: &std::path::Path) -> Result<bool, CpdError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(CpdError::new(
                ErrorCode::Internal,
                format!("cannot read {}: {e}", path.display()),
            ));
        }
    };
    let value: toml::Table = text.parse().map_err(|e| {
        CpdError::new(
            ErrorCode::Internal,
            format!("{} is not TOML: {e}", path.display()),
        )
    })?;
    Ok(value
        .get("pr")
        .and_then(|v| v.get("post"))
        .and_then(|v| v.get("merged_reaction"))
        .is_some())
}

/// Re-enqueues each parked merge reaction whose Workflow's policy now names another, non-empty emoji (§4.3): a new mutation with
/// the new `vars_json`, because a `failed` one never revalidates. The newest parked row of each Workflow is the one looked at.
fn requeue_parked(cx: &mut Cx<'_>, now: OffsetDateTime) -> Result<(), CpdError> {
    let parked: Vec<(String, String, String, String, String)> = cx.store.read(|c| {
        let mut stmt = c.prepare(
            "SELECT json_extract(vars_json, '$.plan'), json_extract(vars_json, '$.workflow'), json_extract(vars_json, '$.channel'),
                    json_extract(vars_json, '$.ts'), json_extract(vars_json, '$.emoji')
               FROM swr_mutations WHERE id IN (SELECT max(id) FROM swr_mutations
                 WHERE system = 'slack' AND op_kind = 'reaction' AND json_extract(vars_json, '$.plan') IS NOT NULL
                 GROUP BY json_extract(vars_json, '$.plan'), json_extract(vars_json, '$.workflow'))
                AND status = 'failed' ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })?;
    for (plan, workflow, channel, ts, emoji) in parked {
        let home = cx.home.clone();
        // A policy that does not resolve is reported where the policy is used; nothing is re-queued for it.
        let Ok(resolved) = cx.store.read(|c| policy_emoji(c, &home, &plan, &workflow)) else {
            continue;
        };
        if resolved.emoji.is_empty() || resolved.emoji == emoji {
            continue;
        }
        let vars = reaction_vars(&channel, &ts, &resolved.emoji, Some((&plan, &workflow)));
        let at = timestamp(now);
        cx.store.write("swr slack requeue", |tx| {
            insert(tx, &at, "reaction", &channel, &vars, true)?;
            Ok(())
        })?;
    }
    Ok(())
}

/// Runs the helper `call` through the Runner and returns its `ok` answer; a helper that failed is the error it printed.
fn run_call(cx: &mut Cx<'_>, call: &crate::host::runner::Call) -> Result<Value, CpdError> {
    let out = cx.ports.runner.run(call).map_err(|f| match f {
        Failed::Io(e) => CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot run {}: {e}.", call.args.join(" ")),
        ),
        Failed::Timeout(e) => e,
    })?;
    crate::slack::answer(&out, &call.args)
}

/// A refusal's Slack error name: the `reason` of its details, else `unknown`.
fn reason_of(e: &CpdError) -> String {
    e.details()["reason"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned()
}

/// Raises the one `error` Event `slack_refused` of a reaction that will not be placed, in its own transaction; scoped to the Plan
/// for a merge reaction, with the policy file to fix.
fn raise_refused(
    cx: &mut Cx<'_>,
    vars: &Value,
    emoji: &str,
    reason: &str,
    file: Option<&std::path::Path>,
) -> Result<(), CpdError> {
    let text_of = |name: &str| vars[name].as_str().unwrap_or_default().to_owned();
    let (channel, ts) = (text_of("channel"), text_of("ts"));
    let mut details = json!({"emoji": emoji, "reason": reason, "channel": channel, "ts": ts});
    let what = if reason == "policy_empty" {
        "The policy names no emoji (pr.post.merged_reaction is empty)".to_owned()
    } else {
        format!("Slack refused the reaction `{emoji}`: {reason}")
    };
    let plan = vars["plan"].as_str().map(str::to_owned);
    let text = match (&plan, file) {
        (Some(plan), Some(file)) => {
            let workflow = text_of("workflow");
            details["plan"] = json!(plan);
            details["pr"] = json!(workflow);
            details["file"] = json!(file.display().to_string());
            format!(
                "The merge reaction of Workflow {workflow} of Plan {plan} on Slack message {ts} of {channel} was not placed. {what}. Set pr.post.merged_reaction in {} to an emoji of the workspace; the engine tries again when the policy names another emoji.",
                file.display()
            )
        }
        _ => format!(
            "The reaction `{emoji}` on Slack message {ts} of {channel} was not placed. {what}. The engine does not try this reaction again."
        ),
    };
    let mut event = NewEvent::error(
        text.clone(),
        "watch",
        ErrorCode::SlackRefused.as_str(),
        &text,
        details,
    );
    if let Some(plan) = &plan {
        event = event.plan(PlanId::parse(plan)?);
    }
    cx.store.write("swr slack refused", |tx| {
        append(tx, event)?;
        Ok(())
    })
}

/// The failure of a Slack call that is not a refusal, recorded on the endpoint (the outage Event is the endpoint's, A3).
fn record_endpoint_failure(cx: &mut Cx<'_>, e: &CpdError, now: OffsetDateTime) {
    let text = format!("{}: {}", e.code().as_str(), e.message());
    let kind = match e.code() {
        ErrorCode::SlackAuth
        | ErrorCode::SlackNotConfigured
        | ErrorCode::SlackHelperFailed
        | ErrorCode::PiNotFound
        | ErrorCode::SpawnFailed => endpoint::Kind::Permanent,
        _ => endpoint::Kind::Transient,
    };
    // A store that cannot take the note leaves the mutation's own retry to report.
    let _ = endpoint::record_failure(cx.store, "slack", now, &text, kind);
}

fn execute_reaction(cx: &mut Cx<'_>, vars: &Value) -> ExecOutcome {
    let text = |name: &str| vars[name].as_str().unwrap_or_default().to_owned();
    let (channel, ts) = (text("channel"), text("ts"));
    let mut emoji = text("emoji");
    if let (Some(plan), Some(workflow)) = (vars["plan"].as_str(), vars["workflow"].as_str()) {
        // A merge reaction's emoji is the policy's at the time it is placed (contract §3.7 A1). A policy that does not resolve
        // is reported where the policy is used: the emoji queued with the reaction stands.
        let home = cx.home.clone();
        if let Ok(resolved) = cx.store.read(|c| policy_emoji(c, &home, plan, workflow)) {
            if resolved.emoji.is_empty() {
                let refused = raise_refused(
                    cx,
                    vars,
                    &emoji,
                    "policy_empty",
                    Some(resolved.file.as_path()),
                );
                return match refused {
                    Ok(()) => ExecOutcome::Failed {
                        error: "policy_empty".into(),
                    },
                    Err(_) => ExecOutcome::Retry {
                        after: REACTION_RETRY,
                    },
                };
            }
            emoji = resolved.emoji;
        }
    }
    let now = cx.ports.clock.utc();
    let call = match crate::slack::call(
        cx.config,
        "slack-react.ts",
        &[("channel", &channel), ("ts", &ts), ("emoji", &emoji)],
        None,
        SEND_DEADLINE,
    ) {
        Ok(call) => call,
        Err(e) => {
            record_endpoint_failure(cx, &e, now);
            return ExecOutcome::Retry {
                after: REACTION_RETRY,
            };
        }
    };
    match run_call(cx, &call) {
        Ok(_) => ExecOutcome::Done,
        Err(e) if e.code() == ErrorCode::SlackRefused => {
            let reason = reason_of(&e);
            let file = match (vars["plan"].as_str(), vars["workflow"].as_str()) {
                (Some(plan), Some(workflow)) => {
                    let home = cx.home.clone();
                    cx.store
                        .read(|c| policy_emoji(c, &home, plan, workflow))
                        .ok()
                        .map(|p| p.file)
                }
                _ => None,
            };
            match raise_refused(cx, vars, &emoji, &reason, file.as_deref()) {
                Ok(()) => ExecOutcome::Failed { error: reason },
                Err(_) => ExecOutcome::Retry {
                    after: REACTION_RETRY,
                },
            }
        }
        Err(e) => {
            record_endpoint_failure(cx, &e, now);
            ExecOutcome::Retry {
                after: REACTION_RETRY,
            }
        }
    }
}

/// A send that failed is never retried: it may have gone out before the answer was lost, and a second send is a duplicate.
fn send_failed(e: &CpdError) -> ExecOutcome {
    ExecOutcome::Failed {
        error: format!("{}: {}", e.code().as_str(), e.message()),
    }
}

fn execute_post(cx: &mut Cx<'_>, vars: &Value) -> ExecOutcome {
    let text = |name: &str| vars[name].as_str().unwrap_or_default().to_owned();
    let (channel, plan, workflow, message) = (
        text("channel"),
        text("plan"),
        text("workflow"),
        text("text"),
    );
    let sent = crate::slack::call(
        cx.config,
        "slack-post.ts",
        &[("channel", &channel)],
        Some(message.into_bytes()),
        SEND_DEADLINE,
    )
    .and_then(|call| run_call(cx, &call));
    let sent = match sent {
        Ok(sent) => sent,
        Err(e) => return send_failed(&e),
    };
    let field = |name: &str| sent[name].as_str().unwrap_or_default().to_owned();
    let now = cx.ports.clock.utc();
    // The merge reaction the policy wants, as `pr post` recorded it at post time.
    let home = cx.home.clone();
    let reaction = PlanId::parse(&plan)
        .and_then(|p| WorkflowKey::parse(&workflow).map(|w| (p, w)))
        .and_then(|(p, w)| {
            cx.store
                .read(|c| crate::pr::policy_of_workflow(c, &home, &p, &w))
        })
        .ok()
        .map(|p| p.pr.post.merged_reaction)
        .filter(|r| !r.is_empty());
    let record = PostRecord {
        channel: field("channel"),
        ts: field("ts"),
        link: field("link"),
        posted_at: timestamp(now),
        reaction,
        reacted_at: None,
        auto_merge: "pending".to_owned(),
        auto_merge_at: None,
    };
    let stored = cx.store.write("swr slack post", |tx| {
        if posts::insert(tx, &plan, &workflow, &record)? {
            jira_sync::queue(tx, &plan, &workflow, jira_sync::Target::InReview)?;
        }
        Ok(())
    });
    match stored {
        Ok(()) => ExecOutcome::Done,
        Err(e) => ExecOutcome::Failed {
            error: format!(
                "The message WAS posted ({}) but recording it failed: {}",
                record.link,
                e.message()
            ),
        },
    }
}

fn execute_repost(cx: &mut Cx<'_>, vars: &Value) -> ExecOutcome {
    let text = |name: &str| vars[name].as_str().unwrap_or_default().to_owned();
    let (channel, thread_ts, plan, workflow, message) = (
        text("channel"),
        text("thread_ts"),
        text("plan"),
        text("workflow"),
        text("text"),
    );
    let sent = crate::slack::call(
        cx.config,
        "slack-post.ts",
        &[
            ("channel", &channel),
            ("thread-ts", &thread_ts),
            ("reply-broadcast", "true"),
        ],
        Some(message.into_bytes()),
        SEND_DEADLINE,
    )
    .and_then(|call| run_call(cx, &call));
    if let Err(e) = sent {
        return send_failed(&e);
    }
    match cx.store.write("swr slack repost", |tx| {
        posts::mark_bumped(tx, &plan, &workflow)?;
        Ok(())
    }) {
        Ok(()) => ExecOutcome::Done,
        Err(e) => ExecOutcome::Failed {
            error: format!(
                "The reminder WAS posted but recording it failed: {}",
                e.message()
            ),
        },
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

/// Runs the channel helper and returns its `ok` answer; a helper that failed is the error it printed.
fn read(cx: &mut Cx<'_>, channel: &str) -> Result<Value, CpdError> {
    let input = stdin(cx, channel)?;
    let call = crate::slack::call(
        cx.config,
        "slack-channel.ts",
        &[("channel", channel), ("me", &cx.config.inbox.slack_user_id)],
        Some(input.to_string().into_bytes()),
        DEADLINE,
    )?;
    run_call(cx, &call)
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
