//! The poll reads comments only when something moved (pro-d385, pro-d387): an owned open pull request is read when its `updatedAt`
//! moved or when a stored `Thread` feedback item is no longer an open thread in the search. In process on the janitor's `Rig`:
//! `gh` is scripted on the runner, and counts the comments reads. No test starts a process or sleeps.

use std::sync::{Arc, Mutex};

use super::PollArgs;
use super::poll::poll;
use crate::args::PlanArg;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::ports::Ports;

/// The `gh` of the poll: the search lists `acme/pay#801`, open, updated at `polled_at`, with the review threads `threads`
/// (`(id, isResolved)`); a comments read answers the pull request with no thread, comment or review, and is counted.
struct ThreadsGh {
    polled_at: &'static str,
    threads: Vec<(&'static str, bool)>,
    reads: Arc<Mutex<usize>>,
}

impl Handler for ThreadsGh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                let nodes: Vec<_> = self
                    .threads
                    .iter()
                    .map(|(id, resolved)| serde_json::json!({"id": id, "isResolved": resolved}))
                    .collect();
                let node = serde_json::json!({
                    "repository": {"nameWithOwner": "acme/pay"}, "number": 801,
                    "url": "https://github.com/acme/pay/pull/801", "title": "Workflow 801",
                    "isDraft": false, "state": "OPEN", "createdAt": "opened", "updatedAt": self.polled_at,
                    "mergedAt": null, "reviewDecision": null, "mergeable": "UNKNOWN", "mergeStateStatus": "UNKNOWN",
                    "viewerDidAuthor": true, "viewerLatestReviewRequest": null,
                    "commits": {"nodes": [{"commit": {"statusCheckRollup": null}}]},
                    "reviewThreads": {"nodes": nodes},
                });
                Reply::ok(
                    &serde_json::json!({"data": {
                        "search": {"issueCount": 1, "nodes": [node]},
                        "reviewRequests": {"issueCount": 0, "nodes": []},
                        "rateLimit": {"cost": 1, "remaining": 4987, "resetAt": "2026-10-20T11:00:00Z"},
                    }})
                    .to_string(),
                )
            }
            ["api", "graphql", "-f", query, ..]
                if query.contains("reviewThreads") && query.contains(": repository(") =>
            {
                *self.reads.lock().unwrap() += 1;
                let empty = serde_json::json!({"totalCount": 0, "pageInfo": {"hasPreviousPage": false, "hasNextPage": false}, "nodes": []});
                let mut data = serde_json::json!({"viewer": {"login": "me"}});
                for i in 0..query.matches(": repository(").count() {
                    data[format!("p{i}")] = serde_json::json!({"pullRequest": {"id": format!("PR_{i}"), "updatedAt": self.polled_at, "reviewThreads": empty, "comments": empty, "reviews": empty}});
                }
                Reply::ok(&serde_json::json!({"data": data}).to_string())
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

const THREAD: &str = r#"[{"id":"T1","kind":"thread","last":"C1","author":"rev","bot":false,"path":"a.rs","outdated":false,"url":"https://github.com/acme/pay/pull/801#discussion_r1","excerpt":"fix this"}]"#;

/// Polls once the open row `acme/pay#801` stored with `updated_at` U1 and the open Thread `T1`, under a search that lists it
/// updated at `polled_at` with `threads`. Returns the number of comments reads and the stored feedback's item ids.
fn poll_once(polled_at: &'static str, threads: Vec<(&'static str, bool)>) -> (usize, Vec<String>) {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    let reads = Arc::new(Mutex::new(0));
    let _ = (*rig.fakes.runner).clone().route(
        "gh",
        Arc::new(ThreadsGh {
            polled_at,
            threads,
            reads: reads.clone(),
        }),
    );
    let mut store = crate::store::Store::open(&rig.home, 5000).unwrap();
    store
        .write("test", |tx| {
            tx.execute(
                "UPDATE pull_requests SET updated_at = 'U1', feedback = ?1 WHERE number = 801",
                [THREAD],
            )?;
            Ok(())
        })
        .unwrap();
    // `Env::open_with` reads `$CPD_HOME`: the one variable this test sets, to its own temp dir (as `poll_tests.rs`).
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    let out = Ports::scope(&rig.fakes.ports, || {
        poll(
            &rig.fakes.ports,
            PollArgs {
                plan: PlanArg { plan: None },
            },
        )
    })
    .unwrap_or_else(|e| panic!("the poll failed: {e:?}"));
    let stored: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT feedback FROM pull_requests WHERE number = 801",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap_or_else(|e| panic!("{e:?} {out:?}"));
    let ids = serde_json::from_str::<Vec<serde_json::Value>>(&stored)
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_owned())
        .collect();
    let count = *reads.lock().unwrap();
    (count, ids)
}

#[test]
fn idle_tick_with_unchanged_open_threads_makes_no_comment_read() {
    let (reads, ids) = poll_once("U1", vec![("T1", false)]);
    assert_eq!(reads, 0, "an idle tick reads no comments");
    assert_eq!(ids, ["T1"], "the stored feedback is untouched");
}

#[test]
fn resolved_thread_with_unchanged_updated_at_is_re_read_once() {
    let (reads, ids) = poll_once("U1", vec![("T1", true)]);
    assert_eq!(reads, 1, "a resolved thread is read once");
    assert!(ids.is_empty(), "no open Thread item is stored: {ids:?}");
}

#[test]
fn moved_updated_at_with_open_threads_still_reads_comments() {
    let (reads, _) = poll_once("U2", vec![("T1", false)]);
    assert_eq!(reads, 1, "a moved updatedAt is read");
}
