//! The rules of the Inbox (contract §2.2 "Inbox item", step E8 in §3.7): from the
//! store, the clock and the engine's active errors, the [`Condition`]s that need
//! the User right now. Each kind is one small function and one query; none writes.
//! [`super::model::reconcile`] opens and closes the items.

use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::model::{Condition, Kind, Urgency, open_items};
use super::team::{self, GhPr, Lookup, Memo, Post, Status};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::addr::review_host;
use crate::ids::{ContextId, JobKey, OwnerLogin, PlanId, RecordId, RepoName, SessionId};
use crate::policy::{Checks, PolicyKey, owner_unset, resolve, resolve_unowned_row};
use crate::plan::Phase;
use crate::store::Store;

/// An `error` the engine reported for a step of its own and has not cleared (§3.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub step: String,
    pub scope: String,
    pub code: String,
    /// The id of its `error` Event.
    pub event: i64,
}

/// The `state_label`s of a `blocked` Session that are not a dialog: a Worker's
/// question goes to its Lead, a plan review has its own kind.
const NOT_A_DIALOG: [&str; 2] = ["cpd: Worker question", "cpd: plan review"];

/// What the team review rule needs besides the cache (contract §2.2 "The team review rule"): `inbox.bot_reviewers` and
/// the GitHub state of the posted pull requests, one call for all of them.
pub struct Team<'a> {
    pub bot_reviewers: &'a [String],
    pub github: &'a mut dyn FnMut(&[(String, u64)]) -> Result<Lookup, CpdError>,
}

/// The GitHub answer E8 reuses between ticks ([`team::GITHUB_TTL`]).
static GITHUB: Mutex<Option<Memo>> = Mutex::new(None);

/// Every condition of the kinds, in the order of the kinds. `now` and `stuck_after` decide `job-stuck`; `errors` are
/// the engine's active ones. The team review rule is on when `inbox.team_channel` is set (§8.2), and reads GitHub
/// through `gh`; [`conditions_with`] takes both as arguments.
pub fn conditions(
    store: &mut Store,
    home: &CpdHome,
    now: OffsetDateTime,
    stuck_after: Duration,
    errors: &[EngineError],
) -> Result<Vec<Condition>, CpdError> {
    let config = Config::load(home)?;
    let mut github = |refs: &[(String, u64)]| {
        let mut held = GITHUB.lock().unwrap_or_else(PoisonError::into_inner);
        held.get_or_insert_with(Memo::default).get(refs, Instant::now(), |refs| team::github_lookup(home, refs))
    };
    let team = (!config.inbox.team_channel.is_empty()).then_some(Team { bot_reviewers: &config.inbox.bot_reviewers, github: &mut github });
    conditions_with(store, home, now, stuck_after, errors, team)
}

/// [`conditions`] with the team review rule's inputs given; `None` switches that kind off.
pub fn conditions_with(
    store: &mut Store,
    home: &CpdHome,
    now: OffsetDateTime,
    stuck_after: Duration,
    errors: &[EngineError],
    team: Option<Team<'_>>,
) -> Result<Vec<Condition>, CpdError> {
    let mut all = store.read(dialogs)?;
    all.extend(store.read(|conn| plan_reviews(conn, home))?);
    all.extend(store.read(unread)?);
    all.extend(store.read(|conn| job_stuck(conn, now, stuck_after))?);
    all.extend(engine_errors(errors));
    all.extend(store.read(questions)?);
    all.extend(store.read(|conn| pr_failing(conn, home))?);
    all.extend(store.read(|conn| pr_changes(conn, home))?);
    all.extend(store.read(|conn| pr_ready(conn, home))?);
    all.extend(review_requested(store, home, now, team)?);
    Ok(all)
}

/// A stored value that does not parse: the database was written outside cpd.
fn corrupt(what: &str, value: &str) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The store holds an invalid {what}: {value:?}.")).with("value", value)
}

fn id<T>(what: &str, value: &str, parse: fn(&str) -> Result<T, CpdError>) -> Result<T, CpdError> {
    parse(value).map_err(|_| corrupt(what, value))
}

fn time_of(what: &str, value: &str) -> Result<OffsetDateTime, CpdError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| corrupt(what, value))
}

// ---- Sessions: dialog and unread ---------------------------------------------------------------------------------

/// The id of a Session's latest `session-state` Event.
const LATEST_STATE: &str = "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-state')";
/// The id of its latest finished reply turn: its latest `session-state` Event from `working` or `blocked` to `idle`
/// with `data.reply` true, a turn that answered the User (a start or reload, `stopped` to `idle`, is not one; a turn
/// that answered an Event or another Session is not one either; hs-d242, hs-d278).
const LATEST_IDLE: &str = "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-state'
                                AND json_extract(e.data, '$.to') = 'idle'
                                AND json_extract(e.data, '$.from') IN ('working', 'blocked')
                                AND json_extract(e.data, '$.reply') = 1)";
/// The id of its latest `session-moved` Event with `place: chat`.
const LATEST_CHAT: &str = "(SELECT max(e.id) FROM events e WHERE e.session = s.id AND e.kind = 'session-moved'
                                AND json_extract(e.data, '$.place') = 'chat')";
/// Where its pane is: the `place` of its latest `session-moved` Event.
const PLACE: &str = "(SELECT json_extract(e.data, '$.place') FROM events e
                       WHERE e.session = s.id AND e.kind = 'session-moved' ORDER BY e.id DESC LIMIT 1)";

/// A Session that can need the User, with what its condition links to.
struct Seat {
    session: SessionId,
    context: ContextId,
    plan: Option<PlanId>,
    phase: Option<Phase>,
    /// The tree's label of the Session: its Plan id, `<plan>·<suffix>` for a fork, the Context id for a Manager.
    who: String,
    event: Option<i64>,
}

/// The live Lead, fork and Manager Sessions (never a Worker) that match `filter`, a SQL condition over `s`, oldest
/// first. `event` is the SQL for the id of the Event that causes the condition.
fn seats(conn: &Connection, event: &str, filter: &str) -> Result<Vec<Seat>, CpdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT s.id, s.context, s.plan, p.phase,
                EXISTS (SELECT 1 FROM plans q WHERE q.lead_session = s.id),
                s.role = 'manager',
                {event}
           FROM sessions s LEFT JOIN plans p ON p.id = s.plan
          WHERE s.role <> 'worker' AND s.archived_at IS NULL AND {filter}
          ORDER BY s.created_at, s.id"
    ))?;
    let mut rows = stmt.query([])?;
    let mut seats = Vec::new();
    while let Some(row) = rows.next()? {
        let (session, context, plan, phase): (String, String, Option<String>, Option<String>) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        let (plan_lead, manager): (bool, bool) = (row.get(4)?, row.get(5)?);
        let plan = plan.map(|p| id("Plan id", &p, PlanId::parse)).transpose()?;
        let who = match (&plan, plan_lead, manager) {
            (Some(plan), true, _) => plan.to_string(),
            (_, _, true) => context.clone(),
            (Some(plan), false, false) => format!("{plan}·{}", session.chars().take(4).collect::<String>()),
            (None, _, false) => session.chars().take(8).collect(),
        };
        seats.push(Seat {
            session: id("Session id", &session, SessionId::parse)?,
            context: id("Context id", &context, ContextId::parse)?,
            plan,
            phase: phase.map(|p| Phase::parse(&p).ok_or_else(|| corrupt("Plan phase", &p))).transpose()?,
            who,
            event: row.get(6)?,
        });
    }
    Ok(seats)
}

fn seat_condition(seat: Seat, kind: Kind, urgency: Urgency, label: String) -> Condition {
    Condition {
        kind,
        urgency,
        subject: format!("session:{}", seat.session),
        label,
        url: None,
        context: Some(seat.context),
        plan: seat.plan,
        job: None,
        session: Some(seat.session),
        record: None,
        event: seat.event,
    }
}

/// `dialog`: a Lead, fork or Manager asks the User something. Red when its Plan is in `dispatch` or `deploy`, else magenta.
fn dialogs(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let [worker_question, plan_review] = NOT_A_DIALOG;
    let filter = format!("s.state = 'blocked' AND s.state_label NOT IN ('{worker_question}', '{plan_review}')");
    Ok(seats(conn, LATEST_STATE, &filter)?
        .into_iter()
        .map(|seat| {
            let urgency = match seat.phase {
                Some(Phase::Dispatch | Phase::Deploy) => Urgency::Red,
                _ => Urgency::Magenta,
            };
            let label = format!("{} asks you", seat.who);
            seat_condition(seat, Kind::Dialog, urgency, label)
        })
        .collect())
}

/// `unread`: the rule of the tree's white dot. A Lead, fork or Manager is `idle`, not in the chat slot, and its
/// latest finished reply turn (`working` or `blocked` to `idle`, `data.reply` true) is newer than the last time its chat was in the slot
/// (never in the slot counts as older than everything).
fn unread(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let filter = format!("s.state = 'idle' AND {PLACE} IS NOT 'chat' AND {LATEST_IDLE} > COALESCE({LATEST_CHAT}, 0)");
    Ok(seats(conn, LATEST_IDLE, &filter)?
        .into_iter()
        .map(|seat| {
            let label = format!("{} replied", seat.who);
            seat_condition(seat, Kind::Unread, Urgency::White, label)
        })
        .collect())
}

// ---- Plans and Jobs: plan-review and job-stuck -------------------------------------------------------------------

/// The Plan's plannotator page, when its port file holds a port (the convention of `session::port`), on the review host
/// ([`review_host`], hs-d264).
fn plannotator_url(home: &CpdHome, plan: &PlanId) -> Result<Option<String>, CpdError> {
    let path: &Path = &home.plannotator_port(plan);
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text
            .trim()
            .parse::<u16>()
            .ok()
            .map(|port| format!("http://{}:{port}/", review_host(&mut std::io::stderr())))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CpdError::new(ErrorCode::StoreFailed, format!("Cannot read {}: {e}.", path.display()))
            .with("path", path.display().to_string())),
    }
}

/// `plan-review`: a Plan's latest review is `pending`. Its subject is Plan-qualified because review ids (`r1`)
/// repeat across Plans.
fn plan_reviews(conn: &Connection, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT v.plan, v.id, p.context, p.lead_session
           FROM plan_reviews v JOIN plans p ON p.id = v.plan
          WHERE v.status = 'pending' AND v.number = (SELECT max(w.number) FROM plan_reviews w WHERE w.plan = v.plan)
          ORDER BY v.requested_at, v.plan",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (plan, review, context, lead): (String, String, String, String) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        let plan = id("Plan id", &plan, PlanId::parse)?;
        out.push(Condition {
            kind: Kind::PlanReview,
            urgency: Urgency::Magenta,
            subject: format!("review:{plan}/{review}"),
            label: format!("{plan} plan review"),
            url: plannotator_url(home, &plan)?,
            context: Some(id("Context id", &context, ContextId::parse)?),
            plan: Some(plan),
            job: None,
            session: Some(id("Session id", &lead, SessionId::parse)?),
            record: None,
            event: None,
        });
    }
    Ok(out)
}

/// `job-stuck`: a Job is `failed` and its latest `job-failed` Event is at least `stuck_after` old.
fn job_stuck(conn: &Connection, now: OffsetDateTime, stuck_after: Duration) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT j.plan, j.key, p.context, p.lead_session, e.id, e.at
           FROM jobs j
           JOIN plans p ON p.id = j.plan
           JOIN events e ON e.id = (SELECT max(f.id) FROM events f WHERE f.plan = j.plan AND f.job = j.key AND f.kind = 'job-failed')
          WHERE j.status = 'failed'
          ORDER BY e.id",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (plan, key, context, lead, event, at): (String, String, String, String, i64, String) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?);
        if now - time_of("Event time", &at)? < stuck_after {
            continue;
        }
        let plan = id("Plan id", &plan, PlanId::parse)?;
        out.push(Condition {
            kind: Kind::JobStuck,
            urgency: Urgency::Red,
            subject: format!("job:{plan}/{key}"),
            label: format!("{plan}/{key} failed"),
            url: None,
            context: Some(id("Context id", &context, ContextId::parse)?),
            job: Some(id("Job key", &key, JobKey::parse)?),
            plan: Some(plan),
            session: Some(id("Session id", &lead, SessionId::parse)?),
            record: None,
            event: Some(event),
        });
    }
    Ok(out)
}

// ---- The engine: engine-error ------------------------------------------------------------------------------------

/// `engine-error`: one per error the engine reported and has not cleared.
fn engine_errors(errors: &[EngineError]) -> Vec<Condition> {
    errors
        .iter()
        .map(|e| Condition {
            kind: Kind::EngineError,
            urgency: Urgency::Red,
            subject: format!("error:{}/{}/{}", e.step, e.scope, e.code),
            label: format!("engine {}: {}", e.step, e.code),
            url: None,
            context: None,
            plan: None,
            job: None,
            session: None,
            record: None,
            event: Some(e.event),
        })
        .collect()
}

// ---- Contexts: question ------------------------------------------------------------------------------------------

/// `question`: a Context record of kind `question` with no `answer` record pointing at it. Read from the records'
/// index, as `context list` counts its open questions (`repo` is private to the context area, and the index is one
/// query per tick). Its subject is Context-qualified because record ids (`hs-q1`) repeat across Contexts.
fn questions(conn: &Connection) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT q.context, q.id FROM context_records q
          WHERE q.kind = 'question'
            AND NOT EXISTS (SELECT 1 FROM context_records a
                            WHERE a.context = q.context AND a.kind = 'answer' AND a.question = q.id)
          ORDER BY q.seq",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (context, record): (String, String) = (row.get(0)?, row.get(1)?);
        let context = id("Context id", &context, ContextId::parse)?;
        out.push(Condition {
            kind: Kind::Question,
            urgency: Urgency::Magenta,
            subject: format!("record:{context}/{record}"),
            label: format!("{record} open question"),
            url: None,
            context: Some(context),
            plan: None,
            job: None,
            session: None,
            record: Some(id("Record id", &record, RecordId::parse)?),
            event: None,
        });
    }
    Ok(out)
}

// ---- Pull requests: pr-failing, pr-changes, pr-ready, review-requested -------------------------------------------

/// How a kind reads a row's checks against its `pr.feedback.checks` policy (§8.3).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChecksRule {
    /// Checks are no part of the kind.
    Ignore,
    /// Failed checks count only under `any` (`pr-failing`).
    Failing,
    /// Checks must have passed only under `any`; under `none` any checks do (`pr-ready`).
    Ready,
}

/// The open `pull_requests` rows that match `filter`, a SQL condition over `r`, oldest first, as conditions of `kind`
/// and `urgency`. `label` makes the label from the row's repo without its owner, its number, and whether checks
/// failed. A `prs` row that points to the pull request links its Plan, Context and Lead. A `rule`d row whose policy
/// does not resolve is one `engine-error` condition naming the file and code, beside the others (§8.3).
fn pull_request_conditions(
    conn: &Connection,
    home: &CpdHome,
    kind: Kind,
    urgency: Urgency,
    filter: &str,
    rule: ChecksRule,
    label: fn(&str, i64, bool) -> String,
) -> Result<Vec<Condition>, CpdError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT r.repo, r.number, r.url, r.checks = 'failed', r.checks, p.context, prs.plan, p.lead_session, r.mergeable = 'conflicting',
                c.owner, prs.name
           FROM pull_requests r
           LEFT JOIN prs ON prs.pull_request = r.id
           LEFT JOIN plans p ON p.id = prs.plan
           LEFT JOIN contexts c ON c.id = p.context
          WHERE r.state = 'open' AND {filter}
          ORDER BY r.id"
    ))?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let (repo, number, url, mut checks_failed): (String, i64, String, bool) =
            (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?);
        let checks: String = row.get(4)?;
        let conflicting: bool = row.get(8)?;
        let (owner, pr_name): (Option<String>, Option<String>) = (row.get(9)?, row.get(10)?);
        let (context, plan, lead): (Option<String>, Option<String>, Option<String>) =
            (row.get(5)?, row.get(6)?, row.get(7)?);
        let name = repo.split_once('/').map_or(repo.as_str(), |(_, name)| name);
        let links = |kind, urgency, subject, label, url| -> Result<Condition, CpdError> {
            Ok(Condition {
                kind,
                urgency,
                subject,
                label,
                url,
                context: context.as_deref().map(|c| id("Context id", c, ContextId::parse)).transpose()?,
                plan: plan.as_deref().map(|p| id("Plan id", p, PlanId::parse)).transpose()?,
                job: None,
                session: lead.as_deref().map(|s| id("Session id", s, SessionId::parse)).transpose()?,
                record: None,
                event: None,
            })
        };
        // Checks count only where the row's policy says `any` (§8.3): `pr-failing` counts failed ones, and `pr-ready`
        // needs passed ones, so it asks the policy for any row whose checks did not pass.
        let asks = match rule {
            ChecksRule::Ignore => false,
            ChecksRule::Failing => checks_failed,
            ChecksRule::Ready => checks != "passed",
        };
        if asks {
            // A PR points to the row: its Context's owner keys it, and an unset one is `owner_unset`, never the unowned rule.
            let owned = plan.as_ref().map(|_| Owned { context: context.as_deref().unwrap_or_default(), owner: owner.as_deref(), name: pr_name.as_deref() });
            let counts = match checks_policy(home, &repo, owned)? {
                Ok(policy) => policy == Checks::Any,
                Err(error) => {
                    // The row's checks are unknown. `pr-failing` tells the error of a failed row (its conflict still as
                    // `kind`); `pr-ready` tells the others, since a failed row's error is already `pr-failing`'s.
                    if rule == ChecksRule::Failing || checks != "failed" {
                        let code = error.code().as_str();
                        out.push(links(
                            Kind::EngineError,
                            Urgency::Red,
                            format!("error:E8/pr:{repo}#{number}/{code}"),
                            format!("policy of {name}#{number}: {code} ({})", failed_file(home, &error)),
                            Some(url.clone()),
                        )?);
                    }
                    if rule == ChecksRule::Ready {
                        continue;
                    }
                    false
                }
            };
            match rule {
                ChecksRule::Ready if counts => continue,
                ChecksRule::Failing => {
                    checks_failed = counts;
                    if !checks_failed && !conflicting {
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(links(kind, urgency, format!("pr:{repo}#{number}"), label(name, number, checks_failed), Some(url))?);
    }
    Ok(out)
}

/// What a resolve error names: the policy file (`policy_invalid`, `owner_unknown`), or the Context (`owner_unset`).
fn failed_file(home: &CpdHome, error: &CpdError) -> String {
    let detail = |key: &str| error.details().get(key).and_then(|v| v.as_str());
    match (detail("file"), detail("owner"), detail("context")) {
        (Some(file), _, _) => file.to_owned(),
        (_, Some(owner), _) => home.root().join("policy").join("owners").join(format!("{owner}.toml")).display().to_string(),
        (_, _, Some(context)) => format!("Context {context}"),
        _ => error.message().to_owned(),
    }
}

/// The PR a `pull_requests` row points to, as the Inbox query reads it: the Context's `owner` (NULL for a legacy
/// Context) and the PR's repo `name` (NULL before the migration).
struct Owned<'a> {
    context: &'a str,
    owner: Option<&'a str>,
    name: Option<&'a str>,
}

/// `pr.feedback.checks` of a pull request row: the key of the PR that points to it (`Some`), else `Repo` of the
/// row's `owner/name` through the unowned-row entry point (`None`). The outer error is a stored value that does not
/// parse; the inner one the resolve's, which the caller reports for its row.
fn checks_policy(home: &CpdHome, repo: &str, owned: Option<Owned<'_>>) -> Result<Result<Checks, CpdError>, CpdError> {
    let parse = |what: &str| corrupt(what, repo);
    let resolved = match owned {
        Some(Owned { context, owner: None, .. }) => Err(owner_unset(&id("Context id", context, ContextId::parse)?)),
        Some(Owned { owner: Some(owner), name, .. }) => {
            let owner = OwnerLogin::parse(owner)?;
            match name {
                Some(name) => resolve(home, &PolicyKey::Repo(owner, RepoName::parse(name)?)),
                None => resolve(home, &PolicyKey::Owner(owner)),
            }
        }
        None => {
            let (owner, name) = repo.split_once('/').ok_or_else(|| parse("pull request repo"))?;
            let key = PolicyKey::Repo(
                OwnerLogin::parse(owner).map_err(|_| parse("pull request repo owner"))?,
                RepoName::parse(name).map_err(|_| parse("pull request repo name"))?,
            );
            resolve_unowned_row(home, &key)
        }
    };
    Ok(resolved.map(|resolved| resolved.policy.pr.feedback.checks))
}

/// `pr-failing`: an authored open pull request has failed checks or a merge conflict (`unknown` is no conflict).
/// The label names the conflict only when checks did not fail.
fn pr_failing(conn: &Connection, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
    pull_request_conditions(
        conn,
        home,
        Kind::PrFailing,
        Urgency::Red,
        "r.authored = 1 AND (r.checks = 'failed' OR r.mergeable = 'conflicting')",
        ChecksRule::Failing,
        |name, n, checks_failed| {
            if checks_failed { format!("{name}#{n} checks failed") } else { format!("{name}#{n} merge conflict") }
        },
    )
}

/// `pr-changes`: an authored open pull request has `changes_requested`.
fn pr_changes(conn: &Connection, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
    pull_request_conditions(
        conn,
        home,
        Kind::PrChanges,
        Urgency::Red,
        "r.authored = 1 AND r.review_decision = 'changes_requested'",
        ChecksRule::Ignore,
        |name, n, _| format!("{name}#{n} changes requested"),
    )
}

/// `pr-ready`: an authored, open, non-draft pull request has passed checks (any checks under `feedback.checks = none`), no conflict and no changes requested.
fn pr_ready(conn: &Connection, home: &CpdHome) -> Result<Vec<Condition>, CpdError> {
    pull_request_conditions(
        conn,
        home,
        Kind::PrReady,
        Urgency::Yellow,
        "r.authored = 1 AND r.draft = 0 AND r.mergeable = 'mergeable'
         AND r.review_decision <> 'changes_requested'",
        ChecksRule::Ready,
        |name, n, _| format!("{name}#{n} ready to merge"),
    )
}

/// `review-requested`: a pull request posted in the team channel in the last 7 days that hermes' rule calls
/// `NEEDS_REVIEW` (contract §2.2 "The team review rule", hs-d320). GitHub's own review requests open nothing.
///
/// The posts are `run/team-review.json` (E10); a missing file is "unknown", not "empty", and so is a GitHub that
/// cannot be read (the call fails, or no pull request of the posts can be read, hermes `main`): the open items stay
/// as they are, and a failure that is not just "not yet" is one `engine-error` condition beside them. A single
/// pull request GitHub cannot find is dropped from this run, as hermes' `build` drops it.
fn review_requested(store: &mut Store, home: &CpdHome, now: OffsetDateTime, team: Option<Team<'_>>) -> Result<Vec<Condition>, CpdError> {
    let Some(team) = team else { return Ok(Vec::new()) };
    let cache = match team::read_cache(home) {
        Ok(Some(cache)) => cache,
        Ok(None) => return kept_review_items(store, None),
        Err(e) => return kept_review_items(store, Some(&e)),
    };
    let cutoff = now.unix_timestamp() as f64 - team::WINDOW.as_secs_f64();
    let posts: Vec<&Post> = cache.posts.iter().filter(|p| p.primary && p.ts >= cutoff).collect();
    if posts.is_empty() {
        return Ok(Vec::new());
    }
    let refs: Vec<(String, u64)> = posts.iter().map(|p| (p.repo.clone(), p.number)).collect();
    let lookup = match (team.github)(&refs) {
        Ok(lookup) => lookup,
        Err(e) => return kept_review_items(store, Some(&e)),
    };
    let read = |p: &Post| -> Option<&GhPr> { lookup.prs.get(&(p.repo.clone(), p.number)).and_then(Option::as_ref) };
    if posts.iter().all(|p| read(p).is_none()) {
        let none = CpdError::new(ErrorCode::GhFailed, "GitHub could not read any pull request posted in the team channel.");
        return kept_review_items(store, Some(&none));
    }
    Ok(posts
        .into_iter()
        .filter(|post| {
            read(post).is_some_and(|pr| matches!(team::status(post, pr, &lookup.me, team.bot_reviewers), Status::NeedsReview(_)))
        })
        .map(|post| {
            let name = post.repo.split_once('/').map_or(post.repo.as_str(), |(_, name)| name);
            Condition {
                kind: Kind::ReviewRequested,
                urgency: Urgency::Yellow,
                subject: format!("pr:{}#{}", post.repo, post.number),
                label: format!("review {name}#{}", post.number),
                url: Some(format!("https://github.com/{}/pull/{}", post.repo, post.number)),
                context: None,
                plan: None,
                job: None,
                session: None,
                record: None,
                event: None,
            }
        })
        .collect())
}

/// The open `review-requested` items as conditions (nothing changes), and `why` as an `engine-error` when given.
fn kept_review_items(store: &mut Store, why: Option<&CpdError>) -> Result<Vec<Condition>, CpdError> {
    let mut out: Vec<Condition> =
        store.read(open_items)?.iter().filter(|i| i.kind == Kind::ReviewRequested).map(Condition::from).collect();
    if let Some(e) = why {
        let code = e.code().as_str();
        out.push(Condition {
            kind: Kind::EngineError,
            urgency: Urgency::Red,
            subject: format!("error:E8/team-review/{code}"),
            label: format!("team review: {code}"),
            url: None,
            context: None,
            plan: None,
            job: None,
            session: None,
            record: None,
            event: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use serde_json::{Value, json};

    use super::*;
    use crate::host::addr::URL_HOST;
    use crate::inbox::model::reconcile;
    use crate::inbox::team::{GhState, Review};

    /// [`super::conditions_with`] with the team review rule off: these tests need no config file.
    fn conditions(
        store: &mut Store,
        home: &CpdHome,
        now: OffsetDateTime,
        stuck_after: Duration,
        errors: &[EngineError],
    ) -> Result<Vec<Condition>, CpdError> {
        conditions_with(store, home, now, stuck_after, errors, None)
    }

    const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);
    const TEN: Duration = Duration::from_secs(600);
    const T: &str = "2026-01-01T00:00:00.000Z";
    const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct Fixture {
        _dir: tempfile::TempDir,
        home: CpdHome,
        store: Store,
    }

    /// A Context `ctx` with its Manager `mgr`, and a Plan `pay` in `phase` with its Lead `l-pay`; both `working`.
    fn fixture(phase: &str) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let store = Store::open(&home, 5000).unwrap();
        let mut f = Fixture { _dir: dir, home, store };
        let locked = if matches!(phase, "grounding" | "plan") { "NULL".to_owned() } else { format!("'{T}'") };
        let done = if phase == "done" { format!("'{T}'") } else { "NULL".to_owned() };
        f.sql(&format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
             INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('pay', 'ctx', 'Pay', '{phase}', 'l-pay', '{T}', {locked}, {done});"
        ));
        f
    }

    impl Fixture {
        fn sql(&mut self, sql: &str) {
            self.store
                .write("fixture", |tx| {
                    tx.execute_batch(sql)?;
                    Ok(())
                })
                .unwrap();
        }

        /// Appends an Event and returns its id.
        fn event(&mut self, kind: &str, session: Option<&str>, job: Option<&str>, at: &str, data: &str) -> i64 {
            let session = session.map_or("NULL".to_owned(), |s| format!("'{s}'"));
            let (plan, job) = job.map_or(("NULL".to_owned(), "NULL".to_owned()), |j| ("'pay'".to_owned(), format!("'{j}'")));
            self.sql(&format!(
                "INSERT INTO events (txn, at, kind, context, plan, job, session, text, data)
                 VALUES (cpd_txn(), '{at}', '{kind}', 'ctx', {plan}, {job}, {session}, 'x', '{data}');"
            ));
            self.store.read(|c| Ok(c.query_row("SELECT max(id) FROM events", [], |r| r.get(0))?)).unwrap()
        }

        /// The Session's state changes: the row and its `session-state` Event (`from` is the state it was in).
        fn state(&mut self, session: &str, state: &str, label: Option<&str>) -> i64 {
            self.state_as(session, state, label, false)
        }

        /// A turn that ended in `idle` as a reply to the User (`--reply`).
        fn reply(&mut self, session: &str) -> i64 {
            self.state_as(session, "idle", None, true)
        }

        fn state_as(&mut self, session: &str, state: &str, label: Option<&str>, reply: bool) -> i64 {
            let from: String = self.store.read(|c| Ok(c.query_row("SELECT state FROM sessions WHERE id = ?1", [session], |r| r.get(0))?)).unwrap();
            let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
            self.sql(&format!("UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = '{session}'"));
            let reply = if reply { r#","reply":true"# } else { "" };
            self.event("session-state", Some(session), None, T, &format!(r#"{{"from":"{from}","label":null,"to":"{state}"{reply}}}"#))
        }

        fn moved(&mut self, session: &str, place: &str) -> i64 {
            self.event("session-moved", Some(session), None, T, &format!(r#"{{"from":"none","place":"{place}"}}"#))
        }

        fn session(&mut self, id: &str, role: &str, plan: Option<&str>) {
            let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
            let attempt = if role == "worker" { "'w-a1'" } else { "NULL" };
            if role == "worker" {
                self.job("w", "running");
                self.sql(&format!(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                     VALUES ('pay', 'w-a1', 'w', 1, 'S2', 'ladder', 'running', 'cpd/pay--w-a1', '/w', '{T}');"
                ));
            }
            self.sql(&format!(
                "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
                 VALUES ('{id}', '{role}', 'ctx', {plan}, {attempt}, 'n', '/', 'O3', 'working', '{T}', '{T}');"
            ));
        }

        fn job(&mut self, key: &str, status: &str) {
            let cause = if status == "failed" { "'crashed'" } else { "NULL" };
            self.sql(&format!(
                "INSERT OR IGNORE INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pay', 'p1', '/r', 'main', 'dispatch', 'cpd/pay/p1', '/lw', 1, '{T}', '{T}');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                   fingerprint, created_txn, created_at, failed_cause)
                   VALUES ('pay', 'p1', '{key}', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{FP}', 1, '{T}', {cause});"
            ));
        }

        fn review(&mut self, id: &str, status: &str) {
            let number = &id[1..];
            let decided = if status == "pending" { "NULL".to_owned() } else { format!("'{T}'") };
            self.sql(&format!(
                "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
                 VALUES ('pay', '{id}', {number}, '{FP}', '{status}', '{T}', {decided});"
            ));
        }

        fn of(&mut self, kind: Kind) -> Vec<Condition> {
            let all = conditions(&mut self.store, &self.home, NOW, TEN, &[]).unwrap();
            all.into_iter().filter(|c| c.kind == kind).collect()
        }
    }

    fn one(mut found: Vec<Condition>) -> Condition {
        assert_eq!(found.len(), 1, "{found:?}");
        found.remove(0)
    }

    // ---- dialog ----

    #[test]
    fn a_blocked_lead_is_a_dialog_with_its_plans_urgency() {
        for (phase, urgency) in [
            ("grounding", Urgency::Magenta),
            ("plan", Urgency::Magenta),
            ("dispatch", Urgency::Red),
            ("deploy", Urgency::Red),
            ("done", Urgency::Magenta),
        ] {
            let mut f = fixture(phase);
            let event = f.state("l-pay", "blocked", Some("Which repo?"));
            let c = one(f.of(Kind::Dialog));
            assert_eq!(c.urgency, urgency, "{phase}");
            assert_eq!(c.subject, "session:l-pay");
            assert_eq!(c.label, "pay asks you");
            assert_eq!((c.context.as_ref().map(|c| c.as_str()), c.plan.as_ref().map(|p| p.as_str())), (Some("ctx"), Some("pay")));
            assert_eq!(c.session.as_ref().map(|s| s.as_str()), Some("l-pay"));
            assert_eq!(c.event, Some(event));
        }
    }

    #[test]
    fn a_dialog_is_gone_when_the_session_is_no_longer_blocked() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "blocked", Some("Which repo?"));
        assert_eq!(f.of(Kind::Dialog).len(), 1);
        f.state("l-pay", "working", None);
        assert!(f.of(Kind::Dialog).is_empty());
    }

    #[test]
    fn a_manager_and_a_fork_are_dialogs_with_their_own_labels_and_urgency() {
        let mut f = fixture("dispatch");
        f.session("fork-1234", "lead", Some("pay"));
        f.session("loose-fork-id", "lead", None);
        for session in ["mgr", "fork-1234", "loose-fork-id"] {
            f.state(session, "blocked", Some("Which?"));
        }
        let found = f.of(Kind::Dialog);
        let by = |subject: &str| found.iter().find(|c| c.subject == subject).unwrap().clone();
        assert_eq!((by("session:mgr").label.as_str(), by("session:mgr").urgency), ("ctx asks you", Urgency::Magenta));
        assert_eq!((by("session:fork-1234").label.as_str(), by("session:fork-1234").urgency), ("pay·fork asks you", Urgency::Red));
        assert_eq!(
            (by("session:loose-fork-id").label.as_str(), by("session:loose-fork-id").urgency),
            ("loose-fo asks you", Urgency::Magenta)
        );
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn a_worker_question_and_a_plan_review_label_are_not_dialogs() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "blocked", Some("cpd: Worker question"));
        f.state("mgr", "blocked", Some("cpd: plan review"));
        assert!(f.of(Kind::Dialog).is_empty());
    }

    #[test]
    fn an_archived_session_has_no_dialog_and_no_unread() {
        let mut f = fixture("dispatch");
        f.session("fork", "lead", Some("pay"));
        f.state("l-pay", "blocked", Some("Which?"));
        f.state("fork", "idle", None);
        f.sql("UPDATE sessions SET archived_at = 't' WHERE id IN ('l-pay', 'fork')");
        assert!(f.of(Kind::Dialog).is_empty());
        assert!(f.of(Kind::Unread).is_empty());
    }

    #[test]
    fn a_worker_session_never_yields_an_item() {
        let mut f = fixture("dispatch");
        f.session("wk", "worker", Some("pay"));
        f.state("wk", "blocked", Some("Which?"));
        assert!(f.of(Kind::Dialog).is_empty());
        f.state("wk", "idle", None);
        assert!(f.of(Kind::Unread).is_empty());
        f.state("wk", "blocked", Some("cpd: Worker question"));
        assert!(conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().is_empty());
    }

    // ---- unread ----

    #[test]
    fn an_idle_turn_outside_the_chat_slot_is_unread_until_the_chat_is_seen() {
        let mut f = fixture("dispatch");
        let idle = f.reply("l-pay");
        let c = one(f.of(Kind::Unread));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::White, "session:l-pay", "pay replied"));
        assert_eq!(c.event, Some(idle), "the idle Event");
        assert_eq!(c.plan.as_ref().map(|p| p.as_str()), Some("pay"));

        f.moved("l-pay", "parked");
        assert_eq!(f.of(Kind::Unread).len(), 1, "parked is not seen");
        f.moved("l-pay", "chat");
        assert!(f.of(Kind::Unread).is_empty(), "in the slot: seen");
        f.moved("l-pay", "parked");
        assert!(f.of(Kind::Unread).is_empty(), "out of the slot again, but the turn is not newer than the sight");
        f.state("l-pay", "working", None);
        let idle = f.reply("l-pay");
        assert_eq!(one(f.of(Kind::Unread)).event, Some(idle), "a new idle turn after being seen");
    }

    #[test]
    fn a_turn_that_did_not_answer_the_user_is_not_unread() {
        let mut f = fixture("dispatch");
        f.state("l-pay", "idle", None);
        assert!(f.of(Kind::Unread).is_empty(), "a non-reply turn alone");
        f.state("l-pay", "working", None);
        f.state("l-pay", "idle", None);
        assert!(f.of(Kind::Unread).is_empty(), "working -> idle without a reply");
    }

    #[test]
    fn a_later_non_reply_turn_does_not_hide_an_unread_reply() {
        let mut f = fixture("dispatch");
        let reply = f.reply("l-pay");
        f.state("l-pay", "working", None);
        f.state("l-pay", "idle", None);
        let c = one(f.of(Kind::Unread));
        assert_eq!(c.event, Some(reply), "the reply Event, not the later turn");
        f.moved("l-pay", "chat");
        f.moved("l-pay", "parked");
        f.state("l-pay", "working", None);
        f.state("l-pay", "idle", None);
        assert!(f.of(Kind::Unread).is_empty(), "seen, and the later turn was no reply");
    }

    #[test]
    fn a_reload_is_not_a_finished_turn() {
        let mut f = fixture("dispatch");
        f.reply("l-pay");
        f.moved("l-pay", "chat");
        f.moved("l-pay", "parked");
        assert!(f.of(Kind::Unread).is_empty(), "seen");
        f.state("l-pay", "stopped", None);
        f.state("l-pay", "idle", None);
        assert!(f.of(Kind::Unread).is_empty(), "idle -> stopped -> idle is a reload, not a turn");
        f.state("l-pay", "stopped", None);
        f.reply("l-pay");
        assert!(f.of(Kind::Unread).is_empty(), "stopped -> idle is a reload even when it says reply");
    }

    #[test]
    fn a_working_or_blocked_session_going_idle_after_the_visit_is_unread() {
        for before in ["working", "blocked"] {
            let mut f = fixture("dispatch");
            f.reply("l-pay");
            f.moved("l-pay", "chat");
            f.moved("l-pay", "parked");
            f.state("l-pay", before, (before == "blocked").then_some("Which?"));
            let idle = f.reply("l-pay");
            assert_eq!(one(f.of(Kind::Unread)).event, Some(idle), "{before} -> idle");
        }
    }

    #[test]
    fn a_session_in_the_chat_slot_or_not_idle_is_not_unread() {
        let mut f = fixture("dispatch");
        f.reply("l-pay");
        f.moved("l-pay", "chat");
        assert!(f.of(Kind::Unread).is_empty());
        f.state("mgr", "working", None);
        f.state("mgr", "blocked", Some("Which?"));
        assert!(f.of(Kind::Unread).is_empty(), "blocked is a dialog, not unread");
        f.state("mgr", "stopped", None);
        assert!(f.of(Kind::Unread).is_empty());
    }

    #[test]
    fn a_manager_and_a_fork_can_be_unread() {
        let mut f = fixture("dispatch");
        f.session("fork-1234", "lead", Some("pay"));
        f.reply("mgr");
        f.reply("fork-1234");
        let mut labels: Vec<_> = f.of(Kind::Unread).into_iter().map(|c| c.label).collect();
        labels.sort();
        assert_eq!(labels, ["ctx replied", "pay·fork replied"]);
    }

    // ---- plan-review ----

    #[test]
    fn a_pending_review_is_a_plan_review_with_its_plannotator_page() {
        let mut f = fixture("plan");
        f.review("r1", "pending");
        let c = one(f.of(Kind::PlanReview));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Magenta, "review:pay/r1", "pay plan review"));
        assert_eq!(c.url, None, "no port file");
        assert_eq!(c.session.as_ref().map(|s| s.as_str()), Some("l-pay"), "its Lead");
        assert_eq!((c.context.as_ref().map(|c| c.as_str()), c.plan.as_ref().map(|p| p.as_str())), (Some("ctx"), Some("pay")));

        let port = f.home.plannotator_port(&PlanId::parse("pay").unwrap());
        std::fs::create_dir_all(port.parent().unwrap()).unwrap();
        std::fs::write(&port, "20000\n").unwrap();
        // the review host: `PLANNOTATOR_URL_HOST` from cpd's environment (the only test that sets it)
        unsafe { std::env::set_var(URL_HOST, "review.example") };
        let url = one(f.of(Kind::PlanReview)).url;
        unsafe { std::env::remove_var(URL_HOST) };
        assert_eq!(url.as_deref(), Some("http://review.example:20000/"));
    }

    #[test]
    fn a_decided_review_is_no_plan_review_and_only_the_latest_counts() {
        let mut f = fixture("plan");
        f.review("r1", "rejected");
        assert!(f.of(Kind::PlanReview).is_empty());
        f.review("r2", "pending");
        assert_eq!(one(f.of(Kind::PlanReview)).subject, "review:pay/r2");
        f.sql("UPDATE plan_reviews SET status = 'approved', decided_at = 't' WHERE id = 'r2'");
        assert!(f.of(Kind::PlanReview).is_empty());
    }

    // ---- job-stuck ----

    #[test]
    fn a_job_failed_for_the_stuck_time_is_job_stuck_and_clears_on_retry() {
        let mut f = fixture("dispatch");
        f.job("a", "failed");
        f.event("job-failed", None, Some("a"), "2026-01-01T00:00:00.000Z", r#"{}"#);
        let latest = f.event("job-failed", None, Some("a"), "2026-01-01T23:50:00.000Z", r#"{}"#);
        let c = one(f.of(Kind::JobStuck));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "job:pay/a", "pay/a failed"));
        assert_eq!(c.job.as_ref().map(|j| j.as_str()), Some("a"));
        assert_eq!(c.session.as_ref().map(|s| s.as_str()), Some("l-pay"), "the Plan's Lead");
        assert_eq!(c.event, Some(latest), "the latest job-failed Event");

        f.sql("UPDATE jobs SET status = 'queued', failed_cause = NULL WHERE key = 'a'");
        assert!(f.of(Kind::JobStuck).is_empty(), "retried");
    }

    #[test]
    fn a_job_failed_less_than_the_stuck_time_ago_is_not_yet_stuck() {
        let mut f = fixture("dispatch");
        f.job("a", "failed");
        f.event("job-failed", None, Some("a"), "2026-01-01T23:50:01.000Z", r#"{}"#);
        assert!(f.of(Kind::JobStuck).is_empty(), "9 min 59 s");
        f.event("job-failed", None, Some("a"), "2026-01-01T23:50:00.000Z", r#"{}"#);
        assert_eq!(f.of(Kind::JobStuck).len(), 1, "exactly the stuck time counts");
        f.job("b", "queued");
        f.event("job-failed", None, Some("b"), T, r#"{}"#);
        assert_eq!(f.of(Kind::JobStuck).len(), 1, "a Job that is not failed is never stuck");
    }

    // ---- engine-error ----

    #[test]
    fn each_engine_error_is_an_item_and_none_when_cleared() {
        let mut f = fixture("dispatch");
        let error = EngineError { step: "E3".into(), scope: "plan:pay".into(), code: "store_failed".into(), event: 41 };
        let found = conditions(&mut f.store, &f.home, NOW, TEN, std::slice::from_ref(&error)).unwrap();
        let c = one(found);
        assert_eq!(c.kind, Kind::EngineError);
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "error:E3/plan:pay/store_failed", "engine E3: store_failed"));
        assert_eq!((c.event, c.session, c.plan), (Some(41), None, None));
        assert!(conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().is_empty());
    }

    // ---- question ----

    /// A record of `kind` in `context` (made if need be); an `answer` answers `hs-q1`.
    fn record(f: &mut Fixture, context: &str, id: &str, kind: &str) {
        let a = if kind == "answer" { "'hs-q1'" } else { "NULL" };
        f.sql(&format!(
            "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('{context}', 'C', '{T}', 'm1');
             INSERT INTO context_records (context, id, kind, at, machine, text, question)
             VALUES ('{context}', '{id}', '{kind}', '{T}', 'hs', 't', {a});"
        ));
    }

    #[test]
    fn a_question_without_an_answer_is_an_item_and_gone_when_answered() {
        let mut f = fixture("dispatch");
        record(&mut f, "ctx", "hs-q1", "question");
        record(&mut f, "ctx", "hs-d1", "decision");
        let c = one(f.of(Kind::Question));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Magenta, "record:ctx/hs-q1", "hs-q1 open question"));
        assert_eq!(c.record.as_ref().map(|r| r.to_string()), Some("hs-q1".to_owned()));
        assert_eq!((c.context.as_ref().map(|c| c.as_str()), c.plan, c.session), (Some("ctx"), None, None));

        record(&mut f, "ctx", "hs-a1", "answer");
        assert!(f.of(Kind::Question).is_empty());
    }

    #[test]
    fn the_same_record_id_in_two_contexts_is_two_questions_and_an_answer_clears_only_its_own() {
        let mut f = fixture("dispatch");
        record(&mut f, "ctx", "hs-q1", "question");
        record(&mut f, "other", "hs-q1", "question");
        let subjects: Vec<_> = f.of(Kind::Question).into_iter().map(|c| c.subject).collect();
        assert_eq!(subjects, ["record:ctx/hs-q1", "record:other/hs-q1"]);
        record(&mut f, "other", "hs-a1", "answer");
        assert_eq!(one(f.of(Kind::Question)).subject, "record:ctx/hs-q1");
    }

    // ---- pull requests ----

    /// An open, authored, non-draft pull request `acme/shop#n` with the given checks, mergeable and review values.
    fn pull_request(f: &mut Fixture, n: i64, checks: &str, mergeable: &str, review: &str) {
        f.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                        mergeable, authored, review_requested)
             VALUES ('acme/shop', {n}, 'https://github.com/acme/shop/pull/{n}', 'T', 0, 'open', '{review}', '{checks}', '{T}', '{T}',
                     '{mergeable}', 1, 0);"
        ));
    }

    fn set(f: &mut Fixture, n: i64, assignments: &str) {
        f.sql(&format!("UPDATE pull_requests SET {assignments} WHERE number = {n}"));
    }

    fn pr_kinds(f: &mut Fixture) -> Vec<Kind> {
        let all = conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap();
        all.into_iter().map(|c| c.kind).collect()
    }

    #[test]
    fn failed_checks_open_pr_failing_and_clear_when_they_pass() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 7, "failed", "mergeable", "none");
        let c = one(f.of(Kind::PrFailing));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "pr:acme/shop#7", "shop#7 checks failed"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/shop/pull/7"));
        assert_eq!((c.context, c.plan, c.session, c.job, c.record, c.event), (None, None, None, None, None, None));
        set(&mut f, 7, "checks = 'passed'");
        assert!(f.of(Kind::PrFailing).is_empty());
    }

    #[test]
    fn a_conflicting_pr_is_failing_with_the_conflict_label_and_never_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 8, "passed", "conflicting", "approved");
        let c = one(f.of(Kind::PrFailing));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "pr:acme/shop#8", "shop#8 merge conflict"));
        assert!(f.of(Kind::PrReady).is_empty());
        set(&mut f, 8, "checks = 'failed'");
        assert_eq!(one(f.of(Kind::PrFailing)).label, "shop#8 checks failed", "failed checks name themselves first");
        set(&mut f, 8, "checks = 'passed', mergeable = 'mergeable'");
        assert!(f.of(Kind::PrFailing).is_empty());
        assert_eq!(f.of(Kind::PrReady).len(), 1);
    }

    #[test]
    fn an_unknown_mergeable_is_neither_failing_nor_ready() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 9, "passed", "unknown", "approved");
        assert!(pr_kinds(&mut f).is_empty());
    }

    #[test]
    fn a_pr_with_changes_requested_is_pr_changes_and_clears_with_the_decision() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 3, "pending", "mergeable", "changes_requested");
        let c = one(f.of(Kind::PrChanges));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Red, "pr:acme/shop#3", "shop#3 changes requested"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/shop/pull/3"));
        set(&mut f, 3, "review_decision = 'approved'");
        assert!(f.of(Kind::PrChanges).is_empty());
    }

    #[test]
    fn a_passing_mergeable_pr_is_ready_until_it_is_a_draft_or_changes_are_requested() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 4, "passed", "mergeable", "approved");
        let c = one(f.of(Kind::PrReady));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Yellow, "pr:acme/shop#4", "shop#4 ready to merge"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/shop/pull/4"));
        set(&mut f, 4, "review_decision = 'none'");
        assert_eq!(f.of(Kind::PrReady).len(), 1, "no review is no objection");
        set(&mut f, 4, "draft = 1");
        assert!(f.of(Kind::PrReady).is_empty(), "a draft is never ready");
        set(&mut f, 4, "draft = 0, review_decision = 'changes_requested'");
        assert!(f.of(Kind::PrReady).is_empty());
        set(&mut f, 4, "review_decision = 'approved', checks = 'pending'");
        assert!(f.of(Kind::PrReady).is_empty());
    }

    #[test]
    fn with_feedback_checks_none_a_mergeable_pr_is_ready_whatever_its_checks() {
        let mut f = fixture("dispatch");
        let owners = f.home.root().join("policy/owners");
        std::fs::create_dir_all(&owners).unwrap();
        std::fs::write(owners.join("acme.toml"), "[pr]\ngate = \"local\"\nfeedback.checks = \"none\"\n").unwrap();
        for (n, checks) in [(1, "none"), (2, "failed"), (3, "pending")] {
            pull_request(&mut f, n, checks, "mergeable", "none");
        }
        assert_eq!(f.of(Kind::PrReady).len(), 3);
        assert!(f.of(Kind::PrFailing).is_empty());
        set(&mut f, 1, "draft = 1");
        set(&mut f, 2, "review_decision = 'changes_requested'");
        set(&mut f, 3, "mergeable = 'conflicting'");
        assert!(f.of(Kind::PrReady).is_empty(), "draft, changes requested and conflict still bar it");
    }

    #[test]
    fn with_feedback_checks_any_a_pr_is_ready_only_when_its_checks_passed() {
        let mut f = fixture("dispatch");
        for (n, checks) in [(1, "none"), (2, "failed"), (3, "pending"), (4, "passed")] {
            pull_request(&mut f, n, checks, "mergeable", "none");
        }
        assert_eq!(one(f.of(Kind::PrReady)).subject, "pr:acme/shop#4");
    }

    #[test]
    fn only_authored_pull_requests_are_failing_changed_or_ready_and_a_github_request_opens_nothing() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "conflicting", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        f.sql("UPDATE pull_requests SET authored = 0, review_requested = 1");
        assert!(pr_kinds(&mut f).is_empty(), "review_requested alone is no item (hs-d320)");
    }

    #[test]
    fn a_pull_request_that_is_not_open_is_no_item() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 1, "failed", "conflicting", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        assert_eq!(pr_kinds(&mut f).len(), 3);
        f.sql("UPDATE pull_requests SET state = 'closed'");
        assert!(pr_kinds(&mut f).is_empty());
    }

    #[test]
    fn a_pull_request_owned_by_a_plan_links_the_plan_its_context_and_its_lead() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 6, "failed", "mergeable", "changes_requested");
        pull_request(&mut f, 7, "passed", "mergeable", "approved");
        f.job("a", "queued");
        // A PR points to the row: its Context's owner keys it (an unset one is `owner_unset`, not this test's subject).
        f.sql("UPDATE contexts SET owner = 'acme'");
        let owners = f.home.root().join("policy/owners");
        std::fs::create_dir_all(&owners).unwrap();
        std::fs::write(owners.join("acme.toml"), "").unwrap();
        f.sql("UPDATE prs SET status = 'open', pull_request = (SELECT id FROM pull_requests WHERE number = 6),
                              pr_number = 6, pr_url = 'https://github.com/acme/shop/pull/6', pr_opened_at = 't'");
        for kind in [Kind::PrFailing, Kind::PrChanges] {
            let c = one(f.of(kind));
            assert_eq!(
                (c.context.as_ref().map(|c| c.as_str()), c.plan.as_ref().map(|p| p.as_str()), c.session.as_ref().map(|s| s.as_str())),
                (Some("ctx"), Some("pay"), Some("l-pay")),
                "{kind:?}"
            );
        }
        let c = one(f.of(Kind::PrReady));
        assert_eq!((c.subject.as_str(), c.plan, c.session), ("pr:acme/shop#7", None, None), "a pull request no PR owns");
    }

    // ---- review-requested: the team review rule (hs-d320) ----

    const BOTS: [&str; 1] = ["github-actions"];
    const ME_LOGIN: &str = "clayton-duarte";

    /// `ts` of a post an hour before `NOW`.
    fn recent() -> f64 {
        NOW.unix_timestamp() as f64 - 3600.0
    }

    /// A primary post with no reaction, no claim and no tag, as the helper prints it (commands.md §21.6).
    fn post(repo: &str, number: u64) -> Value {
        json!({"repo": repo, "number": number, "ts": recent(), "ts_raw": "1.1", "author": "Ryan Case", "primary": true,
               "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false})
    }

    fn claimed(mut post: Value, eyes_me: Value) -> Value {
        post["reactions"] = json!(["eyes"]);
        post["claim"] = json!({"kind": "claimed", "reason": "claimed with :eyes: in Slack"});
        post["eyes_me"] = eyes_me;
        post
    }

    fn tagged(mut post: Value) -> Value {
        post["thread_tags_me"] = json!(true);
        post
    }

    fn write_cache(f: &Fixture, posts: &[Value]) {
        let path = f.home.run_dir().join("team-review.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let cache = json!({"version": 1, "polled_at": T, "channel": "C02JU6C2Z1D", "read": 10, "posts": posts});
        std::fs::write(path, cache.to_string()).unwrap();
    }

    fn open_pr() -> GhPr {
        GhPr { draft: false, state: GhState::Open, author: "ryan".to_owned(), decision: None, reviews: Vec::new() }
    }

    fn reviewed_by(login: &str) -> GhPr {
        GhPr { reviews: vec![Review { author: login.to_owned(), state: "COMMENTED".to_owned() }], ..open_pr() }
    }

    fn lookup(prs: &[(&str, u64, GhPr)]) -> Lookup {
        Lookup { me: ME_LOGIN.to_owned(), prs: prs.iter().map(|(repo, n, pr)| (((*repo).to_owned(), *n), Some(pr.clone()))).collect() }
    }

    fn team<'a>(github: &'a mut dyn FnMut(&[(String, u64)]) -> Result<Lookup, CpdError>) -> Team<'a> {
        static BOT_LIST: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| BOTS.map(str::to_owned).to_vec());
        Team { bot_reviewers: &BOT_LIST, github }
    }

    /// The `review-requested` conditions for the cache's `posts` and GitHub's `prs`.
    fn reviews(f: &mut Fixture, posts: &[Value], prs: &[(&str, u64, GhPr)]) -> Vec<Condition> {
        write_cache(f, posts);
        let lookup = lookup(prs);
        let all = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| Ok(lookup.clone())))).unwrap();
        all.into_iter().filter(|c| c.kind == Kind::ReviewRequested).collect()
    }

    #[test]
    fn a_post_with_no_review_opens_an_item_with_the_posts_pull_request() {
        let mut f = fixture("dispatch");
        let c = one(reviews(&mut f, &[post("acme/pay", 812)], &[("acme/pay", 812, open_pr())]));
        assert_eq!((c.urgency, c.subject.as_str(), c.label.as_str()), (Urgency::Yellow, "pr:acme/pay#812", "review pay#812"));
        assert_eq!(c.url.as_deref(), Some("https://github.com/acme/pay/pull/812"));
        assert_eq!((c.context, c.plan, c.session, c.job, c.record, c.event), (None, None, None, None, None, None));
    }

    /// One case per hermes rule: what GitHub says, what Slack says, and whether the item opens.
    #[test]
    fn each_hermes_rule_opens_the_item_or_not() {
        let changes = GhPr { decision: Some("CHANGES_REQUESTED".to_owned()), ..open_pr() };
        let approved = GhPr { decision: Some("APPROVED".to_owned()), ..open_pr() };
        let cases: Vec<(&str, Value, GhPr, bool)> = vec![
            ("no review yet", post("acme/pay", 1), open_pr(), true),
            ("AI review only", post("acme/pay", 1), reviewed_by("github-actions"), true),
            ("a human is reviewing", post("acme/pay", 1), reviewed_by("wes"), false),
            ("draft", post("acme/pay", 1), GhPr { draft: true, ..open_pr() }, false),
            ("the User's own PR", post("acme/pay", 1), GhPr { author: ME_LOGIN.to_owned(), ..open_pr() }, false),
            ("merged", post("acme/pay", 1), GhPr { state: GhState::Merged, ..open_pr() }, false),
            ("closed", post("acme/pay", 1), GhPr { state: GhState::Closed, ..open_pr() }, false),
            ("approved", post("acme/pay", 1), approved, false),
            ("changes requested", post("acme/pay", 1), changes, false),
            ("claimed by a peer's :eyes:", claimed(post("acme/pay", 1), json!(false)), open_pr(), false),
            ("claimed, attribution unavailable", claimed(post("acme/pay", 1), Value::Null), open_pr(), false),
            ("assigned to someone in Slack", {
                let mut p = post("acme/pay", 1);
                p["claim"] = json!({"kind": "claimed", "reason": "assigned to someone in Slack"});
                p
            }, open_pr(), false),
            ("the User's own :eyes: alone", claimed(post("acme/pay", 1), json!(true)), open_pr(), true),
            ("tagged in the thread of a claimed post", tagged(claimed(post("acme/pay", 1), json!(false))), open_pr(), true),
            ("tagged in the thread of a reviewed PR", tagged(post("acme/pay", 1)), reviewed_by("wes"), true),
            ("tagged under a merged PR", tagged(post("acme/pay", 1)), GhPr { state: GhState::Merged, ..open_pr() }, false),
            ("tagged under changes requested", tagged(post("acme/pay", 1)), GhPr { decision: Some("CHANGES_REQUESTED".to_owned()), ..open_pr() }, false),
            ("tagged under a draft", tagged(post("acme/pay", 1)), GhPr { draft: true, ..open_pr() }, false),
        ];
        let mut f = fixture("dispatch");
        for (rule, post, pr, opens) in cases {
            let found = reviews(&mut f, &[post], &[("acme/pay", 1, pr)]);
            assert_eq!(found.len(), usize::from(opens), "{rule}: {found:?}");
        }
    }

    #[test]
    fn a_github_review_request_alone_opens_nothing() {
        let mut f = fixture("dispatch");
        pull_request(&mut f, 5, "none", "unknown", "none");
        set(&mut f, 5, "authored = 0, review_requested = 1");
        assert!(reviews(&mut f, &[post("acme/pay", 1)], &[("acme/pay", 1, reviewed_by("wes"))]).is_empty(), "posts of other PRs");
        assert!(f.of(Kind::ReviewRequested).is_empty(), "no rule at all");
        std::fs::remove_file(f.home.run_dir().join("team-review.json")).unwrap();
        let none = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| panic!("no post, no call")))).unwrap();
        assert!(none.is_empty(), "no cache: {none:?}");
    }

    #[test]
    fn only_primary_posts_within_seven_days_are_read_and_asked_of_github() {
        let mut f = fixture("dispatch");
        let day = 24.0 * 3600.0;
        let old = |n, age: f64| {
            let mut p = post("acme/pay", n);
            p["ts"] = json!(NOW.unix_timestamp() as f64 - age);
            p
        };
        let mut mention = post("acme/pay", 3);
        mention["primary"] = json!(false);
        write_cache(&f, &[old(1, 6.9 * day), old(2, 7.1 * day), mention]);
        let mut asked = Vec::new();
        let answer = lookup(&[("acme/pay", 1, open_pr())]);
        let all = conditions_with(
            &mut f.store,
            &f.home,
            NOW,
            TEN,
            &[],
            Some(team(&mut |refs| {
                asked = refs.to_vec();
                Ok(answer.clone())
            })),
        )
        .unwrap();
        assert_eq!(asked, [("acme/pay".to_owned(), 1)]);
        assert_eq!(all.iter().filter(|c| c.kind == Kind::ReviewRequested).map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["pr:acme/pay#1"]);
    }

    #[test]
    fn with_the_rule_off_nothing_opens_and_open_items_close() {
        let mut f = fixture("dispatch");
        let opened = reviews(&mut f, &[post("acme/pay", 1)], &[("acme/pay", 1, open_pr())]);
        reconcile(&mut f.store, &opened, T).unwrap();
        let off = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], None).unwrap();
        assert!(off.is_empty());
        reconcile(&mut f.store, &off, T).unwrap();
        assert!(f.store.read(open_items).unwrap().is_empty());
    }

    /// The open items after reconciling `conditions` over them.
    fn open_subjects(f: &mut Fixture) -> Vec<String> {
        f.store.read(open_items).unwrap().into_iter().map(|i| i.subject).collect()
    }

    #[test]
    fn a_missing_cache_leaves_open_items_as_they_are_and_a_pull_request_that_leaves_it_closes_its_item() {
        let mut f = fixture("dispatch");
        let two = [post("acme/pay", 1), post("acme/pay", 2)];
        let both = [("acme/pay", 1, open_pr()), ("acme/pay", 2, open_pr())];
        let opened = reviews(&mut f, &two, &both);
        reconcile(&mut f.store, &opened, T).unwrap();
        assert_eq!(open_subjects(&mut f), ["pr:acme/pay#1", "pr:acme/pay#2"]);

        std::fs::remove_file(f.home.run_dir().join("team-review.json")).unwrap();
        let unknown = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| panic!("nothing to ask")))).unwrap();
        assert_eq!(unknown.len(), 2, "no file is unknown, not empty: {unknown:?}");
        reconcile(&mut f.store, &unknown, T).unwrap();
        assert_eq!(open_subjects(&mut f), ["pr:acme/pay#1", "pr:acme/pay#2"]);

        let rest = reviews(&mut f, &[post("acme/pay", 2)], &[("acme/pay", 2, open_pr())]);
        reconcile(&mut f.store, &rest, T).unwrap();
        assert_eq!(open_subjects(&mut f), ["pr:acme/pay#2"], "#1 left the file");
    }

    #[test]
    fn an_unreadable_cache_keeps_the_items_and_says_so() {
        let mut f = fixture("dispatch");
        let opened = reviews(&mut f, &[post("acme/pay", 1)], &[("acme/pay", 1, open_pr())]);
        reconcile(&mut f.store, &opened, T).unwrap();
        std::fs::write(f.home.run_dir().join("team-review.json"), "{ not json").unwrap();
        let all = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| panic!("unreadable")))).unwrap();
        assert_eq!(all.iter().filter(|c| c.kind == Kind::ReviewRequested).count(), 1);
        let error = all.iter().find(|c| c.kind == Kind::EngineError).expect("an engine-error condition");
        assert_eq!((error.urgency, error.subject.as_str(), error.label.as_str()), (Urgency::Red, "error:E8/team-review/store_failed", "team review: store_failed"));
        std::fs::write(f.home.run_dir().join("team-review.json"), r#"{"version": 2, "posts": []}"#).unwrap();
        let all = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| panic!("unreadable")))).unwrap();
        assert!(all.iter().any(|c| c.kind == Kind::EngineError), "another version is unreadable too");
    }

    #[test]
    fn a_github_that_fails_keeps_the_items_and_says_so() {
        let mut f = fixture("dispatch");
        let opened = reviews(&mut f, &[post("acme/pay", 1)], &[("acme/pay", 1, open_pr())]);
        reconcile(&mut f.store, &opened, T).unwrap();
        let all = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| Err(CpdError::new(ErrorCode::GhFailed, "down"))))).unwrap();
        assert_eq!(all.iter().filter(|c| c.kind == Kind::ReviewRequested).count(), 1, "kept");
        assert_eq!(all.iter().find(|c| c.kind == Kind::EngineError).map(|c| c.subject.as_str()), Some("error:E8/team-review/gh_failed"));
    }

    #[test]
    fn a_pull_request_github_cannot_read_is_dropped_and_all_unreadable_keeps_the_queue() {
        let mut f = fixture("dispatch");
        write_cache(&f, &[post("acme/pay", 1), post("acme/gone", 2)]);
        let mut answer = lookup(&[("acme/pay", 1, open_pr())]);
        answer.prs.insert(("acme/gone".to_owned(), 2), None);
        let all = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| Ok(answer.clone())))).unwrap();
        assert_eq!(all.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["pr:acme/pay#1"], "the readable one only, and no error");

        let opened = reviews(&mut f, &[post("acme/pay", 1)], &[("acme/pay", 1, open_pr())]);
        reconcile(&mut f.store, &opened, T).unwrap();
        let blind = Lookup { me: ME_LOGIN.to_owned(), prs: [(("acme/pay".to_owned(), 1), None)].into() };
        let all = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| Ok(blind.clone())))).unwrap();
        assert_eq!(all.iter().filter(|c| c.kind == Kind::ReviewRequested).count(), 1, "every PR unreadable: the queue stays");
        assert!(all.iter().any(|c| c.kind == Kind::EngineError));
    }

    // ---- all together ----

    #[test]
    fn conditions_come_in_the_order_of_the_kinds_and_a_quiet_store_has_none() {
        let mut f = fixture("dispatch");
        assert!(conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().is_empty());
        f.state("l-pay", "blocked", Some("Which?"));
        f.review("r1", "pending");
        f.reply("mgr");
        f.job("a", "failed");
        f.event("job-failed", None, Some("a"), T, r#"{}"#);
        record(&mut f, "ctx", "hs-q1", "question");
        let error = EngineError { step: "E8".into(), scope: "machine".into(), code: "x".into(), event: 1 };
        let kinds: Vec<_> =
            conditions(&mut f.store, &f.home, NOW, TEN, &[error]).unwrap().into_iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [Kind::Dialog, Kind::PlanReview, Kind::Unread, Kind::JobStuck, Kind::EngineError, Kind::Question]
        );
        pull_request(&mut f, 1, "failed", "mergeable", "changes_requested");
        pull_request(&mut f, 2, "passed", "mergeable", "approved");
        f.sql("INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored, review_requested)
               VALUES ('o/r', 3, 'u', 'T', 0, 'open', 'none', 'none', 't', 't', 0, 1)");
        let tail: Vec<_> = conditions(&mut f.store, &f.home, NOW, TEN, &[]).unwrap().into_iter().map(|c| c.kind).skip(5).collect();
        assert_eq!(tail, [Kind::PrFailing, Kind::PrChanges, Kind::PrReady]);
        write_cache(&f, &[post("acme/pay", 812)]);
        let lookup = lookup(&[("acme/pay", 812, open_pr())]);
        let kinds: Vec<_> = conditions_with(&mut f.store, &f.home, NOW, TEN, &[], Some(team(&mut |_| Ok(lookup.clone()))))
            .unwrap()
            .into_iter()
            .map(|c| c.kind)
            .collect();
        assert_eq!(kinds[5..], [Kind::PrFailing, Kind::PrChanges, Kind::PrReady, Kind::ReviewRequested]);
    }
}
