//! A Plan held in `deploy` by a non-final Job (accepted assumptions A1, A3, A4) moves to `done` when the last blocking Job ends
//! `merged` or `cancelled`: `job cancel`, a running Job ended `cancelled`, a Job merge, an explore Job merge. The transition is the
//! one of `plan::done::settle_done`: `done_at`, one `plan-done` Event, pending plan reviews superseded.
//! In process on the janitor's `Rig`: git is the `Universe`, the store a temp database. No process, no sleep.

use std::path::Path;

use crate::attempt::ledger::{self, End, Ladder, Stage};
use crate::config::{Catalog, Config, RoleTiers, Tier, TierModel};
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::janitor_tests::Rig;
use crate::merge::{self, Ctx};
use crate::ports::Ports;
use crate::store::Store;

/// The held Plan: `pay` in `deploy`, Workflow `api` merged.
fn held() -> Rig {
    let mut rig = Rig::new();
    rig.workflow("api", "merged");
    rig
}

/// Inserts Job `key` of status `status`: of Workflow `api`, or an explore Job (`scout`, no Workflow) when `kind` is `scout`.
fn job(rig: &Rig, key: &str, kind: &str, status: &str) {
    let workflow = if kind == "scout" { "NULL" } else { "'api'" };
    let touches = if kind == "scout" {
        format!("[\"run/pay/explore/{key}.md\"]")
    } else {
        "[\"docs/research/x.md\"]".to_owned()
    };
    rig.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind,
                           status, failed_cause, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', {workflow}, '{key}', 'B', 'H2', 'g', 'c', '{touches}', '[\"b\"]', '[]', '{kind}', '{status}', {cause}, 0, '{fp}', cpd_txn(), 't');",
        cause = if status == "failed" { "'crashed'" } else { "NULL" },
        fp = "e".repeat(64)
    ));
}

/// `(phase, done_at is null, number of plan-done Events)` of Plan `pay`.
fn state(rig: &Rig) -> (String, bool, i64) {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT phase, done_at IS NULL, (SELECT count(*) FROM events WHERE kind = 'plan-done') FROM plans WHERE id = 'pay'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap()
}

fn deploy_held() -> (String, bool, i64) {
    ("deploy".to_owned(), true, 0)
}

fn done_once() -> (String, bool, i64) {
    ("done".to_owned(), false, 1)
}

/// `job cancel` of Job `key`, on the rig's `$CPD_HOME`.
fn cancel(rig: &Rig, key: &str) {
    // SAFETY: nextest gives each test a process of its own.
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    crate::job::cancel::cancel_without_shape(
        &PlanId::parse("pay").unwrap(),
        JobKey::parse(key).unwrap(),
        "no longer needed",
    )
    .expect("the cancel runs");
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![1, 2, 3],
    };
    Catalog {
        j: crate::config::TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel {
            model: "g/m".to_owned(),
            efforts: vec![],
        },
    }
}

fn attempt_of(rig: &Rig, id: &str) -> merge::Attempt {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            ledger::load(
                c,
                &PlanId::parse("pay").unwrap(),
                &AttemptId::parse(id).unwrap(),
            )
        })
        .unwrap()
}

#[test]
fn cancelling_the_only_failed_job_of_a_held_plan_ends_it_done() {
    let rig = held();
    job(&rig, "api-fix", "work", "failed");
    assert_eq!(state(&rig), deploy_held());

    cancel(&rig, "api-fix");

    assert_eq!(state(&rig), done_once());
}

#[test]
fn cancelling_one_of_two_blocking_jobs_keeps_the_plan_in_deploy() {
    let rig = held();
    job(&rig, "fix-a", "work", "failed");
    job(&rig, "fix-b", "work", "failed");

    cancel(&rig, "fix-a");
    assert_eq!(state(&rig), deploy_held(), "fix-b still blocks the Plan");

    cancel(&rig, "fix-b");
    assert_eq!(state(&rig), done_once(), "the last blocker ends the Plan");
}

/// A `running` Job `key` of Workflow `api` with its `merging` Attempt on the Lead Branch, as the merge takes it.
fn merging(rig: &Rig, key: &str) {
    let (lead, branch) = (
        rig.workflows[0].lead.display().to_string(),
        rig.workflows[0].lead_branch.clone(),
    );
    let tip = rig
        .universe
        .lock()
        .unwrap()
        .ref_oid(rig.repo.id(), &format!("refs/heads/{branch}"))
        .expect("the Lead Branch exists");
    job(rig, key, "work", "running");
    rig.sql(&format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, submitted_sha, submitted_at, driver_pid, started_at)
           VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'merging', '{branch}', '{lead}', '{tip}', '{tip}', 't', 4242, 't');",
        tip = tip.as_str()
    ));
}

/// `merge::merge` of Attempt `<key>-a1`.
fn merge_of(rig: &Rig, key: &str) {
    let policy = rig.home.root().join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("owner.toml"), OWNER_POLICY).unwrap();
    let config = Config::parse(
        "version = 1\nmachine = \"test\"\n",
        Path::new("config.toml"),
    )
    .unwrap()
    .0;
    let catalog = catalog();
    let metrics = |_a: &merge::Attempt| None;
    let ctx = Ctx {
        home: &rig.home,
        config: &config,
        catalog: &catalog,
        source: "test",
        metrics: &metrics,
        classifier: merge::no_verdict_classifier(),
    };
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let out = Ports::scope(&rig.fakes.ports, || {
        merge::merge(
            &mut store,
            &ctx,
            &PlanId::parse("pay").unwrap(),
            &AttemptId::parse(&format!("{key}-a1")).unwrap(),
        )
    })
    .expect("the merge itself does not fail");
    assert!(
        matches!(out.end, merge::End::Merged { .. }),
        "the Job merges: {:?}",
        out.end
    );
}

#[test]
fn merging_the_last_blocking_job_ends_a_held_plan_done() {
    let rig = held();
    merging(&rig, "api-retry");
    assert_eq!(state(&rig), deploy_held());

    merge_of(&rig, "api-retry");

    assert_eq!(state(&rig), done_once());
}

/// A `running` Job `key` with a `running` Attempt, the Job's cancel requested (`job cancel` of a running Job).
fn running_attempt(rig: &Rig, key: &str, kind: &str) {
    job(rig, key, kind, "running");
    rig.sql(&format!(
        "UPDATE jobs SET cancel_reason = 'Gave up.', cancel_requested_at = 't' WHERE plan = 'pay' AND key = '{key}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, stop_request, started_at)
           VALUES ('pay', '{key}-a1', '{key}', 1, 'H2', 'ladder', 'running', 'cpd/pay--{key}-a1', '/w', 4242, 'cancelled', 't');"
    ));
}

#[test]
fn cancelling_a_running_job_that_was_the_last_blocker_ends_the_plan_done() {
    let rig = held();
    running_attempt(&rig, "api-run", "work");
    let attempt = attempt_of(&rig, "api-run-a1");
    let catalog = catalog();
    let ladder = Ladder {
        catalog: &catalog,
        retry_cap: 3,
        roles: RoleTiers {
            decide: crate::config::Tier::x(crate::config::Letter::O),
            auxiliar: Tier::parse("GX").unwrap(),
            worker: Tier::parse("H0").unwrap(),
            lead: Tier::parse("S4").unwrap(),
            manager: Tier::parse("O4").unwrap(),
        },
    };
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .write("test", |tx| {
            ledger::end(tx, &attempt, &End::Cancelled, None, &ladder, Stage::Driver)
        })
        .unwrap();

    assert_eq!(state(&rig), done_once());
}

#[test]
fn merging_the_last_blocking_explore_job_ends_a_held_plan_done() {
    let rig = held();
    running_attempt(&rig, "scout1", "scout");
    rig.sql("UPDATE jobs SET cancel_reason = NULL, cancel_requested_at = NULL WHERE key = 'scout1'; UPDATE attempts SET stop_request = NULL WHERE id = 'scout1-a1';");
    let attempt = attempt_of(&rig, "scout1-a1");
    assert_eq!(state(&rig), deploy_held());

    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .write("test", |tx| {
            ledger::end_explore(tx, &attempt, &"a".repeat(64), "found it")
        })
        .unwrap();

    assert_eq!(state(&rig), done_once());
}

#[test]
fn cancelling_a_job_while_a_workflow_is_still_open_leaves_the_phase() {
    let mut rig = held();
    rig.workflow("web", "open");
    job(&rig, "fix-a", "work", "failed");
    job(&rig, "fix-b", "work", "failed");

    cancel(&rig, "fix-a");
    assert_eq!(state(&rig), deploy_held(), "web is still open");

    // web merges while fix-b still blocks; the last blocker then ends the Plan.
    rig.sql("UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'pay' AND key = 'web';");
    assert_eq!(state(&rig), deploy_held());
    cancel(&rig, "fix-b");
    assert_eq!(state(&rig), done_once());
}

#[test]
fn the_done_transition_after_a_job_event_supersedes_pending_plan_reviews() {
    let rig = held();
    job(&rig, "api-fix", "work", "failed");
    rig.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, feedback, requested_at, decided_at, title)
           VALUES ('pay', 'r1', 1, '{}', 'pending', 'feedback', 't', NULL, 'Title');",
        "a".repeat(64)
    ));

    cancel(&rig, "api-fix");

    let mut store = Store::open(&rig.home, 5000).unwrap();
    let status: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT status FROM plan_reviews WHERE plan = 'pay' AND id = 'r1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(status, "superseded");
    assert_eq!(state(&rig), done_once());
}

#[test]
fn a_plan_already_done_gets_no_second_plan_done_event() {
    let rig = held();
    job(&rig, "fix-a", "work", "failed");
    cancel(&rig, "fix-a");
    assert_eq!(state(&rig), done_once(), "the last blocker ends the Plan");

    job(&rig, "fix-late", "work", "failed");
    cancel(&rig, "fix-late");

    assert_eq!(state(&rig), done_once(), "still done, one plan-done Event");
}

/// cpd's compiled default policy as a complete owner scope, so `policy_of_job` resolves.
const OWNER_POLICY: &str = "\
[gate]
merge = \"none\"
pr = \"none\"
text = \"\"

[pr]
text = \"\"

[pr.feedback]
checks = \"any\"

[deploy]
text = \"\"

[code]
text = \"\"

[tests]
text = \"\"

[monitor]
text = \"\"

[instructions]
text = \"\"
";

#[test]
fn cancelling_the_last_job_settles_a_dispatch_plan_to_done_without_illegal_transition() {
    let rig = held();
    rig.sql("UPDATE plans SET phase = 'dispatch' WHERE id = 'pay'");
    job(&rig, "api-fix", "work", "failed");

    cancel(&rig, "api-fix");

    assert_eq!(state(&rig), done_once());
}
