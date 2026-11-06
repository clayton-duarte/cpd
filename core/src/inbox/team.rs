//! The team review rule of the Inbox kind `review-requested` (contract §2.2 "The team review rule", hs-d320; commands.md
//! §21.5): the posts of the `slack` source in `swr_slack_posts`, the GitHub state of their pull requests, and hermes' own
//! classification of the two (hermes-config `plugins/cockpit/classify.py` `classify`, `refresh.py` `build`). Only
//! `NEEDS_REVIEW` is the User's move; this module decides that, the rules in [`super::rules`] turn it into items.
//!
//! Nothing here calls GitHub or reads a file: the GitHub state is the SWR cache's effective rows (`swr::github`), which the
//! one pass wrote, and the posts are the `slack` source's rows.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::error::CpdError;
use crate::store::Store;
use rusqlite::OptionalExtension;

/// How long a post counts after its posting (hermes `refresh.RETAIN_DAYS`, 7; contract §2.2).
pub const WINDOW: Duration = Duration::from_secs(7 * 24 * 3600);

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

// ---- the cache ---------------------------------------------------------------------------------------------------

/// The state of each of `refs` as a reader of the cache sees it (the effective row of `swr::github`, its detail for the
/// reviews and the head's checks), and the User's login (`swr_viewer`; empty until the pass has read it, when no pull request
/// is the User's own). A pull request with no row is `None` in the map.
pub fn lookup_from_store(store: &mut Store, refs: &[(String, u64)]) -> Result<Lookup, CpdError> {
    store.read(|conn| {
        let me = conn
            .query_row("SELECT login FROM swr_viewer WHERE id = 1", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .unwrap_or_default();
        let mut workflows = BTreeMap::new();
        for (repo, number) in refs {
            let found = crate::swr::github::effective_in(conn, repo, *number as i64)?;
            workflows.insert((repo.clone(), *number), found.map(|e| gh_pr(&e, &me)));
        }
        Ok(Lookup { me, workflows })
    })
}

fn gh_pr(e: &crate::swr::github::EffectivePr, me: &str) -> GhPr {
    let detail = e.detail.as_ref();
    let reviews = detail
        .and_then(|d| d["latestReviews"]["nodes"].as_array())
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|r| Review {
            author: r["author"]["login"].as_str().unwrap_or_default().to_owned(),
            state: r["state"].as_str().unwrap_or_default().to_owned(),
            commit_oid: r["commit"]["oid"].as_str().map(str::to_owned),
        })
        .collect();
    let checks_state = detail
        .and_then(|d| d["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"].as_str())
        .map(str::to_owned)
        .or_else(|| match e.checks.as_str() {
            "passed" => Some("SUCCESS".to_owned()),
            "failed" => Some("FAILURE".to_owned()),
            "pending" => Some("PENDING".to_owned()),
            _ => None,
        });
    GhPr {
        title: e.title.clone(),
        draft: e.row.draft,
        state: match e.state.as_str() {
            "open" => GhState::Open,
            "closed" => GhState::Closed,
            _ => GhState::Merged,
        },
        author: if e.row.authored && !me.is_empty() {
            me.to_owned()
        } else {
            "someone".to_owned()
        },
        decision: match e.review_decision.as_str() {
            "approved" => Some("APPROVED".to_owned()),
            "changes_requested" => Some("CHANGES_REQUESTED".to_owned()),
            "required" => Some("REVIEW_REQUIRED".to_owned()),
            _ => None,
        },
        reviews,
        head_oid: e.head_oid.clone(),
        checks_state,
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

#[cfg(test)]
mod tests {
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
}
