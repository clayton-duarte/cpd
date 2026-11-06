//! `review submit` (commands.md §31.1; pro-d107, pro-d108, pro-d110): posts a GitHub review through `gh`, then, with
//! `--slack`, queues the verdict's reaction (§21.9). It needs no Plan, Context or Workflow key and checks nothing of the
//! review, and of the pull request only its head (`--head`, pro-d169): GitHub's own refusal is relayed.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::merge::PullRef;
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::home::CpdHome;
use crate::ids::{ContextId, PlanId};
use crate::output::Output;
use crate::ports::Ports;
use crate::store::{Store, WriteTxn};
use crate::swr::github::effective_pr;
use crate::swr::source::github_write::{enqueue, queued, target_of};

const STORE_BUSY_MS: u64 = 5000;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Post a GitHub review (APPROVE or REQUEST_CHANGES) and, with --slack, queue its reaction (§31.1)
    Submit(SubmitArgs),
}

#[derive(clap::Args, Debug)]
pub struct SubmitArgs {
    /// The pull request: its URL, or owner/repo#n
    #[arg(long = "pr", value_name = "url | owner/repo#n")]
    workflow: String,
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
    let bad = || {
        usage(format!(
            "--pr {text:?} is not https://github.com/<owner>/<repo>/pull/<n> or <owner>/<repo>#<n>."
        ))
    };
    let Some(rest) = text.strip_prefix("https://github.com/") else {
        return text.parse().map_err(|_| bad());
    };
    let parts: Vec<&str> = rest.strip_suffix('/').unwrap_or(rest).split('/').collect();
    match parts[..] {
        [owner, repo, "pull", n] => format!("{owner}/{repo}#{n}").parse().map_err(|_| bad()),
        _ => Err(bad()),
    }
}

/// `<channel>/<ts>`: channel `^C[A-Z0-9]{8,}$`, ts `^[0-9]{10}\.[0-9]{6}$`.
fn parse_slack(text: &str) -> Result<(&str, &str), CpdError> {
    let bad = || {
        usage(format!(
            "--slack {text:?} is not <channel>/<ts>, like C02JU6C2Z1D/1793040197.490209."
        ))
    };
    let (channel, ts) = text.split_once('/').ok_or_else(bad)?;
    let channel_ok = channel.len() >= 9
        && channel.starts_with('C')
        && channel
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase());
    let ts_ok = ts.split_once('.').is_some_and(|(s, f)| {
        s.len() == 10
            && f.len() == 6
            && s.bytes().all(|b| b.is_ascii_digit())
            && f.bytes().all(|b| b.is_ascii_digit())
    });
    if channel_ok && ts_ok {
        Ok((channel, ts))
    } else {
        Err(bad())
    }
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
    let pull = parse_pull(&args.workflow)?;
    let emoji = match args.event.as_str() {
        "APPROVE" => "github-check-mark",
        "REQUEST_CHANGES" => "github-changes-requested",
        other => {
            return Err(usage(format!(
                "--event {other:?} is not APPROVE or REQUEST_CHANGES."
            )));
        }
    };
    let slack = args.slack.as_deref().map(parse_slack).transpose()?;
    let head = parse_head(&args.head)?;
    let text = std::fs::read_to_string(&args.input).map_err(|e| {
        usage(format!(
            "Cannot read --input {}: {e}.",
            args.input.display()
        ))
    })?;
    let mut review: Value = serde_json::from_str(&text).map_err(|e| {
        usage(format!(
            "--input {} is not JSON: {e}.",
            args.input.display()
        ))
    })?;
    let Value::Object(fields) = &mut review else {
        return Err(usage(format!(
            "--input {} is not a JSON object.",
            args.input.display()
        )));
    };
    if fields.contains_key("event") {
        return Err(usage(
            "--input carries its own `event` key: the verdict is --event.".to_owned(),
        ));
    }
    if fields.contains_key("commit_id") {
        return Err(usage(
            "--input carries its own `commit_id` key: the head is --head.".to_owned(),
        ));
    }
    fields.insert("event".to_owned(), json!(args.event));
    fields.insert("commit_id".to_owned(), json!(head));

    let repo = pull.to_string();
    let (repo, number) = repo.split_once('#').expect("a PullRef prints owner/repo#n");
    let number: i64 = number.parse().expect("a PullRef number is an integer");
    // The head is checked against the cache (swr-github.md §9); GitHub itself refuses a review of a head that moved since,
    // through `commit_id`.
    let home = CpdHome::from_env()?;
    let mut main = Store::open(&home, STORE_BUSY_MS)?;
    let cached = effective_pr(&mut main, repo, number)?.and_then(|e| e.head_oid);
    if let Some(actual) = cached.filter(|actual| !actual.eq_ignore_ascii_case(&head)) {
        return Err(CpdError::new(
            ErrorCode::HeadMoved,
            format!("The head of {repo}#{number} is {actual}, not {head}: nothing was queued."),
        )
        .with("pr", format!("{repo}#{number}"))
        .with("expected", head)
        .with("actual", actual));
    }
    let patch = match args.event.as_str() {
        "APPROVE" => Some(json!({"review_decision": "approved"})),
        _ => Some(json!({"review_decision": "changes_requested"})),
    };
    let mutation = enqueue(
        &mut main,
        ports.clock.utc(),
        &target_of(repo, number),
        "approve",
        review.clone(),
        patch,
    )?;
    let review = json!({"event": args.event, "commit_id": head, "queued": true});

    let plan = std::env::var("CPD_PLAN")
        .ok()
        .and_then(|v| PlanId::parse(&v).ok());
    let reaction = if slack.is_none() && plan.is_none() {
        Value::Null
    } else {
        let queued = Store::open(&CpdHome::from_env()?, STORE_BUSY_MS)
            .and_then(|mut store| {
                store.write("review submit", |tx| {
                    let queued = match slack {
                        Some((channel, ts)) => {
                            Some(super::reactions::queue(tx, channel, ts, emoji)?)
                        }
                        None => None,
                    };
                    if let Some(plan) = &plan {
                        if args.event == "APPROVE" {
                            finish_plan(tx, plan)?;
                        } else {
                            request_changes(tx, plan)?;
                        }
                    }
                    Ok(queued)
                })
            })
            .map_err(|e| e.with("review", review.clone()))?;
        match (slack, queued) {
            (Some((channel, ts)), Some(queued)) => {
                json!({"channel": channel, "ts": ts, "emoji": emoji, "already": !queued})
            }
            _ => Value::Null,
        }
    };
    queued(
        &mut main,
        repo,
        number,
        mutation,
        json!({"review": review, "reaction": reaction}),
    )
}

/// Finishes a `codereview` Plan (FR10): `grounding → done` with `locked_at` and `done_at` the transaction's time, and a quiet `plan-done`.
pub(crate) fn finish_plan(tx: &WriteTxn<'_>, plan: &PlanId) -> Result<(), CpdError> {
    let moved = tx.execute(
        "UPDATE plans SET phase = 'done', locked_at = (SELECT at FROM txns WHERE id = cpd_txn()), done_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE id = ?1 AND context = 'codereview' AND deleted_at IS NULL AND phase <> 'done'",
        [plan.as_str()],
    )?;
    if moved == 1 {
        let text = format!("Plan {plan} is done: review submitted.");
        append(
            tx,
            NewEvent::plan_done_quiet(text)
                .plan(plan.clone())
                .context(ContextId::parse("codereview")?),
        )?;
    }
    Ok(())
}

/// REQUEST_CHANGES on a `codereview` Plan (pro-d416): the Plan stays open; `changes_requested_at` is the transaction's time.
fn request_changes(tx: &WriteTxn<'_>, plan: &PlanId) -> Result<(), CpdError> {
    tx.execute(
        "UPDATE plans SET changes_requested_at = (SELECT at FROM txns WHERE id = cpd_txn())
          WHERE id = ?1 AND context = 'codereview' AND deleted_at IS NULL AND phase = 'grounding'",
        [plan.as_str()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_and_the_hash_form_name_one_pull_request() {
        for text in [
            "o/r#7",
            "https://github.com/o/r/pull/7",
            "https://github.com/o/r/pull/7/",
        ] {
            assert_eq!(
                parse_pull(text).unwrap(),
                "o/r#7".parse::<PullRef>().unwrap(),
                "{text}"
            );
        }
        for bad in [
            "https://github.com/o/r/issues/7",
            "https://github.com/o/r/pull/x",
            "https://github.com/o/pull/7",
            "o/r",
        ] {
            assert_eq!(
                parse_pull(bad).unwrap_err().code(),
                ErrorCode::Usage,
                "{bad}"
            );
        }
    }

    #[test]
    fn slack_is_a_channel_and_a_ts() {
        assert!(parse_slack("C02JU6C2Z1D/1793040197.490209").is_ok());
        for bad in [
            "",
            "C02JU6C2Z1D",
            "c02ju6c2z1d/1793040197.490209",
            "C0123/1793040197.490209",
            "C02JU6C2Z1D/1793040197490209",
            "C02JU6C2Z1D/179304019.4902099",
        ] {
            assert_eq!(
                parse_slack(bad).unwrap_err().code(),
                ErrorCode::Usage,
                "{bad}"
            );
        }
    }
}
