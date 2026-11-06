//! The team review rule of the Inbox kind `review-requested` (contract §2.2 "The team review rule", hs-d320; commands.md
//! §21.5): the posts of the `slack` source in `swr_slack_posts`, the GitHub state of their pull requests, and hermes' own
//! classification of the two (hermes-config `plugins/cockpit/classify.py` `classify`, `refresh.py` `build`). Only
//! `NEEDS_REVIEW` is the User's move; this module decides that, the rules in [`super::rules`] turn it into items.
//!
//! GitHub is read here, not in E11 (E11 calls no GitHub): one `gh api graphql` call for every primary post of the
//! window, memoised ([`GITHUB_TTL`]) so the five-second E8 tick does not repeat it.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::host::runner::Runner;
use crate::ports::Ports;
use crate::store::Store;
use rusqlite::OptionalExtension;

/// How long a post counts after its posting (hermes `refresh.RETAIN_DAYS`, 7; contract §2.2).
pub const WINDOW: Duration = Duration::from_secs(7 * 24 * 3600);

/// How long a GitHub answer is reused while the posts are the same. E8 runs every tick and the posts change at E11's
/// 30-minute cadence; thirty seconds keeps a review that lands on GitHub visible soon, at one GraphQL point per query.
pub const GITHUB_TTL: Duration = Duration::from_secs(30);

/// The deadline of the GitHub call (the pull request poll's, `pr::gh::POLL_TIMEOUT`).
const GITHUB_DEADLINE: Duration = Duration::from_secs(15);

/// The posts of the team channel as `swr_slack_posts` holds them (commands.md §21.5).
#[derive(Debug)]
pub struct Cache {
    /// The Slack channel id the posts were read from.
    pub channel: String,
    pub posts: Vec<Post>,
}

/// A post as the helper printed it (commands.md §21.6, `pi/lib/slack-parse.ts` `Post`).
#[derive(Clone, Debug, Deserialize)]
pub struct Post {
    pub repo: String,
    pub number: u64,
    /// Slack's timestamp, seconds.
    pub ts: f64,
    /// Slack's timestamp as text, the post's id in its channel.
    pub ts_raw: String,
    pub primary: bool,
    pub reactions: Vec<String>,
    /// `Some` when Slack shows someone took it (`slack_claim`).
    pub claim: Option<Claim>,
    /// Whether the User's own `:eyes:` is the only claim; `None` when unread.
    pub eyes_me: Option<bool>,
    pub thread_tags_me: bool,
}

/// What `slack_claim` found; only its presence matters here.
#[derive(Clone, Debug, Deserialize)]
pub struct Claim {
    pub kind: String,
    pub reason: String,
}

/// The cache of the team channel, read from `swr_slack_posts` (swr-slack.md §2): the primary rows the channel answer still
/// holds. `None` when the channel was never read (the `slack:channel:*` query has no data yet): "unknown", not "empty".
pub fn read_cache(store: &mut Store) -> Result<Option<Cache>, CpdError> {
    store.read(|conn| {
        let channel: Option<String> = conn
            .query_row(
                "SELECT substr(key, length('slack:channel:') + 1) FROM swr_queries
                  WHERE key LIKE 'slack:channel:%' AND data_hash IS NOT NULL ORDER BY key LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let Some(channel) = channel else {
            return Ok(None);
        };
        let mut stmt = conn.prepare(
            "SELECT repo, number, ts, primary_pr, reactions_json, claim_json, eyes_me, thread_tags_me
               FROM swr_slack_posts WHERE channel = ?1 AND inactive_at IS NULL ORDER BY ts DESC, repo, number",
        )?;
        let posts = stmt
            .query_map([&channel], |r| {
                let ts_raw: String = r.get(2)?;
                let reactions: String = r.get(4)?;
                let claim: Option<String> = r.get(5)?;
                Ok(Post {
                    repo: r.get(0)?,
                    number: r.get::<_, i64>(1)?.cast_unsigned(),
                    ts: ts_raw.parse().unwrap_or(0.0),
                    ts_raw,
                    primary: r.get::<_, i64>(3)? == 1,
                    reactions: serde_json::from_str::<serde_json::Map<String, Value>>(&reactions)
                        .map(|m| m.into_iter().map(|(k, _)| k).collect())
                        .unwrap_or_default(),
                    claim: claim.and_then(|c| serde_json::from_str(&c).ok()),
                    eyes_me: r.get::<_, Option<i64>>(6)?.map(|v| v == 1),
                    thread_tags_me: r.get::<_, Option<i64>>(7)? == Some(1),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Some(Cache { channel, posts }))
    })
}

impl Cache {
    /// The primary post of pull request `repo#number`: the newest by `ts`.
    pub fn primary_post(&self, repo: &str, number: u64) -> Option<&Post> {
        self.posts
            .iter()
            .filter(|p| p.primary && p.repo == repo && p.number == number)
            .max_by(|a, b| a.ts.total_cmp(&b.ts))
    }
}

/// A pull request's GitHub state, the fields hermes' `classify` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GhPr {
    /// The pull request's GitHub title, Row 2 of its `review-requested` item.
    pub title: String,
    pub draft: bool,
    pub state: GhState,
    pub author: String,
    /// `reviewDecision`: `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED` or none.
    pub decision: Option<String>,
    /// `latestReviews`: the last review of each reviewer.
    pub reviews: Vec<Review>,
    /// `headRefOid`: the pull request's head commit.
    pub head_oid: Option<String>,
    /// The state of the head commit's `statusCheckRollup`: `SUCCESS`, `PENDING`, `FAILURE`, ...
    pub checks_state: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GhState {
    Open,
    Merged,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub author: String,
    pub state: String,
    /// `commit { oid }`: the commit the review was made on.
    pub commit_oid: Option<String>,
}

/// What one GitHub call answered: the User's login (`ME`) and each pull request, `None` for one it cannot read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub me: String,
    pub workflows: BTreeMap<(String, u64), Option<GhPr>>,
}

/// hermes' statuses (`classify.py` `STATUS_*`); [`Status::Hidden`] is `None` there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The User's own pull request.
    Hidden,
    Draft,
    Done,
    ChangesRequested,
    InReview,
    /// The only status that opens an item, with hermes' reason.
    NeedsReview(&'static str),
}

pub const NO_REVIEW: &str = "no review yet";
pub const AI_ONLY: &str = "AI review only";
pub const CLAIMED_BY_ME: &str = "you claimed this — approve or request changes";
pub const FOLLOW_UP: &str = "changes you requested were pushed — review again";
pub const TAGGED: &str = "you were tagged in the thread";

/// `classify.classify`: the status from GitHub alone. `me` is the User's GitHub login, `bots` `inbox.bot_reviewers`.
pub fn classify(workflow: &GhPr, me: &str, bots: &[String]) -> Status {
    if workflow.draft {
        return Status::Draft;
    }
    if workflow.author == me {
        return Status::Hidden;
    }
    if workflow.state != GhState::Open {
        return Status::Done;
    }
    if followup_oids(workflow, me).is_some() {
        return Status::NeedsReview(FOLLOW_UP);
    }
    match workflow.decision.as_deref() {
        Some("APPROVED") => return Status::Done,
        Some("CHANGES_REQUESTED") => return Status::ChangesRequested,
        _ => {}
    }
    let is_bot = |login: &str| bots.iter().any(|b| b == login);
    if workflow
        .reviews
        .iter()
        .any(|r| !is_bot(&r.author) && r.author != me)
    {
        return Status::InReview;
    }
    if workflow.reviews.iter().any(|r| is_bot(&r.author)) {
        return Status::NeedsReview(AI_ONLY);
    }
    Status::NeedsReview(NO_REVIEW)
}

/// The follow-up condition: an open pull request whose head moved past the commit of the User's latest review, a
/// CHANGES_REQUESTED, with the head's checks exactly `SUCCESS`. The head and the reviewed commit.
pub fn followup_oids<'a>(workflow: &'a GhPr, me: &str) -> Option<(&'a str, &'a str)> {
    if workflow.draft
        || workflow.state != GhState::Open
        || workflow.checks_state.as_deref() != Some("SUCCESS")
    {
        return None;
    }
    let reviewed = workflow
        .reviews
        .iter()
        .find(|r| r.author == me && r.state == "CHANGES_REQUESTED")?
        .commit_oid
        .as_deref()?;
    let head = workflow.head_oid.as_deref()?;
    (head != reviewed).then_some((head, reviewed))
}

/// `classify` and then the Slack overrides of `refresh.build`: a claim in Slack makes `NEEDS_REVIEW` `IN_REVIEW`,
/// unless the User's own `:eyes:` is the only claim; a mention of the User in the thread makes `IN_REVIEW` or
/// `NEEDS_REVIEW` `NEEDS_REVIEW` again.
pub fn status(post: &Post, workflow: &GhPr, me: &str, bots: &[String]) -> Status {
    let mut status = classify(workflow, me, bots);
    if status == Status::NeedsReview(FOLLOW_UP) {
        return status;
    }
    if matches!(status, Status::NeedsReview(_)) && post.claim.is_some() {
        status = if post.eyes_me == Some(true) && !post.reactions.is_empty() {
            Status::NeedsReview(CLAIMED_BY_ME)
        } else {
            Status::InReview
        };
    }
    if matches!(status, Status::InReview | Status::NeedsReview(_)) && post.thread_tags_me {
        status = Status::NeedsReview(TAGGED);
    }
    status
}

// ---- GitHub ------------------------------------------------------------------------------------------------------

/// `owner/name` with each part of `[A-Za-z0-9_.-]+`: nothing else reaches a query text.
fn repo_parts(repo: &str) -> Option<(&str, &str)> {
    let part = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    repo.split_once('/')
        .filter(|(owner, name)| part(owner) && part(name))
}

fn gh_failed(message: String, stderr: &str) -> CpdError {
    CpdError::new(ErrorCode::GhFailed, message).with(
        "stderr",
        stderr
            .lines()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The one `gh api graphql` call: `viewer { login }` and one alias `p<i>` per pull request, 15 s at most. A pull
/// request that GitHub cannot find (or whose repo is no `owner/name`) is `None`; any other error of the call, or
/// an answer without `data`, is `gh_failed`, never "no pull requests". An empty `refs` makes no call.
pub fn github_lookup(home: &CpdHome, refs: &[(String, u64)]) -> Result<Lookup, CpdError> {
    github_lookup_with(&*Ports::current().runner, home, refs)
}

/// [`github_lookup`] on `runner`.
pub fn github_lookup_with(
    runner: &dyn Runner,
    home: &CpdHome,
    refs: &[(String, u64)],
) -> Result<Lookup, CpdError> {
    let mut query = String::from("query { viewer { login }");
    for (i, (repo, number)) in refs.iter().enumerate() {
        let Some((owner, name)) = repo_parts(repo) else {
            continue;
        };
        query.push_str(&format!(
            " p{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ pullRequest(number: {number}) {{ title isDraft state reviewDecision headRefOid commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }} author {{ login }} latestReviews(first: 50) {{ nodes {{ state author {{ login }} commit {{ oid }} }} }} }} }}"
        ));
    }
    query.push_str(" }");
    let mut cmd = Command::new("gh");
    cmd.args(["api", "graphql", "-f", &format!("query={query}")])
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .current_dir(home.root())
        .stdin(std::process::Stdio::null());
    let out = match runner.output(&mut cmd, "gh", GITHUB_DEADLINE) {
        Ok(out) => out,
        Err(Failed::Io(e)) => {
            return Err(gh_failed(format!("Cannot run gh: {e}."), &e.to_string()));
        }
        Err(Failed::Timeout(e)) => return Err(e),
    };
    // `gh` exits 1 when an alias is NOT_FOUND, with data: the stdout decides, whatever the exit.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let reply: Value = serde_json::from_str(&stdout).map_err(|e| {
        gh_failed(
            format!("gh api graphql printed unexpected JSON: {e}."),
            &stderr,
        )
    })?;
    parse_lookup(&reply, refs).map_err(|message| gh_failed(message, &stderr))
}

/// The reply of [`github_lookup`]; `Err` is the message of its `gh_failed`.
pub fn parse_lookup(reply: &Value, refs: &[(String, u64)]) -> Result<Lookup, String> {
    let errors = reply["errors"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    if let Some(other) = errors
        .iter()
        .find(|e| e["type"].as_str() != Some("NOT_FOUND"))
    {
        return Err(format!(
            "gh api graphql answered with errors: {}",
            other["message"].as_str().unwrap_or("(no message)")
        ));
    }
    let data = reply["data"]
        .as_object()
        .ok_or("gh api graphql answered without data.")?;
    let me = data["viewer"]["login"]
        .as_str()
        .filter(|l| !l.is_empty())
        .ok_or("gh api graphql answered without viewer.login.")?
        .to_owned();
    let mut workflows = BTreeMap::new();
    for (i, key) in refs.iter().enumerate() {
        let raw = &data
            .get(&format!("p{i}"))
            .map_or(&Value::Null, |v| &v["pullRequest"]);
        let workflow = if raw.is_null() {
            None
        } else {
            Some(parse_pr(raw)?)
        };
        workflows.insert(key.clone(), workflow);
    }
    Ok(Lookup { me, workflows })
}

fn parse_pr(raw: &Value) -> Result<GhPr, String> {
    let state = match raw["state"].as_str() {
        Some("OPEN") => GhState::Open,
        Some("MERGED") => GhState::Merged,
        Some("CLOSED") => GhState::Closed,
        other => {
            return Err(format!(
                "gh api graphql answered a pull request state {other:?}."
            ));
        }
    };
    let login = |v: &Value| v["author"]["login"].as_str().unwrap_or_default().to_owned();
    let reviews = raw["latestReviews"]["nodes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|r| Review {
            author: login(r),
            state: r["state"].as_str().unwrap_or_default().to_owned(),
            commit_oid: r["commit"]["oid"].as_str().map(str::to_owned),
        })
        .collect();
    Ok(GhPr {
        title: raw["title"]
            .as_str()
            .ok_or("gh api graphql answered a pull request without title.")?
            .to_owned(),
        draft: raw["isDraft"]
            .as_bool()
            .ok_or("gh api graphql answered a pull request without isDraft.")?,
        state,
        author: login(raw),
        decision: raw["reviewDecision"].as_str().map(str::to_owned),
        reviews,
        head_oid: raw["headRefOid"].as_str().map(str::to_owned),
        checks_state: raw["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"]
            .as_str()
            .map(str::to_owned),
    })
}

/// The last GitHub answer and when it came, so E8's tick does not ask again for the same posts within [`GITHUB_TTL`].
/// A failure is held as well: a broken `gh` is asked again after the same time, not every tick.
/// The posted pull requests asked, when, and the answer.
type Held = (
    Vec<(String, u64)>,
    String,
    Instant,
    Result<Lookup, CpdError>,
);

#[derive(Default)]
pub struct Memo {
    held: Option<Held>,
}

impl Memo {
    /// The held answer when `refs` are the same and it is younger than [`GITHUB_TTL`] and the store's
    /// invalidation `token` is unchanged, else `fetch`'s.
    pub fn get(
        &mut self,
        refs: &[(String, u64)],
        token: &str,
        now: Instant,
        fetch: impl FnOnce(&[(String, u64)]) -> Result<Lookup, CpdError>,
    ) -> Result<Lookup, CpdError> {
        if let Some((held, held_token, at, answer)) = &self.held
            && held == refs
            && held_token == token
            && now.saturating_duration_since(*at) < GITHUB_TTL
        {
            return answer.clone();
        }
        let answer = fetch(refs);
        self.held = Some((refs.to_vec(), token.to_owned(), now, answer.clone()));
        answer
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ME: &str = "clayton-duarte";

    fn bots() -> Vec<String> {
        vec![
            "mission-lane-claude-code-assistant".to_owned(),
            "github-actions".to_owned(),
        ]
    }

    fn workflow() -> GhPr {
        GhPr {
            title: "Add retries".to_owned(),
            draft: false,
            state: GhState::Open,
            author: "ryan".to_owned(),
            decision: None,
            reviews: Vec::new(),
            head_oid: None,
            checks_state: None,
        }
    }

    fn review(author: &str, state: &str) -> Review {
        Review {
            author: author.to_owned(),
            state: state.to_owned(),
            commit_oid: None,
        }
    }

    fn post() -> Post {
        Post {
            repo: "acme/pay".to_owned(),
            number: 1,
            ts: 0.0,
            ts_raw: String::new(),
            primary: true,
            reactions: Vec::new(),
            claim: None,
            eyes_me: None,
            thread_tags_me: false,
        }
    }

    fn claimed(mut p: Post, eyes_me: Option<bool>) -> Post {
        p.reactions = vec!["eyes".to_owned()];
        p.claim = Some(Claim {
            kind: "claimed".to_owned(),
            reason: "claimed with :eyes: in Slack".to_owned(),
        });
        p.eyes_me = eyes_me;
        p
    }

    // ---- classify ----

    #[test]
    fn no_review_and_a_bot_only_review_need_review() {
        assert_eq!(
            classify(&workflow(), ME, &bots()),
            Status::NeedsReview("no review yet")
        );
        let bot = GhPr {
            reviews: vec![review("github-actions", "COMMENTED")],
            ..workflow()
        };
        assert_eq!(
            classify(&bot, ME, &bots()),
            Status::NeedsReview("AI review only")
        );
        let own_review_only = GhPr {
            reviews: vec![review(ME, "COMMENTED")],
            ..workflow()
        };
        assert_eq!(
            classify(&own_review_only, ME, &bots()),
            Status::NeedsReview("no review yet"),
            "the User's own review is no human's"
        );
    }

    #[test]
    fn a_human_reviewer_is_in_review_even_beside_a_bot() {
        let human = GhPr {
            reviews: vec![
                review("github-actions", "COMMENTED"),
                review("wes", "COMMENTED"),
            ],
            ..workflow()
        };
        assert_eq!(classify(&human, ME, &bots()), Status::InReview);
    }

    #[test]
    fn a_draft_is_a_draft_whoever_wrote_it() {
        assert_eq!(
            classify(
                &GhPr {
                    draft: true,
                    ..workflow()
                },
                ME,
                &bots()
            ),
            Status::Draft
        );
        assert_eq!(
            classify(
                &GhPr {
                    draft: true,
                    author: ME.to_owned(),
                    ..workflow()
                },
                ME,
                &bots()
            ),
            Status::Draft
        );
    }

    #[test]
    fn the_users_own_pull_request_is_hidden() {
        assert_eq!(
            classify(
                &GhPr {
                    author: ME.to_owned(),
                    ..workflow()
                },
                ME,
                &bots()
            ),
            Status::Hidden
        );
    }

    #[test]
    fn merged_closed_and_approved_are_done() {
        for state in [GhState::Merged, GhState::Closed] {
            assert_eq!(
                classify(
                    &GhPr {
                        state,
                        ..workflow()
                    },
                    ME,
                    &bots()
                ),
                Status::Done
            );
        }
        assert_eq!(
            classify(
                &GhPr {
                    decision: Some("APPROVED".to_owned()),
                    ..workflow()
                },
                ME,
                &bots()
            ),
            Status::Done
        );
        assert_eq!(
            classify(
                &GhPr {
                    decision: Some("REVIEW_REQUIRED".to_owned()),
                    ..workflow()
                },
                ME,
                &bots()
            ),
            Status::NeedsReview("no review yet")
        );
    }

    #[test]
    fn changes_requested_by_anyone_is_the_authors_move() {
        let theirs = GhPr {
            decision: Some("CHANGES_REQUESTED".to_owned()),
            reviews: vec![review("wes", "CHANGES_REQUESTED")],
            ..workflow()
        };
        assert_eq!(classify(&theirs, ME, &bots()), Status::ChangesRequested);
        let mine = GhPr {
            decision: Some("CHANGES_REQUESTED".to_owned()),
            reviews: vec![review(ME, "CHANGES_REQUESTED")],
            ..workflow()
        };
        assert_eq!(classify(&mine, ME, &bots()), Status::ChangesRequested);
    }

    // ---- the Slack overrides ----

    #[test]
    fn a_claim_in_slack_makes_needs_review_in_review() {
        assert_eq!(
            status(&claimed(post(), Some(false)), &workflow(), ME, &bots()),
            Status::InReview
        );
        assert_eq!(
            status(&claimed(post(), None), &workflow(), ME, &bots()),
            Status::InReview,
            "attribution unavailable: a peer's"
        );
        let mention_only = Post {
            claim: Some(Claim {
                kind: "claimed".to_owned(),
                reason: "assigned to someone in Slack".to_owned(),
            }),
            ..post()
        };
        assert_eq!(
            status(&mention_only, &workflow(), ME, &bots()),
            Status::InReview
        );
        assert_eq!(
            status(&post(), &workflow(), ME, &bots()),
            Status::NeedsReview("no review yet"),
            "no claim"
        );
    }

    #[test]
    fn the_users_own_eyes_alone_keeps_it_needing_review() {
        assert_eq!(
            status(&claimed(post(), Some(true)), &workflow(), ME, &bots()),
            Status::NeedsReview(CLAIMED_BY_ME)
        );
    }

    #[test]
    fn a_tag_in_the_thread_makes_in_review_and_needs_review_need_review() {
        let tagged = Post {
            thread_tags_me: true,
            ..post()
        };
        assert_eq!(
            status(
                &Post {
                    thread_tags_me: true,
                    ..claimed(post(), Some(false))
                },
                &workflow(),
                ME,
                &bots()
            ),
            Status::NeedsReview(TAGGED)
        );
        let human = GhPr {
            reviews: vec![review("wes", "COMMENTED")],
            ..workflow()
        };
        assert_eq!(
            status(&tagged, &human, ME, &bots()),
            Status::NeedsReview(TAGGED)
        );
        assert_eq!(
            status(&tagged, &workflow(), ME, &bots()),
            Status::NeedsReview(TAGGED)
        );
    }

    #[test]
    fn a_tag_does_not_revive_finished_or_hidden_work() {
        let tagged = Post {
            thread_tags_me: true,
            ..post()
        };
        for (workflow, want) in [
            (
                GhPr {
                    state: GhState::Merged,
                    ..workflow()
                },
                Status::Done,
            ),
            (
                GhPr {
                    decision: Some("APPROVED".to_owned()),
                    ..workflow()
                },
                Status::Done,
            ),
            (
                GhPr {
                    decision: Some("CHANGES_REQUESTED".to_owned()),
                    ..workflow()
                },
                Status::ChangesRequested,
            ),
            (
                GhPr {
                    draft: true,
                    ..workflow()
                },
                Status::Draft,
            ),
            (
                GhPr {
                    author: ME.to_owned(),
                    ..workflow()
                },
                Status::Hidden,
            ),
        ] {
            assert_eq!(status(&tagged, &workflow, ME, &bots()), want);
        }
    }

    // ---- the GitHub reply ----

    fn reply() -> Value {
        json!({"data": {
            "viewer": {"login": ME},
            "p0": {"pullRequest": {"title": "Add retries", "isDraft": false, "state": "OPEN", "reviewDecision": null, "author": {"login": "ryan"},
                   "latestReviews": {"nodes": [{"state": "COMMENTED", "author": {"login": "github-actions"}}]}}},
            "p1": null
        }, "errors": [{"type": "NOT_FOUND", "path": ["p1"], "message": "Could not resolve"}]})
    }

    #[test]
    fn a_reply_is_the_users_login_and_each_pull_request_or_none_when_not_found() {
        let refs = [("acme/pay".to_owned(), 1), ("acme/gone".to_owned(), 2)];
        let lookup = parse_lookup(&reply(), &refs).unwrap();
        assert_eq!(lookup.me, ME);
        assert_eq!(
            lookup.workflows[&refs[0]],
            Some(GhPr {
                reviews: vec![review("github-actions", "COMMENTED")],
                ..workflow()
            })
        );
        assert_eq!(lookup.workflows[&refs[1]], None);
    }

    #[test]
    fn a_reply_with_another_error_or_no_data_is_a_failure_never_an_empty_answer() {
        let refs = [("acme/pay".to_owned(), 1)];
        let mut other = reply();
        other["errors"] = json!([{"type": "RATE_LIMITED", "message": "slow down"}]);
        assert!(
            parse_lookup(&other, &refs)
                .unwrap_err()
                .contains("slow down")
        );
        assert!(
            parse_lookup(&json!({"errors": []}), &refs)
                .unwrap_err()
                .contains("without data")
        );
        let mut state = reply();
        state["data"]["p0"]["pullRequest"]["state"] = json!("WEIRD");
        assert!(parse_lookup(&state, &refs).unwrap_err().contains("state"));
    }

    // ---- the memo ----

    #[test]
    fn the_memo_asks_again_only_for_other_posts_or_after_the_ttl() {
        let (a, b) = (
            vec![("acme/pay".to_owned(), 1)],
            vec![("acme/pay".to_owned(), 2)],
        );
        let lookup = Lookup {
            me: ME.to_owned(),
            workflows: BTreeMap::new(),
        };
        let mut calls = 0;
        let mut memo = Memo::default();
        let t0 = Instant::now();
        let mut ask = |memo: &mut Memo, refs: &[(String, u64)], at: Instant| {
            memo.get(refs, "", at, |_| {
                calls += 1;
                Ok(lookup.clone())
            })
            .unwrap();
        };
        ask(&mut memo, &a, t0);
        ask(&mut memo, &a, t0 + Duration::from_secs(29));
        ask(&mut memo, &b, t0 + Duration::from_secs(29));
        ask(&mut memo, &b, t0 + Duration::from_secs(29 + 30));
        assert_eq!(calls, 3, "first, other posts, and past the ttl");
    }

    #[test]
    fn the_memo_holds_a_failure_too() {
        let a = vec![("acme/pay".to_owned(), 1)];
        let mut memo = Memo::default();
        let t0 = Instant::now();
        let mut calls = 0;
        for _ in 0..3 {
            let err = memo
                .get(&a, "", t0, |_| {
                    calls += 1;
                    Err(CpdError::new(ErrorCode::GhFailed, "down"))
                })
                .unwrap_err();
            assert_eq!(err.code(), ErrorCode::GhFailed);
        }
        assert_eq!(calls, 1);
    }
}

/// Seeds the `slack` source's rows for a test, from posts in the helper's older shape (`repo`, `number`, `ts`, `ts_raw`,
/// `primary`, `reactions` names, `claim`, `eyes_me`, `thread_tags_me`): the channel's query has data and one row per post.
/// The row's `ts` is the post's `ts_raw` when that is the post's time, else the time itself (a row has no separate number).
#[cfg(test)]
pub(crate) fn seed_cache(store: &mut Store, channel: &str, posts: &[Value]) {
    store
        .write("seed", |tx| {
            tx.execute("DELETE FROM swr_slack_posts", [])?;
            tx.execute("DELETE FROM swr_queries WHERE key LIKE 'slack:channel:%'", [])?;
            tx.execute(
                "INSERT INTO swr_queries (key, system, status, data_hash) VALUES (?1, 'slack', 'success', 'h')",
                [format!("slack:channel:{channel}")],
            )?;
            for p in posts {
                let time = p["ts"].as_f64().unwrap();
                let ts = match p["ts_raw"].as_str() {
                    Some(raw) if raw.parse::<f64>().is_ok_and(|v| (v - time).abs() < 1.0) => raw.to_owned(),
                    _ => format!("{time:.6}"),
                };
                let reactions: serde_json::Map<String, Value> = p["reactions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| (n.as_str().unwrap().to_owned(), Value::from(1)))
                    .collect();
                let flag = |v: &Value| v.as_bool().map(i64::from);
                tx.execute(
                    "INSERT INTO swr_slack_posts (channel, ts, repo, number, primary_pr, author_id, author, author_is_me,
                       reactions_json, claim_json, eyes_me, thread_tags_me, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, '', ?6, 0, ?7, ?8, ?9, ?10, 't')",
                    rusqlite::params![
                        channel,
                        ts,
                        p["repo"].as_str().unwrap(),
                        p["number"].as_i64().unwrap(),
                        i64::from(p["primary"].as_bool().unwrap_or(true)),
                        p["author"].as_str().unwrap_or(""),
                        Value::Object(reactions).to_string(),
                        (!p["claim"].is_null()).then(|| p["claim"].to_string()),
                        flag(&p["eyes_me"]),
                        flag(&p["thread_tags_me"]),
                    ],
                )?;
            }
            Ok(())
        })
        .unwrap();
}
