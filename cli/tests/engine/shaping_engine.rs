//! The engine's dispatch, settling and archiving of a Lead-less `shaping` Plan (contract §2.2 "Explore Jobs", §3.1
//! "Shaping", §3.2 "Ready", §3.7 E2/E3/E13/E14; air-d19, air-d22; docs/plan-lifecycle.md "Pre-Lead shaping and Manager
//! explore", seams 1–3). A **shaping Plan** is Lead-less (`lead_session` NULL, phase `shaping`): its explore Jobs are
//! dispatched by E3 like a Lead Plan's, the engine settles it to `done` when every Job is final, E14 archives it
//! directly after the archive delay, and E2 starts no Lead for it.
//!
//! The engine runs in-process (`Fx::watch_ticks`, on the `ManualClock`) with the fake tmux and the fake `pi` of the
//! routes: no process, no tmux, no wait. The Plan's Job ends go through the same ledger call the driver and the engine
//! use (`attempt::ledger::end_explore`, contract §7.7), so the settle runs in the Job's own transaction.
//!
//! The engine does not handle a `shaping` Plan yet, so every test here is red until the feature lands.

use crate::common;

use common::fx::Fx;
use cpd_core::attempt::ledger;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::plan::{jobs, locked_fingerprint};
use serde_json::json;

/// The Lead-less Plan of the tests, and its Context.
const PLAN: &str = "shape";
const CONTEXT: &str = "ctx";

/// A home for the engine on virtual time, with Context `ctx` (owner `acme`) and the pi install and Tier catalog E3
/// needs. No tmux server: a step that reaches tmux fails as it does in the other engine tests.
fn home() -> Fx {
    home_with("")
}

/// [`home`] with `extra` in `config.toml` (top-level keys or tables, before `[watch]`).
fn home_with(extra: &str) -> Fx {
    let fx = Fx::with_ports();
    fx.engine_config(extra);
    fx.engine_routes();
    // The driver's executable must exist for E3's spawn to record a launch (`FakeSpawn` runs nothing), and its name is
    // what E5 (`cpd pr poll`) is routed by, so E5 finds no pull request.
    let exe = fx.env.root().join("cpd-installed");
    std::fs::write(&exe, "").unwrap();
    fx.fakes.spawn.set_self_exe(&exe);
    let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
    fx.fake("cpd-installed", Reply::ok(&poll.to_string()));
    fx.env.owner_policy("acme", "");
    let install = fx.env.root().join("install");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("current-version"), "1.0.0\n").unwrap();
    let scope = install.join("releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        std::fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        std::fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    fx.env.sql(&format!(
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
           ('1.0.0', 'O', 'p/opus', '[0,1,2,3,4,5]', 't'), ('1.0.0', 'S', 'p/sonnet', '[1,2,3,4,5]', 't'),
           ('1.0.0', 'H', 'p/haiku', '[0,1,2,3]', 't'), ('1.0.0', 'G', 'p/gemini', '[]', 't');
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('{CONTEXT}', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr', 'manager', '{CONTEXT}', 'Mgr', '/', 'O3', 'stopped', 't', 't');"
    ));
    fx
}

/// A Lead-less shaping Plan `plan` (`lead_session` NULL, phase `shaping`).
fn shaping_plan(fx: &Fx, plan: &str) {
    fx.env.sql(&format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('{plan}', '{CONTEXT}', 'Shaping', 'shaping', NULL, 't', NULL);"
    ));
}

/// An explore Job `key` (`kind` `scout`/`research`, Tier `tier`) of `plan`, `queued` with its Locked-Job fingerprint,
/// then moved by `then` (SQL run after the fingerprint is set).
fn add_explore(fx: &Fx, plan: &str, key: &str, kind: &str, tier: &str, then: &str) {
    fx.env.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('{plan}', NULL, '{key}', '{kind}', 'Find {key}.', '{tier}', 'Find {key}.', 'Because.', '[\"{key}.md\"]', '[\"answered\"]', '[]', 'draft', 0, cpd_txn(), 't');"
    ));
    let id = PlanId::parse(plan).unwrap();
    let all = fx.env.store().read(|conn| jobs::load(conn, &id)).unwrap();
    let job = all.iter().find(|j| j.key == key).unwrap();
    fx.env.sql(&format!(
        "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{plan}' AND key = '{key}'; {then}",
        locked_fingerprint(job)
    ));
}

/// SQL that moves explore Job `key` of `plan` to `running` and gives it a `running` Attempt `<key>-a1`.
fn running(plan: &str, key: &str, tier: &str) -> String {
    format!(
        "UPDATE jobs SET status = 'running' WHERE plan = '{plan}' AND key = '{key}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
           VALUES ('{plan}', '{key}-a1', '{key}', 1, '{tier}', 'ladder', 'running', 'cpd/{plan}--{key}-a1', '/explore', 't', 4242);"
    )
}

/// Ends the `running` Attempt `<key>-a1` of `plan` as merged through the ledger, as `attempt submit` and the driver do
/// (contract §7.7 "Explore kinds at submit"): the Job merges and the Plan's done guard is evaluated in the transaction.
fn merge_explore(fx: &Fx, plan: &str, key: &str) {
    let mut store = fx.env.store();
    let id = PlanId::parse(plan).unwrap();
    let attempt = store
        .read(|conn| ledger::load(conn, &id, &AttemptId::parse(&format!("{key}-a1")).unwrap()))
        .unwrap();
    store
        .write("shaping_engine", |tx| {
            ledger::end_explore(tx, &attempt, &"a".repeat(64), "Found it.")
        })
        .unwrap();
}

fn phase(fx: &Fx, plan: &str) -> String {
    fx.env
        .query::<String>(&format!("SELECT phase FROM plans WHERE id = '{plan}'"))
}

fn count(fx: &Fx, what: &str) -> i64 {
    fx.env.query::<i64>(&format!("SELECT count(*) FROM {what}"))
}

fn done_events(fx: &Fx, plan: &str) -> i64 {
    fx.env.query::<i64>(&format!(
        "SELECT count(*) FROM events WHERE kind = 'plan-done' AND plan = '{plan}'"
    ))
}

#[test]
fn shaping_engine_e3_starts_explore_job_of_shaping_plan() {
    let fx = home();
    shaping_plan(&fx, PLAN);
    add_explore(&fx, PLAN, "look", "scout", "H1", "");

    fx.watch_first_tick().unwrap();

    assert_eq!(
        count(&fx, "attempts"),
        1,
        "E3 starts a queued scout of a Lead-less shaping Plan like any explore Job"
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE plan = 'shape' AND key = 'look'"),
        "running"
    );
    let expected = fx.env.cpd_home().join("run").join(PLAN).join("explore");
    let worktree = fx
        .env
        .query::<String>("SELECT worktree FROM attempts WHERE id = 'look-a1'");
    assert_eq!(
        worktree,
        expected.display().to_string(),
        "the explore Worker's directory is run/<plan>/explore"
    );
    // The driver E3 spawned names the Plan, so the Worker it starts has CPD_PLAN set (contract §3.4).
    let launches = fx.fakes.spawn.launches_but_janitor();
    assert!(
        launches
            .iter()
            .any(|l| l.args == ["attempt", "run", "look-a1", "--plan", "shape"]),
        "E3 spawned the driver of look-a1 for Plan shape: {launches:?}"
    );
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM job_tenants WHERE plan = 'shape' AND job = 'look' AND pool = 'workers'"
        ),
        1,
        "the explore Job takes a workers slot like any Job"
    );
}

#[test]
fn shaping_engine_settles_shaping_plan_done_when_all_jobs_final() {
    let fx = home();
    shaping_plan(&fx, PLAN);
    add_explore(
        &fx,
        PLAN,
        "look",
        "scout",
        "H1",
        &running(PLAN, "look", "H1"),
    );

    merge_explore(&fx, PLAN, "look");

    assert_eq!(
        phase(&fx, PLAN),
        "done",
        "every Job is final: the Lead-less Plan settles to done"
    );
    assert_eq!(done_events(&fx, PLAN), 1, "one plan-done Event");
    assert!(
        fx.env
            .query::<Option<String>>("SELECT done_at FROM plans WHERE id = 'shape'")
            .is_some(),
        "done_at is set"
    );
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT locked_at FROM plans WHERE id = 'shape'"),
        None,
        "a Lead-less Plan was never locked"
    );
}

#[test]
fn shaping_engine_keeps_shaping_with_queued_running_or_failed_job() {
    for status in ["queued", "running", "failed"] {
        let fx = home();
        shaping_plan(&fx, PLAN);
        let then = match status {
            "running" => running(PLAN, "look", "H1"),
            "failed" => format!(
                "{} UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = 'shape' AND key = 'look';",
                running(PLAN, "look", "H1")
            ),
            _ => String::new(),
        };
        add_explore(&fx, PLAN, "look", "scout", "H1", &then);
        // The control: a sibling shaping Plan whose only Job is final settles, so "still shaping" above is the Job's
        // doing and not the Plan's.
        shaping_plan(&fx, "ctrl");
        add_explore(
            &fx,
            "ctrl",
            "look",
            "scout",
            "H1",
            &running("ctrl", "look", "H1"),
        );
        merge_explore(&fx, "ctrl", "look");

        fx.watch_ticks(1).unwrap();

        assert_eq!(phase(&fx, PLAN), "shaping", "a {status} Job holds the Plan");
        assert_eq!(
            done_events(&fx, PLAN),
            0,
            "a {status} Job writes no plan-done"
        );
        assert_eq!(
            phase(&fx, "ctrl"),
            "done",
            "control: the all-final Plan settled"
        );
    }
}

#[test]
fn shaping_engine_empty_shaping_plan_is_not_settled() {
    let fx = home();
    shaping_plan(&fx, PLAN);

    fx.watch_ticks(2).unwrap();

    assert_eq!(
        phase(&fx, PLAN),
        "shaping",
        "a shaping Plan with no Job stays shaping"
    );
    assert_eq!(done_events(&fx, PLAN), 0);

    // The control: a shaping Plan with an all-final Job settles, so the empty Plan is held by having no Job, not by the
    // settle being absent.
    shaping_plan(&fx, "full");
    add_explore(
        &fx,
        "full",
        "look",
        "scout",
        "H1",
        &running("full", "look", "H1"),
    );
    merge_explore(&fx, "full", "look");
    assert_eq!(phase(&fx, "full"), "done", "control: a Job settles it");
}

#[test]
fn shaping_engine_settle_is_idempotent() {
    let fx = home();
    shaping_plan(&fx, PLAN);
    add_explore(
        &fx,
        PLAN,
        "look",
        "scout",
        "H1",
        &running(PLAN, "look", "H1"),
    );

    merge_explore(&fx, PLAN, "look");
    assert_eq!(phase(&fx, PLAN), "done");
    assert_eq!(done_events(&fx, PLAN), 1);

    // A later tick writes nothing more for the settled Plan.
    fx.watch_ticks(3).unwrap();
    assert_eq!(phase(&fx, PLAN), "done");
    assert_eq!(done_events(&fx, PLAN), 1, "one plan-done Event, not two");
    assert_eq!(count(&fx, "events WHERE kind = 'error'"), 0);
}

#[test]
fn shaping_engine_archives_leadless_done_plan() {
    let fx = home_with("\n[timeouts]\narchive_sleep_seconds = 1\n");
    // `shape` finished past the archive delay; `fresh` finished just now.
    fx.env.sql(&format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('{PLAN}', '{CONTEXT}', 'Shaping', 'done', NULL, 't', NULL, '{}'),
                  ('fresh', '{CONTEXT}', 'Fresh', 'done', NULL, 't', NULL, '{}');",
        fx.stamp_ago(2_000),
        fx.stamp_ago(0)
    ));

    fx.watch_ticks(1).unwrap();

    assert!(
        fx.env
            .query::<Option<String>>("SELECT archived_at FROM plans WHERE id = 'shape'")
            .is_some(),
        "E14 archives a Lead-less done Plan directly after the delay from done_at"
    );
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM events WHERE kind = 'plan-archived' AND plan = 'shape'"
        ),
        1,
        "one plan-archived Event"
    );
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT archived_at FROM plans WHERE id = 'fresh'"),
        None,
        "not before the delay from done_at"
    );
}

#[test]
fn shaping_engine_never_starts_a_lead_for_shaping_plan() {
    let fx = home();
    shaping_plan(&fx, PLAN);
    add_explore(&fx, PLAN, "look", "scout", "H1", "");

    fx.watch_first_tick().unwrap();

    // The Plan is live: its explore Job was dispatched (E3), which is the fact this test guards.
    assert_eq!(
        count(&fx, "attempts"),
        1,
        "E3 dispatched the Plan's explore Job"
    );
    // E2 starts no Lead for it: no Lead Session exists, and the Plan stays Lead-less.
    assert_eq!(
        count(&fx, "sessions WHERE role = 'lead'"),
        0,
        "no Lead Session is started"
    );
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT lead_session FROM plans WHERE id = 'shape'"),
        None,
        "the Plan stays Lead-less"
    );
    assert_eq!(count(&fx, "events WHERE kind = 'error'"), 0);
}

#[test]
fn shaping_engine_inbox_and_readers_skip_leadless_plan() {
    let fx = home();
    shaping_plan(&fx, PLAN);
    add_explore(&fx, PLAN, "look", "scout", "H1", "");

    fx.watch_first_tick().unwrap();

    // The Plan is live: E3 dispatched its explore Job, so E8's reconcile runs over an active Lead-less Plan.
    assert_eq!(
        count(&fx, "attempts"),
        1,
        "E3 dispatched the Plan's explore Job"
    );
    assert_eq!(
        count(&fx, "events WHERE kind = 'error'"),
        0,
        "the engine reports nothing about a Lead-less Plan"
    );
    // E8 opens no Inbox item that needs a Lead for it.
    assert_eq!(count(&fx, "inbox_items"), 0);

    // Every reader renders or skips the Lead-less Plan without error.
    let list = fx.cpd(&["plan", "list"]).ok();
    assert_eq!(list["plans"][0]["phase"], json!("shaping"));
    assert_eq!(list["plans"][0]["lead_session"], json!(null));
    fx.cpd(&["plan", "show", "--plan", PLAN]).ok();
    fx.cpd(&["context", "pending", "--context", CONTEXT]).ok();
    // `--markdown` prints on the process's own stdout (`print!`): the binary, which `Fx::cpd` cannot read.
    let run = fx
        .env
        .cpd(&["context", "pending", "--context", CONTEXT, "--markdown"]);
    assert_eq!(run.exit, 0, "{run:?}");
}
