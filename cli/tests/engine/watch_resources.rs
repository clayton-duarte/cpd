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
    fx.env
        .sql("UPDATE workflows SET name = 'pay' WHERE plan = 'pay';");
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
    let list = if resources.is_empty() {
        "NULL".to_owned()
    } else {
        format!("'{}'", json!(resources))
    };
    fx.env.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at, resources)
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
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    assert!(
        lock::try_lock(&file).unwrap(),
        "the test could not take {name}"
    );
    fs::write(&path, record).unwrap();
    file
}

/// Holds slot `slot` of the repo pool `name` of `acme/pay`, as `hold` does slot 1.
fn hold_slot(fx: &Fx, name: &str, slot: u8, record: &str) -> File {
    let path = fx
        .env
        .cpd_home()
        .join(format!("locks/resource.repo.acme.pay.{name}.{slot}.lock"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    assert!(
        lock::try_lock(&file).unwrap(),
        "the test could not take slot {slot} of {name}"
    );
    fs::write(&path, record).unwrap();
    file
}

fn status(fx: &Fx, key: &str) -> String {
    fx.env.query(&format!(
        "SELECT status FROM jobs WHERE plan = '{PLAN}' AND key = '{key}'"
    ))
}

fn attempts(fx: &Fx) -> i64 {
    fx.env.query("SELECT count(*) FROM attempts")
}

fn started(fx: &Fx) -> Vec<String> {
    fx.env.read(|conn| {
        let mut stmt =
            conn.prepare("SELECT job FROM events WHERE kind = 'attempt-started' ORDER BY id")?;
        stmt.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()
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
    fx.env
        .query("SELECT count(*) FROM events WHERE kind = 'error'")
}

#[test]
fn resource_hold_e3_keeps_a_job_queued_while_its_resource_is_held() {
    let fx = engine(POLICY);
    add(&fx, "api", &["e2e-port"], "");
    let _other = hold(&fx, "e2e-port", &common::command_record());

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
    // Three Worker slots: the skipped `api` reserves nothing (contract §3.7 E3: skipped, nothing reserved), the others are free.
    let fx = engine_with(POLICY, 3);
    // `api` is older and held; `web` lists a free resource and `ui` lists none.
    add(&fx, "api", &["e2e-port"], "");
    add(&fx, "web", &["db"], "");
    add(&fx, "ui", &[], "");
    let _other = hold(&fx, "e2e-port", &common::command_record());

    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["web", "ui"]);
    assert_eq!(
        (status(&fx, "api"), status(&fx, "web"), status(&fx, "ui")),
        ("queued".into(), "running".into(), "running".into())
    );
    assert_eq!(attempts(&fx), 2);
}

#[test]
fn resource_hold_e3_starts_the_job_when_the_holder_ends() {
    let fx = engine(POLICY);
    add(&fx, "api", &["e2e-port"], "");
    let other = hold(&fx, "e2e-port", &common::command_record());
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
    let other = hold(&fx, "e2e-port", &common::command_record());

    // A long hold: many ticks of virtual time, longer than the longest back-off (300 s).
    fx.watch_ticks(3).unwrap();
    fx.advance(std::time::Duration::from_secs(600));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.watch_first_tick().unwrap();
    assert_eq!(
        attempts(&fx),
        0,
        "a held resource creates no Attempt, so nothing to count"
    );

    // The tick after the holder ends starts it: no back-off delays it.
    drop(other);
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), ["api"]);
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM attempts WHERE status = 'ended'"),
        0
    );
}

#[test]
fn resource_hold_an_urgent_job_waits_too() {
    let fx = engine(POLICY);
    // The one slot is taken by a running Attempt; an urgent Job could take the one beyond it, but its resource is held.
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &fx.stamp_ago(60_000));
    add(
        &fx,
        "hot",
        &["e2e-port"],
        &format!("UPDATE jobs SET urgent = 1 WHERE plan = '{PLAN}' AND key = 'hot';"),
    );
    let other = hold(&fx, "e2e-port", &common::command_record());

    fx.watch_ticks(2).unwrap();

    assert_eq!(status(&fx, "hot"), "queued");
    assert_eq!(started(&fx), Vec::<String>::new());
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM attempts WHERE job = 'hot'"),
        0
    );

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
    fx.env
        .repo_policy("acme", "pay", "[resources.e2e-port]\n[resources.gone]\n");
    fx.watch_first_tick().unwrap();
    assert_eq!(started(&fx), ["api"]);
    assert_eq!(errors(&fx, "resource_undeclared").len(), 1);
}

const ATTEMPT_RECORD: &str = r#"{"by":"attempt","plan":"other","job":"db","attempt":"db-a1","at":"2026-10-26T21:41:00.000Z"}"#;

#[test]
fn job_stays_queued_while_every_slot_is_held_and_job_show_lists_the_holders() {
    let fx = engine("[resources.e2e-port]\nslots = 2\n");
    add(&fx, "api", &["e2e-port"], "");
    let _first = hold_slot(&fx, "e2e-port", 1, &common::command_record());
    let _second = hold_slot(&fx, "e2e-port", 2, ATTEMPT_RECORD);

    fx.watch_ticks(3).unwrap();

    assert_eq!(
        (status(&fx, "api"), attempts(&fx), all_errors(&fx)),
        ("queued".into(), 0, 0)
    );
    assert_eq!(fx.fakes.spawn.launches_but_janitor(), []);
    let shown = fx.cpd(&["job", "show", "api", "--plan", PLAN]).ok();
    let held = |record: &str, slot: u8| json!({"slot": slot, "holder": serde_json::from_str::<Value>(record).unwrap()});
    assert_eq!(
        shown["job"]["waiting_for"],
        json!([{
            "resource": "e2e-port", "scope": "repo", "slots": 2,
            "holders": [held(&common::command_record(), 1), held(ATTEMPT_RECORD, 2)],
        }]),
        "the held command slots, each with its slot number, in the shape of `resource list`"
    );
}

#[test]
fn job_starts_when_one_of_two_slots_is_free() {
    let fx = engine("[resources.e2e-port]\nslots = 2\n");
    add(&fx, "api", &["e2e-port"], "");
    let _first = hold_slot(&fx, "e2e-port", 1, &common::command_record());

    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["api"], "the Job takes the free slot");
    assert_eq!(status(&fx, "api"), "running");
    // The Job is the tenant of the pool in the Ledger: its row commits with its Attempt, and it takes no lock file.
    assert_eq!(
        fx.env.query::<i64>(&format!(
            "SELECT count(*) FROM job_tenants WHERE job = 'api' AND pool = '{PORT_POOL}'"
        )),
        1
    );
    assert!(
        !fx.env
            .cpd_home()
            .join("locks/resource.repo.acme.pay.e2e-port.2.lock")
            .exists(),
        "a Job writes no slot file"
    );
    assert_eq!(
        fs::read_to_string(
            fx.env
                .cpd_home()
                .join("locks/resource.repo.acme.pay.e2e-port.1.lock")
        )
        .unwrap(),
        common::command_record()
    );
}

/// The `skipped_n` of Job `key` (NULL: not skipped).
fn skipped_n(fx: &Fx, key: &str) -> Option<i64> {
    fx.env.query(&format!(
        "SELECT skipped_n FROM jobs WHERE plan = '{PLAN}' AND key = '{key}'"
    ))
}

/// Job `key` as a tenant: `running` with its first Attempt, its `job_pools` rows `pools` (the starter's), and its driver gone:
/// its Attempt `ended` as `failed` when `failed`, and the Job `failed` with it. Lists `resources` in its Spec.
fn tenant(fx: &Fx, key: &str, resources: &[&str], pools: &[&str], failed: bool) {
    let mut then = format!(
        "UPDATE jobs SET status = 'running' WHERE plan = '{PLAN}' AND key = '{key}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('{PLAN}', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'running', 'cpd/{PLAN}--{key}-a1', '/w', 't');"
    );
    // The Attempt turned `running`: E2 reads this Event for a live Attempt, as `Fx::add_running` writes it.
    then += &format!(
        "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
           VALUES (cpd_txn(), '{}', 'attempt-status', 'ctx', '{PLAN}', '{key}', '{key}-a1', 'running', '{{\"from\":\"starting\",\"to\":\"running\"}}');",
        fx.stamp_ago(60_000)
    );
    for pool in pools {
        then += &format!(
            "INSERT INTO job_pools (plan, job, pool) VALUES ('{PLAN}', '{key}', '{pool}');"
        );
    }
    if failed {
        then += &format!(
            "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = '{key}-a1';
             UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = '{PLAN}' AND key = '{key}';"
        );
    }
    add(fx, key, resources, &then);
}

const PORT_POOL: &str = "repo:acme/pay:e2e-port";

#[test]
fn watch_skipped_job_gets_its_skip_order_once_and_keeps_it_across_ticks() {
    // One Worker slot, held by a Job that holds `e2e-port`: `api` and then `web` are Ready, miss it, and are skipped.
    let fx = engine(POLICY);
    tenant(&fx, "busy", &["e2e-port"], &[PORT_POOL], false);
    let _driver = fx.live_driver("busy-a1");
    add(&fx, "api", &["e2e-port"], "");
    add(&fx, "web", &[], "");
    assert_eq!(
        (skipped_n(&fx, "api"), skipped_n(&fx, "web")),
        (None, None),
        "no Job is skipped before E3 walks"
    );

    fx.watch_first_tick().unwrap();

    // Recorded once, in the order of the walk (`jobs.seq`): a number above every other skip.
    assert_eq!(
        (skipped_n(&fx, "api"), skipped_n(&fx, "web")),
        (Some(1), Some(2))
    );
    let events = |key: &str| {
        fx.env
            .query::<i64>(&format!("SELECT count(*) FROM events WHERE job = '{key}'"))
    };
    let seen = (events("api"), events("web"));

    // Later ticks record nothing again and change nothing: the Jobs keep their place, and a skip appends no Event.
    fx.watch_ticks(3).unwrap();
    assert_eq!(
        (skipped_n(&fx, "api"), skipped_n(&fx, "web")),
        (Some(1), Some(2))
    );
    assert_eq!((events("api"), events("web")), seen);
    assert_eq!(
        (status(&fx, "api"), status(&fx, "web"), all_errors(&fx)),
        ("queued".into(), "queued".into(), 0)
    );
}

#[test]
fn watch_skip_order_is_cleared_when_the_job_starts() {
    let fx = engine(POLICY);
    add(&fx, "api", &["e2e-port"], "");
    let holder = hold(&fx, "e2e-port", &common::command_record());
    fx.watch_ticks(2).unwrap();
    assert_eq!(
        skipped_n(&fx, "api"),
        Some(1),
        "skipped for want of the pool"
    );

    drop(holder);
    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["api"]);
    assert_eq!(skipped_n(&fx, "api"), None, "cleared when it starts");
}

#[test]
fn watch_failed_job_keeps_its_resources_and_its_worker_for_the_retry() {
    // Two Worker slots. `api` failed: its last Attempt ended, so it holds no Worker slot, though it stays the tenant of
    // `e2e-port` (contract §3.4 "Tenancy", air-n90).
    let fx = engine_with(POLICY, 2);
    tenant(&fx, "api", &["e2e-port"], &[PORT_POOL], true);
    // Its driver had started before the Attempt ended: no spawn-failure back-off holds the retry below.
    fx.env
        .sql("UPDATE attempts SET driver_pid = 4242 WHERE id = 'api-a1';");
    add(&fx, "other", &["e2e-port"], "");
    add(&fx, "web", &[], "");
    add(&fx, "late", &[], "");

    fx.watch_ticks(2).unwrap();

    // `other` misses the port the failed Job keeps; the failed Job holds no Worker, so `web` and `late` both take one.
    assert_eq!(started(&fx), ["web", "late"]);
    assert_eq!(
        (
            status(&fx, "other"),
            status(&fx, "web"),
            status(&fx, "late")
        ),
        ("queued".into(), "running".into(), "running".into())
    );
    assert_eq!(
        skipped_n(&fx, "other"),
        Some(1),
        "only `other` waits: for the port, not the Worker"
    );
    assert_eq!(
        skipped_n(&fx, "late"),
        None,
        "`late` is not skipped: the failed Job's Worker slot is free"
    );
    assert_eq!(status(&fx, "api"), "failed");
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM job_pools"),
        1,
        "the failed Job's rows stay, and nobody else writes one"
    );
    assert_eq!(all_errors(&fx), 0);

    // A retry re-acquires a Worker slot: its new Attempt counts as a live Attempt of `workers` from the moment it starts.
    fx.env.sql(
        "UPDATE jobs SET status = 'queued', retry_tier = 'S2', failed_cause = NULL WHERE plan = 'pay' AND key = 'api';",
    );
    fx.watch_first_tick().unwrap();
    assert_eq!(
        started(&fx),
        ["web", "late", "api"],
        "the retry starts and holds a Worker slot again"
    );
    assert_eq!(status(&fx, "api"), "running");
    assert_eq!(all_errors(&fx), 0);
}

#[test]
fn watch_resources_are_free_in_the_tick_after_the_job_merges() {
    // Two Worker slots. `api` is a running tenant of `e2e-port`: `web`, which needs it, is skipped.
    let fx = engine_with(POLICY, 2);
    fx.add_running("api", "running", &fx.stamp_ago(60_000));
    fx.env.sql(&format!(
        "INSERT INTO job_pools (plan, job, pool) VALUES ('{PLAN}', 'api', '{PORT_POOL}');"
    ));
    let driver = fx.live_driver("api-a1");
    add(&fx, "web", &["e2e-port"], "");
    fx.watch_ticks(2).unwrap();
    assert_eq!(
        (started(&fx), status(&fx, "web"), skipped_n(&fx, "web")),
        (Vec::<String>::new(), "queued".into(), Some(1))
    );

    // `api` merges, its Attempt ends: no timer and no clock moves, and the very next tick starts `web` on the pool.
    fx.env.sql(&format!(
        "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = 'abc', ended_at = 't' WHERE id = 'api-a1';
         UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = '{PLAN}' AND key = 'api';"
    ));
    drop(driver);
    fx.watch_first_tick().unwrap();

    assert_eq!(started(&fx), ["web"]);
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM job_pools WHERE job = 'web'"),
        1,
        "`web` is the tenant now: its row commits with its Attempt"
    );
}

#[test]
fn watch_job_waits_queued_with_no_attempt_and_no_back_off_while_skipped() {
    // One Worker slot, taken by a running Attempt: `api` is Ready but skipped for want of `workers`.
    let fx = engine("");
    let busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &fx.stamp_ago(60_000));
    add(&fx, "api", &[], "");

    fx.watch_ticks(3).unwrap();

    assert_eq!(status(&fx, "api"), "queued");
    assert_eq!(skipped_n(&fx, "api"), Some(1), "skipped, once");
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"),
        0,
        "a skipped Job has no Attempt"
    );
    assert_eq!(all_errors(&fx), 0);
    let shown = fx.cpd(&["job", "show", "api", "--plan", PLAN]).ok();
    let waiting = shown["job"]["waiting_for"]
        .as_array()
        .unwrap_or_else(|| panic!("`api` waits for workers: {shown}"));
    assert_eq!(
        waiting
            .iter()
            .map(|w| w["resource"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["workers"]
    );

    // A long wait, longer than the longest back-off (300 s), and then the slot ends: the next tick starts it, no back-off delays it.
    fx.advance(std::time::Duration::from_secs(600));
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.watch_first_tick().unwrap();
    assert_eq!(
        attempts(&fx),
        1,
        "still only the running Attempt: the wait counted nothing"
    );
    drop(busy);
    fx.env.sql(&format!(
        "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = 'abc', ended_at = 't' WHERE id = 'busy-a1';
         UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = '{PLAN}' AND key = 'busy';"
    ));
    fx.watch_first_tick().unwrap();
    assert_eq!(
        started(&fx),
        ["api"],
        "`busy` merged: its worker is free in the next tick"
    );
}
