//! GitHub writes through the outbox (docs/architecture/swr-github.md §9): the enqueue every write command calls, and the drain
//! `GithubSource::execute` runs through the Runner. A command never calls GitHub for a write.

use std::path::Path;
use std::time::Duration;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::github::GithubSource;
use super::{Cx, ExecOutcome};
use crate::error::CpdError;
use crate::output::Output;
use crate::pr::auto_merge::{self, Outcome};
use crate::pr::comments::{Kind as ItemKind, MARKER};
use crate::pr::merge_method::{MergeMethods, Method};
use crate::pr::swr_read as gh;
use crate::store::Store;
use crate::swr::github::{effective_pr, pr_key};
use crate::swr::outbox::{self, Coalesce, Mutation, Op};

/// How long a transient failure waits before the drain tries again.
const RETRY: Duration = Duration::from_secs(60);

/// The coalescing of an `op_kind` (swr-github.md §9); `Dedupe` carries the canonical vars once the op is known.
pub(super) fn coalesce_of(op_kind: &str) -> Coalesce {
    match op_kind {
        "describe" | "retitle" | "auto_merge" => Coalesce::LatestWins,
        _ => Coalesce::Dedupe(String::new()),
    }
}

/// Enqueues `op_kind` on `target` and returns the mutation id (the existing one for a dedupe).
pub fn enqueue(
    store: &mut Store,
    now: OffsetDateTime,
    target: &str,
    op_kind: &str,
    vars: Value,
    patch: Option<Value>,
) -> Result<i64, CpdError> {
    let coalesce = match coalesce_of(op_kind) {
        Coalesce::Dedupe(_) => Coalesce::Dedupe(vars.to_string()),
        other => other,
    };
    outbox::enqueue(
        store,
        now,
        Op {
            system: "github".into(),
            target_key: target.into(),
            op_kind: op_kind.into(),
            vars,
            patch,
            context: None,
        },
        coalesce,
    )
}

/// The answer of a write command: `{queued, mutation_id, effective}`, `effective` the state the readers see now.
pub fn queued(
    store: &mut Store,
    repo: &str,
    number: i64,
    id: i64,
    mut fields: Value,
) -> Result<Output, CpdError> {
    let effective = effective_pr(store, repo, number)?.map_or(Value::Null, |e| {
        json!({"state": e.state, "checks": e.checks, "review_decision": e.review_decision, "title": e.title})
    });
    fields["queued"] = json!(true);
    fields["mutation_id"] = json!(id);
    fields["effective"] = effective;
    Ok(Output::success(fields))
}

/// The merge method a repo allows (the first of squash, merge, rebase), from `swr_repos`; `None` when the row is missing.
pub fn merge_method(store: &mut Store, repo: &str) -> Result<Option<Result<Method, MergeMethods>>, CpdError> {
    let flags: Option<MergeMethods> = store.read(|c| {
        Ok(c.query_row(
            "SELECT squash_merge_allowed, merge_commit_allowed, rebase_merge_allowed FROM swr_repos WHERE repo = ?1",
            [repo],
            |r| {
                Ok(MergeMethods {
                    squash: r.get(0)?,
                    merge: r.get(1)?,
                    rebase: r.get(2)?,
                })
            },
        )
        .optional()?)
    })?;
    Ok(flags.map(|f| f.choose().ok_or(f)))
}

fn target(key: &str) -> Option<(&str, &str, i64)> {
    let rest = key.strip_prefix("github:")?;
    let (kind, rest) = rest.split_once(':')?;
    let (repo, n) = rest.rsplit_once('#')?;
    Some((kind, repo, n.parse().ok()?))
}

fn failed(error: impl Into<String>) -> ExecOutcome {
    ExecOutcome::Failed {
        error: error.into(),
    }
}

fn retry(e: &CpdError) -> ExecOutcome {
    let transient = e
        .details()
        .get("transient")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if transient {
        ExecOutcome::Retry { after: RETRY }
    } else {
        failed(e.message())
    }
}

impl GithubSource {
    /// One claimed mutation, through the Runner (swr-github.md §9).
    pub(super) fn run_one(&self, cx: &mut Cx<'_>, m: &Mutation) -> ExecOutcome {
        let Some((kind, repo, number)) = target(&m.target_key) else {
            return failed(format!("{:?} is not a github target.", m.target_key));
        };
        let vars: Value = match serde_json::from_str(&m.vars_json) {
            Ok(v) => v,
            Err(e) => return failed(format!("The vars of mutation {} are not JSON: {e}.", m.id)),
        };
        let ports = cx.ports.clone();
        let runner = &*ports.runner;
        let cwd = cx.home.root().to_path_buf();
        let cwd: &Path = &cwd;
        let text = |k: &str| vars.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        crate::ports::Ports::scope(&ports, || match (kind, m.op_kind.as_str()) {
            ("pr", "merge") => {
                let head = text("expected_head_oid");
                let method = match merge_method(cx.store, repo) {
                    Ok(Some(Ok(method))) => method,
                    Ok(Some(Err(_))) => {
                        return failed(format!("{repo} allows no merge method: squash, merge commit and rebase are all off."));
                    }
                    Ok(None) => return self.wait_for_repo(cx, repo),
                    Err(e) => return retry(&e),
                };
                match gh::merge_pr(runner, cwd, repo, number, method, &head) {
                    Ok(()) => ExecOutcome::Done,
                    Err(e) => failed(refusal(&e)),
                }
            }
            ("pr", "auto_merge") => match auto_merge::attempt(runner, cwd, repo, number) {
                Outcome::Enabled | Outcome::NotAllowed => ExecOutcome::Done,
                Outcome::Refused(why) => failed(why),
                Outcome::Failed(e) => retry(&e),
            },
            ("pr", "approve") => {
                let mut review = vars.clone();
                if let Some(o) = review.as_object_mut() {
                    o.entry("event").or_insert(json!("APPROVE"));
                }
                let body = review.to_string().into_bytes();
                match gh::post_review(runner, repo, &number.to_string(), body) {
                    Ok(_) => ExecOutcome::Done,
                    Err(e) => retry(&e),
                }
            }
            ("deploy", "approve") => {
                let run_id = u64::try_from(number).unwrap_or_default();
                match crate::pr::deploys::approvable(runner, cx.home, repo, run_id) {
                    Ok(envs) if envs.is_empty() => ExecOutcome::Done,
                    Ok(envs) => match crate::pr::deploys::approve(runner, cx.home, repo, run_id, &envs) {
                        Ok(()) => ExecOutcome::Done,
                        Err(e) => retry(&e),
                    },
                    Err(e) => retry(&e),
                }
            }
            ("pr", "reply") => self.reply(cx, runner, cwd, repo, number, &vars),
            ("pr", "post_comment") => {
                let sent = gh::pull_request_id(runner, cwd, repo, number)
                    .and_then(|id| gh::reply_to_comment(runner, cwd, &id, &text("body")));
                sent.map_or_else(|e| retry(&e), |_| ExecOutcome::Done)
            }
            ("pr", "describe") => gh::describe_pr(cwd, repo, number, &text("body"))
                .map_or_else(|e| retry(&e), |()| ExecOutcome::Done),
            ("pr", "retitle") => gh::retitle_pr(cwd, repo, number, &text("title"))
                .map_or_else(|e| retry(&e), |()| ExecOutcome::Done),
            ("pr", "rerun") => {
                for id in vars["run_ids"].as_array().into_iter().flatten().filter_map(Value::as_i64) {
                    if let Err(e) = gh::rerun_failed(runner, cwd, id) {
                        // Stops at the first failure, which is tried again (swr-github.md §9).
                        let _ = e;
                        return ExecOutcome::Retry { after: RETRY };
                    }
                }
                ExecOutcome::Done
            }
            // The claim's Effects are the command's own (they write no GitHub state): the op only carries the overlay.
            ("pr", "claim") => ExecOutcome::Done,
            (_, other) => failed(format!("github has no op {other:?} on {kind}.")),
        })
    }

    /// No metadata of the repo yet: ask the next pass for it and try again later; no `gh` call (swr-github.md §9).
    fn wait_for_repo(&self, cx: &mut Cx<'_>, repo: &str) -> ExecOutcome {
        match crate::swr::cache::invalidate(cx.store, &format!("github:repo:{repo}")) {
            Ok(()) => ExecOutcome::Retry { after: RETRY },
            Err(e) => failed(e.message()),
        }
    }

    fn reply(
        &self,
        cx: &mut Cx<'_>,
        runner: &dyn crate::host::runner::Runner,
        cwd: &Path,
        repo: &str,
        number: i64,
        vars: &Value,
    ) -> ExecOutcome {
        let (to, body) = (
            vars["to"].as_str().unwrap_or_default(),
            vars["body"].as_str().unwrap_or_default(),
        );
        let feedback: Result<Option<String>, CpdError> = cx.store.read(|c| {
            Ok(c.query_row(
                "SELECT feedback FROM pull_requests WHERE repo = ?1 AND number = ?2",
                rusqlite::params![repo, number],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
        });
        let items: Vec<crate::pr::comments::Item> = match feedback {
            Ok(Some(text)) => serde_json::from_str(&text).unwrap_or_default(),
            Ok(None) => Vec::new(),
            Err(e) => return failed(e.message()),
        };
        // The item is gone from the raw feedback: someone answered it already.
        let Some(item) = items.iter().find(|i| i.id == to) else {
            return ExecOutcome::Done;
        };
        let sent = match item.kind {
            ItemKind::Thread => gh::reply_to_thread(runner, cwd, &item.id, &format!("{body}\n\n{MARKER}")),
            ItemKind::Comment | ItemKind::Review => gh::pull_request_id(runner, cwd, repo, number).and_then(|node| {
                gh::reply_to_comment(runner, cwd, &node, &format!("Re: {}\n\n{body}\n\n{MARKER}", item.url))
            }),
        };
        sent.map_or_else(|e| retry(&e), |_| ExecOutcome::Done)
    }
}

/// GitHub's refusal text of a failed write: its stderr when it printed one.
fn refusal(e: &CpdError) -> String {
    let stderr = e
        .details()
        .get("stderr")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match stderr {
        Some(s) => format!("{} {s}", e.message()),
        None => e.message().to_owned(),
    }
}

/// `pr_key` for a caller that holds `repo` and `number`.
#[must_use]
pub fn target_of(repo: &str, number: i64) -> String {
    pr_key(repo, number)
}
