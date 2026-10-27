//! Engine E3 and the resources of a Job (contract §3.7 E3 "Resources", §3.4 "Resources of a Job"): in-process on the fakes, the
//! engine on the `ManualClock`. The test holds a resource with its own open file description of the lock file, as another process
//! (a `cpd resource run`, another Attempt's driver) would. The drivers E3 starts are launches of the `FakeSpawn`.

use crate::common;

use std::fs::{self, File, OpenOptions};

use common::fx::Fx;
use cpd_core::clock::Clock;
use cpd_core::host::runner::Reply;
use cpd_core::ids::PlanId;
use cpd_core::lock;
use cpd_core::plan::{jobs, locked_fingerprint};
use serde_json::{Value, json};

const PLAN: &str = "pay";
const COMMAND_RECORD: &str =
    r#"{"by":"command","pid":4242,"command":"npm run dev","session":"s9","attempt":null,"at":"2026-10-26T21:40:00.000Z"}"#;
const POLICY: &str = "[resources.e2e-port]\n[resources.db]\n";

/// The engine in-process for E3: Plan `pay` (PR `repo` of the repo `acme/pay`, whose policy file is `policy`), one slot
/// (`concurrency = 1`, `retry_cap = 0`). The drivers are launches of the `FakeSpawn`, whose `self_exe` is a file that exists.
fn engine(policy: &str) -> Fx {
    let fx = Fx::with_ports();
    fx.engine_config("concurrency = 1\nretry_cap = 0\n");
    fx.engine_routes();
    fx.env.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');");
    fx.engine_plan();
    fx.env.sql("UPDATE prs SET name = 'pay' WHERE plan = 'pay';");
    fx.env.repo_policy("acme", "pay", policy);
    let exe = fx.env.root().join("cpd-installed");
    fs::write(&exe, "").unwrap();
    fx.fakes.spawn.set_self_exe(&exe);
    let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
    fx.fake("cpd-installed", Reply::ok(&poll.to_string()));
    fx
}

/// Job `key` of PR `repo`, Tier `S2`, queued with its Locked-Job fingerprint, listing `resources` in its Spec (none: null; a
/// Locked Job's Spec cannot change, so they are written while it is a draft), then moved by `then`.
fn add(fx: &Fx, key: &str, resources: &[&str], then: &str) {
    let list = if resources.is_empty() { "NULL".to_owned() } else { format!("'{}'", json!(resources)) };
    fx.env.sql(&format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at, resources)
         VALUES ('{PLAN}', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't', {list});"
    ));
    let plan = PlanId::parse(PLAN).unwrap();
    let all = fx.env.store().read(|conn| jobs::load(conn, &plan)).unwrap();
    let job = all.iter().find(|j| j.key == key).unwrap();
    fx.env.sql(&format!(
        "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{PLAN}' AND key = '{key}'; {then}",
        locked_fingerprint(job)
    ));
}

/// Holds `name` as another process would: its own open file description, with `record` in the file.
fn hold(fx: &Fx, name: &str, record: &str) -> File {
    let dir = fx.env.cpd_home().join("locks");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("resource-acme--pay--{name}.lock"));
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    assert!(lock::try_lock(&file).unwrap(), "the test could not take {name}");
    fs::write(&path, record).unwrap();
    file
}

fn status(fx: &Fx, key: &str) -> String {
    fx.env.query(&format!("SELECT status FROM jobs WHERE plan = '{PLAN}' AND key = '{key}'"))
}

fn attempts(fx: &Fx) -> i64 {
    fx.env.query("SELECT count(*) FROM attempts")
}

fn started(fx: &Fx) -> Vec<String> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT job FROM events WHERE kind = 'attempt-started' ORDER BY id")?;
        Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
    })
}

/// The `error` Events of `code`: their `data`.
fn errors(fx: &Fx, code: &str) -> Vec<Value> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = ?1 ORDER BY id")?;
        let rows = stmt.query_map([code], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().map(|d| serde_json::from_str(&d).unwrap()).collect())
    })
}

fn all_errors(fx: &Fx) -> i64 {
    fx.env.query("SELECT count(*) FROM events WHERE kind = 'error'")
}

#[test]
fn resource_hold_e3_keeps_a_job_queued_while_its_resource_is_held() {
    let fx = engine(POLICY);
    add(&fx, "api", &["e2e-port"], "");
    let _other = hold(&fx, "e2e-port", COMMAND_RECORD);

    fx.watch_ticks(3).unwrap();

    // No Attempt, no driver, no Worker slot used, and E3 raises nothing: `resource_busy` is never an `error` Event.
    assert_eq!(status(&fx, "api"), "queued");
    assert_eq!(attempts(&fx), 0);
    assert_eq!(fx.fakes.spawn.launches_but_janitor(), []);
    assert_eq!(started(&fx), Vec::<String>::new());
    assert_eq!(all_errors(&fx), 0);
}

#[test]
fn resource_hold_e3_starts_the_next_ready_job_past_a_held_one() {
    let fx = engine(POLICY);
    // `api` is older and held; `web` lists a free resource and `ui` lists none; there is one slot.
    add(&fx, "api", &["e2e-port"], "");
    add(&fx, "web", &["db"], "");
    add(&fx, "ui", &[], "");
    let _other = hold(&fx, "e2e-port", COMMAND_RECORD);

    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["web"]);
    assert_eq!((status(&fx, "api"), status(&fx, "web"), status(&fx, "ui")), ("queued".into(), "running".into(), "queued".into()));
    assert_eq!(attempts(&fx), 1);
}

#[test]
fn resource_hold_e3_starts_the_job_when_the_holder_ends() {
    let fx = engine(POLICY);
    add(&fx, "api", &["e2e-port"], "");
    let other = hold(&fx, "e2e-port", COMMAND_RECORD);
    fx.watch_ticks(2).unwrap();
    assert_eq!((status(&fx, "api"), attempts(&fx)), ("queued".into(), 0));

    // The holder ends: the next tick starts the Job.
    drop(other);
    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["api"]);
    assert_eq!(status(&fx, "api"), "running");
    assert_eq!(fx.fakes.spawn.launches_but_janitor().len(), 1);
}

#[test]
fn resource_hold_a_held_resource_is_not_a_spawn_failure() {
    let fx = engine(POLICY);
    add(&fx, "api", &["e2e-port"], "");
    let other = hold(&fx, "e2e-port", COMMAND_RECORD);

    // A long hold: many ticks of virtual time, longer than the longest back-off (300 s).
    fx.watch_ticks(3).unwrap();
    fx.advance(std::time::Duration::from_secs(600));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.watch_first_tick().unwrap();
    assert_eq!(attempts(&fx), 0, "a held resource creates no Attempt, so nothing to count");

    // The tick after the holder ends starts it: no back-off delays it.
    drop(other);
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), ["api"]);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE status = 'ended'"), 0);
}

#[test]
fn resource_hold_an_urgent_job_waits_too() {
    let fx = engine(POLICY);
    // The one slot is taken by a running Attempt; an urgent Job could take the one beyond it, but its resource is held.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &fx.stamp_ago(60_000));
    add(&fx, "hot", &["e2e-port"], &format!("UPDATE jobs SET urgent = 1 WHERE plan = '{PLAN}' AND key = 'hot';"));
    let other = hold(&fx, "e2e-port", COMMAND_RECORD);

    fx.watch_ticks(2).unwrap();

    assert_eq!(status(&fx, "hot"), "queued");
    assert_eq!(started(&fx), Vec::<String>::new());
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'hot'"), 0);

    // Free: it takes the slot beyond `concurrency`.
    drop(other);
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), ["hot"]);
}

#[test]
fn resource_hold_e3_raises_resource_undeclared_once_and_leaves_the_job_queued() {
    // The policy no longer declares `gone`, which `api` lists (it did when the Job was added).
    let fx = engine("[resources.e2e-port]\n");
    add(&fx, "api", &["gone"], "");

    fx.watch_ticks(3).unwrap();

    let raised = errors(&fx, "resource_undeclared");
    assert_eq!(raised.len(), 1, "once until it clears: {raised:?}");
    assert_eq!(status(&fx, "api"), "queued");
    assert_eq!(attempts(&fx), 0);
    assert_eq!(fx.fakes.spawn.launches_but_janitor(), []);

    // It clears when the policy declares the name again: the Job starts.
    fx.env.repo_policy("acme", "pay", "[resources.e2e-port]\n[resources.gone]\n");
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), ["api"]);
    assert_eq!(errors(&fx, "resource_undeclared").len(), 1);
}
