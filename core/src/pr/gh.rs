//! The one gateway to `gh` (contract §7.6; commands.md §9): its output is
//! parsed once here, into typed values; the rest of `pr` never sees raw JSON.
//! Every failure is `gh_failed` with `argv`, `exit` and the last 60 lines of stderr.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::comments;
use super::merge_method::{MergeMethods, Method};
use super::status::{Approval, ChecksSummary, MergeState, Mergeable, PrState, ReviewDecision};
use crate::error::{CpdError, ErrorCode};
use crate::git::{TAIL_LINES, tail};
use crate::host::deadline::Failed;
use crate::host::limit::limit;
use crate::host::runner::{Call, Runner};
use crate::ports::Ports;

/// A `gh` call that takes longer than this is killed (`gh_failed`).
const TIMEOUT: Duration = Duration::from_secs(60);

/// The poller's calls (`search_open`, `lookup`) wait this long: `gh` has no
/// timeout of its own and GitHub cuts at 10 s.
const POLL_TIMEOUT: Duration = Duration::from_secs(15);

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
    /// A CheckRun's workflow name (`gh pr view`: `workflowName`); none for a third-party app.
    #[serde(rename = "workflowName", default)]
    pub workflow: Option<String>,
    /// A CheckRun's `startedAt`, a StatusContext's `startedAt` (`gh pr view`) or `createdAt` (GraphQL); RFC 3339 UTC.
    #[serde(rename = "startedAt", default)]
    pub started_at: Option<String>,
}

/// A Workflow as `gh` names it.
#[derive(Debug, Deserialize)]
pub struct PrRef {
    pub number: i64,
    pub url: String,
}

fn failed(argv: &[&str], message: String, exit: Option<i32>, stderr: &str) -> CpdError {
    let mut all = vec!["gh"];
    all.extend_from_slice(argv);
    let e = CpdError::new(ErrorCode::GhFailed, message)
        .with("argv", json!(all))
        .with("exit", json!(exit))
        .with("stderr", tail(stderr, TAIL_LINES));
    if rate_limited("", stderr) {
        e.with("rate_limited", true)
    } else {
        e
    }
}

/// Whether `gh` said GitHub's rate limit is spent: a GraphQL `RATE_LIMITED` error or an "API rate limit" message in stdout or
/// stderr. Shared by the poll [`Reply`] (E5) and the CLI [`failed`] path (the merge). Never transient; the merge classifies it
/// as a transient provider failure and E5 pauses on the mark.
fn rate_limited(stdout: &str, stderr: &str) -> bool {
    stdout.contains("\"RATE_LIMITED\"")
        || stdout.contains("API rate limit")
        || stderr.contains("API rate limit")
}

/// What a finished `gh` call printed; `exit` is `None` when a signal ended it.
pub(super) struct Output {
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) exit: Option<i32>,
}

/// `gh` ran past its timeout and was killed; `stderr` is what it had printed.
pub(super) struct Killed {
    pub(super) stderr: String,
}

/// The `gh_failed` of a call killed after `timeout`.
fn killed(args: &[&str], timeout: Duration, stderr: &str) -> CpdError {
    failed(
        args,
        format!(
            "gh {} took longer than {}s and was killed.",
            args[0],
            timeout.as_secs_f64()
        ),
        None,
        stderr,
    )
}

/// Runs `gh <args>` in `cwd` for at most `timeout`; a spawn failure and a
/// timeout are `gh_failed`, any exit code is a result. Never prompts, never colours.
pub(super) fn run_raw_with(
    runner: &dyn Runner,
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, CpdError> {
    run_timed_with(runner, cwd, args, timeout)?.map_err(|k| killed(args, timeout, &k.stderr))
}

/// [`run_raw_with`], telling a kill at the timeout (`Err(Killed)`) from a failure to run `gh` (`Err` of the outer result):
/// one `Runner` call of `gh <args>` in `cwd` under `timeout`, with `GH_PROMPT_DISABLED=1` and `NO_COLOR=1`.
pub(super) fn run_timed_with(
    runner: &dyn Runner,
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Result<Output, Killed>, CpdError> {
    let mut command = Command::new("gh");
    command
        .args(args)
        .current_dir(cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null());
    match runner.output(&mut command, "gh", timeout) {
        Ok(out) => Ok(Ok(Output {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            exit: out.status.code(),
        })),
        Err(Failed::Timeout(_)) => Ok(Err(Killed {
            stderr: String::new(),
        })),
        Err(Failed::Io(e)) => Err(failed(
            args,
            format!("Cannot run gh: {e}."),
            None,
            &e.to_string(),
        )),
    }
}

/// Runs `gh <args>` in `cwd` and returns its stdout; a non-zero exit, a signal
/// and a timeout (60 s) are `gh_failed`.
fn run(cwd: &Path, args: &[&str]) -> Result<String, CpdError> {
    let ports = Ports::current();
    if IN_MERGE.with(std::cell::Cell::get) {
        return run_in_merge(&*ports.runner, &*ports.clock, cwd, args);
    }
    run_on(&*ports.runner, cwd, args)
}

std::thread_local! {
    /// Set while the merge runs its `gh` calls: [`run`] then waits out a rate limit instead of failing (contract §7.4).
    static IN_MERGE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with its `gh` calls waiting out a rate limit (see [`run_in_merge`]); the flag is restored after.
pub(crate) fn in_merge<T>(f: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            IN_MERGE.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(IN_MERGE.with(|c| c.replace(true)));
    f()
}

/// The longest the waits of one merge total, in seconds: one GitHub rate window.
const MERGE_WAIT_BOUND: u64 = 3_600;
/// Added to the reset before the call is repeated, in seconds.
const MERGE_WAIT_GRACE: u64 = 3;
/// The wait when the reset cannot be read, in seconds.
const MERGE_WAIT_FALLBACK: u64 = 60;

/// A merge's `gh <args>` call on `runner` and `clock`: a rate-limited reply waits in place until the reset (read with
/// `gh api rate_limit`: `graphql` or `core`, plus 3 s; 60 s when unreadable), repeats the same argv, burns no try, and past
/// 3 600 s of waiting in total fails `gh_failed` with the message `rate_limited: <message>` (contract §7.4).
pub(super) fn run_in_merge(
    runner: &dyn Runner,
    clock: &dyn crate::clock::Clock,
    cwd: &Path,
    args: &[&str],
) -> Result<String, CpdError> {
    let mut waited = 0u64;
    loop {
        let out = run_raw_with(runner, cwd, args, TIMEOUT)?;
        let limited = out.exit != Some(0) && rate_limited(&out.stdout, &out.stderr);
        let graphql = out.stdout.contains("RATE_LIMITED");
        let e = match finish(out, args) {
            Ok(stdout) => return Ok(stdout),
            Err(e) => e,
        };
        if !limited {
            return Err(e);
        }
        let wait = reset_wait(runner, clock, cwd, if graphql { "graphql" } else { "core" });
        if waited + wait > MERGE_WAIT_BOUND {
            return Err(CpdError::new(
                ErrorCode::GhFailed,
                format!("rate_limited: {}", e.message()),
            )
            .with("argv", e.details().get("argv").cloned().unwrap_or_default())
            .with("exit", e.details().get("exit").cloned().unwrap_or_default())
            .with(
                "stderr",
                e.details().get("stderr").cloned().unwrap_or_default(),
            ));
        }
        waited += wait;
        clock.sleep(Duration::from_secs(wait));
    }
}

/// Seconds to wait for `resource`'s reset plus the grace; the fallback when `gh api rate_limit` fails or has no reset.
fn reset_wait(
    runner: &dyn Runner,
    clock: &dyn crate::clock::Clock,
    cwd: &Path,
    resource: &str,
) -> u64 {
    let reset = run_raw_with(runner, cwd, &["api", "rate_limit"], TIMEOUT)
        .ok()
        .filter(|o| o.exit == Some(0))
        .and_then(|o| serde_json::from_str::<serde_json::Value>(&o.stdout).ok())
        .and_then(|v| v["resources"][resource]["reset"].as_u64());
    match reset {
        Some(reset) => {
            let now = u64::try_from(clock.utc().unix_timestamp()).unwrap_or(0);
            reset.saturating_sub(now) + MERGE_WAIT_GRACE
        }
        None => MERGE_WAIT_FALLBACK,
    }
}

/// [`run`] on `runner`.
fn run_on(runner: &dyn Runner, cwd: &Path, args: &[&str]) -> Result<String, CpdError> {
    finish(run_raw_with(runner, cwd, args, TIMEOUT)?, args)
}

/// A finished call's stdout, or its `gh_failed`.
fn finish(out: Output, args: &[&str]) -> Result<String, CpdError> {
    match out.exit {
        Some(0) => Ok(out.stdout),
        Some(code) => Err(failed(
            args,
            format!("gh {} exited {code}.", args[0]),
            Some(code),
            &out.stderr,
        )),
        None => Err(failed(
            args,
            format!("gh {} was killed by a signal.", args[0]),
            None,
            &out.stderr,
        )),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(args: &[&str], stdout: &str) -> Result<T, CpdError> {
    serde_json::from_str(stdout).map_err(|e| {
        failed(
            args,
            format!("gh {} printed unexpected JSON: {e}.", args[0]),
            Some(0),
            "",
        )
        .with("stdout", tail(stdout, TAIL_LINES))
    })
}

/// A pull request's state as `gh pr list --json state` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum HeadState {
    #[serde(rename = "OPEN")]
    Open,
    #[serde(rename = "CLOSED")]
    Closed,
    #[serde(rename = "MERGED")]
    Merged,
}

/// A pull request of a head branch, in any state.
#[derive(Debug, Deserialize)]
pub struct HeadPr {
    pub number: i64,
    pub url: String,
    pub state: HeadState,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

/// The newest (highest number) pull request whose head is `head`, in any state, created at or after `since`, if any. A pull
/// request older than `since` (the row's start) is not the row's and is not seen, whatever its state; `None` (an adopted Workflow)
/// sees every pull request. Both times are compared as instants.
pub fn head_pr(
    cwd: &Path,
    head: &str,
    since: Option<jiff::Timestamp>,
) -> Result<Option<HeadPr>, CpdError> {
    let args = [
        "pr",
        "list",
        "--head",
        head,
        "--state",
        "all",
        "--json",
        "number,url,state,createdAt",
    ];
    let found: Vec<HeadPr> = parse(&args, &run(cwd, &args)?)?;
    let mut kept = Vec::new();
    for f in found {
        if let Some(since) = since {
            let created: jiff::Timestamp = f.created_at.parse().map_err(|e| {
                failed(
                    &args,
                    format!(
                        "gh printed the createdAt {:?} of pull request #{}: {e}",
                        f.created_at, f.number
                    ),
                    Some(0),
                    "",
                )
            })?;
            if created < since {
                continue;
            }
        }
        kept.push(f);
    }
    Ok(kept.into_iter().max_by_key(|f| f.number))
}

/// The pull request `workflow add --adopt` reads (`gh pr view <n> --json number,url,state,isCrossRepository,headRefName,baseRefName,author`)
/// and the login of the User (`gh api user --jq .login`), parsed once.
#[derive(Debug, Deserialize)]
struct AdoptView {
    number: i64,
    url: String,
    state: String,
    #[serde(rename = "isCrossRepository")]
    is_cross_repository: bool,
    #[serde(rename = "headRefName")]
    head_ref: String,
    #[serde(rename = "baseRefName")]
    base_ref: String,
    author: Login,
}

/// A pull request to adopt, as GitHub names it, and who the User is.
#[derive(Debug)]
pub struct Adoptable {
    pub number: i64,
    pub url: String,
    /// `OPEN`, `CLOSED` or `MERGED`.
    pub state: String,
    pub fork: bool,
    pub head_ref: String,
    pub base: String,
    /// The author's login.
    pub author: String,
    /// The login of the User (`gh api user`).
    pub user: String,
}

/// Reads the pull request `number` of the repo at `cwd` (gh resolves it through its `origin`) and the User's login.
pub fn adoptable(cwd: &Path, number: i64) -> Result<Adoptable, CpdError> {
    let n = number.to_string();
    let view = [
        "pr",
        "view",
        &n,
        "--json",
        "number,url,state,isCrossRepository,headRefName,baseRefName,author",
    ];
    let v: AdoptView = parse(&view, &run(cwd, &view)?)?;
    let who = ["api", "user", "--jq", ".login"];
    let user = run(cwd, &who)?.trim().to_owned();
    if user.is_empty() {
        return Err(failed(
            &who,
            "gh api user printed no login.".to_owned(),
            Some(0),
            "",
        ));
    }
    Ok(Adoptable {
        number: v.number,
        url: v.url,
        state: v.state,
        fork: v.is_cross_repository,
        head_ref: v.head_ref,
        base: v.base_ref,
        author: v.author.login,
        user,
    })
}

/// `gh pr create`. It has no `--json`: the URL is its last stdout line, and the
/// number is the last path segment of that URL.
pub fn create_pr(
    cwd: &Path,
    base: &str,
    head: &str,
    title: &str,
    body: &str,
) -> Result<PrRef, CpdError> {
    let args = [
        "pr", "create", "--base", base, "--head", head, "--title", title, "--body", body,
    ];
    let stdout = run(cwd, &args)?;
    let url = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let number = url
        .rsplit_once("/pull/")
        .filter(|(host, _)| host.starts_with("http"))
        .and_then(|(_, n)| n.parse::<i64>().ok());
    match number {
        Some(number) => Ok(PrRef {
            number,
            url: url.to_owned(),
        }),
        None => Err(failed(
            &args,
            "gh pr create printed no pull request URL.".to_owned(),
            Some(0),
            "",
        )
        .with("stdout", tail(&stdout, TAIL_LINES))),
    }
}

/// `gh run rerun <id> --failed`: re-runs only the failed jobs of a GitHub Actions workflow run.
pub fn rerun_failed(runner: &dyn Runner, cwd: &Path, id: i64) -> Result<(), CpdError> {
    let n = id.to_string();
    run_on(runner, cwd, &["run", "rerun", &n, "--failed"]).map(drop)
}

/// `gh pr close <number> --repo <repo>`. A pull request that is already closed or merged is the
/// state wanted: when `gh` fails, `gh pr view` says whether it is, and only then is the failure not one.
pub fn close_pr(cwd: &Path, repo: &str, number: i64) -> Result<(), CpdError> {
    #[derive(Deserialize)]
    struct State {
        state: String,
    }
    let n = number.to_string();
    let close = ["pr", "close", &n, "--repo", repo];
    let Err(failure) = run(cwd, &close) else {
        return Ok(());
    };
    let view = ["pr", "view", &n, "--repo", repo, "--json", "state"];
    match run(cwd, &view).and_then(|out| parse::<State>(&view, &out)) {
        Ok(State { state }) if state == "CLOSED" || state == "MERGED" => Ok(()),
        _ => Err(failure),
    }
}

/// Retitles the pull request `repo`#`number` (`owner/name`): `gh api -X PATCH repos/<repo>/pulls/<n> -f title=<title>`.
/// Not `gh pr edit`: it fails on repos with the deprecated classic Projects (commands.md §9.5, hs-d285). `describe_pr` shares that reason.
pub fn retitle_pr(cwd: &Path, repo: &str, number: i64, title: &str) -> Result<(), CpdError> {
    let endpoint = format!("repos/{repo}/pulls/{number}");
    let field = format!("title={title}");
    run(cwd, &["api", "-X", "PATCH", &endpoint, "-f", &field]).map(drop)
}

/// Replaces the body of the pull request `repo`#`number` (`owner/name`): `gh api -X PATCH repos/<repo>/pulls/<n> --input -`
/// with `{"body": <body>}` on stdin. Not `gh pr edit`: it fails on repos with the deprecated classic Projects
/// (commands.md §9.5) and needs a repository cwd. A `repo` that is not `owner/name` is `gh_failed`, before any call.
pub fn describe_pr(cwd: &Path, repo: &str, number: i64, body: &str) -> Result<(), CpdError> {
    repo_parts(repo).ok_or_else(|| {
        CpdError::new(
            ErrorCode::GhFailed,
            format!("{repo:?} is not an owner/name repo."),
        )
        .with("repo", repo)
    })?;
    let endpoint = format!("repos/{repo}/pulls/{number}");
    let args = ["api", "-X", "PATCH", endpoint.as_str(), "--input", "-"];
    let call = Call {
        tool: "gh".to_owned(),
        program: "gh".to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: vec![
            ("GH_PROMPT_DISABLED".to_owned(), Some("1".to_owned())),
            ("NO_COLOR".to_owned(), Some("1".to_owned())),
        ],
        cwd: Some(cwd.to_path_buf()),
        deadline: TIMEOUT,
        stdin: Some(serde_json::json!({"body": body}).to_string().into_bytes()),
        stderr_to: None,
    };
    let out = match Ports::current().runner.run(&call) {
        Ok(out) => out,
        Err(Failed::Timeout(_)) => return Err(killed(&args, TIMEOUT, "")),
        Err(Failed::Io(e)) => {
            return Err(failed(
                &args,
                format!("Cannot run gh: {e}."),
                None,
                &e.to_string(),
            ));
        }
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    match out.status.code() {
        Some(0) => Ok(()),
        Some(code) => Err(failed(
            &args,
            format!("gh {} exited {code}.", args[0]),
            Some(code),
            &stderr,
        )),
        None => Err(failed(
            &args,
            format!("gh {} was killed by a signal.", args[0]),
            None,
            &stderr,
        )),
    }
}

/// `headRefOid` of the pull request `number` of `repo`: its head commit, which GitHub keeps after a merge and the
/// deletion of the branch (contract §7.2 "The recorded head").
pub fn head_ref_oid(cwd: &Path, repo: &str, number: i64) -> Result<String, CpdError> {
    head_ref_oid_on(&*Ports::current().runner, cwd, repo, number)
}

/// [`head_ref_oid`] on an explicit `runner`, never the ambient `Ports` (swr-github.md §7.3).
pub fn head_ref_oid_on(
    runner: &dyn Runner,
    cwd: &Path,
    repo: &str,
    number: i64,
) -> Result<String, CpdError> {
    #[derive(Deserialize)]
    struct Head {
        #[serde(rename = "headRefOid")]
        oid: String,
    }
    let n = number.to_string();
    let args = ["pr", "view", &n, "--repo", repo, "--json", "headRefOid"];
    parse::<Head>(&args, &run_on(runner, cwd, &args)?).map(|h| h.oid)
}

/// Whether `head` contains the current head of `base`, read fresh through GitHub's compare API
/// (`gh api repos/{owner}/{repo}/compare/{base}...{head}`): `status` is `ahead` or `identical` when the base's head is an
/// ancestor of `head`, `diverged` or `behind` when it is not. A non-zero exit, a signal and a timeout (60 s) are `gh_failed`.
pub fn head_contains_base(
    runner: &dyn Runner,
    cwd: &Path,
    repo: &str,
    base: &str,
    head: &str,
) -> Result<bool, CpdError> {
    #[derive(Deserialize)]
    struct Compare {
        status: String,
    }
    let endpoint = format!("repos/{repo}/compare/{base}...{head}");
    let args = ["api", endpoint.as_str()];
    let compare: Compare = parse(&args, &run_on(runner, cwd, &args)?)?;
    Ok(matches!(compare.status.as_str(), "ahead" | "identical"))
}

/// The repo settings a `swr_repos` row holds, as `gh api repos/{owner}/{repo}` prints them (air-n70).
#[derive(Deserialize)]
struct RawRepoSettings {
    default_branch: Option<String>,
    archived: bool,
    #[serde(default)]
    allow_auto_merge: bool,
    allow_merge_commit: bool,
    allow_squash_merge: bool,
    allow_rebase_merge: bool,
    delete_branch_on_merge: bool,
}

/// The repo's settings (`gh api repos/{owner}/{repo}`), one read (air-n70): the REST fields map onto the `swr_repos`
/// row. A non-zero exit, a signal and a timeout (60 s) are `gh_failed`; a tolerated reply is parsed once here.
pub fn repo_settings(runner: &dyn Runner, cwd: &Path, repo: &str) -> Result<RepoMeta, CpdError> {
    let endpoint = format!("repos/{repo}");
    let args = ["api", endpoint.as_str()];
    let raw: RawRepoSettings = parse(&args, &run_on(runner, cwd, &args)?)?;
    Ok(RepoMeta {
        default_branch: raw.default_branch,
        is_archived: raw.archived,
        auto_merge_allowed: raw.allow_auto_merge,
        squash_merge_allowed: raw.allow_squash_merge,
        merge_commit_allowed: raw.allow_merge_commit,
        rebase_merge_allowed: raw.allow_rebase_merge,
        delete_branch_on_merge: raw.delete_branch_on_merge,
    })
}

/// `gh pr merge <number> --repo <repo> --<method> --match-head-commit <head>`: GitHub
/// refuses when the branch is no longer at `head`. When GitHub refuses because the pull request is the base of a
/// stack, it falls back to the asynchronous REST merge endpoint (`merge-async`, pro-d355). It still does not pass `--delete-branch`: GitHub's own
/// `delete_branch_on_merge` deletes the branch and repoints a stacked pull request (pro-d240).
pub fn merge_pr(
    runner: &dyn Runner,
    cwd: &Path,
    repo: &str,
    number: i64,
    method: Method,
    head: &str,
) -> Result<(), CpdError> {
    let n = number.to_string();
    let flag = format!("--{}", method.as_str());
    let args = [
        "pr",
        "merge",
        &n,
        "--repo",
        repo,
        flag.as_str(),
        "--match-head-commit",
        head,
    ];
    match run_on(runner, cwd, &args) {
        Ok(_) => Ok(()),
        Err(err) if is_stack_refusal(&err) => run_on(
            runner,
            cwd,
            &[
                "api",
                "--method",
                "PUT",
                &format!("repos/{repo}/pulls/{number}/merge-async"),
                "-f",
                &format!("merge_method={}", method.as_str()),
                "-f",
                &format!("sha={head}"),
            ],
        )
        .map(drop),
        Err(err) => Err(err),
    }
}

/// Whether a refused `gh pr merge` says the pull request is the base of a stack (pro-d355).
fn is_stack_refusal(err: &CpdError) -> bool {
    let text = match err.details().get("stderr").and_then(|v| v.as_str()) {
        Some(stderr) => stderr,
        None => err.message(),
    };
    text.contains("asynchronous merge REST API") || text.contains("part of a stack")
}

/// `gh api --method POST repos/<repo>/pulls/<number>/reviews --input -` with `body` on stdin: GitHub's create-review
/// call; its answer's stdout. A non-zero exit, a signal and a timeout (60 s) are `gh_failed`.
pub fn post_review(
    runner: &dyn Runner,
    repo: &str,
    number: &str,
    body: Vec<u8>,
) -> Result<String, CpdError> {
    let endpoint = format!("repos/{repo}/pulls/{number}/reviews");
    let args = ["api", "--method", "POST", endpoint.as_str(), "--input", "-"];
    let call = Call {
        tool: "gh".to_owned(),
        program: "gh".to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: vec![
            ("GH_PROMPT_DISABLED".to_owned(), Some("1".to_owned())),
            ("NO_COLOR".to_owned(), Some("1".to_owned())),
        ],
        cwd: Some(Path::new(".").to_path_buf()),
        deadline: TIMEOUT,
        stdin: Some(body),
        stderr_to: None,
    };
    let out = match runner.run(&call) {
        Ok(out) => out,
        Err(Failed::Timeout(_)) => return Err(killed(&args, TIMEOUT, "")),
        Err(Failed::Io(e)) => {
            return Err(failed(
                &args,
                format!("Cannot run gh: {e}."),
                None,
                &e.to_string(),
            ));
        }
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    match out.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Some(code) => Err(failed(
            &args,
            format!("gh api exited {code}."),
            Some(code),
            &stderr,
        )),
        None => Err(failed(
            &args,
            "gh api was killed by a signal.".to_owned(),
            None,
            &stderr,
        )),
    }
}

// The poller (contract §7.6, commands.md §9.2): one search, one lookup.

/// The repository selection, the settings form of it carrying the fields the poll fills `swr_repos` from (air-n71).
macro_rules! repo_node {
    () => {
        "repository { nameWithOwner defaultBranchRef { name } }"
    };
    (settings) => {
        "repository { nameWithOwner defaultBranchRef { name } isArchived autoMergeAllowed squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed deleteBranchOnMerge }"
    };
}

/// The fields of a pull request, as the search and the lookup both select them; `$repo` is the [`repo_node!`] selection.
macro_rules! pr_fields {
    () => {
        pr_fields!(repo_node!())
    };
    ($repo:expr) => {
        concat!($repo, " number url title isDraft state createdAt updatedAt mergedAt baseRefName reviewDecision latestOpinionatedReviews(first: 20) { nodes { state author { __typename login } } } \
         mergeable mergeStateStatus headRefOid commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } \
         reviewThreads(first: 100) { nodes { id isResolved } }")
    };
}

/// [`pr_fields!`] of the search, whose `repository` node carries the settings (air-n71): the poll writes `swr_repos`
/// from this document, never a second read.
macro_rules! search_pr_fields {
    () => {
        pr_fields!(repo_node!(settings))
    };
}

/// The fields of a pull request the review-requested search selects: `search_pr_fields!` without the reviews, the checks rollup
/// and the review threads, which nothing reads for a pull request the User neither owns nor authored (A7, pro-d392).
macro_rules! slim_pr_fields {
    () => {
        concat!(repo_node!(settings), " number url title isDraft state createdAt updatedAt mergedAt baseRefName reviewDecision \
         mergeable mergeStateStatus headRefOid")
    };
}

/// The poll query, two searches in one call, each the oldest 100 open pull
/// requests: the User's (`search`) and those that request the User's review
/// (`reviewRequests`). GitHub ignores an unknown search qualifier and answers 0
/// Workflows, so a test pins this text.
pub const SEARCH_QUERY: &str = concat!(
    "query { search(type: ISSUE, query: \"is:pr is:open author:@me archived:false sort:created-asc\", first: 100) { ",
    "issueCount nodes { ... on PullRequest { ",
    search_pr_fields!(),
    " } } } reviewRequests: search(type: ISSUE, query: \"review-requested:@me is:pr is:open archived:false sort:created-asc\", first: 100) { ",
    "issueCount nodes { ... on PullRequest { ",
    slim_pr_fields!(),
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
    /// GitHub's `updatedAt`: when it moves, the pull request's comments are read again (`read_comments`).
    pub updated_at: String,
    pub merged_at: Option<String>,
    /// The base branch (`baseRefName`); `None` when the reply has none (a test's fake `gh`).
    pub base_ref: Option<String>,
    /// The repo's default branch (`defaultBranchRef.name`); `None` when GitHub has none.
    pub default_branch: Option<String>,
    /// The ids of the first 100 review threads that are not resolved: resolving one does not move `updatedAt` (pro-d231), so `due_reads` compares them with the stored open threads.
    pub open_threads: Vec<String>,
    /// GitHub's `isArchived` of the repository: the searches say `archived:false`, so only the direct read can find it true.
    pub archived: bool,
}

/// Which of the poll's two searches listed a pull request: what the row's
/// `authored` and `review_requested` hold. A pull request only a lookup found
/// (merged or closed, so in neither open search) has neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Membership {
    /// In the `author:@me` search.
    pub authored: bool,
    /// In the `review-requested:@me` search.
    pub review_requested: bool,
}

/// A pull request with the searches that listed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub pr: PolledPr,
    pub membership: Membership,
}

/// GitHub's rate limit after the call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rate {
    pub cost: u32,
    pub remaining: u32,
    pub reset_at: String,
}

/// The first page of both searches: `issue_count` is the total GitHub matched by
/// the two. `prs` holds each pull request once: one in both searches has both
/// memberships.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub issue_count: u32,
    pub prs: Vec<Listed>,
    pub rate: Rate,
    /// The settings the search's `repository` nodes carried, one per repo, in first-seen order (air-n71): empty when a
    /// canned reply did not select them, and the poll then writes no row.
    pub repo_settings: Vec<(String, RepoMeta)>,
    /// The `headRefOid` the read carried for each pull request, as `(owner/name, number, oid)`: the head the containment
    /// flag is computed on (contract §2.2 "Ready to merge"), never the stored one.
    pub heads: Vec<(String, i64, String)>,
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
    #[serde(rename = "reviewRequests")]
    review_requests: Option<RawSearch>,
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
struct RawBranch {
    name: String,
}

#[derive(Deserialize)]
struct RawRepoName {
    #[serde(rename = "defaultBranchRef", default)]
    default_branch_ref: Option<RawBranch>,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    /// Absent from a canned reply that lacks it; the poll writes no `swr_repos` row when one is (air-n71).
    #[serde(rename = "isArchived", default)]
    is_archived: Option<bool>,
    #[serde(rename = "autoMergeAllowed", default)]
    auto_merge_allowed: Option<bool>,
    #[serde(rename = "squashMergeAllowed", default)]
    squash_merge_allowed: Option<bool>,
    #[serde(rename = "mergeCommitAllowed", default)]
    merge_commit_allowed: Option<bool>,
    #[serde(rename = "rebaseMergeAllowed", default)]
    rebase_merge_allowed: Option<bool>,
    #[serde(rename = "deleteBranchOnMerge", default)]
    delete_branch_on_merge: Option<bool>,
}

impl RawRepoName {
    /// The repo settings the node carries, when GitHub answered every flag (air-n71): a reply that lacks one is `None`,
    /// so the poll writes no row and no flag is ever invented.
    fn settings(&self) -> Option<RepoMeta> {
        Some(RepoMeta {
            default_branch: self.default_branch_ref.as_ref().map(|b| b.name.clone()),
            is_archived: self.is_archived?,
            auto_merge_allowed: self.auto_merge_allowed?,
            squash_merge_allowed: self.squash_merge_allowed?,
            merge_commit_allowed: self.merge_commit_allowed?,
            rebase_merge_allowed: self.rebase_merge_allowed?,
            delete_branch_on_merge: self.delete_branch_on_merge?,
        })
    }
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

#[derive(Default, Deserialize)]
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
    #[serde(rename = "updatedAt")]
    updated_at: String,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    /// The search always selects it; it is optional only so that a reply without it (a test's fake `gh`) follows nothing.
    #[serde(rename = "baseRefName", default)]
    base_ref: Option<String>,
    /// The head commit; optional only so that a reply without it (a test's fake `gh`) carries no fresh head.
    #[serde(rename = "headRefOid", default)]
    head_ref_oid: Option<String>,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
    /// `latestReviews` of the Light read (pro-d453); the legacy search still selects `latestOpinionatedReviews`, the same nodes.
    #[serde(rename = "latestReviews", alias = "latestOpinionatedReviews", default)]
    latest_reviews: RawReviews,
    mergeable: String,
    #[serde(rename = "mergeStateStatus")]
    merge_state: String,
    /// Absent in the review-requested search (slim).
    #[serde(default)]
    commits: RawCommits,
    #[serde(rename = "reviewThreads", default)]
    review_threads: RawThreads,
    /// Selected by the direct read only (`read_absent`); `None` when the reply has no such key, `Some(Null)` for a null.
    #[serde(rename = "viewerDidAuthor", default)]
    viewer_did_author: Option<bool>,
    #[serde(
        rename = "viewerLatestReviewRequest",
        default,
        deserialize_with = "present"
    )]
    viewer_review_request: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct RawThreads {
    nodes: Vec<RawThread>,
}

#[derive(Debug, Deserialize)]
struct RawThread {
    id: String,
    #[serde(rename = "isResolved")]
    resolved: bool,
}

#[derive(Debug, Default, Deserialize)]
struct RawReviews {
    nodes: Vec<RawReview>,
}

#[derive(Debug, Deserialize)]
struct RawReview {
    state: String,
    author: Option<RawReviewer>,
}

#[derive(Debug, Deserialize)]
struct RawReviewer {
    #[serde(rename = "__typename")]
    kind: String,
    login: String,
}

/// A key that is in the reply, null or not: `Some`.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
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
        let e = failed(self.args, message, self.out.exit, &self.out.stderr)
            .with("stdout", tail(&self.out.stdout, TAIL_LINES));
        if self.rate_limited() {
            e.with("rate_limited", true)
        } else {
            e
        }
    }

    /// Whether `gh` said GitHub's rate limit is spent: a GraphQL `RATE_LIMITED` error or an "API rate limit" message. Never
    /// transient and never retried (A2); E5 pauses on the mark.
    fn rate_limited(&self) -> bool {
        rate_limited(&self.out.stdout, &self.out.stderr)
    }

    /// [`Reply::fail`] of a call that is transient (`transient` true) when `gh` exited non-zero with an HTTP 5xx in its
    /// message (`reason` `HTTP 5xx`) and printed no GraphQL `errors`, or, when its stdout holds no complete JSON
    /// (`unparsable`), with its own failure to parse a response that ended early (`reason` [`CUT_RESPONSE`]; contract
    /// §7.6 step 4, hs-d354).
    fn fail_unless_transient(
        &self,
        message: String,
        graphql_errors: bool,
        unparsable: bool,
    ) -> CpdError {
        let status = if graphql_errors || self.out.exit == Some(0) {
            None
        } else {
            http_5xx(self.out).or_else(|| {
                (unparsable && self.out.stderr.contains(CUT_RESPONSE))
                    .then(|| CUT_RESPONSE.to_owned())
            })
        };
        let e = self.fail(message);
        match status {
            Some(status) => e.with("transient", true).with("reason", status),
            None => e,
        }
    }

    /// The body, whatever the exit code (`gh` exits 1 on a partial result); no
    /// JSON, or no `data`, is `gh_failed`: transient when it is an HTTP 5xx, or a cut response with no JSON.
    fn envelope<T: for<'de> Deserialize<'de>>(&self) -> Result<(T, Vec<GqlError>), CpdError> {
        let envelope: Envelope<T> = serde_json::from_str(&self.out.stdout).map_err(|e| {
            self.fail_unless_transient(
                format!("gh {} printed no usable JSON: {e}.", self.args[0]),
                false,
                true,
            )
        })?;
        let errors = envelope.errors.unwrap_or_default();
        match envelope.data {
            Some(data) => Ok((data, errors)),
            None => Err(self.fail_unless_transient(
                format!(
                    "gh {} answered without data: {}",
                    self.args[0],
                    errors
                        .iter()
                        .filter_map(|e| e.message.as_deref())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                !errors.is_empty(),
                false,
            )),
        }
    }

    fn polled(&self, raw: RawPr, bots: &[String]) -> Result<PolledPr, CpdError> {
        let state =
            PrState::of_graphql(&raw.state).map_err(|e| self.fail(e.message().to_owned()))?;
        let mergeable =
            Mergeable::of_graphql(&raw.mergeable).map_err(|e| self.fail(e.message().to_owned()))?;
        let merge_state = MergeState::of_graphql(&raw.merge_state)
            .map_err(|e| self.fail(e.message().to_owned()))?;
        let approvals: Vec<Approval> = raw
            .latest_reviews
            .nodes
            .iter()
            .filter(|r| r.state == "APPROVED")
            .filter_map(|r| r.author.as_ref())
            .map(|a| Approval {
                login: a.login.clone(),
                bot_type: a.kind == "Bot",
            })
            .collect();
        let rollup = raw
            .commits
            .nodes
            .first()
            .and_then(|n| n.commit.as_ref())
            .and_then(|c| c.rollup.as_ref());
        Ok(PolledPr {
            repo: raw.repository.name_with_owner,
            default_branch: raw.repository.default_branch_ref.map(|b| b.name),
            archived: raw.repository.is_archived.unwrap_or(false),
            base_ref: raw.base_ref,
            number: raw.number,
            url: raw.url,
            title: raw.title,
            draft: raw.draft,
            state,
            review_decision: ReviewDecision::of_reviews(
                raw.review_decision.as_deref(),
                &approvals,
                bots,
            ),
            checks: ChecksSummary::of_rollup(rollup.map(|r| r.state.as_str())),
            mergeable,
            merge_state,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
            merged_at: raw.merged_at,
            open_threads: raw
                .review_threads
                .nodes
                .into_iter()
                .filter(|t| !t.resolved)
                .map(|t| t.id)
                .collect(),
        })
    }
}

/// What `gh` prints when it cannot parse a response that ended early (hs-d354): the cut response, transient.
const CUT_RESPONSE: &str = "unexpected end of JSON input";

/// `HTTP 5xx` as `gh` prints it (`gh: HTTP 502: Bad Gateway`, `... (HTTP 503)`), in its stderr, or in its stdout when
/// stderr is empty: the status, `HTTP 502`. Matches `HTTP 5\d\d`.
fn http_5xx(out: &Output) -> Option<String> {
    let text = if out.stderr.trim().is_empty() {
        &out.stdout
    } else {
        &out.stderr
    };
    text.match_indices("HTTP 5").find_map(|(at, _)| {
        let status = text.get(at..at + "HTTP 5".len() + 2)?;
        status["HTTP 5".len()..]
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| status.to_owned())
    })
}

/// A failure of the poll's call `call` (`search` or `lookup`): `details` always holds `call`, `transient` and `reason`
/// (an HTTP 5xx or a timeout is transient; nothing else is), so the engine parses one shape (contract §7.6 step 4).
fn polled_call(call: &str, e: CpdError) -> CpdError {
    let transient = e
        .details()
        .get("transient")
        .cloned()
        .unwrap_or(Value::Bool(false));
    let reason = e.details().get("reason").cloned().unwrap_or(Value::Null);
    e.with("call", call)
        .with("transient", transient)
        .with("reason", reason)
}

/// Runs the GraphQL `query` inline (`-f query=@file` would send the literal text) in `cwd`, killed after
/// [`POLL_TIMEOUT`] (a timeout is transient, `reason` `timeout`; tests: `CPD_TEST_DEADLINE_GH`, [`limit`]).
fn graphql(
    runner: &dyn Runner,
    cwd: &Path,
    query: &str,
) -> Result<(Vec<String>, Output), CpdError> {
    graphql_with(runner, cwd, query, &[])
}

/// [`graphql`] with string variables, each sent as its own `-f name=value` (a body keeps its newlines).
fn graphql_with(
    runner: &dyn Runner,
    cwd: &Path,
    query: &str,
    vars: &[(&str, &str)],
) -> Result<(Vec<String>, Output), CpdError> {
    let mut args = vec![
        "api".to_owned(),
        "graphql".to_owned(),
        "-f".to_owned(),
        format!("query={query}"),
    ];
    for (name, value) in vars {
        args.push("-f".to_owned());
        args.push(format!("{name}={value}"));
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let timeout = limit("DEADLINE_GH", POLL_TIMEOUT);
    match run_timed_with(runner, cwd, &refs, timeout)? {
        Ok(out) => Ok((args, out)),
        Err(k) => Err(killed(&refs, timeout, &k.stderr)
            .with("transient", true)
            .with("reason", "timeout")),
    }
}

/// The GraphQL node id of the pull request `repo`#`number`.
pub fn pull_request_id(
    runner: &dyn Runner,
    cwd: &Path,
    repo: &str,
    number: i64,
) -> Result<String, CpdError> {
    let (owner, name) = repo_parts(repo).ok_or_else(|| {
        CpdError::new(
            ErrorCode::GhFailed,
            format!("{repo:?} is not an owner/name repo."),
        )
        .with("repo", repo)
    })?;
    let query = format!(
        "query {{ repository(owner: \"{owner}\", name: \"{name}\") {{ pullRequest(number: {number}) {{ id }} }} }}"
    );
    posted(runner, cwd, &query, &[], "/repository/pullRequest/id")
}

/// Replies in the review thread `thread` (never resolves it); the new comment's url.
pub fn reply_to_thread(
    runner: &dyn Runner,
    cwd: &Path,
    thread: &str,
    body: &str,
) -> Result<String, CpdError> {
    let query = "mutation($thread: ID!, $body: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $thread, body: $body}) { comment { id url } } }";
    posted(
        runner,
        cwd,
        query,
        &[("thread", thread), ("body", body)],
        "/addPullRequestReviewThreadReply/comment/url",
    )
}

/// Comments on the pull request whose node id is `pull`; the new comment's url.
pub fn reply_to_comment(
    runner: &dyn Runner,
    cwd: &Path,
    pull: &str,
    body: &str,
) -> Result<String, CpdError> {
    let query = "mutation($pull: ID!, $body: String!) { addComment(input: {subjectId: $pull, body: $body}) { commentEdge { node { id url } } } }";
    posted(
        runner,
        cwd,
        query,
        &[("pull", pull), ("body", body)],
        "/addComment/commentEdge/node/url",
    )
}

/// Runs a call and reads the string at `pointer` of its `data`; any failure is `gh_failed`.
fn posted(
    runner: &dyn Runner,
    cwd: &Path,
    query: &str,
    vars: &[(&str, &str)],
    pointer: &str,
) -> Result<String, CpdError> {
    let (args, out) = graphql_with(runner, cwd, query, vars)?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let reply = Reply {
        args: &refs,
        out: &out,
    };
    if out.exit != Some(0) {
        return Err(reply.fail(format!(
            "gh {} exited {}.",
            refs[0],
            out.exit
                .map_or("with a signal".to_owned(), |c| c.to_string())
        )));
    }
    let (data, errors): (Value, _) = reply.envelope()?;
    if let Some(first) = errors.first() {
        return Err(reply.fail(format!(
            "gh api graphql answered with errors: {}",
            first.message.as_deref().unwrap_or("(no message)")
        )));
    }
    data.pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| reply.fail(format!("gh api graphql answered without data{pointer}.")))
}

/// What the read query of `pr post`'s auto-merge found (commands.md §9.8b).
#[derive(Debug, PartialEq, Eq)]
pub struct AutoMergeRead {
    /// The repo allows auto-merge (`autoMergeAllowed`).
    pub allowed: bool,
    /// The pull request's GraphQL node id.
    pub id: String,
    /// The pull request already has an auto-merge request (`autoMergeRequest` is set).
    pub requested: bool,
    /// The merge methods the repo allows.
    pub methods: MergeMethods,
}

#[derive(Deserialize)]
struct RawAutoRead {
    repository: RawAutoRepo,
}

#[derive(Deserialize)]
struct RawAutoRepo {
    #[serde(rename = "autoMergeAllowed")]
    allowed: bool,
    #[serde(flatten)]
    methods: MergeMethods,
    #[serde(rename = "pullRequest")]
    pull_request: Option<RawAutoPull>,
}

#[derive(Deserialize)]
struct RawAutoPull {
    id: String,
    #[serde(rename = "autoMergeRequest")]
    request: Option<Value>,
}

/// The ONE read of `pr post`'s auto-merge: whether the repo allows it, the pull request's node id and whether it has a request.
pub fn auto_merge_read(
    runner: &dyn Runner,
    cwd: &Path,
    repo: &str,
    number: i64,
) -> Result<AutoMergeRead, CpdError> {
    let (owner, name) = repo_parts(repo).ok_or_else(|| {
        CpdError::new(
            ErrorCode::GhFailed,
            format!("{repo:?} is not an owner/name repo."),
        )
        .with("repo", repo)
    })?;
    let query = format!(
        "query {{ repository(owner: \"{owner}\", name: \"{name}\") {{ autoMergeAllowed squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed pullRequest(number: {number}) {{ id autoMergeRequest {{ enabledAt }} }} }} }}"
    );
    let (args, out) = graphql_with(runner, cwd, &query, &[])?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let reply = Reply {
        args: &refs,
        out: &out,
    };
    if out.exit != Some(0) {
        return Err(reply.fail(format!(
            "gh {} exited {}.",
            refs[0],
            out.exit
                .map_or("with a signal".to_owned(), |c| c.to_string())
        )));
    }
    let (data, errors): (RawAutoRead, _) = reply.envelope()?;
    if let Some(first) = errors.first() {
        return Err(reply.fail(format!(
            "gh api graphql answered with errors: {}",
            first.message.as_deref().unwrap_or("(no message)")
        )));
    }
    let pull = data
        .repository
        .pull_request
        .ok_or_else(|| reply.fail(format!("GitHub has no pull request {repo}#{number}.")))?;
    Ok(AutoMergeRead {
        allowed: data.repository.allowed,
        id: pull.id,
        requested: pull.request.is_some(),
        methods: data.repository.methods,
    })
}

/// Turns on auto-merge with `method` for the pull request whose node id is `pull` (`enablePullRequestAutoMerge`, never `gh pr merge`).
/// `Ok(Err(message))` is GitHub's refusal (GraphQL `errors`); a `gh` that failed or printed no usable answer is `gh_failed`.
pub fn enable_auto_merge(
    runner: &dyn Runner,
    cwd: &Path,
    pull: &str,
    method: Method,
) -> Result<Result<(), String>, CpdError> {
    let query = &format!(
        "mutation($pull: ID!) {{ enablePullRequestAutoMerge(input: {{pullRequestId: $pull, mergeMethod: {}}})  {{ pullRequest {{ autoMergeRequest {{ enabledAt }} }} }} }}",
        method.graphql()
    );
    let (args, out) = graphql_with(runner, cwd, query, &[("pull", pull)])?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let reply = Reply {
        args: &refs,
        out: &out,
    };
    let body: Option<Envelope<Value>> = serde_json::from_str(&out.stdout).ok();
    if let Some(first) = body
        .as_ref()
        .and_then(|b| b.errors.as_ref())
        .and_then(|e| e.first())
    {
        return Ok(Err(first
            .message
            .clone()
            .unwrap_or_else(|| "(no message)".to_owned())));
    }
    if out.exit != Some(0) {
        return Err(reply.fail(format!(
            "gh {} exited {}.",
            refs[0],
            out.exit
                .map_or("with a signal".to_owned(), |c| c.to_string())
        )));
    }
    let (data, _): (Value, _) = reply.envelope()?;
    match data
        .pointer("/enablePullRequestAutoMerge/pullRequest/autoMergeRequest/enabledAt")
        .and_then(Value::as_str)
    {
        Some(_) => Ok(Ok(())),
        None => {
            Err(reply.fail("gh api graphql answered without an auto-merge request.".to_owned()))
        }
    }
}

/// The User's open pull requests (`SEARCH_QUERY`), 15 s at most. A response with
/// `errors`, without `data` or without JSON is `gh_failed`, never "no pull requests".
pub fn search_open(runner: &dyn Runner, cwd: &Path, bots: &[String]) -> Result<Search, CpdError> {
    let search = || {
        // The poll is the answer to a request for one (`run/pr-poll-now`): consumed right before the search reads GitHub, so a
        // request made after this point is kept for the next poll.
        super::consume_poll_now();
        let (args, out) = graphql(runner, cwd, SEARCH_QUERY)?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        parse_search(
            &Reply {
                args: &refs,
                out: &out,
            },
            bots,
        )
    };
    search().map_err(|e| polled_call("search", e))
}

fn parse_search(reply: &Reply, bots: &[String]) -> Result<Search, CpdError> {
    let (data, errors): (RawSearchData, _) = reply.envelope()?;
    if let Some(first) = errors.first() {
        return Err(reply.fail(format!(
            "gh api graphql answered with errors: {}",
            first.message.as_deref().unwrap_or("(no message)")
        )));
    }
    let (Some(authored), Some(requested), Some(rate)) =
        (data.search, data.review_requests, data.rate_limit)
    else {
        return Err(reply.fail(
            "gh api graphql answered without search, reviewRequests or rateLimit.".to_owned(),
        ));
    };
    let issue_count = authored.issue_count + requested.issue_count;
    let mut prs: Vec<Listed> = Vec::new();
    let mut repo_settings: Vec<(String, RepoMeta)> = Vec::new();
    let mut heads: Vec<(String, i64, String)> = Vec::new();
    for (nodes, is_authored) in [(authored.nodes, true), (requested.nodes, false)] {
        for raw in nodes {
            let settings = raw.repository.settings();
            let head = raw.head_ref_oid.clone();
            let pr = reply.polled(raw, bots)?;
            if let Some(head) = head
                && !heads
                    .iter()
                    .any(|(r, n, _)| (r, n) == (&pr.repo, &pr.number))
            {
                heads.push((pr.repo.clone(), pr.number, head));
            }
            if let Some(meta) = settings
                && !repo_settings.iter().any(|(repo, _)| repo == &pr.repo)
            {
                repo_settings.push((pr.repo.clone(), meta));
            }
            let at = match prs
                .iter()
                .position(|l| (&l.pr.repo, l.pr.number) == (&pr.repo, pr.number))
            {
                Some(at) => at,
                None => {
                    prs.push(Listed {
                        pr,
                        membership: Membership::default(),
                    });
                    prs.len() - 1
                }
            };
            let membership = &mut prs[at].membership;
            if is_authored {
                membership.authored = true;
            } else {
                membership.review_requested = true;
            }
        }
    }
    Ok(Search {
        issue_count,
        prs,
        repo_settings,
        heads,
        rate: Rate {
            cost: rate.cost,
            remaining: rate.remaining,
            reset_at: rate.reset_at,
        },
    })
}

/// `owner/name` with each part of `[A-Za-z0-9_.-]+`: nothing else reaches a query text.
pub(super) fn repo_parts(repo: &str) -> Option<(&str, &str)> {
    let part = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    repo.split_once('/')
        .filter(|(owner, name)| part(owner) && part(name))
}

/// The pull requests `(owner/name, number)`, one alias each, in one call, 15 s at
/// most; `None` for one that GitHub cannot find (deleted, or no access). `gh`
/// exits 1 on such an alias: stdout is parsed whatever the exit, and a call
/// without `data` is `gh_failed`. An empty `prs` makes no call.
pub fn lookup(
    runner: &dyn Runner,
    cwd: &Path,
    prs: &[(String, i64)],
    bots: &[String],
) -> Result<Vec<Option<PolledPr>>, CpdError> {
    if prs.is_empty() {
        return Ok(Vec::new());
    }
    let found = lookup_call(runner, cwd, prs, false, bots).map_err(|e| polled_call("lookup", e))?;
    Ok(found.into_iter().map(|p| p.map(|l| l.pr)).collect())
}

/// The direct read of the poll (§7.6 step 3): [`lookup`]'s call with `viewerDidAuthor` and `viewerLatestReviewRequest`
/// selected, both required in the reply. An `OPEN` pull request carries what the viewer's own flags are
/// (`authored` = `viewerDidAuthor`, `review_requested` = a latest review request); a merged or closed one has neither.
/// `None` for one GitHub cannot find. Failures are the lookup's (`details.call` `lookup`). An empty `prs` makes no call.
pub fn read_absent(
    runner: &dyn Runner,
    cwd: &Path,
    prs: &[(String, i64)],
    bots: &[String],
) -> Result<Vec<Option<Listed>>, CpdError> {
    if prs.is_empty() {
        return Ok(Vec::new());
    }
    lookup_call(runner, cwd, prs, true, bots).map_err(|e| polled_call("lookup", e))
}

/// The lookup's query: one alias `p<i>` per pull request; `viewer` adds the two fields of the direct read.
fn lookup_query(prs: &[(String, i64)], viewer: bool) -> Result<String, CpdError> {
    let extra = if viewer {
        " viewerDidAuthor viewerLatestReviewRequest { id } repository { isArchived }"
    } else {
        ""
    };
    let mut query = String::from("query {");
    for (i, (repo, number)) in prs.iter().enumerate() {
        let (owner, name) = repo_parts(repo).ok_or_else(|| {
            CpdError::new(
                ErrorCode::GhFailed,
                format!("{repo:?} is not an owner/name repo."),
            )
            .with("repo", repo.as_str())
        })?;
        query.push_str(&format!(
            " p{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ pullRequest(number: {number}) {{ {}{extra} }} }}",
            pr_fields!()
        ));
    }
    query.push_str(" }");
    Ok(query)
}

/// The comments of the pull requests `(owner/name, number)`, one alias each, in one call, 15 s at most: what
/// `comments::query` reads, `parse`d with the `bots` logins (contract §7.6 step 3a). Any failure is `gh_failed` with
/// the poll's `details` (`call` `lookup`). An empty `prs` makes no call.
pub fn read_comments(
    runner: &dyn Runner,
    cwd: &Path,
    prs: &[(String, i64)],
    bots: &[String],
) -> Result<Vec<comments::Read>, CpdError> {
    if prs.is_empty() {
        return Ok(Vec::new());
    }
    let read = || {
        for (repo, _) in prs {
            repo_parts(repo).ok_or_else(|| {
                CpdError::new(
                    ErrorCode::GhFailed,
                    format!("{repo:?} is not an owner/name repo."),
                )
                .with("repo", repo.as_str())
            })?;
        }
        let pairs: Vec<(&str, i64)> = prs
            .iter()
            .map(|(repo, number)| (repo.as_str(), *number))
            .collect();
        let (args, out) = graphql(runner, cwd, &comments::query(&pairs))?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let reply = Reply {
            args: &refs,
            out: &out,
        };
        if out.exit != Some(0) {
            let json = serde_json::from_str::<Value>(&out.stdout);
            let errors = json.as_ref().is_ok_and(|v| v.get("errors").is_some());
            return Err(reply.fail_unless_transient(
                format!(
                    "gh {} exited {}.",
                    refs[0],
                    out.exit
                        .map_or("with a signal".to_owned(), |c| c.to_string())
                ),
                errors,
                json.is_err(),
            ));
        }
        comments::parse(&pairs, &out.stdout, bots).map_err(|e| reply.fail(e.message().to_owned()))
    };
    read().map_err(|e| polled_call("lookup", e))
}

/// One pull request's checks read: the head commit's contexts, 100 at a time (contract §7.6 step 3b).
const CHECKS_FIELDS: &str = "nodes { __typename ... on CheckRun { name status conclusion startedAt checkSuite { workflowRun { workflow { name } } } } ... on StatusContext { context state createdAt } }";

/// The checks read's query: one alias `p<i>` per `(owner/name, number, after)`, `after` the cursor of the page wanted.
fn checks_query(prs: &[(&str, i64, Option<&str>)]) -> String {
    let mut query = String::from("query {");
    for (i, (repo, number, after)) in prs.iter().enumerate() {
        let (owner, name) = repo_parts(repo).unwrap_or_default();
        let page = after.map_or(String::new(), |c| format!(", after: \"{c}\""));
        // The labels are read once, on the first page.
        let labels = if after.is_none() {
            " labels(first: 100) { nodes { name } }"
        } else {
            ""
        };
        query.push_str(&format!(
            " p{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ pullRequest(number: {number}) {{{labels} commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ contexts(first: 100{page}) {{ pageInfo {{ hasNextPage endCursor }} {CHECKS_FIELDS} }} }} }} }} }} }} }}"
        ));
    }
    query.push_str(" }");
    query
}

#[derive(Deserialize)]
struct RawChecksPr {
    #[serde(default)]
    labels: Option<RawLabels>,
    commits: RawChecksCommits,
}
#[derive(Deserialize)]
struct RawLabels {
    nodes: Vec<RawLabel>,
}
#[derive(Deserialize)]
struct RawLabel {
    name: String,
}
#[derive(Deserialize)]
struct RawChecksCommits {
    nodes: Vec<RawChecksNode>,
}
#[derive(Deserialize)]
struct RawChecksNode {
    commit: Option<RawChecksCommit>,
}
#[derive(Deserialize)]
struct RawChecksCommit {
    #[serde(rename = "statusCheckRollup")]
    rollup: Option<RawChecksRollup>,
}
#[derive(Deserialize)]
struct RawChecksRollup {
    contexts: RawContexts,
}
#[derive(Deserialize)]
struct RawContexts {
    #[serde(rename = "pageInfo")]
    page_info: RawPageInfo,
    nodes: Vec<RawCheck>,
}
/// A GraphQL node: the workflow name is nested and a StatusContext's time is `createdAt`.
#[derive(Deserialize)]
struct RawCheck {
    name: Option<String>,
    context: Option<String>,
    conclusion: Option<String>,
    state: Option<String>,
    #[serde(rename = "startedAt", default)]
    started_at: Option<String>,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
    #[serde(rename = "checkSuite", default)]
    check_suite: Option<RawSuite>,
}
#[derive(Deserialize)]
struct RawSuite {
    #[serde(rename = "workflowRun")]
    workflow_run: Option<RawWorkflowRun>,
}
#[derive(Deserialize)]
struct RawWorkflowRun {
    workflow: Option<RawWorkflow>,
}
#[derive(Deserialize)]
struct RawWorkflow {
    name: Option<String>,
}
impl From<RawCheck> for Check {
    fn from(r: RawCheck) -> Check {
        Check {
            name: r.name,
            context: r.context,
            conclusion: r.conclusion,
            state: r.state,
            url: None,
            workflow: r
                .check_suite
                .and_then(|s| s.workflow_run)
                .and_then(|w| w.workflow)
                .and_then(|w| w.name),
            started_at: r.started_at.or(r.created_at),
        }
    }
}
#[derive(Deserialize)]
struct RawPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

/// One page of one pull request: its checks and the cursor of the next page, if any. A null commit or rollup is no checks.
/// The checks of one pull request, the cursor of its next page and its other messages.
type ChecksPage = (Vec<Check>, Option<String>, Vec<String>);

fn parse_checks(reply: &Reply, firsts: &[bool]) -> Result<Vec<ChecksPage>, CpdError> {
    let count = firsts.len();
    #[derive(Deserialize)]
    struct Repo {
        #[serde(rename = "pullRequest")]
        pull_request: Option<RawChecksPr>,
    }
    let (mut data, errors): (BTreeMap<String, Option<Repo>>, _) = reply.envelope()?;
    if let Some(e) = errors.first() {
        return Err(reply.fail(format!(
            "gh api graphql answered with errors: {}",
            e.message.as_deref().unwrap_or("(no message)")
        )));
    }
    let mut pages = Vec::with_capacity(count);
    for (i, &first) in firsts.iter().enumerate() {
        let alias = format!("p{i}");
        let pull = data
            .remove(&alias)
            .ok_or_else(|| reply.fail(format!("gh api graphql answered without {alias}.")))?
            .and_then(|r| r.pull_request)
            .ok_or_else(|| reply.fail(format!("gh api graphql answered null for {alias}.")))?;
        // A first page without `labels` is a broken reply; later pages do not ask for them.
        let labels = match (pull.labels, first) {
            (Some(l), _) => l.nodes.into_iter().map(|l| l.name).collect(),
            (None, false) => Vec::new(),
            (None, true) => {
                return Err(reply.fail(format!("gh api graphql answered {alias} without labels.")));
            }
        };
        let rollup = pull
            .commits
            .nodes
            .into_iter()
            .next()
            .and_then(|n| n.commit)
            .and_then(|c| c.rollup);
        pages.push(match rollup {
            None => (Vec::new(), None, labels),
            Some(r) => {
                let next = match (
                    r.contexts.page_info.has_next_page,
                    r.contexts.page_info.end_cursor,
                ) {
                    (false, _) => None,
                    (true, Some(cursor)) => Some(cursor),
                    (true, None) => {
                        return Err(reply.fail(format!(
                            "gh api graphql answered {alias} with a next page and no endCursor."
                        )));
                    }
                };
                (
                    r.contexts.nodes.into_iter().map(Check::from).collect(),
                    next,
                    labels,
                )
            }
        });
    }
    Ok(pages)
}

/// One pull request's checks read: its contexts and its label names.
#[derive(Default)]
pub struct PrChecks {
    pub checks: Vec<Check>,
    pub labels: Vec<String>,
}

/// The checks of the pull requests `(owner/name, number)` (contract §7.6 step 3b): one alias each, in one call, 15 s at
/// most; each pull request with more than 100 contexts is read again, page by page, with those only. Each result is the
/// Workflow's typed checks (a CheckRun has `name`, a StatusContext `context`). Any failure
/// is `gh_failed` with the poll's `details` (`call` `lookup`). An empty `prs` makes no call.
pub fn read_checks(
    runner: &dyn Runner,
    cwd: &Path,
    prs: &[(String, i64)],
) -> Result<Vec<PrChecks>, CpdError> {
    let mut all: Vec<PrChecks> = prs.iter().map(|_| PrChecks::default()).collect();
    let mut pending: Vec<(usize, Option<String>)> = (0..prs.len()).map(|i| (i, None)).collect();
    while !pending.is_empty() {
        let page = || {
            for (repo, _) in prs {
                repo_parts(repo).ok_or_else(|| {
                    CpdError::new(
                        ErrorCode::GhFailed,
                        format!("{repo:?} is not an owner/name repo."),
                    )
                    .with("repo", repo.as_str())
                })?;
            }
            let asked: Vec<(&str, i64, Option<&str>)> = pending
                .iter()
                .map(|(i, after)| (prs[*i].0.as_str(), prs[*i].1, after.as_deref()))
                .collect();
            let (args, out) = graphql(runner, cwd, &checks_query(&asked))?;
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let reply = Reply {
                args: &refs,
                out: &out,
            };
            if out.exit != Some(0) {
                let json = serde_json::from_str::<Value>(&out.stdout);
                let errors = json.as_ref().is_ok_and(|v| v.get("errors").is_some());
                return Err(reply.fail_unless_transient(
                    format!(
                        "gh {} exited {}.",
                        refs[0],
                        out.exit
                            .map_or("with a signal".to_owned(), |c| c.to_string())
                    ),
                    errors,
                    json.is_err(),
                ));
            }
            let firsts: Vec<bool> = asked.iter().map(|(_, _, after)| after.is_none()).collect();
            parse_checks(&reply, &firsts)
        };
        let pages = page().map_err(|e| polled_call("lookup", e))?;
        let mut next = Vec::new();
        for ((i, after), (checks, cursor, labels)) in pending.iter().zip(pages) {
            all[*i].checks.extend(checks);
            if after.is_none() {
                all[*i].labels = labels;
            }
            if let Some(cursor) = cursor {
                next.push((*i, Some(cursor)));
            }
        }
        pending = next;
    }
    Ok(all)
}

fn lookup_call(
    runner: &dyn Runner,
    cwd: &Path,
    prs: &[(String, i64)],
    viewer: bool,
    bots: &[String],
) -> Result<Vec<Option<Listed>>, CpdError> {
    let query = lookup_query(prs, viewer)?;
    let (args, out) = graphql(runner, cwd, &query)?;
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    parse_lookup(
        &Reply {
            args: &refs,
            out: &out,
        },
        prs.len(),
        viewer,
        bots,
    )
}

/// The `count` aliases `p0`… of a lookup's reply; with `viewer`, each `OPEN` one carries its viewer flags.
fn parse_lookup(
    reply: &Reply,
    count: usize,
    viewer: bool,
    bots: &[String],
) -> Result<Vec<Option<Listed>>, CpdError> {
    let (mut data, errors): (BTreeMap<String, Option<RawRepoLookup>>, _) = reply.envelope()?;
    if let Some(other) = errors
        .iter()
        .find(|e| e.kind.as_deref() != Some("NOT_FOUND"))
    {
        return Err(reply.fail(format!(
            "gh api graphql answered with errors: {}",
            other.message.as_deref().unwrap_or("(no message)")
        )));
    }
    let not_found = |alias: &str| {
        errors.iter().any(|e| {
            e.path
                .as_ref()
                .and_then(|p| p.first())
                .and_then(Value::as_str)
                == Some(alias)
        })
    };
    let mut found = Vec::with_capacity(count);
    for i in 0..count {
        let alias = format!("p{i}");
        let raw = data
            .remove(&alias)
            .ok_or_else(|| reply.fail(format!("gh api graphql answered without {alias}.")))?
            .and_then(|r| r.pull_request);
        found.push(match raw {
            Some(raw) => {
                let mut membership = Membership::default();
                if viewer {
                    let authored = raw.viewer_did_author.ok_or_else(|| {
                        reply.fail(format!(
                            "gh api graphql answered {alias} without viewerDidAuthor."
                        ))
                    })?;
                    let request = raw.viewer_review_request.clone().ok_or_else(|| {
                        reply.fail(format!(
                            "gh api graphql answered {alias} without viewerLatestReviewRequest."
                        ))
                    })?;
                    raw.repository.is_archived.ok_or_else(|| {
                        reply.fail(format!(
                            "gh api graphql answered {alias} without isArchived."
                        ))
                    })?;
                    membership = Membership {
                        authored,
                        review_requested: !request.is_null(),
                    };
                }
                let pr = reply.polled(raw, bots)?;
                if pr.state != PrState::Open {
                    membership = Membership::default();
                }
                Some(Listed { pr, membership })
            }
            None if not_found(&alias) => None,
            None => {
                return Err(reply.fail(format!(
                    "gh api graphql answered null for {alias} without NOT_FOUND."
                )));
            }
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARGS: [&str; 4] = ["api", "graphql", "-f", "query=Q"];

    /// A runner that answers nothing: the tests that use it make no call.
    fn runner() -> crate::host::runner::FakeRunner {
        crate::host::runner::FakeRunner::new(crate::clock::ManualClock::new())
    }

    fn reply(stdout: &str, exit: i32) -> Output {
        Output {
            stdout: stdout.to_owned(),
            stderr: "gh: boom".to_owned(),
            exit: Some(exit),
        }
    }

    fn pr(repo: &str, number: i64, draft: bool, state: &str, rollup: &str) -> String {
        format!(
            r#"{{"repository":{{"nameWithOwner":"{repo}","isArchived":false}},"number":{number},"url":"https://github.com/{repo}/pull/{number}","title":"T{number}",
            "isDraft":{draft},"state":"{state}","createdAt":"2026-05-01T10:00:00Z","updatedAt":"2026-05-02T10:00:00Z","mergedAt":null,"reviewDecision":"APPROVED","latestOpinionatedReviews":{{"nodes":[{{"state":"APPROVED","author":{{"__typename":"User","login":"alice"}}}}]}},
            "mergeable":"CONFLICTING","mergeStateStatus":"DIRTY",
            "reviewThreads":{{"nodes":[{{"id":"T1","isResolved":false}},{{"id":"T2","isResolved":true}}]}},
            "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{rollup}}}}}]}}}}"#
        )
    }

    fn search(out: &Output) -> Result<Search, CpdError> {
        parse_search(&Reply { args: &ARGS, out }, &[])
    }

    fn looked_up(out: &Output, count: usize) -> Result<Vec<Option<PolledPr>>, CpdError> {
        Ok(
            parse_lookup(&Reply { args: &ARGS, out }, count, false, &[])?
                .into_iter()
                .map(|p| p.map(|l| l.pr))
                .collect(),
        )
    }

    fn read(out: &Output, count: usize) -> Result<Vec<Option<Listed>>, CpdError> {
        parse_lookup(&Reply { args: &ARGS, out }, count, true, &[])
    }

    #[test]
    fn the_checks_query_is_pinned() {
        assert_eq!(
            checks_query(&[("o/a", 7, None), ("o/b", 8, Some("c1"))]),
            "query { p0: repository(owner: \"o\", name: \"a\") { pullRequest(number: 7) { labels(first: 100) { nodes { name } } commits(last: 1) { nodes { commit { statusCheckRollup { \
             contexts(first: 100) { pageInfo { hasNextPage endCursor } nodes { __typename ... on CheckRun { name status conclusion startedAt checkSuite { workflowRun { workflow { name } } } } \
             ... on StatusContext { context state createdAt } } } } } } } } } \
             p1: repository(owner: \"o\", name: \"b\") { pullRequest(number: 8) { commits(last: 1) { nodes { commit { statusCheckRollup { \
             contexts(first: 100, after: \"c1\") { pageInfo { hasNextPage endCursor } nodes { __typename ... on CheckRun { name status conclusion startedAt checkSuite { workflowRun { workflow { name } } } } \
             ... on StatusContext { context state createdAt } } } } } } } } } }"
        );
    }

    #[test]
    fn checks_are_parsed_by_name_or_context_and_a_null_rollup_is_no_checks() {
        let body = r#"{"data":{
            "p0":{"pullRequest":{"commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{"pageInfo":{"hasNextPage":true,"endCursor":"c1"},"nodes":[
                {"__typename":"CheckRun","name":"a","status":"COMPLETED","conclusion":"CANCELLED"},
                {"__typename":"CheckRun","name":"b","status":"IN_PROGRESS","conclusion":null},
                {"__typename":"StatusContext","context":"c","state":"PENDING"}]}}}}]}}},
            "p1":{"pullRequest":{"commits":{"nodes":[{"commit":null}]}}},
            "p2":{"pullRequest":{"commits":{"nodes":[{"commit":{"statusCheckRollup":null}}]}}}}}"#;
        let out = reply(body, 0);
        let pages = parse_checks(
            &Reply {
                args: &ARGS,
                out: &out,
            },
            &[false; 3],
        )
        .unwrap();
        assert_eq!(pages[0].1.as_deref(), Some("c1"));
        let got: Vec<(Option<&str>, Option<&str>, Option<&str>)> = pages[0]
            .0
            .iter()
            .map(|c| {
                (
                    c.name.as_deref().or(c.context.as_deref()),
                    c.conclusion.as_deref(),
                    c.state.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                (Some("a"), Some("CANCELLED"), None),
                (Some("b"), None, None),
                (Some("c"), None, Some("PENDING"))
            ]
        );
        assert!(pages[1].0.is_empty() && pages[2].0.is_empty() && pages[1].1.is_none());
    }

    #[test]
    fn labels_are_parsed_and_a_first_page_without_them_is_gh_failed() {
        let with = r#"{"data":{"p0":{"pullRequest":{"labels":{"nodes":[{"name":"dont merge"}]},"commits":{"nodes":[]}}}}}"#;
        let out = reply(with, 0);
        assert_eq!(
            parse_checks(
                &Reply {
                    args: &ARGS,
                    out: &out
                },
                &[true]
            )
            .unwrap()[0]
                .2,
            ["dont merge"]
        );
        let without = r#"{"data":{"p0":{"pullRequest":{"commits":{"nodes":[]}}}}}"#;
        let out = reply(without, 0);
        assert_eq!(
            parse_checks(
                &Reply {
                    args: &ARGS,
                    out: &out
                },
                &[true]
            )
            .unwrap_err()
            .code(),
            ErrorCode::GhFailed
        );
    }

    #[test]
    fn checks_carry_the_workflow_and_the_start_time() {
        let body = r#"{"data":{"p0":{"pullRequest":{"commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
            {"__typename":"CheckRun","name":"a","status":"COMPLETED","conclusion":"FAILURE","startedAt":"2026-10-30T04:19:08Z","checkSuite":{"workflowRun":{"workflow":{"name":"W"}}}},
            {"__typename":"CheckRun","name":"b","status":"COMPLETED","conclusion":null,"startedAt":null,"checkSuite":{"workflowRun":null}},
            {"__typename":"StatusContext","context":"c","state":"SUCCESS","createdAt":"2026-10-30T14:39:33Z"}]}}}}]}}}}}"#;
        let out = reply(body, 0);
        let pages = parse_checks(
            &Reply {
                args: &ARGS,
                out: &out,
            },
            &[false],
        )
        .unwrap();
        let got: Vec<(Option<&str>, Option<&str>)> = pages[0]
            .0
            .iter()
            .map(|c| (c.workflow.as_deref(), c.started_at.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                (Some("W"), Some("2026-10-30T04:19:08Z")),
                (None, None),
                (None, Some("2026-10-30T14:39:33Z"))
            ]
        );
    }

    #[test]
    fn a_start_time_that_is_not_a_string_is_gh_failed() {
        let body = r#"{"data":{"p0":{"pullRequest":{"commits":{"nodes":[{"commit":{"statusCheckRollup":{"contexts":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
            {"__typename":"CheckRun","name":"a","status":"COMPLETED","conclusion":"FAILURE","startedAt":5}]}}}}]}}}}}"#;
        let out = reply(body, 0);
        assert_eq!(
            parse_checks(
                &Reply {
                    args: &ARGS,
                    out: &out
                },
                &[false]
            )
            .unwrap_err()
            .code(),
            ErrorCode::GhFailed
        );
    }

    #[test]
    fn a_checks_reply_missing_an_alias_or_with_a_null_pull_request_is_gh_failed() {
        for body in [
            r#"{"data":{}}"#,
            r#"{"data":{"p0":{"pullRequest":null}}}"#,
            r#"{"data":{"p0":null}}"#,
        ] {
            let out = reply(body, 0);
            assert_eq!(
                parse_checks(
                    &Reply {
                        args: &ARGS,
                        out: &out
                    },
                    &[false]
                )
                .unwrap_err()
                .code(),
                ErrorCode::GhFailed,
                "{body}"
            );
        }
    }

    #[test]
    fn the_search_query_is_pinned() {
        assert_eq!(
            SEARCH_QUERY,
            "query { search(type: ISSUE, query: \"is:pr is:open author:@me archived:false sort:created-asc\", first: 100) { \
             issueCount nodes { ... on PullRequest { repository { nameWithOwner defaultBranchRef { name } isArchived autoMergeAllowed squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed deleteBranchOnMerge } number url title isDraft state createdAt updatedAt mergedAt \
             baseRefName reviewDecision latestOpinionatedReviews(first: 20) { nodes { state author { __typename login } } } mergeable mergeStateStatus headRefOid commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } reviewThreads(first: 100) { nodes { id isResolved } } } } } \
             reviewRequests: search(type: ISSUE, query: \"review-requested:@me is:pr is:open archived:false sort:created-asc\", first: 100) { \
             issueCount nodes { ... on PullRequest { repository { nameWithOwner defaultBranchRef { name } isArchived autoMergeAllowed squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed deleteBranchOnMerge } number url title isDraft state createdAt updatedAt mergedAt \
             baseRefName reviewDecision mergeable mergeStateStatus headRefOid } } } \
             rateLimit { cost remaining resetAt } }"
        );
    }

    #[test]
    fn the_review_nodes_are_typed_with_a_bot_type_a_null_author_and_a_missing_field() {
        let with = |reviews: &str| {
            let node = pr("acme/pay", 1, false, "OPEN", "null");
            let mut v: Value = serde_json::from_str(&node).unwrap();
            match serde_json::from_str::<Value>(reviews).unwrap() {
                Value::Null => {
                    v.as_object_mut()
                        .unwrap()
                        .remove("latestOpinionatedReviews");
                }
                r => v["latestOpinionatedReviews"] = r,
            }
            let out = reply(&format!(r#"{{"data":{{"p0":{{"pullRequest":{v}}}}}}}"#), 0);
            looked_up(&out, 1)
                .unwrap()
                .remove(0)
                .unwrap()
                .review_decision
        };
        let nodes = |n: &str| format!(r#"{{"nodes":[{n}]}}"#);
        let approved = |a: &str| nodes(&format!(r#"{{"state":"APPROVED","author":{a}}}"#));
        assert_eq!(
            with(&approved(r#"{"__typename":"Bot","login":"lane"}"#)),
            ReviewDecision::None
        );
        assert_eq!(with(&approved("null")), ReviewDecision::None);
        assert_eq!(
            with(&approved(r#"{"__typename":"User","login":"alice"}"#)),
            ReviewDecision::Approved
        );
        assert_eq!(with("null"), ReviewDecision::None);
    }

    #[test]
    fn a_search_of_two_pull_requests_is_typed() {
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":2,"nodes":[{},{}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4990,"resetAt":"2026-05-01T11:00:00Z"}}}}}}"#,
            pr("o/a", 7, true, "OPEN", r#"{"state":"FAILURE"}"#),
            pr("o/b", 8, false, "OPEN", "null"),
        );
        let found = search(&reply(&body, 0)).unwrap();
        assert_eq!(
            (found.issue_count, found.rate.cost, found.rate.remaining),
            (2, 1, 4990)
        );
        assert_eq!(found.rate.reset_at, "2026-05-01T11:00:00Z");
        let [a, b] = &found.prs.iter().map(|l| &l.pr).collect::<Vec<_>>()[..] else {
            panic!("{:?}", found.prs)
        };
        assert_eq!(
            (a.repo.as_str(), a.number, a.draft, a.state, a.checks),
            ("o/a", 7, true, PrState::Open, ChecksSummary::Failed)
        );
        assert_eq!(
            (a.review_decision, a.title.as_str(), a.merged_at.as_deref()),
            (ReviewDecision::Approved, "T7", None)
        );
        assert_eq!(a.url, "https://github.com/o/a/pull/7");
        assert_eq!(a.open_threads, vec!["T1".to_owned()]);
        assert_eq!(a.created_at, "2026-05-01T10:00:00Z");
        assert_eq!((b.draft, b.checks), (false, ChecksSummary::None));
        assert_eq!(
            (a.mergeable, a.merge_state),
            (Mergeable::Conflicting, MergeState::Dirty)
        );
    }

    #[test]
    fn a_search_reports_each_pull_requests_base_and_default_branch_and_none_when_the_reply_has_none()
     {
        let based = |node: String, base: &str| {
            node.replace(
                r#""mergedAt":null"#,
                &format!(r#""baseRefName":"{base}","mergedAt":null"#),
            )
            .replace(
                r#"{"nameWithOwner":"o/a","isArchived":false}"#,
                r#"{"nameWithOwner":"o/a","isArchived":false,"defaultBranchRef":{"name":"main"}}"#,
            )
        };
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":2,"nodes":[{},{}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#,
            based(pr("o/a", 1, false, "OPEN", "null"), "feature"),
            pr("o/a", 2, false, "OPEN", "null"),
        );
        let found = search(&reply(&body, 0)).unwrap();
        let [a, b] = &found.prs.iter().map(|l| &l.pr).collect::<Vec<_>>()[..] else {
            panic!("{:?}", found.prs)
        };
        assert_eq!(
            (a.base_ref.as_deref(), a.default_branch.as_deref()),
            (Some("feature"), Some("main"))
        );
        assert_eq!(
            (b.base_ref.as_deref(), b.default_branch.as_deref()),
            (None, None)
        );
    }

    #[test]
    fn the_two_searches_are_merged_into_one_list_with_their_memberships() {
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":2,"nodes":[{},{}]}},
            "reviewRequests":{{"issueCount":2,"nodes":[{},{}]}},
            "rateLimit":{{"cost":2,"remaining":4990,"resetAt":"r"}}}}}}"#,
            pr("o/a", 1, false, "OPEN", "null"),
            pr("o/a", 2, false, "OPEN", "null"),
            pr("o/a", 2, false, "OPEN", "null"),
            pr("o/b", 3, false, "OPEN", "null"),
        );
        let found = search(&reply(&body, 0)).unwrap();
        assert_eq!(found.issue_count, 4);
        let listed: Vec<_> = found
            .prs
            .iter()
            .map(|l| {
                (
                    l.pr.repo.as_str(),
                    l.pr.number,
                    l.membership.authored,
                    l.membership.review_requested,
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("o/a", 1, true, false),
                ("o/a", 2, true, true),
                ("o/b", 3, false, true)
            ]
        );
    }

    #[test]
    fn a_reply_without_the_review_request_search_is_gh_failed() {
        let body = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":9,"resetAt":"r"}}}"#;
        assert_eq!(
            search(&reply(body, 0)).unwrap_err().code(),
            ErrorCode::GhFailed
        );
    }

    #[test]
    fn mergeable_and_merge_state_are_typed_whatever_they_are() {
        for (mergeable, merge_state, want) in [
            (
                "MERGEABLE",
                "BLOCKED",
                (Mergeable::Clean, MergeState::Blocked),
            ),
            (
                "UNKNOWN",
                "UNKNOWN",
                (Mergeable::Unknown, MergeState::Unknown),
            ),
            (
                "MERGEABLE",
                "HAS_HOOKS",
                (Mergeable::Clean, MergeState::HasHooks),
            ),
        ] {
            let node = pr("o/a", 7, false, "OPEN", "null").replace(
                r#""mergeable":"CONFLICTING","mergeStateStatus":"DIRTY""#,
                &format!(r#""mergeable":"{mergeable}","mergeStateStatus":"{merge_state}""#),
            );
            let body = format!(
                r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
            );
            let found = &search(&reply(&body, 0)).unwrap().prs[0].pr;
            assert_eq!(
                (found.mergeable, found.merge_state),
                want,
                "{mergeable} {merge_state}"
            );
        }
    }

    #[test]
    fn a_mergeable_or_merge_state_outside_the_sets_is_gh_failed_in_the_search_and_the_lookup() {
        for (name, from, to) in [
            (
                "mergeable",
                r#""mergeable":"CONFLICTING""#,
                r#""mergeable":"MAYBE""#,
            ),
            (
                "merge_state",
                r#""mergeStateStatus":"DIRTY""#,
                r#""mergeStateStatus":"QUEUED""#,
            ),
            (
                "lowercase",
                r#""mergeStateStatus":"DIRTY""#,
                r#""mergeStateStatus":"dirty""#,
            ),
            (
                "null mergeable",
                r#""mergeable":"CONFLICTING""#,
                r#""mergeable":null"#,
            ),
        ] {
            let node = pr("o/a", 7, false, "OPEN", "null").replace(from, to);
            let body = format!(
                r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
            );
            assert_eq!(
                search(&reply(&body, 0)).unwrap_err().code(),
                ErrorCode::GhFailed,
                "search {name}"
            );
            let body = format!(r#"{{"data":{{"p0":{{"pullRequest":{node}}}}}}}"#);
            assert_eq!(
                looked_up(&reply(&body, 0), 1).unwrap_err().code(),
                ErrorCode::GhFailed,
                "lookup {name}"
            );
        }
    }

    #[test]
    fn a_pull_request_without_the_two_fields_is_gh_failed() {
        let node = pr("o/a", 7, false, "OPEN", "null").replace(
            r#""mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","#,
            "",
        );
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{node}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
        );
        assert_eq!(
            search(&reply(&body, 0)).unwrap_err().code(),
            ErrorCode::GhFailed
        );
    }

    #[test]
    fn a_pull_request_without_a_commit_has_no_checks() {
        let body = pr("o/a", 7, false, "OPEN", "null").replace(
            r#"{"commit":{"statusCheckRollup":null}}"#,
            r#"{"commit":null}"#,
        );
        let body = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{body}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#
        );
        assert_eq!(
            search(&reply(&body, 0)).unwrap().prs[0].pr.checks,
            ChecksSummary::None
        );
    }

    #[test]
    fn a_search_that_is_not_an_answer_is_gh_failed() {
        let nodes = pr("o/a", 7, false, "OPEN", "null");
        let with_errors = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{nodes}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}},"errors":[{{"message":"nope"}}]}}"#
        );
        let unknown_state = format!(
            r#"{{"data":{{"search":{{"issueCount":1,"nodes":[{}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":9,"resetAt":"r"}}}}}}"#,
            pr("o/a", 7, false, "WEIRD", "null")
        );
        for (name, stdout, exit) in [
            ("empty", "", 0),
            ("html", "<html><body>Unicorn!</body></html>", 0),
            (
                "errors only",
                r#"{"errors":[{"message":"Expected NAME"}]}"#,
                1,
            ),
            ("data and errors", with_errors.as_str(), 1),
            (
                "null search",
                r#"{"data":{"search":null,"rateLimit":null}}"#,
                0,
            ),
            ("empty data", "{}", 0),
            ("unknown state", unknown_state.as_str(), 0),
        ] {
            let e = search(&reply(stdout, exit)).unwrap_err();
            assert_eq!(e.code(), ErrorCode::GhFailed, "{name}");
        }
    }

    #[test]
    fn an_http_5xx_is_read_from_stderr_or_from_stdout_when_stderr_is_empty() {
        let out = |stdout: &str, stderr: &str| Output {
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            exit: Some(1),
        };
        assert_eq!(
            http_5xx(&out(
                "",
                "gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)"
            ))
            .as_deref(),
            Some("HTTP 502")
        );
        assert_eq!(
            http_5xx(&out("", "gh: Service Unavailable (HTTP 503)")).as_deref(),
            Some("HTTP 503")
        );
        assert_eq!(
            http_5xx(&out("gh: HTTP 504: Gateway Timeout", "")).as_deref(),
            Some("HTTP 504")
        );
        // stdout is read only when stderr is empty; a 4xx, a bare 5 and a body without a status are no 5xx.
        assert_eq!(http_5xx(&out("HTTP 502", "gh: exit status 1")), None);
        assert_eq!(http_5xx(&out("", "gh: HTTP 401: Bad credentials")), None);
        assert_eq!(http_5xx(&out("", "HTTP 5")), None);
        assert_eq!(http_5xx(&out("<html>502 Bad Gateway</html>", "")), None);
    }

    #[test]
    fn a_failure_of_a_poll_call_holds_call_transient_and_reason_whatever_it_was() {
        let details = |e: CpdError| Value::Object(polled_call("search", e).details().clone());
        let plain = details(CpdError::new(ErrorCode::GhFailed, "x"));
        assert_eq!(
            (&plain["call"], &plain["transient"], &plain["reason"]),
            (&json!("search"), &json!(false), &Value::Null)
        );
        let transient = details(
            CpdError::new(ErrorCode::GhFailed, "x")
                .with("transient", true)
                .with("reason", "HTTP 502"),
        );
        assert_eq!(
            (&transient["transient"], &transient["reason"]),
            (&json!(true), &json!("HTTP 502"))
        );
        // Only an answer that is no GraphQL result and exited non-zero is transient.
        let body = |stdout: &str, exit: i32| {
            search(&Output {
                stdout: stdout.to_owned(),
                stderr: "gh: HTTP 502: Bad Gateway".to_owned(),
                exit: Some(exit),
            })
        };
        let transient =
            |r: Result<Search, CpdError>| r.unwrap_err().details().get("transient").cloned();
        assert_eq!(transient(body("<html>", 1)), Some(json!(true)));
        assert_eq!(transient(body("<html>", 0)), None);
        assert_eq!(transient(body(r#"{"errors":[{"message":"x"}]}"#, 1)), None);
    }

    #[test]
    fn a_failure_carries_argv_exit_and_stderr() {
        let e = search(&reply("", 1)).unwrap_err();
        let details = serde_json::to_value(&e).unwrap();
        let details = &details["error"]["details"];
        assert_eq!(
            details["argv"],
            json!(["gh", "api", "graphql", "-f", "query=Q"])
        );
        assert_eq!(details["exit"], json!(1));
        assert_eq!(details["stderr"], json!("gh: boom"));
    }

    #[test]
    fn a_lookup_with_a_not_found_alias_is_a_result_though_gh_exited_1() {
        let body = format!(
            r#"{{"data":{{"p0":{{"pullRequest":{}}},"p1":{{"pullRequest":null}}}},
            "errors":[{{"type":"NOT_FOUND","path":["p1","pullRequest"],"message":"Could not resolve"}}]}}"#,
            pr("o/a", 7, false, "MERGED", r#"{"state":"SUCCESS"}"#)
                .replace(r#""mergedAt":null"#, r#""mergedAt":"2026-05-02T00:00:00Z""#),
        );
        let found = looked_up(&reply(&body, 1), 2).unwrap();
        let [Some(a), None] = &found[..] else {
            panic!("{found:?}")
        };
        assert_eq!(
            (a.number, a.state, a.checks, a.merged_at.as_deref()),
            (
                7,
                PrState::Merged,
                ChecksSummary::Passed,
                Some("2026-05-02T00:00:00Z")
            )
        );
    }

    #[test]
    fn the_direct_read_selects_the_two_viewer_fields_after_the_poll_fields_and_the_lookup_does_not()
    {
        let prs = [("o/a".to_owned(), 7), ("o/b".to_owned(), 8)];
        let plain = lookup_query(&prs, false).unwrap();
        let direct = lookup_query(&prs, true).unwrap();
        assert!(!plain.contains("viewer"));
        assert_eq!(
            direct
                .matches(" viewerDidAuthor viewerLatestReviewRequest { id } repository { isArchived } } }")
                .count(),
            2
        );
        assert!(direct.starts_with("query { p0: repository(owner: \"o\", name: \"a\") { pullRequest(number: 7) { repository { nameWithOwner defaultBranchRef { name } }"));
        assert_eq!(
            direct.replace(
                " viewerDidAuthor viewerLatestReviewRequest { id } repository { isArchived }",
                ""
            ),
            plain
        );
    }

    fn with_viewer(node: String, author: &str, request: &str) -> String {
        node.replace(r#""mergeStateStatus":"DIRTY","#, &format!(r#""mergeStateStatus":"DIRTY","viewerDidAuthor":{author},"viewerLatestReviewRequest":{request},"#))
    }

    #[test]
    fn the_direct_read_types_the_viewer_flags_of_an_open_pull_request_and_clears_those_of_a_decided_one()
     {
        let open = with_viewer(pr("o/a", 1, false, "OPEN", "null"), "true", r#"{"id":"R"}"#);
        let other = with_viewer(pr("o/a", 2, false, "OPEN", "null"), "false", "null");
        let closed = with_viewer(
            pr("o/a", 3, false, "CLOSED", "null"),
            "true",
            r#"{"id":"R"}"#,
        );
        let body = format!(
            r#"{{"data":{{"p0":{{"pullRequest":{open}}},"p1":{{"pullRequest":{other}}},"p2":{{"pullRequest":{closed}}}}}}}"#
        );
        let found = read(&reply(&body, 0), 3).unwrap();
        let flags: Vec<_> = found
            .iter()
            .map(|l| {
                l.as_ref()
                    .map(|l| (l.membership.authored, l.membership.review_requested))
            })
            .collect();
        assert_eq!(
            flags,
            vec![
                Some((true, true)),
                Some((false, false)),
                Some((false, false))
            ]
        );
    }

    #[test]
    fn the_direct_read_without_either_viewer_field_is_gh_failed_and_a_not_found_alias_is_none() {
        for (name, node) in [
            (
                "no author",
                pr("o/a", 1, false, "OPEN", "null").replace(
                    r#""mergeStateStatus":"DIRTY","#,
                    r#""mergeStateStatus":"DIRTY","viewerLatestReviewRequest":null,"#,
                ),
            ),
            (
                "no request",
                pr("o/a", 1, false, "OPEN", "null").replace(
                    r#""mergeStateStatus":"DIRTY","#,
                    r#""mergeStateStatus":"DIRTY","viewerDidAuthor":true,"#,
                ),
            ),
            ("neither", pr("o/a", 1, false, "MERGED", "null")),
        ] {
            let body = format!(r#"{{"data":{{"p0":{{"pullRequest":{node}}}}}}}"#);
            assert_eq!(
                read(&reply(&body, 0), 1).unwrap_err().code(),
                ErrorCode::GhFailed,
                "{name}"
            );
        }
        let gone =
            r#"{"data":{"p0":null},"errors":[{"type":"NOT_FOUND","path":["p0"],"message":"x"}]}"#;
        assert_eq!(read(&reply(gone, 1), 1).unwrap(), vec![None]);
        assert_eq!(
            read_absent(&runner(), Path::new("/nonexistent"), &[], &[]).unwrap(),
            vec![]
        );
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
            (
                "null without NOT_FOUND",
                r#"{"data":{"p0":{"pullRequest":null}}}"#,
            ),
            (
                "other error",
                r#"{"data":{"p0":null},"errors":[{"type":"RATE_LIMITED","path":["p0"],"message":"x"}]}"#,
            ),
            ("missing alias", r#"{"data":{}}"#),
        ] {
            let e = looked_up(&reply(stdout, 1), 1).unwrap_err();
            assert_eq!(e.code(), ErrorCode::GhFailed, "{name}");
        }
    }

    #[test]
    fn a_repo_that_is_not_owner_name_is_refused_before_gh_runs() {
        let nowhere = Path::new("/nonexistent");
        let runner = runner();
        for repo in [
            "",
            "a",
            "a/",
            "/b",
            "a/b/c",
            "a b/c",
            "o/n\"}) { x",
            "o\"/n",
            "ö/n",
        ] {
            let e = lookup(&runner, nowhere, &[(repo.to_owned(), 1)], &[]).unwrap_err();
            assert_eq!(e.code(), ErrorCode::GhFailed, "{repo:?}");
            assert!(
                e.message().contains("owner/name"),
                "{repo:?}: {}",
                e.message()
            );
        }
    }

    #[test]
    fn an_empty_lookup_makes_no_call() {
        assert_eq!(
            lookup(&runner(), Path::new("/nonexistent"), &[], &[]).unwrap(),
            vec![]
        );
    }

    #[test]
    fn repos_of_every_allowed_character_pass() {
        assert_eq!(
            repo_parts("Some-Org_1/repo.name-2"),
            Some(("Some-Org_1", "repo.name-2"))
        );
    }
}

/// The checks of the head commit of a hydrated pull request (`swr_pr_detail.detail_json`, the `Heavy` selection).
pub fn checks_of_detail(detail: &Value) -> Vec<Check> {
    let text = |n: &Value, k: &str| n.get(k).and_then(Value::as_str).map(str::to_owned);
    detail["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|n| Check {
            name: text(n, "name"),
            context: text(n, "context"),
            conclusion: text(n, "conclusion"),
            state: text(n, "state"),
            url: text(n, "detailsUrl").or_else(|| text(n, "targetUrl")),
            workflow: n["checkSuite"]["workflowRun"]["workflow"]["name"]
                .as_str()
                .map(str::to_owned),
            started_at: text(n, "startedAt").or_else(|| text(n, "createdAt")),
        })
        .collect()
}

/// What one pass of the single GraphQL read needs beyond the fixed part (swr-github.md §3.1): the rows chosen before the call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PollPlan {
    /// `(owner/name, number)` of the pull requests whose `Heavy` selection is sent: the cached header differs from the last
    /// hydrate (`updatedAt` or `headRefOid`), or there is no detail. One `p<i>` alias each, first.
    pub heavy: Vec<(String, i64)>,
    /// `(owner/name, number)` of the absent or first-seen pull requests: one `p<i>` alias each (`viewerDidAuthor`, no `Heavy`).
    pub absent: Vec<(String, i64)>,
    /// `owner/name` of the repositories whose metadata is missing or older than 24 h: one `m<i>` alias each.
    pub meta: Vec<String>,
}

const LIGHT: &str = "fragment Light on PullRequest { number state isDraft mergeable mergeStateStatus reviewDecision baseRefName headRefName headRefOid updatedAt title url createdAt mergedAt author { login } repository { nameWithOwner defaultBranchRef { name } } labels(first: 100) { nodes { name } } autoMergeRequest { enabledAt } commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } latestReviews(first: 20) { nodes { state author { __typename login } commit { oid } } } reviewThreads(first: 100) { nodes { id isResolved } } }";

const HEAVY: &str = "fragment Heavy on PullRequest { latestReviews(first: 20) { nodes { state author { __typename login } commit { oid } } } reviewThreads(first: 100) { nodes { id isResolved isOutdated path comments(first: 20) { nodes { id author { __typename login } body url createdAt } } } } comments(first: 50) { nodes { id author { __typename login } body url createdAt } } commits(last: 1) { nodes { commit { oid statusCheckRollup { state contexts(first: 100) { nodes { __typename ... on CheckRun { name conclusion status detailsUrl checkSuite { workflowRun { databaseId workflow { name } } } } ... on StatusContext { context state targetUrl } } } } } } } }";

/// The one read document of a pass (swr-github.md §3.1): the viewer, the rate limit, the two searches (light), one alias per
/// heavy then absent pull request, one per repository metadata. A repo that is not `owner/name` is skipped.
pub fn build_poll_query(plan: &PollPlan) -> String {
    let search = |alias: &str, q: &str| {
        format!(
            " {alias}: search(type: ISSUE, query: \"{q}\", first: 100) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ ... on PullRequest {{ ...Light }} }} }}"
        )
    };
    let mut query = String::from("query { viewer { login } rateLimit { remaining resetAt cost }");
    query.push_str(&search(
        "authored",
        "is:pr is:open author:@me archived:false sort:created-asc",
    ));
    query.push_str(&search(
        "reviews",
        "is:pr is:open review-requested:@me archived:false sort:created-asc",
    ));
    let aliased = plan
        .heavy
        .iter()
        .map(|p| (p, " ...Heavy"))
        .chain(plan.absent.iter().map(|p| (p, "")));
    for (i, ((repo, number), heavy)) in aliased.enumerate() {
        let (owner, name) = repo_parts(repo).unwrap_or_default();
        query.push_str(&format!(
            " p{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ pullRequest(number: {number}) {{ ...Light{heavy} viewerDidAuthor viewerLatestReviewRequest {{ id }} }} }}"
        ));
    }
    for (i, repo) in plan.meta.iter().enumerate() {
        let (owner, name) = repo_parts(repo).unwrap_or_default();
        query.push_str(&format!(
            " m{i}: repository(owner: \"{owner}\", name: \"{name}\") {{ defaultBranchRef {{ name }} isArchived autoMergeAllowed squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed deleteBranchOnMerge }}"
        ));
    }
    query.push_str(" } ");
    query.push_str(LIGHT);
    if !plan.heavy.is_empty() {
        query.push(' ');
        query.push_str(HEAVY);
    }
    query
}

/// One pull request of the single read: the header, its head commit, the heavy part when the node carried it.
#[derive(Clone, Debug)]
pub struct PollNode {
    pub(in crate::pr) listed: Listed,
    pub head_oid: String,
    /// The Light read's extras that no `PolledPr` field holds (pro-d453): the author's login, the `latestReviews` nodes with
    /// their commit oid, and the label names.
    pub author_login: Option<String>,
    pub reviews: Value,
    pub labels: Value,
    /// The heavy fields as GitHub answered them; `Some` only when the node carried `comments` (the heavy marker).
    pub heavy: Option<Value>,
}

/// A repository's metadata (`m<i>`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoMeta {
    pub default_branch: Option<String>,
    pub is_archived: bool,
    pub auto_merge_allowed: bool,
    pub squash_merge_allowed: bool,
    pub merge_commit_allowed: bool,
    pub rebase_merge_allowed: bool,
    pub delete_branch_on_merge: bool,
}

/// What the document answered.
#[derive(Clone, Debug)]
pub struct PollRead {
    pub viewer: String,
    pub remaining: i64,
    pub reset_at: String,
    pub authored: Vec<PollNode>,
    pub reviews: Vec<PollNode>,
    /// The `p<i>` aliases in document order (`None`: GitHub cannot find it).
    pub aliases: Vec<Option<PollNode>>,
    pub metas: Vec<Option<RepoMeta>>,
}

impl PollNode {
    /// Whether GitHub says the pull request is open.
    pub fn is_open(&self) -> bool {
        self.listed.pr.state == PrState::Open
    }

    /// `(owner/name, number)`.
    pub fn id(&self) -> (String, i64) {
        (self.listed.pr.repo.clone(), self.listed.pr.number)
    }

    /// `(authored, review_requested)`.
    pub fn flags(&self) -> (bool, bool) {
        (
            self.listed.membership.authored,
            self.listed.membership.review_requested,
        )
    }

    /// Adds the searches that listed the pull request.
    pub fn flag(&mut self, authored: bool, review_requested: bool) {
        self.listed.membership.authored |= authored;
        self.listed.membership.review_requested |= review_requested;
    }

    /// Everything the header write stores, as text: equal text, equal header.
    pub fn fingerprint(&self) -> String {
        format!(
            "{:?}|{}|{:?}|{}|{}",
            self.listed, self.head_oid, self.author_login, self.reviews, self.labels
        )
    }

    /// GitHub's `updatedAt`.
    pub fn updated_at(&self) -> &str {
        &self.listed.pr.updated_at
    }
}

fn poll_node(
    reply: &Reply,
    v: Value,
    bots: &[String],
    excused_labels: &[String],
) -> Result<PollNode, CpdError> {
    let head_oid = v
        .get("headRefOid")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            reply.fail("gh api graphql answered a pull request without headRefOid.".into())
        })?
        .to_owned();
    let raw: RawPr = serde_json::from_value(v.clone()).map_err(|e| {
        reply.fail(format!(
            "gh api graphql answered a malformed pull request: {e}."
        ))
    })?;
    let mut pr = reply.polled(raw, bots)?;
    let labels: Vec<String> = v["labels"]["nodes"]
        .as_array()
        .map(|n| {
            n.iter()
                .filter_map(|l| l["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    // pro-d447: a PR carrying an excused label has its failing `danger` check (a CheckRun or a StatusContext) excused; the
    // hold is computed once and `ChecksSummary::of` applies it. Never unconditional: Danger without the label stays failed.
    let hold = super::status::policy_hold(&labels, excused_labels);
    let contexts =
        v["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"].as_array();
    if let Some(nodes) = contexts.filter(|n| !n.is_empty()) {
        let mut checks = Vec::new();
        for n in nodes {
            let raw: RawCheck = serde_json::from_value(n.clone()).map_err(|e| {
                reply.fail(format!("gh api graphql answered a malformed check: {e}."))
            })?;
            checks.push(Check::from(raw));
        }
        pr.checks = ChecksSummary::of(&checks, &[], &[], hold);
    }
    let mut membership = Membership::default();
    if let Some(did) = v.get("viewerDidAuthor").and_then(Value::as_bool) {
        membership = Membership {
            authored: did,
            review_requested: v
                .get("viewerLatestReviewRequest")
                .is_some_and(|r| !r.is_null()),
        };
        if pr.state != PrState::Open {
            membership = Membership::default();
        }
    }
    if let Some(archived) = v["repository"]["isArchived"].as_bool() {
        pr.archived = archived;
    }
    let heavy = v.get("comments").map(|_| {
        json!({
            "latestReviews": v["latestReviews"], "reviewThreads": v["reviewThreads"],
            "comments": v["comments"], "commits": v["commits"], "labels": v["labels"],
        })
    });
    let reviews = v
        .get("latestReviews")
        .filter(|r| !r.is_null())
        .cloned()
        .unwrap_or_else(|| json!({"nodes": []}));
    Ok(PollNode {
        listed: Listed { pr, membership },
        head_oid,
        author_login: v["author"]["login"].as_str().map(str::to_owned),
        reviews,
        labels: json!(labels),
        heavy,
    })
}

fn parse_poll(
    reply: &Reply,
    aliases: usize,
    metas: usize,
    bots: &[String],
    excused_labels: &[String],
) -> Result<PollRead, CpdError> {
    let (data, errors): (Value, _) = reply.envelope()?;
    if let Some(other) = errors
        .iter()
        .find(|e| e.kind.as_deref() != Some("NOT_FOUND"))
    {
        return Err(reply.fail(format!(
            "gh api graphql answered with errors: {}",
            other.message.as_deref().unwrap_or("(no message)")
        )));
    }
    let missing = |what: &str| reply.fail(format!("gh api graphql answered without {what}."));
    let viewer = data["viewer"]["login"]
        .as_str()
        .ok_or_else(|| missing("viewer"))?
        .to_owned();
    let remaining = data["rateLimit"]["remaining"]
        .as_i64()
        .ok_or_else(|| missing("rateLimit"))?;
    let reset_at = data["rateLimit"]["resetAt"]
        .as_str()
        .ok_or_else(|| missing("rateLimit.resetAt"))?
        .to_owned();
    let search = |alias: &str| -> Result<Vec<PollNode>, CpdError> {
        data[alias]["nodes"]
            .as_array()
            .ok_or_else(|| missing(alias))?
            .iter()
            .map(|n| poll_node(reply, n.clone(), bots, excused_labels))
            .collect()
    };
    let (authored, reviews) = (search("authored")?, search("reviews")?);
    let mut found = Vec::with_capacity(aliases);
    for i in 0..aliases {
        let alias = format!("p{i}");
        let slot = data.get(&alias).ok_or_else(|| missing(&alias))?;
        found.push(match slot.get("pullRequest").filter(|p| !p.is_null()) {
            Some(p) => Some(poll_node(reply, p.clone(), bots, excused_labels)?),
            None => None,
        });
    }
    let mut meta = Vec::with_capacity(metas);
    for i in 0..metas {
        let alias = format!("m{i}");
        let m = data.get(&alias).ok_or_else(|| missing(&alias))?;
        meta.push(if m.is_null() {
            None
        } else {
            let flag = |k: &str| {
                m[k].as_bool()
                    .ok_or_else(|| missing(&format!("{alias}.{k}")))
            };
            Some(RepoMeta {
                default_branch: m["defaultBranchRef"]["name"].as_str().map(str::to_owned),
                is_archived: flag("isArchived")?,
                auto_merge_allowed: flag("autoMergeAllowed")?,
                squash_merge_allowed: flag("squashMergeAllowed")?,
                merge_commit_allowed: flag("mergeCommitAllowed")?,
                rebase_merge_allowed: flag("rebaseMergeAllowed")?,
                delete_branch_on_merge: flag("deleteBranchOnMerge")?,
            })
        });
    }
    Ok(PollRead {
        viewer,
        remaining,
        reset_at,
        authored,
        reviews,
        aliases: found,
        metas: meta,
    })
}

/// The one call of a pass: `plan`'s document through `runner`. Failures carry `details.call` `poll`, `transient`, `reason` and,
/// when GitHub's rate limit is spent, `rate_limited`.
pub fn poll_once(
    runner: &dyn Runner,
    cwd: &Path,
    plan: &PollPlan,
    bots: &[String],
    excused_labels: &[String],
) -> Result<PollRead, CpdError> {
    let call = || {
        let (args, out) = graphql(runner, cwd, &build_poll_query(plan))?;
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        parse_poll(
            &Reply {
                args: &refs,
                out: &out,
            },
            plan.heavy.len() + plan.absent.len(),
            plan.meta.len(),
            bots,
            excused_labels,
        )
    };
    call().map_err(|e| polled_call("poll", e))
}
