//! REST call budget and rate-limit backoff tests for deploy workflow polling with ETags (swr-github.md §8).

use serde_json::{Value, json};

use super::deploys::read_into_store;
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{Call, FakeRunner, Handler, Reply};
use crate::store::Store;

fn tmp() -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    (dir, home)
}

fn declare(home: &CpdHome, repo: &str, workflows: Option<&[&str]>) {
    let path = home.root().join("policy").join(format!("{repo}.toml"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = match workflows {
        Some(w) => format!("[deploys]\nworkflows = {}\n", json!(w)),
        None => String::new(),
    };
    std::fs::write(path, text).unwrap();
}

fn store(home: &CpdHome) -> Store {
    Store::open(home, 5000).unwrap()
}

fn read(runner: &FakeRunner, home: &CpdHome, me: &str) -> Result<(), CpdError> {
    let mut store = store(home);
    store
        .write("fixture viewer", |tx| {
            tx.execute(
                "INSERT INTO swr_viewer (id, login, fetched_at) VALUES (1, ?1, 't')
                 ON CONFLICT (id) DO UPDATE SET login = excluded.login",
                [me],
            )?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    read_into_store(
        runner,
        &mut store,
        home,
        crate::clock::Clock::utc(&ManualClock::new()),
    )
}

fn run(id: u64, actor: &str, created_at: &str) -> Value {
    json!({"id": id, "status": "waiting", "display_title": format!("Release {id}"),
           "html_url": format!("https://github.com/acme/pay/actions/runs/{id}"), "created_at": created_at,
           "triggering_actor": {"login": actor}})
}

fn can(env: &str, approve: bool) -> Value {
    json!({"environment": {"id": 1, "name": env}, "wait_timer": 0, "current_user_can_approve": approve, "reviewers": []})
}

/// Test that conditional ETag avoids budget burn (swr-github.md §8.1):
/// 1. First poll pass fetches runs and receives HTTP 200 with an ETag (storing the ETag in swr_deploy_lists).
/// 2. Second poll pass sends `If-None-Match` header with the cached ETag.
/// 3. On HTTP 304 Not Modified, 0 rate budget is consumed, no extra runs or pending_deployments calls are made,
///    and existing cached deploy rows are preserved.
#[test]
fn conditional_etag_avoids_budget_burn() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));

    struct ETagGh {
        pass: std::sync::atomic::AtomicUsize,
    }

    impl Handler for ETagGh {
        fn handle(&self, call: &Call) -> Reply {
            let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
            match args[..] {
                ["api", "user", "--jq", ".login"] => Reply::ok("clayton-duarte\n"),
                _ if args.starts_with(&["api"]) => {
                    let has_if_none_match = args.windows(2).any(|w| {
                        w[0] == "-H" && w[1].to_ascii_lowercase().starts_with("if-none-match:")
                    }) || args.iter().any(|a| {
                        let lower = a.to_ascii_lowercase();
                        lower.starts_with("if-none-match:") || lower.starts_with("-hif-none-match:")
                    });

                    let is_runs_query = args
                        .iter()
                        .any(|a| a.contains("actions/workflows/main.yml/runs"));
                    let is_pending = args.iter().any(|a| a.contains("pending_deployments"));

                    if is_runs_query {
                        let pass = self.pass.load(std::sync::atomic::Ordering::SeqCst);
                        let includes_headers = args.contains(&"--include");
                        if pass == 0 {
                            // Initial pass: returns 200 with ETag header and run 101 for waiting, empty for others.
                            let runs = if args.iter().any(|a| a.contains("status=waiting")) {
                                vec![run(101, "clayton-duarte", "2026-10-27T13:00:00Z")]
                            } else {
                                vec![]
                            };
                            let body = json!({
                                "total_count": runs.len(),
                                "workflow_runs": runs
                            })
                            .to_string();
                            if includes_headers {
                                Reply::ok(&format!(
                                    "HTTP/2 200 OK\r\netag: \"etag-main-12345\"\r\ncontent-type: application/json\r\n\r\n{body}"
                                ))
                            } else {
                                Reply::ok(&body)
                            }
                        } else {
                            // Second pass: must send conditional If-None-Match header.
                            if has_if_none_match {
                                Reply::ok(
                                    "HTTP/2 304 Not Modified\r\netag: \"etag-main-12345\"\r\n\r\n",
                                )
                            } else {
                                Reply::exit(
                                    9,
                                    "",
                                    "expected conditional If-None-Match header on second poll\n",
                                )
                            }
                        }
                    } else if is_pending {
                        Reply::ok(&json!([can("production", true)]).to_string())
                    } else {
                        Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" ")))
                    }
                }
                _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
            }
        }
    }

    let gh = std::sync::Arc::new(ETagGh {
        pass: std::sync::atomic::AtomicUsize::new(0),
    });
    let runner = FakeRunner::new(ManualClock::new()).route("gh", gh.clone());

    // Pass 1: populates cache with deploy run 101 and caches ETag.
    read(&runner, &home, "clayton-duarte")
        .expect("initial deploy read with ETag 200 response should succeed");

    let first_calls_count = runner.calls().len();
    assert!(first_calls_count > 0);

    // Verify initial row was cached.
    let runs: Vec<i64> = store(&home)
        .read(|c| {
            let mut stmt = c.prepare("SELECT run_id FROM swr_deploys WHERE inactive_at IS NULL")?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(runs, vec![101]);

    // Advance pass counter for pass 2.
    gh.pass.store(1, std::sync::atomic::Ordering::SeqCst);

    // Pass 2: sends If-None-Match and receives 304.
    read(&runner, &home, "clayton-duarte")
        .expect("second deploy read with 304 response should succeed");

    // The second pass must send If-None-Match.
    let second_calls: Vec<_> = runner.calls()[first_calls_count..].to_vec();
    let conditional_call = second_calls.iter().find(|c| {
        c.args
            .windows(2)
            .any(|w| w[0] == "-H" && w[1].to_ascii_lowercase().starts_with("if-none-match:"))
            || c.args.iter().any(|a| {
                let lower = a.to_ascii_lowercase();
                lower.starts_with("if-none-match:") || lower.starts_with("-hif-none-match:")
            })
    });
    assert!(
        conditional_call.is_some(),
        "second pass should send If-None-Match header: {second_calls:?}"
    );

    // On 304, zero new pending_deployments calls should be made.
    assert!(
        !second_calls
            .iter()
            .any(|c| c.args.iter().any(|a| a.contains("pending_deployments"))),
        "304 must not trigger any pending_deployments calls: {second_calls:?}"
    );

    // Cached deploy row 101 should still be present and active.
    let runs_after: Vec<i64> = store(&home)
        .read(|c| {
            let mut stmt = c.prepare("SELECT run_id FROM swr_deploys WHERE inactive_at IS NULL")?;
            Ok(stmt
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(runs_after, vec![101]);
}

/// Test that HTTP 403 rate limit response flags rate_limited error (swr-github.md §8.2):
/// When GitHub REST API returns HTTP 403 with "API rate limit exceeded" and x-ratelimit-reset header,
/// deploys::read_into_store fails with CpdError ErrorCode::GhFailed where:
/// - details.rate_limited is true
/// - details.reset_at holds RFC 3339 UTC timestamp parsed from x-ratelimit-reset epoch
/// - details.transient is true
/// - details.reason is "rate_limited"
#[test]
fn rate_limit_403_flags_rate_limited_error() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));

    struct RateLimitedGh;

    impl Handler for RateLimitedGh {
        fn handle(&self, call: &Call) -> Reply {
            let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
            match args[..] {
                ["api", "user", "--jq", ".login"] => Reply::ok("clayton-duarte\n"),
                _ if args
                    .iter()
                    .any(|a| a.contains("actions/workflows/main.yml/runs")) =>
                {
                    // HTTP 403 rate limit reply with x-ratelimit-reset header (epoch 1767229200 -> 2026-01-01T01:00:00Z)
                    let headers = "HTTP/2 403 Forbidden\r\nx-ratelimit-remaining: 0\r\nx-ratelimit-reset: 1767229200\r\ncontent-type: application/json\r\n\r\n";
                    let body = r#"{"message":"API rate limit exceeded for user ID 1.","documentation_url":"https://docs.github.com/rest/overview/resources-in-the-rest-api#rate-limiting"}"#;
                    let stderr = "gh: HTTP 403: API rate limit exceeded (https://docs.github.com/rest/overview/resources-in-the-rest-api#rate-limiting)\n";
                    Reply::exit(1, &format!("{headers}{body}"), stderr)
                }
                _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
            }
        }
    }

    let runner =
        FakeRunner::new(ManualClock::new()).route("gh", std::sync::Arc::new(RateLimitedGh));

    let err = read(&runner, &home, "clayton-duarte")
        .expect_err("HTTP 403 rate limit should result in an error");

    assert_eq!(err.code(), ErrorCode::GhFailed);

    let details = err.details();
    assert_eq!(
        details.get("rate_limited").and_then(Value::as_bool),
        Some(true),
        "details.rate_limited must be true: {details:?}"
    );

    assert_eq!(
        details.get("transient").and_then(Value::as_bool),
        Some(true),
        "details.transient must be true: {details:?}"
    );

    assert_eq!(
        details.get("reason").and_then(Value::as_str),
        Some("rate_limited"),
        "details.reason must be 'rate_limited': {details:?}"
    );

    let reset_at = details
        .get("reset_at")
        .and_then(Value::as_str)
        .expect("details.reset_at must be present as a string");
    assert_eq!(
        reset_at, "2026-01-01T01:00:00Z",
        "details.reset_at must match RFC 3339 UTC timestamp of epoch 1767229200"
    );
}
