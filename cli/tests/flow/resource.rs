//! `cpd resource list` and `cpd resource run` (commands.md §30): in-process on the fakes. The test holds a resource with
//! its own open file description of the lock file, as another process would; `run`'s exec is the `FakeSpawn`'s
//! `exec_keeping`, which records the call and runs a hook while the process "is exec-ed".

use crate::common;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx;
use common::refused;
use cpd_core::lock;
use serde_json::{Value, json};

const ATTEMPT_RECORD: &str = r#"{"by":"attempt","plan":"pl","job":"api","attempt":"api-a1","at":"2026-10-26T21:41:00.000Z"}"#;

/// A repo `acme/cpd` whose policy file is `policy`, in an `Fx` on fake ports.
pub(super) fn world(policy: &str) -> (Fx, PathBuf) {
    let fx = Fx::with_ports();
    let home = fx.env.cpd_home();
    fs::write(home.join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    fs::create_dir_all(home.join("policy/acme")).unwrap();
    fs::write(home.join("policy/acme/owner.toml"), "").unwrap();
    fx.env.repo_policy("acme", "cpd", policy);
    let repo = fx.keyed_repo("cpd");
    (fx, repo)
}

/// Slot `slot` of the repo pool `name` of `acme/cpd` (contract §1.4).
pub(super) fn slot_path(fx: &Fx, name: &str, slot: u8) -> PathBuf {
    fx.env
        .cpd_home()
        .join(format!("locks/resource.repo.acme.cpd.{name}.{slot}.lock"))
}

fn lock_path(fx: &Fx, name: &str) -> PathBuf {
    slot_path(fx, name, 1)
}

/// Holds `name` as another process would: its own open file description, with `record` in the file.
pub(super) fn hold(fx: &Fx, name: &str, record: &str) -> File {
    hold_slot(fx, &slot_path(fx, name, 1), record)
}

fn hold_slot(_fx: &Fx, path: &Path, record: &str) -> File {
    let path = path.to_path_buf();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    assert!(
        lock::try_lock(&file).unwrap(),
        "the test could not take {}",
        path.display()
    );
    fs::write(&path, record).unwrap();
    file
}

/// Whether some open file description holds the lock at `path` (a shared try-lock is refused while one does).
fn is_held(path: &Path) -> bool {
    let file = File::open(path).unwrap();
    !lock::shared(&file).unwrap()
}

fn run_args<'a>(name: &'a str, rest: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["resource", "run", name];
    args.extend_from_slice(rest);
    args
}

/// What the hook of `on_exec` saw while the process was exec-ed: whether the lock was held, and the file's content.
type Seen = Arc<Mutex<Vec<(bool, String)>>>;

fn watch_exec(fx: &Fx, name: &str) -> Seen {
    watch_exec_slot(fx, name, 1)
}

fn watch_exec_slot(fx: &Fx, name: &str, slot: u8) -> Seen {
    let seen: Seen = Arc::default();
    let (path, sink) = (slot_path(fx, name, slot), seen.clone());
    fx.fakes.spawn.on_exec(move |_| {
        sink.lock().unwrap().push((
            is_held(&path),
            fs::read_to_string(&path).unwrap_or_default(),
        ));
    });
    seen
}

#[test]
fn resource_run_refuses_an_undeclared_resource() {
    let (fx, repo) = world("[resources.e2e-port]\n");

    let run = fx.cpd_in(&repo, &run_args("nope", &["--", "npm", "test"]));

    let error = refused(&run, "resource_undeclared", 3);
    assert_eq!(error["details"]["resource"], "nope");
    assert_eq!(error["details"]["repo"], "acme/cpd");
    assert_eq!(error["details"]["declared"], json!(["e2e-port"]));
    assert!(
        error["details"]["file"]
            .as_str()
            .unwrap()
            .ends_with("policy/acme/cpd.toml"),
        "{error}"
    );
    assert!(fx.fakes.spawn.execs().is_empty());
}

#[test]
fn resource_run_holds_the_lock_and_writes_its_record_when_it_execs_the_command() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    let seen = watch_exec(&fx, "e2e-port");

    let _ = fx.cpd_with(
        &repo,
        &run_args("e2e-port", &["--", "npm", "run", "dev"]),
        &[("CPD_SESSION", "s1")],
    );

    let execs = fx.fakes.spawn.execs();
    assert_eq!(execs.len(), 1, "the command is exec-ed once");
    let launch = &execs[0].launch;
    assert_eq!(
        (launch.program.as_str(), launch.args.as_slice()),
        ("npm", ["run".to_owned(), "dev".to_owned()].as_slice())
    );
    assert_eq!(
        launch.cwd.as_deref().map(|d| fs::canonicalize(d).unwrap()),
        Some(fs::canonicalize(&repo).unwrap())
    );
    assert_eq!(
        execs[0].inheritable,
        Some(true),
        "the lock descriptor is left inheritable across the exec"
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (held, content) = &seen[0];
    assert!(held, "the resource is held when the command is exec-ed");
    let record: Value = serde_json::from_str(content)
        .unwrap_or_else(|e| panic!("the record {content:?} is not JSON: {e}"));
    assert_eq!(
        (
            &record["by"],
            &record["pid"],
            &record["command"],
            &record["session"],
            &record["attempt"]
        ),
        (
            &json!("command"),
            &json!(std::process::id()),
            &json!("npm run dev"),
            &json!("s1"),
            &Value::Null
        )
    );
    assert!(record["at"].is_string(), "{record}");
}

#[test]
fn resource_run_takes_a_free_slot_while_another_slot_is_held() {
    let (fx, repo) = world("[resources.e2e-port]\nslots = 2\n");
    let _first = hold(&fx, "e2e-port", &common::command_record());
    let seen = watch_exec_slot(&fx, "e2e-port", 2);

    let _ = fx.cpd_with(
        &repo,
        &run_args("e2e-port", &["--", "npm", "run", "dev"]),
        &[("CPD_SESSION", "s1")],
    );

    assert_eq!(
        fx.fakes.spawn.execs().len(),
        1,
        "the command is exec-ed on the free slot"
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (held, content) = &seen[0];
    assert!(held, "slot 2 is held when the command is exec-ed");
    let record: Value = serde_json::from_str(content)
        .unwrap_or_else(|e| panic!("the record {content:?} is not JSON: {e}"));
    assert_eq!(
        (&record["by"], &record["command"]),
        (&json!("command"), &json!("npm run dev"))
    );
    assert_eq!(
        fs::read_to_string(slot_path(&fx, "e2e-port", 1)).unwrap(),
        common::command_record(),
        "slot 1 is left as it was"
    );
}

#[test]
fn resource_run_is_busy_with_every_holder_when_all_slots_are_held() {
    let (fx, repo) = world("[resources.e2e-port]\nslots = 2\n");
    let _first = hold(&fx, "e2e-port", &common::command_record());
    let _second = hold_slot(&fx, &slot_path(&fx, "e2e-port", 2), ATTEMPT_RECORD);

    let run = fx.cpd_in(&repo, &run_args("e2e-port", &["--", "npm", "test"]));

    let error = refused(&run, "resource_busy", 3);
    let details = &error["details"];
    assert_eq!(details["resource"], "e2e-port");
    assert_eq!(details["repo"], "acme/cpd");
    assert_eq!(
        details["holders"],
        json!([
            {"slot": 1, "holder": serde_json::from_str::<Value>(&common::command_record()).unwrap()},
            {"slot": 2, "holder": serde_json::from_str::<Value>(ATTEMPT_RECORD).unwrap()},
        ]),
        "the holder of every held slot with its slot number, in slot order"
    );
    assert_eq!(details["waited_minutes"], 0);
    assert!(details.get("holder").is_none(), "{details}");
    assert!(fx.fakes.spawn.execs().is_empty());
}

#[test]
fn resource_run_wait_takes_the_lock_when_the_holder_ends_within_the_minutes() {
    let (mut fx, repo) = world("[resources.e2e-port]\n");
    let mut holder = Some(hold(&fx, "e2e-port", &common::command_record()));
    // Another process ends at the run's first wait.
    fx.on_nap(move |_| drop(holder.take()));
    let seen = watch_exec(&fx, "e2e-port");

    let _ = fx.cpd_in(
        &repo,
        &run_args("e2e-port", &["--wait", "2", "--", "npm", "test"]),
    );

    assert_eq!(
        fx.fakes.spawn.execs().len(),
        1,
        "the command is exec-ed once the holder ended"
    );
    let seen = seen.lock().unwrap();
    let (held, content) = &seen[0];
    assert!(held);
    assert_eq!(
        serde_json::from_str::<Value>(content).unwrap()["pid"],
        std::process::id()
    );
}

#[test]
fn resource_run_wait_that_runs_out_is_busy_with_waited_minutes() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    let _holder = hold(&fx, "e2e-port", &common::command_record());

    let run = fx.cpd_in(
        &repo,
        &run_args("e2e-port", &["--wait", "2", "--", "npm", "test"]),
    );

    let error = refused(&run, "resource_busy", 3);
    assert_eq!(error["details"]["waited_minutes"], 2);
    assert!(
        fx.fakes.clock.elapsed() >= Duration::from_secs(120),
        "the two minutes passed on the Ports' clock"
    );
    assert!(fx.fakes.spawn.execs().is_empty());
}

#[test]
fn resource_run_execs_at_once_when_its_own_attempt_holds_the_resource() {
    // The Attempt's Job is a tenant of the pool in the Ledger (commands.md 30.2): it holds the one slot already.
    let (fx, repo) = tenancy_world("[resources.e2e-port]\n");
    job_tenant(&fx, "api", "repo:acme/cpd:e2e-port", API_AT);

    let _ = fx.cpd_with(
        &repo,
        &run_args("e2e-port", &["--", "npm", "test"]),
        &[("CPD_ATTEMPT", "api-a1")],
    );

    let launches = fx.fakes.spawn.launches_but_janitor();
    assert_eq!(
        launches.len(),
        1,
        "the Attempt does not wait on itself: the command is exec-ed at once"
    );
    assert_eq!(
        (launches[0].program.as_str(), launches[0].args.as_slice()),
        ("npm", ["test".to_owned()].as_slice())
    );
    assert!(
        !lock_path(&fx, "e2e-port").exists(),
        "it takes and writes nothing"
    );
    assert!(fx.fakes.clock.naps().is_empty());
}

#[test]
fn resource_run_an_empty_command_is_a_usage_error() {
    let (fx, repo) = world("[resources.e2e-port]\n");

    for args in [
        run_args("e2e-port", &["--"]),
        run_args("e2e-port", &[]),
        run_args("e2e-port", &["--wait", "0", "--", "npm"]),
    ] {
        refused(&fx.cpd_in(&repo, &args), "usage", 2);
    }
    assert!(fx.fakes.spawn.execs().is_empty());
    assert!(!lock_path(&fx, "e2e-port").exists() || !is_held(&lock_path(&fx, "e2e-port")));
}

#[test]
fn resource_run_an_exec_that_fails_is_spawn_failed() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    fx.fakes.spawn.fail_exec(std::io::ErrorKind::NotFound);
    let seen = watch_exec(&fx, "e2e-port");

    let run = fx.cpd_in(&repo, &run_args("e2e-port", &["--", "no-such-program"]));

    refused(&run, "spawn_failed", 1);
    assert!(
        seen.lock().unwrap()[0].0,
        "the exec was tried with the lock held"
    );
    assert!(
        !is_held(&lock_path(&fx, "e2e-port")),
        "the lock goes with the process"
    );
}

#[test]
fn resource_run_a_directory_without_a_keyable_origin_is_repo_unkeyed() {
    let (fx, _repo) = world("[resources.e2e-port]\n");
    let other = fx.git_repo("elsewhere");
    fx.git(
        &other,
        &[
            "remote",
            "add",
            "origin",
            "https://gitlab.com/acme/elsewhere.git",
        ],
    );

    let run = fx.cpd_in(&other, &run_args("e2e-port", &["--", "npm", "test"]));

    refused(&run, "repo_unkeyed", 1);
    assert!(fx.fakes.spawn.execs().is_empty());
}

/// The `resources` of a `resource list` without the built-in `workers`, which every repo has (tested on its own).
pub(super) fn declared_only(out: &Value) -> Value {
    Value::Array(
        out["resources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["name"] != "workers")
            .cloned()
            .collect(),
    )
}

#[test]
fn resource_list_shows_each_slot_with_its_holder_and_the_scope() {
    let (fx, repo) = world("[resources.e2e-port]\nslots = 3\n\n[resources.db]\n");
    fs::write(
        fx.env.cpd_home().join("policy/host.toml"),
        "[resources.shared-port]\nslots = 2\n",
    )
    .unwrap();
    fs::write(
        fx.env.cpd_home().join("policy/acme/owner.toml"),
        "[resources.staging]\n",
    )
    .unwrap();
    let _first = hold(&fx, "e2e-port", &common::command_record());
    let _third = hold_slot(&fx, &slot_path(&fx, "e2e-port", 3), ATTEMPT_RECORD);
    // A record left in the file of a free lock is no holder.
    drop(hold(&fx, "db", &common::command_record()));
    let host_slot = fx
        .env
        .cpd_home()
        .join("locks/resource.host.shared-port.2.lock");
    let _host = hold_slot(&fx, &host_slot, &common::command_record());

    let out = fx.cpd_in(&repo, &["resource", "list"]).ok();

    let command: Value = serde_json::from_str(&common::command_record()).unwrap();
    let attempt: Value = serde_json::from_str(ATTEMPT_RECORD).unwrap();
    assert_eq!(out["repo"], "acme/cpd");
    assert_eq!(
        declared_only(&out),
        json!([
            {"name": "db", "scope": "repo", "slots": 1, "free": 1, "holders": []},
            {"name": "e2e-port", "scope": "repo", "slots": 3, "free": 1,
             "holders": [{"slot": 1, "holder": command}, {"slot": 3, "holder": attempt}]},
            {"name": "shared-port", "scope": "host", "slots": 2, "free": 1, "holders": [{"slot": 2, "holder": command}]},
            {"name": "staging", "scope": "owner", "slots": 1, "free": 1, "holders": []},
        ]),
        "the held slots with their holders and no entry for a free slot (commands.md 30.1)"
    );
}

#[test]
fn resource_list_with_none_declared_is_empty() {
    let (fx, repo) = world("");

    let out = fx.cpd_in(&repo, &["resource", "list"]).ok();

    assert_eq!(out["repo"], "acme/cpd");
    assert_eq!(declared_only(&out), json!([]));
}

#[test]
fn resource_run_refuses_workers_as_usage() {
    let (fx, repo) = world("[resources.e2e-port]\n");

    let run = fx.cpd_in(&repo, &run_args("workers", &["--", "npm", "test"]));

    // `workers` is the built-in resource, taken only by the starter: a human never takes a Worker slot (commands.md §30.2).
    refused(&run, "usage", 2);
    assert!(fx.fakes.spawn.execs().is_empty());
    assert!(
        !fx.env.cpd_home().join("locks").exists()
            || fs::read_dir(fx.env.cpd_home().join("locks"))
                .unwrap()
                .next()
                .is_none()
    );
}

#[test]
fn resource_list_shows_workers_with_the_running_attempts() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    // Concurrency 3, two Attempts not ended (one `starting`, one `running`) and one that ended with its Job `failed`: Plan `pl`'s Jobs `a`, `b` and `c`.
    super::job::setup(&fx.env);
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\nconcurrency = 3\n",
    )
    .unwrap();
    super::job::plan(&fx.env, "pl", "dispatch");
    let fingerprint = "f".repeat(64);
    let mut sql = String::new();
    for (job, status, at) in [
        ("a", "running", "2026-10-26T21:40:00.000Z"),
        ("b", "starting", "2026-10-26T21:41:00.000Z"),
        ("c", "ended", "2026-10-26T21:42:00.000Z"),
    ] {
        let (job_status, cause, ended, ended_values) = if status == "ended" {
            (
                "failed",
                "'crashed'",
                ", outcome, cause, ended_at",
                ", 'failed', 'crashed', 't'",
            )
        } else {
            ("running", "NULL", "", "")
        };
        sql += &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, failed_cause, appended, fingerprint, created_txn, created_at)
               VALUES ('pl', 'repo', '{job}', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{job_status}', {cause}, 0, '{fingerprint}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at{ended})
               VALUES ('pl', '{job}-a1', '{job}', 1, 'S2', 'ladder', '{status}', 'cpd/pl--{job}-a1', '/w', '{at}'{ended_values});"
        );
    }
    super::job::fixture(&fx.env, &sql);

    let out = fx.cpd_in(&repo, &["resource", "list"]).ok();

    // The built-in `workers` is listed with the declared resources, sorted by name (commands.md §30.1): host scope, `concurrency`
    // slots, one holder `{slot, holder}` per tenant Job (`c` is `failed` with its Attempt ended and keeps its worker, contract §3.4
    // "Tenancy"), `holder` the shape `{by: "attempt", plan, job, attempt, at}`.
    let resources = out["resources"].as_array().unwrap();
    assert_eq!(
        resources
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["e2e-port", "workers"],
        "{out}"
    );
    let workers = &resources[1];
    assert_eq!(
        (&workers["scope"], &workers["slots"], &workers["free"]),
        (&json!("host"), &json!(3), &json!(0)),
        "{workers}"
    );
    let holders = workers["holders"].as_array().unwrap();
    assert_eq!(holders.len(), 3, "one per tenant Job: {workers}");
    for (i, (job, at)) in [
        ("a", "2026-10-26T21:40:00.000Z"),
        ("b", "2026-10-26T21:41:00.000Z"),
        ("c", "2026-10-26T21:42:00.000Z"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(holders[i]["slot"], i + 1);
        assert_eq!(
            holders[i]["holder"],
            json!({"by": "attempt", "plan": "pl", "job": job, "attempt": format!("{job}-a1"), "at": at})
        );
    }
}

/// Job `job` of Plan `pl` as a tenant of `pool` (the text of contract §3.4 "Resources of a Job" item 3) with its first Attempt
/// `running` since `at`: the rows the starter writes in one transaction. Plan `pl` and its PR exist after [`tenancy_world`].
pub(super) fn job_tenant(fx: &Fx, job: &str, pool: &str, at: &str) {
    let fingerprint = "f".repeat(64);
    super::job::fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pl', 'repo', '{job}', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fingerprint}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
               VALUES ('pl', '{job}-a1', '{job}', 1, 'S2', 'ladder', 'running', 'cpd/pl--{job}-a1', '/w', '{at}');
             INSERT INTO job_pools (plan, job, pool) VALUES ('pl', '{job}', '{pool}');"
        ),
    );
}

/// [`world`] with Plan `pl` in `dispatch`, whose Jobs the test adds with [`job_tenant`].
pub(super) fn tenancy_world(policy: &str) -> (Fx, PathBuf) {
    let (fx, repo) = world(policy);
    super::job::setup(&fx.env);
    super::job::plan(&fx.env, "pl", "dispatch");
    (fx, repo)
}

pub(super) const API_AT: &str = "2026-10-26T21:41:00.000Z";

pub(super) fn api_holder() -> Value {
    json!({"by": "attempt", "plan": "pl", "job": "api", "attempt": "api-a1", "at": API_AT})
}
