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
    engine_with(policy, 1)
}

/// [`engine`] with `concurrency` Worker slots.
fn engine_with(policy: &str, concurrency: u32) -> Fx {
    let fx = Fx::with_ports();
    fx.engine_config(&format!("concurrency = {concurrency}\nretry_cap = 0\n"));
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
    let path = dir.join(format!("resource.repo.acme.pay.{name}.1.lock"));
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    assert!(lock::try_lock(&file).unwrap(), "the test could not take {name}");
    fs::write(&path, record).unwrap();
    file
}

/// Holds slot `slot` of the repo pool `name` of `acme/pay`, as `hold` does slot 1.
fn hold_slot(fx: &Fx, name: &str, slot: u8, record: &str) -> File {
    let path = fx.env.cpd_home().join(format!("locks/resource.repo.acme.pay.{name}.{slot}.lock"));
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    assert!(lock::try_lock(&file).unwrap(), "the test could not take slot {slot} of {name}");
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
    // Three Worker slots: the blocked `api` holds one back (contract §3.7 E3 reservation), the others are free.
    let fx = engine_with(POLICY, 3);
    // `api` is older and held; `web` lists a free resource and `ui` lists none.
    add(&fx, "api", &["e2e-port"], "");
    add(&fx, "web", &["db"], "");
    add(&fx, "ui", &[], "");
    let _other = hold(&fx, "e2e-port", COMMAND_RECORD);

    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["web", "ui"]);
    assert_eq!((status(&fx, "api"), status(&fx, "web"), status(&fx, "ui")), ("queued".into(), "running".into(), "running".into()));
    assert_eq!(attempts(&fx), 2);
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

const ATTEMPT_RECORD: &str = r#"{"by":"attempt","plan":"other","job":"db","attempt":"db-a1","at":"2026-10-26T21:41:00.000Z"}"#;

#[test]
fn job_stays_queued_while_every_slot_is_held_and_job_show_lists_the_holders() {
    let fx = engine("[resources.e2e-port]\nslots = 2\n");
    add(&fx, "api", &["e2e-port"], "");
    let _first = hold_slot(&fx, "e2e-port", 1, COMMAND_RECORD);
    let _second = hold_slot(&fx, "e2e-port", 2, ATTEMPT_RECORD);

    fx.watch_ticks(3).unwrap();

    assert_eq!((status(&fx, "api"), attempts(&fx), all_errors(&fx)), ("queued".into(), 0, 0));
    assert_eq!(fx.fakes.spawn.launches_but_janitor(), []);
    let shown = fx.cpd(&["job", "show", "api", "--plan", PLAN]).ok();
    assert_eq!(
        shown["job"]["waiting_for"],
        json!([{
            "resource": "e2e-port", "scope": "repo", "slots": 2,
            "holders": [serde_json::from_str::<Value>(COMMAND_RECORD).unwrap(), serde_json::from_str::<Value>(ATTEMPT_RECORD).unwrap()],
        }])
    );
}

#[test]
fn job_starts_when_one_of_two_slots_is_free() {
    let fx = engine("[resources.e2e-port]\nslots = 2\n");
    add(&fx, "api", &["e2e-port"], "");
    let _first = hold_slot(&fx, "e2e-port", 1, COMMAND_RECORD);

    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["api"], "the Job takes the free slot");
    assert_eq!(status(&fx, "api"), "running");
    let record: Value = serde_json::from_str(&fs::read_to_string(fx.env.cpd_home().join("locks/resource.repo.acme.pay.e2e-port.2.lock")).unwrap()).unwrap();
    assert_eq!((record["by"].as_str(), record["job"].as_str()), (Some("attempt"), Some("api")));
    assert_eq!(fs::read_to_string(fx.env.cpd_home().join("locks/resource.repo.acme.pay.e2e-port.1.lock")).unwrap(), COMMAND_RECORD);
}

#[test]
fn watch_blocked_job_keeps_the_freed_slot_from_a_lower_job() {
    // Three Worker slots. `big` (older) needs `e2e-port` and `db`; `small` needs `e2e-port` alone. Both resources are held.
    let fx = engine_with(POLICY, 3);
    add(&fx, "big", &["e2e-port", "db"], "");
    add(&fx, "small", &["e2e-port"], "");
    let port = hold(&fx, "e2e-port", COMMAND_RECORD);
    let db = hold(&fx, "db", COMMAND_RECORD);
    fx.watch_ticks(2).unwrap();
    assert_eq!(started(&fx), Vec::<String>::new());

    // `e2e-port` is freed. `big` still lacks `db`; the slot is earmarked for it (held back), so `small` does not take it.
    drop(port);
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), Vec::<String>::new(), "the freed slot is kept for the Job ahead");
    assert_eq!((status(&fx, "big"), status(&fx, "small")), ("queued".into(), "queued".into()));
    assert_eq!((attempts(&fx), all_errors(&fx)), (0, 0));

    // `db` is freed: `big` takes both resources and starts; `small` waits for `e2e-port` in its turn.
    drop(db);
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), ["big"]);
    assert_eq!((status(&fx, "big"), status(&fx, "small")), ("running".into(), "queued".into()));
}

#[test]
fn watch_job_waits_queued_with_no_attempt_and_no_back_off_while_blocked() {
    // One Worker slot, taken by a running Attempt: `api` is Ready but blocked on `workers`.
    let fx = engine("");
    let busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &fx.stamp_ago(60_000));
    add(&fx, "api", &[], "");

    fx.watch_ticks(3).unwrap();

    assert_eq!(status(&fx, "api"), "queued");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"), 0, "a blocked Job has no Attempt");
    assert_eq!(all_errors(&fx), 0);
    let shown = fx.cpd(&["job", "show", "api", "--plan", PLAN]).ok();
    let waiting = shown["job"]["waiting_for"].as_array().unwrap_or_else(|| panic!("`api` waits for workers: {shown}"));
    assert_eq!(waiting.iter().map(|w| w["resource"].as_str().unwrap()).collect::<Vec<_>>(), ["workers"]);

    // A long wait, longer than the longest back-off (300 s), and then the slot ends: the next tick starts it, no back-off delays it.
    fx.advance(std::time::Duration::from_secs(600));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.watch_first_tick().unwrap();
    assert_eq!(attempts(&fx), 1, "still only the running Attempt: the wait counted nothing");
    drop(busy);
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), ["api"]);
    assert_eq!(status(&fx, "api"), "running");
}
