//! The one gateway to `gh` (contract §7.6; commands.md §9): its output is
//! parsed once here, into typed values; the rest of `pr` never sees raw JSON.
//! Every failure is `gh_failed` with `argv`, `exit` and the last 60 lines of stderr.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{CpdError, ErrorCode};
use super::status::{ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use crate::git::{TAIL_LINES, tail};

/// A `gh` call that takes longer than this is killed (`gh_failed`).
const TIMEOUT: Duration = Duration::from_secs(60);

/// The poller's calls (`search_open`, `lookup`) wait this long: `gh` has no
/// timeout of its own and GitHub cuts at 10 s.
const POLL_TIMEOUT: Duration = Duration::from_secs(15);

/// A review or comment id. GitHub's GraphQL ids (`gh pr view`) are strings and
/// its REST ids (`gh api`) are integers: both become one string.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(from = "RawId")]
pub struct Id(pub String);

#[derive(Deserialize)]
#[serde(untagged)]
enum RawId {
    Text(String),
    Number(u64),
}

impl From<RawId> for Id {
    fn from(raw: RawId) -> Id {
        Id(match raw {
            RawId::Text(s) => s,
            RawId::Number(n) => n.to_string(),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct Login {
    pub login: String,
}

/// One entry of `statusCheckRollup`: a CheckRun has `name` and `conclusion`, a
/// StatusContext has `context` and `state`.
#[derive(Debug, Deserialize)]
pub struct Check {
    pub name: Option<String>,
    pub context: Option<String>,
    pub conclusion: Option<String>,
    pub state: Option<String>,
    /// A CheckRun's `detailsUrl`, else a StatusContext's `targetUrl`.
    #[serde(rename = "detailsUrl", alias = "targetUrl")]
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Review {
    pub id: Id,
    pub author: Option<Login>,
    pub state: String,
    pub body: Option<String>,
}

/// A conversation comment of `gh pr view`.
#[derive(Debug, Deserialize)]
pub struct Comment {
    pub id: Id,
    pub author: Option<Login>,
    pub body: Option<String>,
}

/// An inline review comment of `gh api …/pulls/<n>/comments`.
#[derive(Debug, Deserialize)]
pub struct InlineComment {
    pub id: Id,
    pub user: Option<Login>,
    pub path: Option<String>,
    pub body: Option<String>,
}

/// `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url`.
#[derive(Debug, Deserialize)]
pub struct View {
    pub state: String,
    #[serde(rename = "statusCheckRollup")]
    pub checks: Option<Vec<Check>>,
    pub reviews: Vec<Review>,
    pub comments: Vec<Comment>,
    pub title: String,
    pub url: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED` or empty.
    #[serde(rename = "reviewDecision")]
    pub review_decision: Option<String>,
}

/// A PR as `gh` names it.
#[derive(Debug, Deserialize)]
pub struct PrRef {
    pub number: i64,
    pub url: String,
}

fn failed(argv: &[&str], message: String, exit: Option<i32>, stderr: &str) -> CpdError {
    let mut all = vec!["gh"];
    all.extend_from_slice(argv);
    CpdError::new(ErrorCode::GhFailed, message)
        .with("argv", json!(all))
        .with("exit", json!(exit))
        .with("stderr", tail(stderr, TAIL_LINES))
}

/// What a finished `gh` call printed; `exit` is `None` when a signal ended it.
struct Output {
    stdout: String,
    stderr: String,
    exit: Option<i32>,
}

/// Runs `gh <args>` in `cwd` for at most `timeout`; a spawn failure and a
/// timeout are `gh_failed`, any exit code is a result. Never prompts, never colours.
fn run_raw(cwd: &Path, args: &[&str], timeout: Duration) -> Result<Output, CpdError> {
    let mut child = Command::new("gh")
        .args(args)
        .current_dir(cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(args, format!("Cannot run gh: {e}."), None, &e.to_string()))?;
    // Both pipes are drained while the child runs, or a full pipe would block it.
    let mut out = child.stdout.take().expect("stdout is piped");
    let mut err = child.stderr.take().expect("stderr is piped");
    let stdout = thread::spawn(move || {
        let mut text = Vec::new();
        let _ = out.read_to_end(&mut text);
        text
    });
    let stderr = thread::spawn(move || {
        let mut text = Vec::new();
        let _ = err.read_to_end(&mut text);
        text
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                // The pipes close when the child dies, which ends the readers.
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(failed(args, format!("Cannot wait for gh: {e}."), None, &e.to_string())),
        }
    };
    let stdout = String::from_utf8_lossy(&stdout.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).into_owned();
    let Some(status) = status else {
        return Err(failed(args, format!("gh {} took longer than {}s and was killed.", args[0], timeout.as_secs()), None, &stderr));
    };
    Ok(Output { stdout, stderr, exit: status.code() })
}

/// Runs `gh <args>` in `cwd` and returns its stdout; a non-zero exit, a signal
/// and a timeout (60 s) are `gh_failed`.
fn run(cwd: &Path, args: &[&str]) -> Result<String, CpdError> {
    let out = run_raw(cwd, args, TIMEOUT)?;
    match out.exit {
        Some(0) => Ok(out.stdout),
        Some(code) => Err(failed(args, format!("gh {} exited {code}.", args[0]), Some(code), &out.stderr)),
        None => Err(failed(args, format!("gh {} was killed by a signal.", args[0]), None, &out.stderr)),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(args: &[&str], stdout: &str) -> Result<T, CpdError> {
    serde_json::from_str(stdout).map_err(|e| {
        failed(args, format!("gh {} printed unexpected JSON: {e}.", args[0]), Some(0), "")
            .with("stdout", tail(stdout, TAIL_LINES))
    })
}

/// The open PR whose head is `head`, if any.
pub fn open_pr(cwd: &Path, head: &str) -> Result<Option<PrRef>, CpdError> {
    let args = ["pr", "list", "--head", head, "--state", "open", "--json", "number,url"];
    let found: Vec<PrRef> = parse(&args, &run(cwd, &args)?)?;
    Ok(found.into_iter().next())
}

/// `gh pr create`. It has no `--json`: the URL is its last stdout line, and the
/// number is the last path segment of that URL.
pub fn create_pr(cwd: &Path, base: &str, head: &str, title: &str, body: &str) -> Result<PrRef, CpdError> {
    let args = ["pr", "create", "--base", base, "--head", head, "--title", title, "--body", body];
    let stdout = run(cwd, &args)?;
    let url = stdout.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let number = url
        .rsplit_once("/pull/")
        .filter(|(host, _)| host.starts_with("http"))
        .and_then(|(_, n)| n.parse::<i64>().ok());
    match number {
        Some(number) => Ok(PrRef { number, url: url.to_owned() }),
        None => Err(failed(&args, "gh pr create printed no pull request URL.".to_owned(), Some(0), "")
            .with("stdout", tail(&stdout, TAIL_LINES))),
    }
}

pub fn view(cwd: &Path, number: i64) -> Result<View, CpdError> {
    let n = number.to_string();
    let args = ["pr", "view", &n, "--json", "state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url"];
    parse(&args, &run(cwd, &args)?)
}

/// Inline review comments, which `gh pr view` does not list. `{owner}/{repo}`
/// are filled in by `gh` from `cwd`'s repo.
pub fn inline_comments(cwd: &Path, number: i64) -> Result<Vec<InlineComment>, CpdError> {
    let endpoint = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments");
    let args = ["api", endpoint.as_str()];
    parse(&args, &run(cwd, &args)?)
}

// The poller (contract §7.6, commands.md §9.2): one search, one lookup.

/// The fields of a pull request, as the search and the lookup both select them.
macro_rules! pr_fields {
    () => {
        "repository { nameWithOwner } number url title isDraft state createdAt mergedAt reviewDecision \
         mergeable mergeStateStatus commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }"
    };
}

/// The poll query: the User's open pull requests, oldest first, at most 100. GitHub
/// ignores an unknown search qualifier and answers 0 PRs, so a test pins this text.
pub const SEARCH_QUERY: &str = concat!(
    "query { search(type: ISSUE, query: \"is:pr is:open author:@me archived:false sort:created-asc\", first: 100) { ",
    "issueCount nodes { ... on PullRequest { ",
    pr_fields!(),
    " } } } rateLimit { cost remaining resetAt } }"
);

/// A pull request as the poller sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolledPr {
    /// GitHub's `owner/name`.
    pub repo: String,
    pub number: i64,
    pub url: String,
    pub title: String,
    pub draft: bool,
    pub state: PrState,
    pub review_decision: ReviewDecision,
    pub checks: ChecksSummary,
    pub mergeable: Mergeable,
    pub merge_state: MergeState,
    pub created_at: String,
    pub merged_at: Option<String>,
}

/// GitHub's rate limit after the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rate {
    pub cost: u32,
    pub remaining: u32,
    pub reset_at: String,
}

/// The first page of the search: `issue_count` is the total GitHub matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub issue_count: u32,
    pub prs: Vec<PolledPr>,
    pub rate: Rate,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<T>,
    errors: Option<Vec<GqlError>>,
}

#[derive(Deserialize)]
struct GqlError {
    #[serde(rename = "type")]
    kind: Option<String>,
    path: Option<Vec<Value>>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct RawSearchData {
    search: Option<RawSearch>,
    #[serde(rename = "rateLimit")]
    rate_limit: Option<RawRate>,
}

#[derive(Deserialize)]
struct RawSearch {
    #[serde(rename = "issueCount")]
    issue_count: u32,
    nodes: Vec<RawPr>,
}

#[derive(Deserialize)]
struct RawRate {
    cost: u32,
    remaining: u32,
    #[serde(rename = "resetAt")]
    reset_at: String,
}

#[derive(Deserialize)]
struct RawRepoName {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Deserialize)]
struct RawRollup {
    state: String,
}

#[derive(Deserialize)]
struct RawCommit {
    #[serde(rename = "statusCheckRollup")]
    rollup: Option<RawRollup>,
}

#[derive(Deserialize)]
struct RawCommitNode {
    commit: Option<RawCommit>,
}

#[derive(Deserialize)]
struct RawCommits {
    nodes: Vec<RawCommitNode>,
}

#[derive(Deserialize)]
struct RawPr {
    repository: RawRepoName,
    number: i64,
    url: String,
    title: String,
    #[serde(rename = "isDraft")]
    draft: bool,
    state: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    mergeable: String,
    #[serde(rename = "mergeStateStatus")]
    merge_state: String,
    commits: RawCommits,
}

#[derive(Deserialize)]
struct RawRepoLookup {
    #[serde(rename = "pullRequest")]
    pull_request: Option<RawPr>,
}

/// A poll call and what it printed, for `gh_failed`.
struct Reply<'a> {
    args: &'a [&'a str],
    out: &'a Output,
}

impl Reply<'_> {
    fn fail(&self, message: String) -> CpdError {
        failed(self.args, message, self.out.exit, &self.out.stderr).with("stdout", tail(&self.out.stdout, TAIL_LINES))
    }

    /// The body, whatever the exit code (`gh` exits 1 on a partial result); no
    /// JSON, or no `data`, is `gh_failed`.
    fn envelope<T: for<'de> Deserialize<'de>>(&self) -> Result<(T, Vec<GqlError>), CpdError> {
        let envelope: Envelope<T> = serde_json::from_str(&self.out.stdout)
            .map_err(|e| self.fail(format!("gh {} printed no usable JSON: {e}.", self.args[0])))?;
        let errors = envelope.errors.unwrap_or_default();
        match envelope.data {
            Some(data) => Ok((data, errors)),
            None => Err(self.fail(format!(
                "gh {} answered without data: {}",
                self.args[0],
                errors.iter().filter_map(|e| e.message.as_deref()).collect::<Vec<_>>().join("; ")
            ))),
        }
    }

    fn polled(&self, raw: RawPr) -> Result<PolledPr, CpdError> {
        let state = PrState::of_graphql(&raw.state).map_err(|e| self.fail(e.message().to_owned()))?;
        let mergeable = Mergeable::of_graphql(&raw.mergeable).map_err(|e| self.fail(e.message().to_owned()))?;
        let merge_state = MergeState::of_graphql(&raw.merge_state).map_err(|e| self.fail(e.message().to_owned()))?;
        let rollup = raw.commits.nodes.first().and_then(|n| n.commit.as_ref()).and_then(|c| c.rollup.as_ref());
        Ok(PolledPr {
            repo: raw.repository.name_with_owner,
            number: raw.number,
            url: raw.url,
            title: raw.title,
            draft: raw.draft,
            state,
            review_decision: ReviewDecision::of(raw.review_decision.as_deref()),
            checks: ChecksSummary::of_rollup(rollup.map(|r| r.state.as_str())),
            mergeable,
            merge_state,
            created_at: raw.created_at,
            merged_at: raw.merged_at,
        })
    }
}

/// Runs the GraphQL `query` inline (`-f query=@file` would send the literal text) in `cwd`.
fn graphql(cwd: &Path, query: &str) -> Result<(Vec<String>, Output), CpdError> {
    let args = vec!["api".to_owned(), "graphql".to_owned(), "-f".to_owned(), format!("query={query}")];
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_raw(cwd, &refs, POLL_TIMEOUT)?;
    Ok((args, out))
}

/// The User's open pull requests (`SEARCH_QUERY`), 15 s at most. A response with
/// `errors`, without `data` or without JSON is `gh_failed`, never "no PRs".
#[allow(dead_code)] // Job `pr-poll2` calls it.
pub fn search_open(cwd: &Path) -> Result<Search, CpdError> {
    let (args, out) = graphql(cwd, SEARCH_QUERY)?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    parse_search(&Reply { args: &refs, out: &out })
}

fn parse_search(reply: &Reply) -> Result<Search, CpdError> {
    let (data, errors): (RawSearchData, _) = reply.envelope()?;
    if let Some(first) = errors.first() {
        return Err(reply.fail(format!("gh api graphql answered with errors: {}", first.message.as_deref().unwrap_or("(no message)"))));
    }
    let (Some(search), Some(rate)) = (data.search, data.rate_limit) else {
        return Err(reply.fail("gh api graphql answered without search or rateLimit.".to_owned()));
    };
    let prs = search.nodes.into_iter().map(|raw| reply.polled(raw)).collect::<Result<Vec<_>, _>>()?;
    Ok(Search { issue_count: search.issue_count, prs, rate: Rate { cost: rate.cost, remaining: rate.remaining, reset_at: rate.reset_at } })
}

/// `owner/name` with each part of `[A-Za-z0-9_.-]+`: nothing else reaches a query text.
fn repo_parts(repo: &str) -> Option<(&str, &str)> {
    let part = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    repo.split_once('/').filter(|(owner, name)| part(owner) && part(name))
}

/// The pull requests `(owner/name, number)`, one alias each, in one call, 15 s at
/// most; `None` for one that GitHub cannot find (deleted, or no access). `gh`
/// exits 1 on such an alias: stdout is parsed whatever the exit, and a call
/// without `data` is `gh_failed`. An empty `prs` makes no call.
#[allow(dead_code)] // Job `pr-poll2` calls it.
pub fn lookup(cwd: &Path, prs: &[(String, i64)]) -> Result<Vec<Option<PolledPr>>, CpdError> {
    if prs.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = String::from("query {");
    for (i, (repo, number)) in prs.iter().enumerate() {
        let (owner, name) = repo_parts(repo).ok_or_else(|| {
            CpdError::new(ErrorCode::GhFailed, format!("{repo:?} is not an owner/name repo."))
                .with("repo", repo.as_str())
        })?;
        query.push_str(&format!(" p{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ pullRequest(number: {number}) {{ {} }} }}", pr_fields!()));
    }
    query.push_str(" }");
    let (args, out) = graphql(cwd, &query)?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    parse_lookup(&Reply { args: &refs, out: &out }, prs.len())
}

/// The `count` aliases `p0`… of a lookup's reply.
fn parse_lookup(reply: &Reply, count: usize) -> Result<Vec<Option<PolledPr>>, CpdError> {
    let (mut data, errors): (BTreeMap<String, Option<RawRepoLookup>>, _) = reply.envelope()?;
    if let Some(other) = errors.iter().find(|e| e.kind.as_deref() != Some("NOT_FOUND")) {
        return Err(reply.fail(format!("gh api graphql answered with errors: {}", other.message.as_deref().unwrap_or("(no message)"))));
    }
    let not_found = |alias: &str| errors.iter().any(|e| e.path.as_ref().and_then(|p| p.first()).and_then(Value::as_str) == Some(alias));
    let mut found = Vec::with_capacity(count);
    for i in 0..count {
        let alias = format!("p{i}");
        let raw = data
            .remove(&alias)
            .ok_or_else(|| reply.fail(format!("gh api graphql answered without {alias}.")))?
            .and_then(|r| r.pull_request);
        found.push(match raw {
            Some(raw) => Some(reply.polled(raw)?),
            None if not_found(&alias) => None,
            None => return Err(reply.fail(format!("gh api graphql answered null for {alias} without NOT_FOUND."))),
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARGS: [&str; 4] = ["api", "graphql", "-f", "query=Q"];

    fn reply(stdout: &str, exit: i32) -> Output {
        Output { stdout: stdout.to_owned(), stderr: "gh: boom".to_owned(), exit: Some(exit) }
    }

    fn pr(repo: &str, number: i64, draft: bool, state: &str, rollup: &str) -> String {
        format!(
            r#"{{"repository":{{"nameWithOwner":"{repo}"}},"number":{number},"url":"https://github.com/{repo}/pull/{number}","title":"T{number}",
            "isDraft":{draft},"state":"{state}","createdAt":"2026-05-01T10:00:00Z","mergedAt":null,"reviewDecision":"APPROVED",
            "mergeable":"CONFLICTING","mergeStateStatus":"DIRTY",
            "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{rollup}}}}}]}}}}"#
        )
    }

    fn search(out: &Output) -> Result<Search, CpdError> {
        parse_search(&Reply { args: &ARGS, out })
    }

    fn looked_up(out: &Output, count: usize) -> Result<Vec<Option<PolledPr>>, CpdError> {
        parse_lookup(&Reply { args: &ARGS, out }, count)
    }

    #[test]
    fn the_search_query_is_pinned() {
        assert_eq!(
            SEARCH_QUERY,
            "query { search(type: ISSUE, query: \"is:pr is:open author:@me archived:false sort:created-asc\", first: 100) { \
             issueCount nodes { ... on PullRequest { repository { nameWithOwner } number url title isDraft state createdAt mergedAt \
             reviewDecision mergeable mergeStateStatus commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } } } } \
             rateLimit { cost remaining resetAt } }"
        );
    }

    #[test]
    fn a_search_of_two_pull_requests_is_typed() {
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":2,"nodes":[{},{}]}},"rateLimit":{{"cost":1,"remaining":4990,"resetAt":"2026-05-01T11:00:00Z"}}}}}}"#,
            pr("o/a", 7, true, "OPEN", r#"{"state":"FAILURE"}"#),
            pr("o/b", 8, false, "OPEN", "null"),
        );
        let found = search(&reply(&body, 0)).unwrap();
        assert_eq!((found.issue_count, found.rate.cost, found.rate.remaining), (2, 1, 4990));
        assert_eq!(found.rate.reset_at, "2026-05-01T11:00:00Z");
        let [a, b] = &found.prs[..] else { panic!("{:?}", found.prs) };
        assert_eq!((a.repo.as_str(), a.number, a.draft, a.state, a.checks), ("o/a", 7, true, PrState::Open, ChecksSummary::Failed));
        assert_eq!((a.review_decision, a.title.as_str(), a.merged_at.as_deref()), (ReviewDecision::Approved, "T7", None));
        assert_eq!(a.url, "https://github.com/o/a/pull/7");
        assert_eq!(a.created_at, "2026-05-01T10:00:00Z");
        assert_eq!((b.draft, b.checks), (false, ChecksSummary::None));
        assert_eq!((a.mergeable, a.merge_state), (Mergeable::Conflicting, MergeState::Dirty));
    }

    #[test]
    fn mergeable_and_merge_state_are_typed_whatever_they_are() {
        for (mergeable, merge_state, want) in [
            ("MERGEABLE", "BLOCKED", (Mergeable::Mergeable, MergeState::Blocked)),
            ("UNKNOWN", "UNKNOWN", (Mergeable::Unknown, MergeState::Unknown)),
            ("MERGEABLE", "HAS_HOOKS", (Mergeable::Mergeable, MergeState::HasHooks)),
        ] {
            let node = pr("o/a", 7, false, "OPEN", "null")
                .replace(r#""mergeable":"CONFLICTING","mergeStateStatus":"DIRTY""#, &format!(r#""mergeable":"{mergeable}","mergeStateStatus":"{merge_state}""#));
            let body = format!(
                r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
            );
            let found = &search(&reply(&body, 0)).unwrap().prs[0];
            assert_eq!((found.mergeable, found.merge_state), want, "{mergeable} {merge_state}");
        }
    }

    #[test]
    fn a_mergeable_or_merge_state_outside_the_sets_is_gh_failed_in_the_search_and_the_lookup() {
        for (name, from, to) in [
            ("mergeable", r#""mergeable":"CONFLICTING""#, r#""mergeable":"MAYBE""#),
            ("merge_state", r#""mergeStateStatus":"DIRTY""#, r#""mergeStateStatus":"QUEUED""#),
            ("lowercase", r#""mergeStateStatus":"DIRTY""#, r#""mergeStateStatus":"dirty""#),
            ("null mergeable", r#""mergeable":"CONFLICTING""#, r#""mergeable":null"#),
        ] {
            let node = pr("o/a", 7, false, "OPEN", "null").replace(from, to);
            let body = format!(
                r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
            );
            assert_eq!(search(&reply(&body, 0)).unwrap_err().code(), ErrorCode::GhFailed, "search {name}");
            let body = format!(r#"{{"data":{{"p0":{{"pullRequest":{node}}}}}}}"#);
            assert_eq!(looked_up(&reply(&body, 0), 1).unwrap_err().code(), ErrorCode::GhFailed, "lookup {name}");
        }
    }

    #[test]
    fn a_pull_request_without_the_two_fields_is_gh_failed() {
        let node = pr("o/a", 7, false, "OPEN", "null").replace(r#""mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","#, "");
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
        );
        assert_eq!(search(&reply(&body, 0)).unwrap_err().code(), ErrorCode::GhFailed);
    }

    #[test]
    fn a_pull_request_without_a_commit_has_no_checks() {
        let body = pr("o/a", 7, false, "OPEN", "null").replace(r#"{"commit":{"statusCheckRollup":null}}"#, r#"{"commit":null}"#);
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{body}]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
        );
        assert_eq!(search(&reply(&body, 0)).unwrap().prs[0].checks, ChecksSummary::None);
    }

    #[test]
    fn a_search_that_is_not_an_answer_is_gh_failed() {
        let nodes = pr("o/a", 7, false, "OPEN", "null");
        let with_errors = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{nodes}]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}},"errors":[{{"message":"nope"}}]}}"#
        );
        let unknown_state = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{}]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#,
            pr("o/a", 7, false, "WEIRD", "null")
        );
        for (name, stdout, exit) in [
            ("empty", "", 0),
            ("html", "<html><body>Unicorn!</body></html>", 0),
            ("errors only", r#"{"errors":[{"message":"Expected NAME"}]}"#, 1),
            ("data and errors", with_errors.as_str(), 1),
            ("null search", r#"{"data":{"search":null,"rateLimit":null}}"#, 0),
            ("empty data", "{}", 0),
            ("unknown state", unknown_state.as_str(), 0),
        ] {
            let e = search(&reply(stdout, exit)).unwrap_err();
            assert_eq!(e.code(), ErrorCode::GhFailed, "{name}");
        }
    }

    #[test]
    fn a_failure_carries_argv_exit_and_stderr() {
        let e = search(&reply("", 1)).unwrap_err();
        let details = serde_json::to_value(&e).unwrap();
        let details = &details["error"]["details"];
        assert_eq!(details["argv"], json!(["gh", "api", "graphql", "-f", "query=Q"]));
        assert_eq!(details["exit"], json!(1));
        assert_eq!(details["stderr"], json!("gh: boom"));
    }

    #[test]
    fn a_lookup_with_a_not_found_alias_is_a_result_though_gh_exited_1() {
        let body = format!(
            r#"{{"data":{{"p0":{{"pullRequest":{}}},"p1":{{"pullRequest":null}}}},
            "errors":[{{"type":"NOT_FOUND","path":["p1","pullRequest"],"message":"Could not resolve"}}]}}"#,
            pr("o/a", 7, false, "MERGED", r#"{"state":"SUCCESS"}"#).replace(r#""mergedAt":null"#, r#""mergedAt":"2026-05-02T00:00:00Z""#),
        );
        let found = looked_up(&reply(&body, 1), 2).unwrap();
        let [Some(a), None] = &found[..] else { panic!("{found:?}") };
        assert_eq!((a.number, a.state, a.checks, a.merged_at.as_deref()), (7, PrState::Merged, ChecksSummary::Passed, Some("2026-05-02T00:00:00Z")));
    }

    #[test]
    fn a_gone_repository_is_not_found_too() {
        let body = r#"{"data":{"p0":null},"errors":[{"type":"NOT_FOUND","path":["p0"],"message":"Could not resolve to a Repository"}]}"#;
        assert_eq!(looked_up(&reply(body, 1), 1).unwrap(), vec![None]);
    }

    #[test]
    fn a_lookup_that_is_not_an_answer_is_gh_failed() {
        for (name, stdout) in [
            ("empty", ""),
            ("html", "<html></html>"),
            ("no data", r#"{"errors":[{"message":"Bad credentials"}]}"#),
            ("null data", r#"{"data":null,"errors":[{"message":"x"}]}"#),
            ("null without NOT_FOUND", r#"{"data":{"p0":{"pullRequest":null}}}"#),
            ("other error", r#"{"data":{"p0":null},"errors":[{"type":"RATE_LIMITED","path":["p0"],"message":"x"}]}"#),
            ("missing alias", r#"{"data":{}}"#),
        ] {
            let e = looked_up(&reply(stdout, 1), 1).unwrap_err();
            assert_eq!(e.code(), ErrorCode::GhFailed, "{name}");
        }
    }

    #[test]
    fn a_repo_that_is_not_owner_name_is_refused_before_gh_runs() {
        let nowhere = Path::new("/nonexistent");
        for repo in ["", "a", "a/", "/b", "a/b/c", "a b/c", "o/n\"}) { x", "o\"/n", "ö/n"] {
            let e = lookup(nowhere, &[(repo.to_owned(), 1)]).unwrap_err();
            assert_eq!(e.code(), ErrorCode::GhFailed, "{repo:?}");
            assert!(e.message().contains("owner/name"), "{repo:?}: {}", e.message());
        }
    }

    #[test]
    fn an_empty_lookup_makes_no_call() {
        assert_eq!(lookup(Path::new("/nonexistent"), &[]).unwrap(), vec![]);
    }

    #[test]
    fn repos_of_every_allowed_character_pass() {
        assert_eq!(repo_parts("Some-Org_1/repo.name-2"), Some(("Some-Org_1", "repo.name-2")));
    }
}
