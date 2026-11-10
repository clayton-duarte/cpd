//! The deploy read (contract §7.6 step 8, swr-github.md §8) on a scripted `gh`: which calls it makes and what it writes to
//! `swr_deploys`. No process, no network.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::deploys::read_into_store;
use crate::clock::ManualClock;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{Call, FakeRunner, Handler, Reply};
use crate::store::Store;

/// The `gh` of the read: the login of `gh api user`, the waiting runs of each `<owner>/<repo>/<file>` (`None`: the call
/// fails) and the `pending_deployments` of each run id.
#[derive(Default)]
struct Gh {
    me: String,
    runs: BTreeMap<String, Option<Vec<Value>>>,
    pending: BTreeMap<u64, Value>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "user", "--jq", ".login"] => Reply::ok(&format!("{}\n", self.me)),
            ["api", path] => {
                let parts: Vec<&str> = path.split('/').collect();
                match parts[..] {
                    ["repos", owner, repo, "actions", "workflows", file, query]
                        if query.starts_with("runs?status=")
                            && query.ends_with("&per_page=100") =>
                    {
                        match self.runs.get(&format!("{owner}/{repo}/{file}")) {
                            // The scripted runs wait; the active (`in_progress`, `queued`) reads of the same workflow list none.
                            Some(Some(runs)) => {
                                let runs = if query == "runs?status=waiting&per_page=100" {
                                    runs.clone()
                                } else {
                                    Vec::new()
                                };
                                Reply::ok(
                                    &json!({"total_count": runs.len(), "workflow_runs": runs})
                                        .to_string(),
                                )
                            }
                            Some(None) => Reply::exit(1, "", "gh: HTTP 500: Server Error\n"),
                            None => Reply::exit(9, "", &format!("unscripted workflow: {path}\n")),
                        }
                    }
                    ["repos", _, _, "actions", "runs", id, "pending_deployments"] => {
                        match id.parse().ok().and_then(|id: u64| self.pending.get(&id)) {
                            Some(pending) => Reply::ok(&pending.to_string()),
                            None => Reply::exit(9, "", &format!("unscripted run: {path}\n")),
                        }
                    }
                    _ => Reply::exit(9, "", &format!("unexpected gh call: {path}\n")),
                }
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

fn run(id: u64, actor: &str, created_at: &str) -> Value {
    json!({"id": id, "status": "waiting", "display_title": format!("Release {id}"),
           "html_url": format!("https://github.com/acme/pay/actions/runs/{id}"), "created_at": created_at,
           "triggering_actor": {"login": actor}})
}

fn can(env: &str, approve: bool) -> Value {
    json!({"environment": {"id": 1, "name": env}, "wait_timer": 0, "current_user_can_approve": approve, "reviewers": []})
}

fn tmp() -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    (dir, home)
}

/// The repo policy file `<owner>/<repo>.toml` declaring `workflows` (none: no `[deploys]` table).
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

/// Reads the deploys for the viewer `me` (the login the pass stored in `swr_viewer`).
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

/// Seeds the active rows the old cache held (`runs`: `entry` values).
fn put_cache(home: &CpdHome, runs: Value) {
    let mut store = store(home);
    store
        .write("fixture deploys", |tx| {
            for r in runs.as_array().unwrap() {
                let pending: Vec<Value> = r["environment"]
                    .as_str()
                    .unwrap()
                    .split(", ")
                    .map(|n| json!({"environment": {"id": 1, "name": n}, "current_user_can_approve": true}))
                    .collect();
                let run = json!({"id": r["run_id"], "updated_at": "U0", "display_title": r["title"], "html_url": r["url"],
                                 "created_at": r["created_at"], "_workflow": r["workflow"]});
                tx.execute(
                    "INSERT INTO swr_deploys (repo, run_id, updated_at, run_json, pending_json, inactive_at) VALUES (?1, ?2, 'U0', ?3, ?4, NULL)",
                    rusqlite::params![r["repo"].as_str(), r["run_id"].as_i64(), run.to_string(), Value::Array(pending).to_string()],
                )?;
            }
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

/// The active rows, as the old cache listed them: by `created_at`, then run id.
fn cached(home: &CpdHome) -> Value {
    let rows: Vec<Value> = store(home)
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT repo, run_id, run_json, pending_json FROM swr_deploys WHERE inactive_at IS NULL
                  ORDER BY json_extract(run_json, '$.created_at'), run_id",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    let run: Value = serde_json::from_str(&r.get::<_, String>(2)?).unwrap();
                    let pending: Vec<Value> = serde_json::from_str(&r.get::<_, String>(3)?).unwrap();
                    let mut names: Vec<&str> = pending.iter().filter_map(|p| p["environment"]["name"].as_str()).collect();
                    names.sort_unstable();
                    Ok(json!({"repo": r.get::<_, String>(0)?, "workflow": run["_workflow"], "run_id": r.get::<_, i64>(1)?,
                              "environment": names.join(", "), "title": run["display_title"], "url": run["html_url"],
                              "created_at": run["created_at"]}))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    json!({"runs": rows})
}

fn entry(repo: &str, workflow: &str, run_id: u64, environment: &str, created_at: &str) -> Value {
    json!({"repo": repo, "workflow": workflow, "run_id": run_id, "environment": environment, "title": format!("Release {run_id}"),
           "url": format!("https://github.com/acme/pay/actions/runs/{run_id}"), "created_at": created_at})
}

fn scripted(gh: Gh) -> FakeRunner {
    FakeRunner::new(ManualClock::new()).route("gh", std::sync::Arc::new(gh))
}

fn gh(me: &str, runs: &[(&str, Option<Vec<Value>>)], pending: &[(u64, Value)]) -> Gh {
    Gh {
        me: me.to_owned(),
        runs: runs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
        pending: pending.iter().cloned().collect(),
    }
}

/// The `gh` calls made, as `gh <args>` lines.
fn calls(runner: &FakeRunner) -> Vec<String> {
    runner.calls().iter().map(|c| c.args.join(" ")).collect()
}

#[test]
fn one_waiting_run_of_the_user_is_cached() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));
    let runner = scripted(gh(
        "clayton-duarte",
        &[(
            "acme/pay/main.yml",
            Some(vec![run(
                40508749055,
                "clayton-duarte",
                "2026-10-27T13:05:00Z",
            )]),
        )],
        &[(40508749055, json!([can("production", true)]))],
    ));
    read(&runner, &home, "clayton-duarte").unwrap();

    let file = cached(&home);
    assert_eq!(
        file["runs"],
        json!([entry(
            "acme/pay",
            "main.yml",
            40508749055,
            "production",
            "2026-10-27T13:05:00Z"
        )])
    );
    assert!(
        calls(&runner).contains(
            &"api repos/acme/pay/actions/runs/40508749055/pending_deployments".to_owned()
        ),
        "{:?}",
        calls(&runner)
    );
}

#[test]
fn a_run_of_another_actor_is_skipped_and_its_pending_deployments_not_asked() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));
    let runner = scripted(gh(
        "clayton-duarte",
        &[(
            "acme/pay/main.yml",
            Some(vec![
                run(1, "someone-else", "2026-10-27T13:00:00Z"),
                run(2, "clayton-duarte", "2026-10-27T13:01:00Z"),
            ]),
        )],
        &[
            (1, json!([can("production", true)])),
            (2, json!([can("production", true)])),
        ],
    ));
    read(&runner, &home, "clayton-duarte").unwrap();

    let ids: Vec<u64> = cached(&home)["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [2]);
    let all = calls(&runner);
    assert!(
        all.iter()
            .any(|c| c.contains("/runs/2/pending_deployments")),
        "{all:?}"
    );
    assert!(
        !all.iter()
            .any(|c| c.contains("/runs/1/pending_deployments")),
        "{all:?}"
    );
}

#[test]
fn a_run_nobody_can_approve_by_the_user_is_skipped() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));
    let runner = scripted(gh(
        "clayton-duarte",
        &[(
            "acme/pay/main.yml",
            Some(vec![
                run(1, "clayton-duarte", "2026-10-27T13:00:00Z"),
                run(2, "clayton-duarte", "2026-10-27T13:01:00Z"),
                run(3, "clayton-duarte", "2026-10-27T13:02:00Z"),
            ]),
        )],
        &[
            (1, json!([can("production", false), can("staging", false)])),
            (2, json!([])),
            (3, json!([can("production", true)])),
        ],
    ));
    read(&runner, &home, "clayton-duarte").unwrap();

    let ids: Vec<u64> = cached(&home)["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run_id"].as_u64().unwrap())
        .collect();
    assert_eq!(
        ids,
        [3],
        "a run waiting with no entry the User can approve is no deploy"
    );
}

#[test]
fn environments_the_user_can_approve_join_sorted() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));
    let runner = scripted(gh(
        "clayton-duarte",
        &[(
            "acme/pay/main.yml",
            Some(vec![run(1, "clayton-duarte", "2026-10-27T13:00:00Z")]),
        )],
        &[(
            1,
            json!([
                can("staging", true),
                can("audit", false),
                can("production", true)
            ]),
        )],
    ));
    read(&runner, &home, "clayton-duarte").unwrap();
    assert_eq!(
        cached(&home)["runs"][0]["environment"],
        "production, staging"
    );
}

#[test]
fn the_login_compare_ignores_case() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));
    let runner = scripted(gh(
        "Clayton-Duarte",
        &[(
            "acme/pay/main.yml",
            Some(vec![
                run(1, "clayton-duarte", "2026-10-27T13:00:00Z"),
                run(2, "CLAYTON-DUARTE", "2026-10-27T13:01:00Z"),
                run(3, "clayton-duarte2", "2026-10-27T13:02:00Z"),
            ]),
        )],
        &[
            (1, json!([can("production", true)])),
            (2, json!([can("production", true)])),
            (3, json!([can("production", true)])),
        ],
    ));
    read(&runner, &home, "Clayton-Duarte").unwrap();
    let ids: Vec<u64> = cached(&home)["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [1, 2]);
}

#[test]
fn one_runs_call_per_declared_workflow() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml", "build.yaml"]));
    declare(&home, "acme/web", Some(&["deploy.yml"]));
    declare(&home, "acme/none", Some(&[]));
    declare(&home, "acme/bare", None);
    let runner = scripted(gh(
        "clayton-duarte",
        &[
            (
                "acme/pay/main.yml",
                Some(vec![run(1, "clayton-duarte", "2026-10-27T13:00:00Z")]),
            ),
            ("acme/pay/build.yaml", Some(vec![])),
            ("acme/web/deploy.yml", Some(vec![])),
        ],
        &[(1, json!([can("production", true)]))],
    ));
    read(&runner, &home, "clayton-duarte").unwrap();

    let mut runs_calls: Vec<String> = calls(&runner)
        .into_iter()
        .filter(|c| c.contains("runs?status="))
        .collect();
    runs_calls.sort();
    let expected: Vec<String> = [
        "pay/actions/workflows/build.yaml",
        "pay/actions/workflows/main.yml",
        "web/actions/workflows/deploy.yml",
    ]
    .iter()
    .flat_map(|w| {
        ["in_progress", "queued", "waiting"]
            .map(|s| format!("api repos/acme/{w}/runs?status={s}&per_page=100"))
    })
    .collect();
    assert_eq!(runs_calls, expected);
    assert_eq!(cached(&home)["runs"].as_array().unwrap().len(), 1);

    // Nothing declared anywhere: no runs call, and the file holds no run.
    let (_d, empty) = tmp();
    declare(&empty, "acme/bare", None);
    put_cache(
        &empty,
        json!([entry(
            "acme/bare",
            "main.yml",
            5,
            "production",
            "2026-10-27T12:00:00Z"
        )]),
    );
    let runner = scripted(gh("clayton-duarte", &[], &[]));
    read(&runner, &empty, "clayton-duarte").unwrap();
    assert!(
        !calls(&runner).iter().any(|c| c.contains("runs?status=")),
        "{:?}",
        calls(&runner)
    );
    assert_eq!(cached(&empty)["runs"], json!([]));
}

#[test]
fn a_failed_workflow_call_keeps_its_previous_runs_and_updates_the_others() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["build.yaml", "main.yml"]));
    put_cache(
        &home,
        json!([
            entry(
                "acme/pay",
                "build.yaml",
                1,
                "production",
                "2026-10-27T12:00:00Z"
            ),
            entry(
                "acme/pay",
                "main.yml",
                2,
                "production",
                "2026-10-27T12:30:00Z"
            ),
        ]),
    );
    // `build.yaml` (read first) fails; `main.yml` now holds run 3 and no longer run 2.
    let runner = scripted(gh(
        "clayton-duarte",
        &[
            ("acme/pay/build.yaml", None),
            (
                "acme/pay/main.yml",
                Some(vec![run(3, "clayton-duarte", "2026-10-27T13:00:00Z")]),
            ),
        ],
        &[(3, json!([can("staging", true)]))],
    ));
    let err: CpdError = read(&runner, &home, "clayton-duarte")
        .expect_err("a failed call fails the read like a failed Workflow lookup");
    assert_eq!(err.code(), ErrorCode::GhFailed);

    assert_eq!(
        cached(&home)["runs"],
        json!([
            entry(
                "acme/pay",
                "build.yaml",
                1,
                "production",
                "2026-10-27T12:00:00Z"
            ),
            entry("acme/pay", "main.yml", 3, "staging", "2026-10-27T13:00:00Z"),
        ])
    );
}

#[test]
fn a_removed_declaration_drops_its_runs() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", None);
    declare(&home, "acme/web", Some(&["deploy.yml"]));
    put_cache(
        &home,
        json!([
            entry(
                "acme/pay",
                "main.yml",
                1,
                "production",
                "2026-10-27T12:00:00Z"
            ),
            entry(
                "acme/web",
                "deploy.yml",
                2,
                "production",
                "2026-10-27T12:30:00Z"
            ),
        ]),
    );
    let runner = scripted(gh(
        "clayton-duarte",
        &[(
            "acme/web/deploy.yml",
            Some(vec![run(4, "clayton-duarte", "2026-10-27T13:00:00Z")]),
        )],
        &[(4, json!([can("production", true)]))],
    ));
    read(&runner, &home, "clayton-duarte").unwrap();

    let repos: Vec<(String, u64)> = cached(&home)["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["repo"].as_str().unwrap().to_owned(),
                r["run_id"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(repos, [("acme/web".to_owned(), 4)]);
    assert!(
        !calls(&runner).iter().any(|c| c.contains("acme/pay")),
        "{:?}",
        calls(&runner)
    );
}

#[test]
fn the_rows_come_back_sorted_by_created_at_then_run_id() {
    let (_d, home) = tmp();
    declare(&home, "acme/pay", Some(&["main.yml"]));
    let runner = scripted(gh(
        "clayton-duarte",
        &[(
            "acme/pay/main.yml",
            Some(vec![
                run(30, "clayton-duarte", "2026-10-27T13:00:00Z"),
                run(20, "clayton-duarte", "2026-10-27T13:00:00Z"),
                run(10, "clayton-duarte", "2026-10-27T14:00:00Z"),
                run(40, "clayton-duarte", "2026-10-27T12:00:00Z"),
            ]),
        )],
        &[
            (10, json!([can("p", true)])),
            (20, json!([can("p", true)])),
            (30, json!([can("p", true)])),
            (40, json!([can("p", true)])),
        ],
    ));
    read(&runner, &home, "clayton-duarte").unwrap();

    let file = cached(&home);
    let ids: Vec<u64> = file["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run_id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [40, 20, 30, 10], "by created_at, then run_id");
}
