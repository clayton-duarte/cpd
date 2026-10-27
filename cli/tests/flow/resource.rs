//! `cpd resource list` and `cpd resource run` (commands.md §29): in-process on the fakes. The test holds a resource with
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

const COMMAND_RECORD: &str =
    r#"{"by":"command","pid":4242,"command":"npm run dev","session":"s9","attempt":null,"at":"2026-10-26T21:40:00.000Z"}"#;
const ATTEMPT_RECORD: &str = r#"{"by":"attempt","plan":"pl","job":"api","attempt":"api-a1","at":"2026-10-26T21:41:00.000Z"}"#;

/// A repo `acme/cpd` whose policy file is `policy`, in an `Fx` on fake ports.
fn world(policy: &str) -> (Fx, PathBuf) {
    let fx = Fx::with_ports();
    let home = fx.env.cpd_home();
    fs::write(home.join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    fs::create_dir_all(home.join("policy/acme")).unwrap();
    fs::write(home.join("policy/acme/owner.toml"), "").unwrap();
    fx.env.repo_policy("acme", "cpd", policy);
    let repo = fx.keyed_repo("cpd");
    (fx, repo)
}

fn lock_path(fx: &Fx, name: &str) -> PathBuf {
    fx.env.cpd_home().join(format!("locks/resource-acme--cpd--{name}.lock"))
}

/// Holds `name` as another process would: its own open file description, with `record` in the file.
fn hold(fx: &Fx, name: &str, record: &str) -> File {
    let path = lock_path(fx, name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    assert!(lock::try_lock(&file).unwrap(), "the test could not take {name}");
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
    let seen: Seen = Arc::default();
    let (path, sink) = (lock_path(fx, name), seen.clone());
    fx.fakes.spawn.on_exec(move |_| {
        sink.lock().unwrap().push((is_held(&path), fs::read_to_string(&path).unwrap_or_default()));
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
    assert!(error["details"]["file"].as_str().unwrap().ends_with("policy/acme/cpd.toml"), "{error}");
    assert!(fx.fakes.spawn.execs().is_empty());
}

#[test]
fn resource_run_holds_the_lock_and_writes_its_record_when_it_execs_the_command() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    let seen = watch_exec(&fx, "e2e-port");

    let _ = fx.cpd_with(&repo, &run_args("e2e-port", &["--", "npm", "run", "dev"]), &[("CPD_SESSION", "s1")]);

    let execs = fx.fakes.spawn.execs();
    assert_eq!(execs.len(), 1, "the command is exec-ed once");
    let launch = &execs[0].launch;
    assert_eq!((launch.program.as_str(), launch.args.as_slice()), ("npm", ["run".to_owned(), "dev".to_owned()].as_slice()));
    assert_eq!(launch.cwd.as_deref().map(|d| fs::canonicalize(d).unwrap()), Some(fs::canonicalize(&repo).unwrap()));
    assert_eq!(execs[0].inheritable, Some(true), "the lock descriptor is left inheritable across the exec");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let (held, content) = &seen[0];
    assert!(held, "the resource is held when the command is exec-ed");
    let record: Value = serde_json::from_str(content).unwrap_or_else(|e| panic!("the record {content:?} is not JSON: {e}"));
    assert_eq!(
        (&record["by"], &record["pid"], &record["command"], &record["session"], &record["attempt"]),
        (&json!("command"), &json!(std::process::id()), &json!("npm run dev"), &json!("s1"), &Value::Null)
    );
    assert!(record["at"].is_string(), "{record}");
}

#[test]
fn resource_run_is_busy_while_another_holds_it_and_nothing_is_exec_ed() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    let _holder = hold(&fx, "e2e-port", COMMAND_RECORD);

    let run = fx.cpd_in(&repo, &run_args("e2e-port", &["--", "npm", "test"]));

    let error = refused(&run, "resource_busy", 3);
    assert_eq!(error["details"]["resource"], "e2e-port");
    assert_eq!(error["details"]["repo"], "acme/cpd");
    assert_eq!(error["details"]["holder"], serde_json::from_str::<Value>(COMMAND_RECORD).unwrap());
    assert_eq!(error["details"]["waited_minutes"], 0);
    assert!(fx.fakes.spawn.execs().is_empty());
}

#[test]
fn resource_run_wait_takes_the_lock_when_the_holder_ends_within_the_minutes() {
    let (mut fx, repo) = world("[resources.e2e-port]\n");
    let mut holder = Some(hold(&fx, "e2e-port", COMMAND_RECORD));
    // Another process ends at the run's first wait.
    fx.on_nap(move |_| drop(holder.take()));
    let seen = watch_exec(&fx, "e2e-port");

    let _ = fx.cpd_in(&repo, &run_args("e2e-port", &["--wait", "2", "--", "npm", "test"]));

    assert_eq!(fx.fakes.spawn.execs().len(), 1, "the command is exec-ed once the holder ended");
    let seen = seen.lock().unwrap();
    let (held, content) = &seen[0];
    assert!(held);
    assert_eq!(serde_json::from_str::<Value>(content).unwrap()["pid"], std::process::id());
    assert!(fx.fakes.clock.elapsed() < Duration::from_secs(120), "it took the lock before the minutes ran out");
}

#[test]
fn resource_run_wait_that_runs_out_is_busy_with_waited_minutes() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    let _holder = hold(&fx, "e2e-port", COMMAND_RECORD);

    let run = fx.cpd_in(&repo, &run_args("e2e-port", &["--wait", "2", "--", "npm", "test"]));

    let error = refused(&run, "resource_busy", 3);
    assert_eq!(error["details"]["waited_minutes"], 2);
    assert_eq!(error["details"]["holder"], serde_json::from_str::<Value>(COMMAND_RECORD).unwrap());
    assert!(fx.fakes.clock.elapsed() >= Duration::from_secs(120), "the two minutes passed on the Ports' clock");
    assert!(fx.fakes.spawn.execs().is_empty());
}

#[test]
fn resource_run_execs_at_once_when_its_own_attempt_holds_the_resource() {
    let (fx, repo) = world("[resources.e2e-port]\n");
    let _holder = hold(&fx, "e2e-port", ATTEMPT_RECORD);

    let _ = fx.cpd_with(&repo, &run_args("e2e-port", &["--", "npm", "test"]), &[("CPD_ATTEMPT", "api-a1")]);

    assert_eq!(fx.fakes.spawn.execs().len(), 1, "the Attempt does not wait on itself");
    assert_eq!(fs::read_to_string(lock_path(&fx, "e2e-port")).unwrap(), ATTEMPT_RECORD, "it takes and writes nothing");
    assert!(fx.fakes.clock.naps().is_empty());
}

#[test]
fn resource_run_an_empty_command_is_a_usage_error() {
    let (fx, repo) = world("[resources.e2e-port]\n");

    for args in [run_args("e2e-port", &["--"]), run_args("e2e-port", &[]), run_args("e2e-port", &["--wait", "0", "--", "npm"])] {
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
    assert!(seen.lock().unwrap()[0].0, "the exec was tried with the lock held");
    assert!(!is_held(&lock_path(&fx, "e2e-port")), "the lock goes with the process");
}

#[test]
fn resource_run_a_directory_without_a_keyable_origin_is_repo_unkeyed() {
    let (fx, _repo) = world("[resources.e2e-port]\n");
    let other = fx.git_repo("elsewhere");
    fx.git(&other, &["remote", "add", "origin", "https://gitlab.com/acme/elsewhere.git"]);

    let run = fx.cpd_in(&other, &run_args("e2e-port", &["--", "npm", "test"]));

    refused(&run, "repo_unkeyed", 1);
    assert!(fx.fakes.spawn.execs().is_empty());
}

#[test]
fn resource_list_shows_declared_resources_with_their_holders() {
    let (fx, repo) = world("[resources.e2e-port]\n\n[resources.db]\n");
    let _holder = hold(&fx, "e2e-port", COMMAND_RECORD);
    // A record left in the file of a free lock is no holder.
    drop(hold(&fx, "db", COMMAND_RECORD));

    let out = fx.cpd_in(&repo, &["resource", "list"]).ok();

    assert_eq!(out["repo"], "acme/cpd");
    assert_eq!(
        out["resources"],
        json!([
            {"name": "db", "held": false, "holder": null},
            {"name": "e2e-port", "held": true, "holder": serde_json::from_str::<Value>(COMMAND_RECORD).unwrap()},
        ])
    );
}

#[test]
fn resource_list_with_none_declared_is_empty() {
    let (fx, repo) = world("");

    let out = fx.cpd_in(&repo, &["resource", "list"]).ok();

    assert_eq!(out["repo"], "acme/cpd");
    assert_eq!(out["resources"], json!([]));
}
