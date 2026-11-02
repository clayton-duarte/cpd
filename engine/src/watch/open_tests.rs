//! E4, the open Job (contract §3.7 E4, §3.2 Ready, §2.2 "Job kinds"; hs-d340, hs-d573), one `open::run` per tick on a prepared
//! store. Where a test says E3 starts nothing it runs `schedule::run` too, which starts nothing while no Job is Ready; where a
//! Job is Ready, the test reads `schedule::READY`, the query E3 starts from.

use serde_json::json;

use cpd_core::ids::PlanId;
use cpd_core::plan::{jobs, locked_fingerprint};

use super::testkit::{Fx, PR_TITLE};
use super::{open, schedule};

const MERGED: &str = "UPDATE jobs SET status = 'running' WHERE key = 'KEY'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'KEY';";

fn merged(key: &str) -> String {
    MERGED.replace("KEY", key)
}

/// One tick of E4.
fn tick(fx: &mut Fx) {
    open::run(&mut fx.engine);
}

fn ticks(fx: &mut Fx, n: usize) {
    for _ in 0..n {
        tick(fx);
    }
}

/// Job `api` merged: the Workflow's open Job can be Ready once nothing else of the Workflow is left.
fn add_merged_api(fx: &mut Fx) {
    fx.add_job("api", &[], &merged("api"));
}

/// Job `key` of Workflow `repo`, a draft: the Workflow's open Job waits for it, and E3 never starts it.
fn add_draft(fx: &mut Fx, key: &str) {
    add_draft_in(fx, "repo", key);
}

/// `add_draft` in Workflow `pr`.
fn add_draft_in(fx: &mut Fx, workflow: &str, key: &str) {
    fx.fixture(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', '{workflow}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, cpd_txn(), 't');"
    ));
}

/// Job `key` of Workflow `repo` as cpd adds an `open` or `update` Job (`kind`): `appended`, no Dependencies, `queued` with its
/// fingerprint, then moved by `then` (SQL).
fn add_cpd_job(fx: &mut Fx, key: &str, kind: &str, then: &str) {
    add_cpd_job_in(fx, "repo", key, kind, then);
}

/// `add_cpd_job` in Workflow `pr`.
fn add_cpd_job_in(fx: &mut Fx, workflow: &str, key: &str, kind: &str, then: &str) {
    fx.fixture(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
         VALUES ('pay', '{workflow}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');"
    ));
    fx.lock_job(key, then);
}

/// Workflow `web` of Plan `pay`, `dispatch`, beside Workflow `repo`.
fn add_web_workflow(fx: &mut Fx) {
    let (repo, lead) = (
        fx.engine.home.root().join("repo"),
        fx.engine.home.root().join("lead"),
    );
    fx.fixture(&format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'web', '{}', 'main', 'dispatch', 'cpd/pay/web', '{}', 'Show the receipt', 'config', cpd_txn(), 't', 't');",
        repo.display(),
        lead.display()
    ));
}

/// `(key, status)` of every Job of `kind`, by key.
fn jobs_of_kind(fx: &mut Fx, kind: &str) -> Vec<(String, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT key, status FROM jobs WHERE kind = ?1 ORDER BY key")?;
            Ok(stmt
                .query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn pair(key: &str, status: &str) -> (String, String) {
    (key.to_owned(), status.to_owned())
}

/// The Jobs E3 would start now: `(plan, key)`.
fn ready(fx: &mut Fx) -> Vec<(String, String)> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare(schedule::READY)?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// One tick of E3, E4: E3 starts nothing here, and a Job that was Ready would be a start of a real driver.
fn tick_e3_e4(fx: &mut Fx) {
    schedule::run(&mut fx.engine);
    tick(fx);
}

/// E4 adds the Workflow's `open` Job once the Workflow is `dispatch`, whatever its other Jobs are doing: `open-repo`, Tier `open_tier` (H3),
/// `queued`, `appended`, no Dependency rows (its Ready rule is derived), and one `job-added` Event, from the engine. The ticks
/// that follow add nothing. (A draft of the Workflow keeps it from being Ready, so E3 starts nothing here.)
#[test]
fn e4_adds_one_open_job_to_a_dispatch_pr_whatever_its_other_jobs_are_doing_and_adds_no_second() {
    let mut fx = Fx::with_plan("");
    add_merged_api(&mut fx);
    add_draft(&mut fx, "later");

    ticks(&mut fx, 4);

    assert_eq!(jobs_of_kind(&mut fx, "open"), [pair("open-repo", "draft")]);
    let row: (String, String, String, i64, i64, bool) = fx
        .engine
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT title, tier, workflow, appended, urgent, fingerprint IS NOT NULL FROM jobs WHERE plan = 'pay' AND key = 'open-repo'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        row,
        (
            format!("Open the pull request: {PR_TITLE}"),
            "H3".into(),
            "repo".into(),
            1,
            0,
            false
        )
    );
    assert_eq!(
        fx.query::<i64>(
            "SELECT count(*) FROM dependencies WHERE plan = 'pay' AND job = 'open-repo'"
        ),
        0,
        "its needs are derived"
    );
    let added = fx.events("job-added");
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(
        (&added[0].0, &added[0].1),
        (&Some("pay".to_owned()), &Some("open-repo".to_owned()))
    );
    let data = &added[0].3;
    assert_eq!(
        (
            &data["kind"],
            &data["by"],
            &data["workflow"],
            &data["status"],
            &data["tier"]
        ),
        (
            &json!("open"),
            &json!("engine"),
            &json!("repo"),
            &json!("draft"),
            &json!("H3")
        )
    );
    assert_eq!(data["needs"], json!([]));
    assert_eq!(fx.count("all-merged"), 0, "the all-merged steer is retired");
    assert_eq!(
        fx.query::<Option<String>>("SELECT all_merged_at FROM workflows WHERE plan = 'pay'"),
        None
    );
}

/// B1 (hs-d573): the open Job is a draft while a draft of its Workflow remains, and queued (with its fingerprint) once none does.
#[test]
fn e4_adds_the_open_job_as_a_draft_while_a_draft_of_its_pr_remains_and_queues_it_when_none_does() {
    let mut fx = Fx::with_plan("");
    add_merged_api(&mut fx);
    add_draft(&mut fx, "later");
    // A running Job keeps E3 from starting the open Job once it is queued, and is no draft.
    fx.add_job_in(
        "repo",
        "busy",
        &[],
        "UPDATE jobs SET status = 'running' WHERE key = 'busy';",
    );

    ticks(&mut fx, 4);
    assert_eq!(jobs_of_kind(&mut fx, "open"), [pair("open-repo", "draft")]);
    let row: (String, i64, bool) = fx
        .engine
        .store
        .read(|conn| {
            Ok(conn.query_row("SELECT tier, appended, fingerprint IS NULL FROM jobs WHERE plan = 'pay' AND key = 'open-repo'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?)
        })
        .unwrap();
    assert_eq!(row, ("H3".into(), 1, true));
    assert_eq!(
        fx.query::<i64>(
            "SELECT count(*) FROM dependencies WHERE plan = 'pay' AND job = 'open-repo'"
        ),
        0
    );
    let added = fx.events("job-added");
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(added[0].3["status"], json!("draft"));

    fx.fixture("UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'later';");
    tick(&mut fx);
    assert_eq!(fx.job_status("open-repo"), "queued");
    ticks(&mut fx, 3);

    let plan = PlanId::parse("pay").unwrap();
    let all = fx
        .engine
        .store
        .read(|conn| jobs::load(conn, &plan))
        .unwrap();
    let job = all.iter().find(|j| j.key == "open-repo").unwrap();
    assert_eq!(
        fx.query::<String>("SELECT fingerprint FROM jobs WHERE key = 'open-repo'"),
        locked_fingerprint(job)
    );
    assert_eq!(jobs_of_kind(&mut fx, "open"), [pair("open-repo", "queued")]);
    assert_eq!(fx.count("job-added"), 1);
}

/// B1: one Workflow with a draft gets a draft open Job, one without gets a queued one; later ticks change nothing.
#[test]
fn e4_queues_the_open_job_at_once_when_its_pr_has_no_draft_and_adds_no_second_to_a_draft_one() {
    let mut fx = Fx::with_plan("");
    add_merged_api(&mut fx);
    add_draft(&mut fx, "later");
    add_web_workflow(&mut fx);
    // A running Job keeps E3 from starting the open Job of Workflow web, and is no draft.
    fx.add_job_in(
        "web",
        "ui",
        &[],
        "UPDATE jobs SET status = 'running' WHERE key = 'ui';",
    );

    ticks(&mut fx, 5);

    assert_eq!(
        jobs_of_kind(&mut fx, "open"),
        [pair("open-repo", "draft"), pair("open-web", "queued")]
    );
    assert_eq!(fx.count("job-added"), 2);
}

/// A6 (hs-d573): nothing moves a queued open Job back to draft when a draft is added to its Workflow later.
#[test]
fn e4_leaves_a_queued_open_job_queued_when_a_draft_is_added_to_its_pr_later() {
    let mut fx = Fx::with_plan("");
    add_merged_api(&mut fx);
    add_cpd_job(&mut fx, "open-repo", "open", "");
    add_draft(&mut fx, "later");

    ticks(&mut fx, 4);

    assert_eq!(jobs_of_kind(&mut fx, "open"), [pair("open-repo", "queued")]);
    assert_eq!(fx.count("job-added"), 0);
}

/// `open_tier` is a key of its own (§8.2): the Job takes its Tier from it.
#[test]
fn e4_gives_the_open_job_the_tier_open_tier_names() {
    let mut fx = Fx::with_plan("open_tier = \"H3\"");
    add_merged_api(&mut fx);
    add_draft(&mut fx, "later");

    tick(&mut fx);

    assert_eq!(
        fx.query::<String>("SELECT tier FROM jobs WHERE plan = 'pay' AND key = 'open-repo'"),
        "H3"
    );
}

/// Only a Workflow that is `dispatch`, of a Plan that is not archived, gets one.
#[test]
fn e4_adds_no_open_job_to_a_waiting_pr_or_to_a_pr_of_an_archived_plan() {
    // A Workflow that is still `waiting`, beside the Workflow that is `dispatch`.
    let mut waiting = Fx::with_plan("");
    add_draft(&mut waiting, "later");
    let repo = waiting.engine.home.root().join("repo");
    waiting.fixture(&format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, title, shape, created_txn, created_at)
           VALUES ('pay', 'web', '{}', 'main', 'waiting', 'cpd/pay/web', 'Show the receipt', 'config', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay', 'web', 'repo');",
        repo.display()
    ));
    ticks(&mut waiting, 4);
    assert_eq!(jobs_of_kind(&mut waiting, "open").len(), 1);
    assert_eq!(
        waiting.query::<String>("SELECT workflow FROM jobs WHERE kind = 'open'"),
        "repo"
    );

    // The Plan is archived: its dispatch Workflow gets none.
    let mut archived = Fx::with_plan("");
    add_merged_api(&mut archived);
    archived.fixture("UPDATE plans SET archived_at = 't' WHERE id = 'pay';");
    ticks(&mut archived, 3);
    assert_eq!(
        jobs_of_kind(&mut archived, "open"),
        Vec::<(String, String)>::new()
    );
}

/// A Workflow that has an open Job `queued` or `failed` gets no second one (a `failed` one waits for the Lead: `job retry`, or `job cancel`).
/// Workflow `web`, which has none, gets its own in the same ticks: the engine has looked at both.
#[test]
fn e4_adds_no_open_job_to_a_pr_whose_open_job_is_queued_or_failed() {
    for then in [
        "",
        "UPDATE jobs SET status = 'running' WHERE key = 'open-repo'; UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'open-repo';",
    ] {
        let mut fx = Fx::with_plan("");
        add_merged_api(&mut fx);
        add_draft(&mut fx, "later");
        add_cpd_job(&mut fx, "open-repo", "open", then);
        add_web_workflow(&mut fx);
        fx.add_job_in("web", "ui", &[], &merged("ui"));
        add_draft_in(&mut fx, "web", "later-web");

        ticks(&mut fx, 4);

        let open: Vec<String> = jobs_of_kind(&mut fx, "open")
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(open, ["open-repo", "open-web"], "{then:?}");
        assert_eq!(
            fx.count("job-added"),
            1,
            "{then:?}: only Workflow web's was added"
        );
    }
}

/// A `merged` open Job on a Workflow that is still `dispatch` (its merge died before it opened the pull request, §7.4 R) counts for nothing:
/// the next tick adds a fresh one, `open-repo-2`. Only a `merged` one: a `cancelled` one is the Lead's and stands (next test).
#[test]
fn e4_adds_a_fresh_open_job_after_a_merged_one_on_a_pr_still_dispatch() {
    let mut fx = Fx::with_plan("");
    add_merged_api(&mut fx);
    add_draft(&mut fx, "later");
    add_cpd_job(&mut fx, "open-repo", "open", &merged("open-repo"));

    ticks(&mut fx, 4);

    assert_eq!(
        jobs_of_kind(&mut fx, "open"),
        [pair("open-repo", "merged"), pair("open-repo-2", "draft")]
    );
    assert_eq!(fx.count("job-added"), 1);
}

/// The Lead's cancel of a Workflow's `open` Job stands (§3.7 E4, hs-d301): the Workflow stays `dispatch` and no `open-repo-2` follows, over several
/// ticks and across an engine restart. Seen: Plan ui-design's `open-proto` was cancelled and `open-proto-2` came four seconds later.
/// Workflow `web`, which has none, gets `open-web`: the engine has looked at both.
#[test]
fn e4_adds_no_open_job_to_a_dispatch_pr_whose_open_job_the_lead_cancelled() {
    let mut fx = Fx::with_plan("");
    add_merged_api(&mut fx);
    add_cpd_job(
        &mut fx,
        "open-repo",
        "open",
        "UPDATE jobs SET status = 'cancelled', cancel_reason = 'never to be opened', cancel_requested_at = 't' WHERE key = 'open-repo';",
    );
    add_web_workflow(&mut fx);
    add_draft_in(&mut fx, "web", "later-web");

    for _ in 0..2 {
        let mut engine = fx.restarted();
        for _ in 0..4 {
            open::run(&mut engine);
        }
    }

    assert_eq!(
        jobs_of_kind(&mut fx, "open"),
        [pair("open-repo", "cancelled"), pair("open-web", "draft")]
    );
    assert_eq!(fx.count("job-added"), 1, "only Workflow web's was added");
    assert_eq!(
        fx.query::<String>("SELECT status FROM workflows WHERE plan = 'pay' AND key = 'repo'"),
        "dispatch"
    );
}

/// A draft added after the lock (`job add --draft`, contract §3.2): E3 never starts it, and the Workflow's open Job waits while one remains (§3.2
/// Ready: the Workflow waits for the batch). Cancelling it frees the open Job: `the_open_job_is_ready_only_when_...` below.
#[test]
fn e3_never_starts_a_draft_and_the_open_job_waits_while_one_remains() {
    let mut fx = Fx::with_plan("");
    add_merged_api(&mut fx);
    // `job add --draft`: a draft, appended.
    add_draft(&mut fx, "later");
    assert_eq!(fx.job_status("later"), "draft");
    assert!(fx.query::<bool>("SELECT appended FROM jobs WHERE key = 'later'"));

    for _ in 0..4 {
        tick_e3_e4(&mut fx);
    }
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM attempts"),
        0,
        "a draft never gets an Attempt"
    );
    assert_eq!(fx.job_status("later"), "draft");
    assert_eq!(fx.count("all-merged"), 0, "all-merged is retired");
    assert_eq!(
        jobs_of_kind(&mut fx, "open"),
        [pair("open-repo", "draft")],
        "the open Job is added as a draft while the draft remains"
    );
}

/// An open Job is Ready only when every other Job of its Workflow is `merged` or `cancelled` and one is `merged` (§3.2): a `draft`, a
/// `queued`, a `running` or a `failed` Job keeps it back, and so do Jobs that are all `cancelled`.
#[test]
fn the_open_job_is_ready_only_when_every_other_job_is_merged_or_cancelled_and_one_is_merged() {
    // A draft holds it; cancelling the draft (`job cancel`) frees it: E3 would start `open-repo`.
    let mut draft = Fx::with_plan("");
    add_merged_api(&mut draft);
    add_draft(&mut draft, "later");
    for _ in 0..4 {
        tick_e3_e4(&mut draft);
    }
    assert_eq!(
        jobs_of_kind(&mut draft, "open"),
        [pair("open-repo", "draft")]
    );
    assert_eq!(
        draft.count("attempt-started"),
        0,
        "a draft holds the open Job back"
    );
    assert_eq!(ready(&mut draft), []);
    draft.fixture("UPDATE jobs SET status = 'cancelled', cancel_reason = 'Not needed.', cancel_requested_at = 't' WHERE key = 'later';");
    tick(&mut draft);
    assert_eq!(
        ready(&mut draft),
        [("pay".to_owned(), "open-repo".to_owned())],
        "the open Job is the one Job E3 starts"
    );

    // A `failed` Job holds it too, as it holds a Dependent back.
    let mut failed = Fx::with_plan("");
    add_merged_api(&mut failed);
    failed.add_job("broken", &[], "UPDATE jobs SET status = 'running' WHERE key = 'broken'; UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'broken';");
    for _ in 0..4 {
        tick_e3_e4(&mut failed);
    }
    assert_eq!(
        jobs_of_kind(&mut failed, "open"),
        [pair("open-repo", "queued")]
    );
    assert_eq!(
        failed.count("attempt-started"),
        0,
        "a failed Job holds the open Job back"
    );
    assert_eq!(ready(&mut failed), []);

    // No Job merged: a Workflow whose Jobs are all cancelled has nothing to open.
    let mut cancelled = Fx::with_plan("");
    cancelled.add_job("api", &[], "UPDATE jobs SET status = 'cancelled', cancel_reason = 'no', cancel_requested_at = 't' WHERE key = 'api';");
    for _ in 0..4 {
        tick_e3_e4(&mut cancelled);
    }
    assert_eq!(
        jobs_of_kind(&mut cancelled, "open"),
        [pair("open-repo", "queued")]
    );
    assert_eq!(
        cancelled.count("attempt-started"),
        0,
        "no Job merged: the open Job is not Ready"
    );
    assert_eq!(ready(&mut cancelled), []);
}
