//! The one gateway to `gh` (contract §7.6; commands.md §9): its output is
//! parsed once here, into typed values; the rest of `pr` never sees raw JSON.
//! Every failure is `gh_failed` with `argv`, `exit` and the last 60 lines of stderr.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;

use crate::error::{CpdError, ErrorCode};
use crate::git::{TAIL_LINES, tail};

/// A `gh` call that takes longer than this is killed (`gh_failed`).
const TIMEOUT: Duration = Duration::from_secs(60);

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

/// `gh pr view <n> --json state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision`.
#[derive(Debug, Deserialize)]
pub struct View {
    pub state: String,
    #[serde(rename = "mergedAt")]
    pub merged_at: Option<String>,
    #[serde(rename = "statusCheckRollup")]
    pub checks: Option<Vec<Check>>,
    pub reviews: Vec<Review>,
    pub comments: Vec<Comment>,
    pub title: String,
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

/// Runs `gh <args>` in `cwd` and returns its stdout; a non-zero exit, a signal
/// and a timeout are `gh_failed`. Never prompts, never colours.
fn run(cwd: &Path, args: &[&str]) -> Result<String, CpdError> {
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
    let deadline = Instant::now() + TIMEOUT;
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
        return Err(failed(args, format!("gh {} took longer than {}s and was killed.", args[0], TIMEOUT.as_secs()), None, &stderr));
    };
    match status.code() {
        Some(0) => Ok(stdout),
        Some(code) => Err(failed(args, format!("gh {} exited {code}.", args[0]), Some(code), &stderr)),
        None => Err(failed(args, format!("gh {} was killed by a signal.", args[0]), None, &stderr)),
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
    let args = ["pr", "view", &n, "--json", "state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision"];
    parse(&args, &run(cwd, &args)?)
}

/// Inline review comments, which `gh pr view` does not list. `{owner}/{repo}`
/// are filled in by `gh` from `cwd`'s repo.
pub fn inline_comments(cwd: &Path, number: i64) -> Result<Vec<InlineComment>, CpdError> {
    let endpoint = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments");
    let args = ["api", endpoint.as_str()];
    parse(&args, &run(cwd, &args)?)
}
