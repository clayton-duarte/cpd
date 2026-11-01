//! The feedback items of a pull request (what a person said and nobody answered yet), derived from GitHub's
//! GraphQL answer (contract §2.2 "Feedback item", §7.6 step 3a). Pure: no I/O.

use serde::{Deserialize, Serialize};

use crate::error::{CpdError, ErrorCode};

/// The last line of every reply `cpd pr reply` posts.
pub const MARKER: &str = "<!-- cpd:reply -->";

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Thread,
    Comment,
    Review,
}

/// One open thing a person said on the pull request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub last: String,
    pub author: String,
    pub bot: bool,
    pub path: Option<String>,
    pub outdated: bool,
    pub url: String,
    pub excerpt: String,
}

/// One pull request as the comments read saw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Read {
    pub repo: String,
    pub number: i64,
    pub updated_at: String,
    pub items: Vec<Item>,
}

/// The most any connection of the read holds: `first: 100` and `last: 100`. More is `gh_failed`.
const PAGE: u32 = 100;

/// The fields of one connection page: its size and whether more exist.
const PAGE_INFO: &str = "totalCount pageInfo { hasPreviousPage hasNextPage }";

/// The fields of one comment.
const COMMENT: &str = "id author { __typename login } body url createdAt";

/// The GraphQL text reading `prs` (`owner/name`, number pairs): alias `p<i>` for the i-th. The caller has checked
/// that each repo is `owner/name`.
pub fn query(workflows: &[(&str, i64)]) -> String {
    let mut text = String::from("query { viewer { login } rateLimit { cost remaining resetAt }");
    for (i, (repo, number)) in workflows.iter().enumerate() {
        let (owner, name) = repo.split_once('/').unwrap_or((repo, ""));
        text.push_str(&format!(
            " p{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ pullRequest(number: {number}) {{ id updatedAt \
             reviewThreads(first: {PAGE}) {{ {PAGE_INFO} nodes {{ id isResolved isOutdated path comments(first: {PAGE}) {{ {PAGE_INFO} nodes {{ {COMMENT} }} }} }} }} \
             comments(last: {PAGE}) {{ {PAGE_INFO} nodes {{ {COMMENT} }} }} \
             reviews(last: {PAGE}) {{ {PAGE_INFO} nodes {{ id state body author {{ __typename login }} url createdAt }} }} }} }}"
        ));
    }
    text.push_str(" }");
    text
}

#[derive(Deserialize)]
struct Answer {
    data: Option<Data>,
    errors: Option<Vec<AnswerError>>,
}

#[derive(Deserialize)]
struct AnswerError {
    message: Option<String>,
}

#[derive(Deserialize)]
struct Data {
    viewer: Login,
    #[serde(flatten)]
    pulls: std::collections::BTreeMap<String, Option<Repository>>,
}

#[derive(Deserialize)]
struct Repository {
    #[serde(rename = "pullRequest")]
    pull_request: Option<Pull>,
}

#[derive(Deserialize)]
struct Login {
    login: String,
    #[serde(rename = "__typename", default)]
    kind: String,
}

#[derive(Deserialize)]
struct Pull {
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(rename = "reviewThreads")]
    threads: Connection<Thread>,
    comments: Connection<Comment>,
    reviews: Connection<Review>,
}

#[derive(Deserialize)]
struct Connection<T> {
    #[serde(rename = "totalCount")]
    total: u32,
    #[serde(rename = "pageInfo")]
    page: PageInfo,
    nodes: Vec<T>,
}

#[derive(Deserialize)]
struct PageInfo {
    #[serde(rename = "hasPreviousPage")]
    previous: bool,
    #[serde(rename = "hasNextPage")]
    next: bool,
}

impl<T> Connection<T> {
    /// Whether the page holds the whole connection.
    fn whole(&self) -> bool {
        self.total <= PAGE && !self.page.previous && !self.page.next
    }
}

#[derive(Deserialize)]
struct Thread {
    id: String,
    #[serde(rename = "isResolved")]
    resolved: bool,
    #[serde(rename = "isOutdated")]
    outdated: bool,
    path: Option<String>,
    comments: Connection<Comment>,
}

#[derive(Deserialize)]
struct Comment {
    id: String,
    author: Option<Login>,
    body: String,
    url: String,
    #[serde(rename = "createdAt")]
    created_at: String,
}

#[derive(Deserialize)]
struct Review {
    id: String,
    state: String,
    body: String,
    author: Option<Login>,
    url: String,
    #[serde(rename = "createdAt")]
    created_at: String,
}

/// A bot is an author typed `Bot`, whose login ends `[bot]`, or whose login is listed in `bots`.
fn author_is_bot(author: &Option<Login>, bots: &[String]) -> bool {
    author
        .as_ref()
        .is_some_and(|a| a.kind == "Bot" || super::status::is_bot(&a.login, bots))
}

/// The login GitHub shows for an author that no longer exists.
const GHOST: &str = "ghost";

fn login(author: &Option<Login>) -> &str {
    author.as_ref().map_or(GHOST, |a| a.login.as_str())
}

/// Whether the body ends with the marker line.
fn marked(body: &str) -> bool {
    body.trim_end()
        .lines()
        .next_back()
        .is_some_and(|line| line.trim() == MARKER)
}

/// The first 160 characters of `body`, whitespace collapsed.
fn excerpt(body: &str) -> String {
    body.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

fn failed(repo: &str, number: i64, why: &str) -> CpdError {
    CpdError::new(
        ErrorCode::GhFailed,
        format!("The comments read of {repo}#{number} {why}."),
    )
    .with("repo", repo)
    .with("number", number)
}

/// The reads in `answer`, alias `p<i>` for the i-th pair of `prs`. An `errors` array, a missing alias, field or pull
/// request, and more than 100 threads, comments or reviews (or comments of a thread) are `gh_failed`.
pub fn parse(workflows: &[(&str, i64)], answer: &str, bots: &[String]) -> Result<Vec<Read>, CpdError> {
    let gh = |message: String| CpdError::new(ErrorCode::GhFailed, message);
    let answer: Answer = serde_json::from_str(answer)
        .map_err(|e| gh(format!("The comments read printed unexpected JSON: {e}.")))?;
    if let Some(first) = answer.errors.iter().flatten().next() {
        return Err(gh(format!(
            "The comments read answered with errors: {}",
            first.message.as_deref().unwrap_or("(no message)")
        )));
    }
    let mut data = answer
        .data
        .ok_or_else(|| gh("The comments read answered without data.".to_owned()))?;
    let me = data.viewer.login.clone();
    let mut reads = Vec::with_capacity(workflows.len());
    for (i, (repo, number)) in workflows.iter().enumerate() {
        let (repo, number) = (*repo, *number);
        let pull = data
            .pulls
            .remove(&format!("p{i}"))
            .ok_or_else(|| failed(repo, number, &format!("is missing: no alias p{i}")))?
            .and_then(|r| r.pull_request)
            .ok_or_else(|| failed(repo, number, "found no pull request"))?;
        let whole = pull.threads.whole()
            && pull.comments.whole()
            && pull.reviews.whole()
            && pull.threads.nodes.iter().all(|t| t.comments.whole());
        if !whole {
            return Err(failed(
                repo,
                number,
                "has more than 100 review threads, comments, reviews or comments in a thread: it cannot be read whole",
            ));
        }
        let mut items = Vec::new();
        for thread in &pull.threads.nodes {
            let (Some(first), Some(last)) =
                (thread.comments.nodes.first(), thread.comments.nodes.last())
            else {
                continue;
            };
            if thread.resolved || marked(&last.body) {
                continue;
            }
            let author = login(&first.author);
            items.push(Item {
                id: thread.id.clone(),
                kind: Kind::Thread,
                last: last.id.clone(),
                author: author.to_owned(),
                bot: author_is_bot(&first.author, bots),
                path: thread.path.clone(),
                outdated: thread.outdated,
                url: last.url.clone(),
                excerpt: excerpt(&last.body),
            });
        }
        // An issue comment or review is answered by a marked issue comment made after it.
        let answered_after = |at: &str, position: Option<usize>| {
            pull.comments.nodes.iter().enumerate().any(|(j, c)| {
                marked(&c.body)
                    && (c.created_at.as_str() > at
                        || (c.created_at == at && position.is_some_and(|i| j > i)))
            })
        };
        let spoken = |author: &Option<Login>| login(author) != me && !author_is_bot(author, bots);
        for (i, comment) in pull.comments.nodes.iter().enumerate() {
            let author = login(&comment.author);
            if !spoken(&comment.author)
                || marked(&comment.body)
                || answered_after(&comment.created_at, Some(i))
            {
                continue;
            }
            items.push(Item {
                id: comment.id.clone(),
                kind: Kind::Comment,
                last: comment.id.clone(),
                author: author.to_owned(),
                bot: false,
                path: None,
                outdated: false,
                url: comment.url.clone(),
                excerpt: excerpt(&comment.body),
            });
        }
        for review in &pull.reviews.nodes {
            let author = login(&review.author);
            let said = matches!(review.state.as_str(), "COMMENTED" | "CHANGES_REQUESTED")
                && !review.body.trim().is_empty();
            if !said || !spoken(&review.author) || answered_after(&review.created_at, None) {
                continue;
            }
            items.push(Item {
                id: review.id.clone(),
                kind: Kind::Review,
                last: review.id.clone(),
                author: author.to_owned(),
                bot: false,
                path: None,
                outdated: false,
                url: review.url.clone(),
                excerpt: excerpt(&review.body),
            });
        }
        reads.push(Read {
            repo: repo.to_owned(),
            number,
            updated_at: pull.updated_at,
            items,
        });
    }
    Ok(reads)
}

/// The items of `after` whose `last` is the `last` of no item of `before` (`None` counts as empty).
pub fn new_items<'a>(before: Option<&[Item]>, after: &'a [Item]) -> Vec<&'a Item> {
    let known = before.unwrap_or_default();
    after
        .iter()
        .filter(|item| !known.iter().any(|k| k.last == item.last))
        .collect()
}
