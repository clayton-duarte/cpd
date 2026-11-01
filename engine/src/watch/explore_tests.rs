//! Red tests of E3, the timeout and the ladder for explore Jobs (`scout`, `research`; Plan explore-roles R1, R2, R6, R7), on the
//! fake ports: time moves only on the `ManualClock`, no process starts, no tmux. An explore Job has no Workflow (`jobs.pr` is NULL).

use std::time::Duration;

use cpd_core::attempt::ledger::{self, Cause, End, Ladder, Stage};
use cpd_core::clock::{Clock, timestamp};
use cpd_core::config::{Catalog, TierModel};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::ports::Fakes;

use super::testkit::Fx;
use super::{liveness, schedule};

/// An explore Job `key` of Plan `pay` (no Workflow), `queued` with its fingerprint, then moved by `then` (SQL).
fn add_explore(fx: &mut Fx, kind: &str, tier: &str, key: &str, needs: &[&str], then: &str) {
    let mut sql = format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, created_txn, created_at)
         VALUES ('pay', NULL, '{key}', 'Title of {key}', '{tier}', 'Find {key}.', 'Because.', '[\"{key}.md\"]', '[\"it is answered\"]', '[]', '{kind}', 'draft', 0, cpd_txn(), 't');"
    );
    for need in needs {
        sql += &format!(
            "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');"
        );
    }
    fx.fixture(&sql);
    fx.lock_job(key, then);
}

/// SQL that makes explore Job `key` merged through `running`, with its summary.
fn merged(key: &str) -> String {
    format!(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';
         UPDATE jobs SET status = 'merged', merged_sha = 'none', summary = 'Found it.' WHERE plan = 'pay' AND key = '{key}';"
    )
}

/// SQL for the `running` Attempt `<key>-a<number>` of an explore Job, started at `at`, its driver started.
fn running_attempt(key: &str, number: u32, tier: &str, at: &str) -> String {
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at, driver_pid)
           VALUES ('pay', '{key}-a{number}', '{key}', {number}, '{tier}', 'ladder', 'running', 'cpd/pay--{key}-a{number}', '/explore', '{at}', 4242);
         INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
           VALUES (cpd_txn(), '{at}', 'attempt-status', 'ctx', 'pay', '{key}', '{key}-a{number}', 'running', '{{\"from\":\"starting\",\"to\":\"running\"}}');"
    )
}

fn running(key: &str) -> String {
    format!("UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}';")
}

fn fx_with_fakes(phase: &str, config: &str) -> (Fx, Fakes) {
    // A Plan in `plan` is not locked, like one in `grounding`: the kit builds that one and the phase is set after.
    let mut fx = Fx::with_plan_in(if phase == "plan" { "grounding" } else { phase }, config);
    if phase == "plan" {
        fx.fixture("UPDATE plans SET phase = 'plan' WHERE id = 'pay';");
    }
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    fx.engine.ports = fakes.ports.clone();
    (fx, fakes)
}

fn tick(fx: &mut Fx) {
    schedule::run(&mut fx.engine);
}

fn attempts(fx: &mut Fx) -> i64 {
    fx.query("SELECT count(*) FROM attempts")
}

#[test]
fn e3_starts_a_queued_scout_in_phase_grounding() {
    let (mut fx, _fakes) = fx_with_fakes("grounding", "");
    add_explore(&mut fx, "scout", "H1", "base", &[], &merged("base"));
    add_explore(&mut fx, "scout", "H2", "look", &["base"], "");

    tick(&mut fx);

    assert_eq!(attempts(&mut fx), 1, "an Attempt row for the scout");
    assert_eq!(fx.query::<String>("SELECT job FROM attempts"), "look");
    assert_eq!(fx.job_status("look"), "running");
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn e3_starts_a_queued_research_job_in_phases_plan_dispatch_and_deploy() {
    for phase in ["plan", "dispatch", "deploy"] {
        let (mut fx, _fakes) = fx_with_fakes(phase, "");
        add_explore(&mut fx, "research", "S1", "web", &[], "");

        tick(&mut fx);

        assert_eq!(
            attempts(&mut fx),
            1,
            "phase {phase}: an Attempt row for the research Job"
        );
        assert_eq!(fx.job_status("web"), "running", "phase {phase}");
        assert_eq!(
            fx.count("error"),
            0,
            "phase {phase}: {:?}",
            fx.events("error")
        );
    }
}

#[test]
fn e3_does_not_start_an_explore_job_whose_need_is_unmerged() {
    let (mut fx, _fakes) = fx_with_fakes("dispatch", "");
    add_explore(&mut fx, "scout", "H1", "base", &[], "");
    add_explore(&mut fx, "research", "S1", "web", &["base"], "");

    tick(&mut fx);

    // `base` is Ready and starts; `web` needs it and waits for its merge.
    assert_eq!(fx.job_status("base"), "running", "the need itself is Ready");
    assert_eq!(fx.job_status("web"), "queued");
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'web'"),
        0
    );
    tick(&mut fx);
    assert_eq!(
        fx.job_status("web"),
        "queued",
        "still queued while its need is running"
    );
    assert_eq!(attempts(&mut fx), 1);
}

#[test]
fn e3_does_not_start_an_explore_job_of_a_done_or_archived_plan() {
    // The control: the same Jobs in a live Plan start, so that "nothing starts" below is the Plan's doing.
    let (mut live, _live_fakes) = fx_with_fakes("dispatch", "");
    add_explore(&mut live, "scout", "H1", "look", &[], "");
    tick(&mut live);
    assert_eq!(
        attempts(&mut live),
        1,
        "control: a queued scout of a live Plan starts"
    );

    for (what, phase, set) in [
        (
            "done",
            "dispatch",
            "UPDATE plans SET phase = 'deploy' WHERE id = 'pay'; UPDATE plans SET phase = 'done', done_at = 't' WHERE id = 'pay';",
        ),
        (
            "archived",
            "dispatch",
            "UPDATE plans SET archived_at = 't' WHERE id = 'pay';",
        ),
    ] {
        let (mut fx, _fakes) = fx_with_fakes(phase, "");
        fx.fixture(set);
        add_explore(&mut fx, "scout", "H1", "look", &[], "");
        add_explore(&mut fx, "research", "S1", "web", &[], "");

        tick(&mut fx);

        assert_eq!(attempts(&mut fx), 0, "a {what} Plan starts nothing");
        assert_eq!(fx.job_status("look"), "queued", "{what}");
        assert_eq!(fx.job_status("web"), "queued", "{what}");
    }
}

#[test]
fn explore_jobs_take_worker_slots_under_concurrency() {
    let (mut fx, fakes) = fx_with_fakes("dispatch", "concurrency = 1\n");
    let started = timestamp(fakes.clock.utc());
    add_explore(
        &mut fx,
        "scout",
        "H1",
        "holder",
        &[],
        &format!(
            "{}{}",
            running("holder"),
            running_attempt("holder", 1, "H1", &started)
        ),
    );
    fx.add_job("api", &[], "");
    add_explore(&mut fx, "research", "S1", "web", &[], "");

    tick(&mut fx);

    assert_eq!(
        attempts(&mut fx),
        1,
        "the running explore Job holds the only slot: nothing else starts"
    );
    assert_eq!(fx.job_status("api"), "queued");
    assert_eq!(fx.job_status("web"), "queued");
    // E3's skip order: the Ready Jobs that found no slot are numbered in queue order, as for any other Job.
    let skipped = |fx: &mut Fx, key: &str| {
        fx.query::<Option<i64>>(&format!(
            "SELECT skipped_n FROM jobs WHERE plan = 'pay' AND key = '{key}'"
        ))
    };
    assert_eq!(skipped(&mut fx, "api"), Some(1));
    assert_eq!(skipped(&mut fx, "web"), Some(2));
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

#[test]
fn explore_attempt_times_out_at_explore_minutes() {
    let (mut fx, fakes) = fx_with_fakes(
        "dispatch",
        "\n[timeouts]\nexplore_minutes = 1\nattempt_minutes = 90\n",
    );
    assert_eq!(fx.engine.config.timeouts.explore_minutes, 1);
    let started = timestamp(fakes.clock.utc());
    add_explore(
        &mut fx,
        "scout",
        "H1",
        "look",
        &[],
        &format!(
            "{}{}",
            running("look"),
            running_attempt("look", 1, "H1", &started)
        ),
    );
    let stop = |fx: &mut Fx| {
        fx.query::<Option<String>>("SELECT stop_request FROM attempts WHERE id = 'look-a1'")
    };
    // A held driver lock is what a live driver looks like to E2; the sessions step runs on the in-process tmux.
    let _driver = cpd_core::driver::lock::take_free_driver_lock(
        &fx.engine.home,
        &PlanId::parse("pay").unwrap(),
        &AttemptId::parse("look-a1").unwrap(),
    )
    .unwrap()
    .expect("free");
    fx.on_fake_tmux();

    fakes.clock.advance(Duration::from_secs(50));
    liveness::run(&mut fx.engine);
    assert_eq!(
        stop(&mut fx),
        None,
        "50 s of active time is under one minute"
    );

    fakes.clock.advance(Duration::from_secs(11));
    liveness::run(&mut fx.engine);
    assert_eq!(
        stop(&mut fx).as_deref(),
        Some("timed_out"),
        "one active minute passed: attempt_minutes (90) does not apply"
    );
    assert_eq!(fx.count("error"), 0, "{:?}", fx.events("error"));
}

/// The catalog of the testkit (contract §9): H0-H3, S0-S5.
fn catalog() -> Catalog {
    Catalog {
        pi_version: "1.0.0".into(),
        o: TierModel {
            model: "p/opus".into(),
            efforts: vec![1, 2, 3, 4, 5],
        },
        s: TierModel {
            model: "p/sonnet".into(),
            efforts: vec![0, 1, 2, 3, 4, 5],
        },
        h: TierModel {
            model: "p/haiku".into(),
            efforts: vec![0, 1, 2, 3],
        },
        g: TierModel {
            model: "p/gemini".into(),
            efforts: vec![],
        },
    }
}

/// Ends `look-a<number>` (running at `tier`) with `cause` through the ledger, as the driver does; whether it could.
fn fail_attempt(
    fx: &mut Fx,
    key: &str,
    number: u32,
    tier: &str,
    cause: Cause,
    retry_cap: u32,
) -> bool {
    let (plan, id) = (
        PlanId::parse("pay").unwrap(),
        AttemptId::parse(&format!("{key}-a{number}")).unwrap(),
    );
    let at = timestamp(time::OffsetDateTime::now_utc());
    fx.fixture(&running_attempt(key, number, tier, &at));
    let Ok(attempt) = fx.engine.store.read(|conn| ledger::load(conn, &plan, &id)) else {
        return false;
    };
    let catalog = catalog();
    let ladder = Ladder {
        catalog: &catalog,
        retry_cap,
    };
    let end = End::Failed {
        cause,
        detail: "It failed.".into(),
    };
    fx.engine
        .store
        .write("test", |tx| {
            ledger::end(tx, &attempt, &end, None, &ladder, Stage::Driver)
        })
        .is_ok()
}

fn tiers_retried(fx: &mut Fx) -> Vec<String> {
    fx.events("attempt-retry")
        .into_iter()
        .map(|(.., data)| data.to_string())
        .collect()
}

#[test]
fn explore_ladder_stays_inside_the_kind_range() {
    // A scout climbs (only `no_submit` climbs) H1 > H2 > H3, then fails: after H3 the ladder would leave H for S1, and an explore Job never does.
    let (mut fx, _fakes) = fx_with_fakes("dispatch", "");
    add_explore(&mut fx, "scout", "H1", "look", &[], &running("look"));
    assert!(
        fail_attempt(&mut fx, "look", 1, "H1", Cause::NoSubmit, 5),
        "the ledger ends an explore Attempt"
    );
    assert_eq!(fx.job_status("look"), "queued");
    assert!(
        tiers_retried(&mut fx)
            .last()
            .is_some_and(|d| d.contains("H2")),
        "{:?}",
        tiers_retried(&mut fx)
    );
    fx.fixture(&running("look"));
    assert!(fail_attempt(&mut fx, "look", 2, "H2", Cause::NoSubmit, 5));
    assert_eq!(fx.job_status("look"), "queued");
    assert!(
        tiers_retried(&mut fx)
            .last()
            .is_some_and(|d| d.contains("H3")),
        "{:?}",
        tiers_retried(&mut fx)
    );
    fx.fixture(&running("look"));
    assert!(fail_attempt(&mut fx, "look", 3, "H3", Cause::NoSubmit, 5));
    assert_eq!(
        fx.job_status("look"),
        "failed",
        "after H3 the scout fails, it never becomes S1"
    );
    assert_eq!(
        fx.query::<i64>(
            "SELECT count(*) FROM jobs WHERE key = 'look' AND failed_cause = 'no_submit'"
        ),
        1
    );

    // A research Job climbs S1 > S2 > S3, then fails: it never leaves S.
    add_explore(&mut fx, "research", "S1", "web", &[], &running("web"));
    assert!(fail_attempt(&mut fx, "web", 1, "S1", Cause::NoSubmit, 5));
    assert_eq!(fx.job_status("web"), "queued");
    fx.fixture(&running("web"));
    assert!(fail_attempt(&mut fx, "web", 2, "S2", Cause::NoSubmit, 5));
    assert_eq!(fx.job_status("web"), "queued");
    fx.fixture(&running("web"));
    assert!(fail_attempt(&mut fx, "web", 3, "S3", Cause::NoSubmit, 5));
    assert_eq!(
        fx.job_status("web"),
        "failed",
        "after S3 the research Job fails: S4 is not a Worker Tier"
    );

    // retry_cap is unchanged: with retry_cap 1 a scout fails at its second Attempt, though H3 is left on the ladder.
    add_explore(&mut fx, "scout", "H1", "capped", &[], &running("capped"));
    assert!(fail_attempt(&mut fx, "capped", 1, "H1", Cause::NoSubmit, 1));
    assert_eq!(fx.job_status("capped"), "queued");
    fx.fixture(&running("capped"));
    assert!(fail_attempt(&mut fx, "capped", 2, "H2", Cause::NoSubmit, 1));
    assert_eq!(
        fx.job_status("capped"),
        "failed",
        "1 + retry_cap Attempts at most"
    );

    // provider_error is unchanged: it requeues the Job at the same Tier and uses no rung.
    add_explore(&mut fx, "scout", "H3", "flaky", &[], &running("flaky"));
    assert!(fail_attempt(
        &mut fx,
        "flaky",
        1,
        "H3",
        Cause::ProviderError,
        2
    ));
    assert_eq!(
        fx.job_status("flaky"),
        "queued",
        "a provider error at the top rung retries in place"
    );
}

#[test]
fn explore_crash_at_the_top_rung_is_retried_in_place_under_the_cap() {
    // A `crashed` Attempt is retried at its own Tier (pro-d265), also at H3 or S3, until 1 + retry_cap Attempts.
    let (mut fx, _fakes) = fx_with_fakes("dispatch", "");
    for (kind, tier, key) in [("scout", "H3", "look"), ("research", "S3", "web")] {
        add_explore(&mut fx, kind, tier, key, &[], &running(key));
        for number in 1..=2 {
            if number > 1 {
                fx.fixture(&running(key));
            }
            assert!(fail_attempt(&mut fx, key, number, tier, Cause::Crashed, 2));
            assert_eq!(
                fx.job_status(key),
                "queued",
                "{key} attempt {number}: retried in place"
            );
            let last = tiers_retried(&mut fx).pop().unwrap_or_default();
            let data: serde_json::Value = serde_json::from_str(&last).unwrap();
            assert_eq!(data["next_tier"], tier, "{last}");
        }
        fx.fixture(&running(key));
        assert!(fail_attempt(&mut fx, key, 3, tier, Cause::Crashed, 2));
        assert_eq!(fx.job_status(key), "failed", "{key}: the cap ends it");
        assert_eq!(
            fx.query::<i64>(&format!(
                "SELECT count(*) FROM jobs WHERE key = '{key}' AND failed_cause = 'crashed'"
            )),
            1
        );
    }

    // A scout at H2 that crashed stays at H2.
    add_explore(&mut fx, "scout", "H2", "mid", &[], &running("mid"));
    assert!(fail_attempt(&mut fx, "mid", 1, "H2", Cause::Crashed, 2));
    assert_eq!(fx.job_status("mid"), "queued");
    let last = tiers_retried(&mut fx).pop().unwrap_or_default();
    let data: serde_json::Value = serde_json::from_str(&last).unwrap();
    assert_eq!(data["next_tier"], "H2", "{last}");
}
