//! `review submit` (commands.md §31.1; pro-d107, pro-d108, pro-d110): posts a GitHub review through `gh`, then, with
//! `--slack`, queues the verdict's reaction (§21.9). It needs no Plan, Context or PR key and checks nothing of the
//! review, and of the pull request only its head (`--head`, pro-d169): GitHub's own refusal is relayed.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::gh;
use super::merge::PullRef;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;
use crate::ports::Ports;
use crate::store::Store;

const STORE_BUSY_MS: u64 = 5000;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Post a GitHub review (APPROVE or REQUEST_CHANGES) and, with --slack, queue its reaction (§31.1)
    Submit(SubmitArgs),
}

#[derive(clap::Args, Debug)]
pub struct SubmitArgs {
    /// The pull request: its URL, or owner/repo#n
    #[arg(long, value_name = "url | owner/repo#n")]
    pr: String,
    /// The review's verdict: APPROVE or REQUEST_CHANGES
    #[arg(long, value_name = "APPROVE|REQUEST_CHANGES")]
    event: String,
    /// The head commit the review is of: 40 hex digits; refused as head_moved when the pull request's head differs
    #[arg(long, value_name = "sha")]
    head: String,
    /// A file holding GitHub's create-review body, a JSON object without an `event` key
    #[arg(long, value_name = "review.json")]
    input: PathBuf,
    /// The Slack post to react to once the review is posted
    #[arg(long, value_name = "channel/ts")]
    slack: Option<String>,
}

pub fn run(ports: &Ports, command: Command) -> Result<Output, CpdError> {
    match command {
        Command::Submit(args) => submit(ports, args),
    }
}

fn usage(message: String) -> CpdError {
    CpdError::new(ErrorCode::Usage, message)
}

/// `https://github.com/<owner>/<repo>/pull/<n>` (an optional trailing slash) or `<owner>/<repo>#<n>`.
fn parse_pull(text: &str) -> Result<PullRef, CpdError> {
    let bad = || usage(format!("--pr {text:?} is not https://github.com/<owner>/<repo>/pull/<n> or <owner>/<repo>#<n>."));
    let Some(rest) = text.strip_prefix("https://github.com/") else { return text.parse().map_err(|_| bad()) };
    let parts: Vec<&str> = rest.strip_suffix('/').unwrap_or(rest).split('/').collect();
    match parts[..] {
        [owner, repo, "pull", n] => format!("{owner}/{repo}#{n}").parse().map_err(|_| bad()),
        _ => Err(bad()),
    }
}

/// `<channel>/<ts>`: channel `^C[A-Z0-9]{8,}$`, ts `^[0-9]{10}\.[0-9]{6}$`.
fn parse_slack(text: &str) -> Result<(&str, &str), CpdError> {
    let bad = || usage(format!("--slack {text:?} is not <channel>/<ts>, like C02JU6C2Z1D/1793040197.490209."));
    let (channel, ts) = text.split_once('/').ok_or_else(bad)?;
    let channel_ok = channel.len() >= 9
        && channel.starts_with('C')
        && channel.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase());
    let ts_ok = ts.split_once('.').is_some_and(|(s, f)| {
        s.len() == 10 && f.len() == 6 && s.bytes().all(|b| b.is_ascii_digit()) && f.bytes().all(|b| b.is_ascii_digit())
    });
    if channel_ok && ts_ok { Ok((channel, ts)) } else { Err(bad()) }
}

/// Exactly 40 ASCII hex digits, either case; the lowercase value.
fn parse_head(text: &str) -> Result<String, CpdError> {
    if text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(text.to_ascii_lowercase())
    } else {
        Err(usage(format!("--head {text:?} is not 40 hex digits.")))
    }
}

fn submit(ports: &Ports, args: SubmitArgs) -> Result<Output, CpdError> {
    let pull = parse_pull(&args.pr)?;
    let emoji = match args.event.as_str() {
        "APPROVE" => "github-check-mark",
        "REQUEST_CHANGES" => "github-changes-requested",
        other => return Err(usage(format!("--event {other:?} is not APPROVE or REQUEST_CHANGES."))),
    };
    let slack = args.slack.as_deref().map(parse_slack).transpose()?;
    let head = parse_head(&args.head)?;
    let text = std::fs::read_to_string(&args.input)
        .map_err(|e| usage(format!("Cannot read --input {}: {e}.", args.input.display())))?;
    let mut review: Value = serde_json::from_str(&text)
        .map_err(|e| usage(format!("--input {} is not JSON: {e}.", args.input.display())))?;
    let Value::Object(fields) = &mut review else {
        return Err(usage(format!("--input {} is not a JSON object.", args.input.display())));
    };
    if fields.contains_key("event") {
        return Err(usage("--input carries its own `event` key: the verdict is --event.".to_owned()));
    }
    if fields.contains_key("commit_id") {
        return Err(usage("--input carries its own `commit_id` key: the head is --head.".to_owned()));
    }
    fields.insert("event".to_owned(), json!(args.event));
    fields.insert("commit_id".to_owned(), json!(head));

    let repo = pull.to_string();
    let (repo, number) = repo.split_once('#').expect("a PullRef prints owner/repo#n");
    let actual = gh::pull_head(&*ports.runner, repo, number)?;
    if !actual.eq_ignore_ascii_case(&head) {
        return Err(CpdError::new(
            ErrorCode::HeadMoved,
            format!("The head of {repo}#{number} is {actual}, not {head}: nothing was posted."),
        )
        .with("pr", format!("{repo}#{number}"))
        .with("expected", head)
        .with("actual", actual));
    }
    let body = serde_json::to_vec(&review).expect("a JSON value serializes");
    let stdout = gh::post_review(&*ports.runner, repo, number, body)?;
    let posted: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
    let (Some(id), Some(url)) = (posted.get("id").filter(|v| v.is_number()), posted.get("html_url").and_then(Value::as_str)) else {
        return Err(CpdError::new(
            ErrorCode::GhFailed,
            format!("gh answered the review without an id or html_url: the review may have been posted; check the pull request {repo}#{number}."),
        )
        .with("stdout", crate::git::tail(&stdout, crate::git::TAIL_LINES)));
    };
    let review = json!({"id": id, "url": url, "state": posted.get("state").cloned().unwrap_or(Value::Null), "commit_id": head});

    let reaction = match slack {
        None => Value::Null,
        Some((channel, ts)) => {
            let queued = Store::open(&CpdHome::from_env()?, STORE_BUSY_MS)
                .and_then(|mut store| store.write("review submit", |tx| super::reactions::queue(tx, channel, ts, emoji)))
                .map_err(|e| e.with("review", review.clone()))?;
            json!({"channel": channel, "ts": ts, "emoji": emoji, "already": !queued})
        }
    };
    Ok(Output::success(json!({"review": review, "reaction": reaction})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_and_the_hash_form_name_one_pull_request() {
        for text in ["o/r#7", "https://github.com/o/r/pull/7", "https://github.com/o/r/pull/7/"] {
            assert_eq!(parse_pull(text).unwrap(), "o/r#7".parse::<PullRef>().unwrap(), "{text}");
        }
        for bad in ["https://github.com/o/r/issues/7", "https://github.com/o/r/pull/x", "https://github.com/o/pull/7", "o/r"] {
            assert_eq!(parse_pull(bad).unwrap_err().code(), ErrorCode::Usage, "{bad}");
        }
    }

    #[test]
    fn slack_is_a_channel_and_a_ts() {
        assert!(parse_slack("C02JU6C2Z1D/1793040197.490209").is_ok());
        for bad in ["", "C02JU6C2Z1D", "c02ju6c2z1d/1793040197.490209", "C0123/1793040197.490209", "C02JU6C2Z1D/1793040197490209", "C02JU6C2Z1D/179304019.4902099"] {
            assert_eq!(parse_slack(bad).unwrap_err().code(), ErrorCode::Usage, "{bad}");
        }
    }
}
