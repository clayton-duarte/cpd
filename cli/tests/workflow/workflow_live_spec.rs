//! The Worker Spec of an `open` or `update` Job is rendered at A3 from the LIVE PR row (contract §3.4 A3, commands.md §16.1):
//! after the PR's base or repo changed it names the live value, never the one written at insert. The stored Spec (the `jobs`
//! row) keeps the text written at insert: nothing is written.

use std::fs;

use super::workflow_crud::{Crud, crud};
use cpd_core::attempt::ledger;
use cpd_core::config::Tier;
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::job::open::{Cause, queue_update};
use cpd_core::store::Store;

fn plan() -> PlanId {
    PlanId::parse("pay-1").unwrap()
}

fn store(fx: &Crud) -> Store {
    Store::open(
        &CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap()
}

/// `pr add <key> --repo <repo> --shape config …`: the draft opener `open-<key>` and its Spec stay.
fn add(fx: &Crud, key: &str, repo: &std::path::Path, extra: &[&str]) {
    let title = format!("Change {key}");
    let mut args = vec![
        "workflow",
        "add",
        key,
        "--repo",
        repo.to_str().unwrap(),
        "--title",
        &title,
        "--shape",
        "config",
    ];
    args.extend(extra);
    args.extend(["--plan", "pay-1"]);
    fx.cpd(&args).ok();
}

fn branch(fx: &Crud, repo: &std::path::Path, name: &str) {
    fx.git(repo, &["branch", name]);
    fx.git(
        repo,
        &["update-ref", &format!("refs/remotes/origin/{name}"), name],
    );
}

fn base_of(fx: &Crud, key: &str) -> String {
    fx.env
        .query::<String>(&format!("SELECT base FROM workflows WHERE key = '{key}'"))
}

/// The stored row of Job `job`: `(goal, status, fingerprint)`; the tests compare the `goal`, since `render` moves the status
/// as the lock and the engine do.
fn stored(fx: &Crud, job: &str) -> (String, String, String) {
    let one = |col: &str| {
        fx.env.query::<String>(&format!(
            "SELECT coalesce({col}, '') FROM jobs WHERE key = '{job}'"
        ))
    };
    (one("goal"), one("status"), one("fingerprint"))
}

/// Starts the Job's PR (an Attempt needs a Lead worktree; `base` and `repo` are left alone), moves Job `job` to `running` the way `plan lock` and the engine do, with Attempt `<job>-a1`, then renders its Worker Spec.
fn render(fx: &Crud, job: &str) -> String {
    let fingerprint = {
        let mut store = store(fx);
        store
            .read(|c| {
                let jobs = cpd_core::plan::jobs::load(c, &plan())?;
                Ok(cpd_core::plan::locked_fingerprint(
                    jobs.iter().find(|j| j.key == job).unwrap(),
                ))
            })
            .unwrap()
    };
    let worktree = fx.env.root().join(format!("worktrees/{job}-a1"));
    fx.env.sql(&format!(
        "UPDATE workflows SET status = 'dispatch', lead_worktree = '/x/lead', started_at = 't' WHERE key = (SELECT workflow FROM jobs WHERE key = '{job}') AND status = 'waiting';
         UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE key = '{job}';
         UPDATE jobs SET status = 'running' WHERE key = '{job}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
           VALUES ('pay-1', '{job}-a1', '{job}', 1, 'H3', 'ladder', 'running', 'cpd/pay-1--{job}-a1', '{wt}', '{sha}', 't');",
        wt = worktree.display(),
        sha = "0".repeat(40),
    ));
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let path = store
        .read(|conn| {
            let target = ledger::load(
                conn,
                &plan(),
                &AttemptId::parse(&format!("{job}-a1")).unwrap(),
            )?;
            let jobs = cpd_core::plan::jobs::load(conn, &plan())?;
            let job = jobs.iter().find(|j| j.key == job).expect("the Job");
            cpd_core::driver::worker::render_spec(conn, &home, &target, job)
        })
        .unwrap();
    fs::read_to_string(path).unwrap()
}

/// The three fragments of a base in a rendered Spec.
fn base_fragments(base: &str, first: &str) -> [String; 3] {
    [
        format!("{first} {base}"),
        format!("git fetch origin {base}"),
        format!("origin/{base} HEAD"),
    ]
}

/// The live base is named and the insert-time one is not; the stored row of `job` is as it was.
fn assert_names(spec: &str, live: &str, old: &str) {
    assert_names_with(spec, live, old, "fit its base");
}

/// As [`assert_names`], with the goal's own words before the base (an update Spec says "up to date with its base").
fn assert_names_with(spec: &str, live: &str, old: &str, first: &str) {
    for f in base_fragments(live, first) {
        assert!(spec.contains(&f), "the Spec lacks {f:?}:\n{spec}");
    }
    for f in base_fragments(old, first) {
        assert!(!spec.contains(&f), "the Spec still has {f:?}:\n{spec}");
    }
}

#[test]
fn an_open_jobs_worker_spec_names_the_base_a_pr_edit_set_before_the_lock() {
    let fx = crud("plan");
    branch(&fx, &fx.pay, "develop");
    add(&fx, "b", &fx.pay, &["--base", "develop"]);
    let before = stored(&fx, "open-b");
    assert!(before.0.contains("develop"), "{before:?}");

    fx.cpd(&["workflow", "edit", "b", "--base", "main", "--plan", "pay-1"])
        .ok();
    assert_eq!(base_of(&fx, "b"), "main");

    let spec = render(&fx, "open-b");
    assert_names(&spec, "main", "develop");
    assert_eq!(stored(&fx, "open-b").0, before.0);
}

#[test]
fn an_open_jobs_worker_spec_names_the_base_a_restack_set_after_the_lock() {
    let fx = crud("plan");
    add(&fx, "a", &fx.pay, &["--no-needs"]);
    add(&fx, "b", &fx.pay, &["--needs", "a"]);
    fx.env
        .sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z'");
    let before = stored(&fx, "open-b");
    assert!(before.0.contains("main"), "{before:?}");

    fx.cpd(&[
        "workflow",
        "edit",
        "b",
        "--base",
        "a",
        "--no-needs",
        "--plan",
        "pay-1",
    ])
    .ok();
    assert_eq!(base_of(&fx, "b"), "pay-1/a");

    let spec = render(&fx, "open-b");
    assert_names(&spec, "pay-1/a", "main");
    assert_eq!(stored(&fx, "open-b").0, before.0);
}

#[test]
fn an_open_jobs_worker_spec_names_the_base_an_unstack_set_when_its_base_pr_merged() {
    let fx = crud("plan");
    add(&fx, "a", &fx.pay, &["--no-needs"]);
    add(&fx, "b", &fx.pay, &["--base", "a", "--no-needs"]);
    assert_eq!(base_of(&fx, "b"), "pay-1/a");
    let before = stored(&fx, "open-b");
    assert!(before.0.contains("pay-1/a"), "{before:?}");
    fx.env.sql(
        "UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z';
         UPDATE workflows SET status = 'dispatch', lead_worktree = '/x/a', started_at = 't' WHERE key = 'a';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
           VALUES ('acme/pay', 1, 'https://github.com/acme/pay/pull/1', 'T', 0, 'open', 'none', 'none', 't', 't');
         UPDATE workflows SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/pay/pull/1', pr_opened_at = 't',
                pull_request = (SELECT id FROM pull_requests WHERE number = 1) WHERE key = 'a';
         UPDATE workflows SET status = 'merged', merged_at = 't' WHERE key = 'a';
         UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE number = 1;",
    );

    let mut s = store(&fx);
    cpd_core::workflow::start::unstack_for_open_job(
        &mut s,
        "test",
        &plan(),
        &cpd_core::ids::WorkflowKey::parse("b").unwrap(),
    )
    .unwrap();
    assert_eq!(base_of(&fx, "b"), "main");

    let spec = render(&fx, "open-b");
    assert_names(&spec, "main", "pay-1/a");
    assert_eq!(stored(&fx, "open-b").0, before.0);
}

#[test]
fn an_open_jobs_worker_spec_names_the_repo_a_pr_edit_set() {
    let fx = crud("plan");
    add(&fx, "b", &fx.pay, &[]);
    let before = stored(&fx, "open-b");
    let (pay, web) = (fx.pay.display().to_string(), fx.web.display().to_string());
    assert!(
        before.0.contains(&format!("in the repo {pay}")) || !before.0.contains(&web),
        "{before:?}"
    );

    fx.cpd(&["workflow", "edit", "b", "--repo", &web, "--plan", "pay-1"])
        .ok();
    assert_eq!(
        fx.env
            .query::<String>("SELECT repo FROM workflows WHERE key = 'b'"),
        web
    );

    let spec = render(&fx, "open-b");
    assert!(
        spec.contains(&format!("in the repo {web}")),
        "the Spec lacks the live repo:\n{spec}"
    );
    assert!(
        !spec.contains(&format!("in the repo {pay}")),
        "the Spec still names the old repo:\n{spec}"
    );
    assert_eq!(stored(&fx, "open-b").0, before.0);
}

#[test]
fn an_update_jobs_worker_spec_names_the_base_a_pr_edit_set() {
    let fx = crud("plan");
    branch(&fx, &fx.pay, "develop");
    add(&fx, "b", &fx.pay, &["--base", "develop"]);
    // The update Job is queued from the PR row BEFORE the edit: its stored text names `develop`.
    let mut s = store(&fx);
    let key = {
        let workflows = s
            .read(|c| cpd_core::plan::fingerprint::load_workflows(c, &plan()))
            .unwrap();
        let pr = workflows
            .iter()
            .find(|p| p.key.as_str() == "b")
            .unwrap()
            .clone();
        s.write("test", |tx| {
            queue_update(
                tx,
                &plan(),
                &pr,
                Tier::parse("H3")?,
                "test",
                Cause::Conflict,
            )
        })
        .unwrap()
        .expect("an update Job")
    };
    let job = key.as_str().to_owned();
    let before = stored(&fx, &job);
    assert!(before.0.contains("develop"), "{before:?}");

    fx.cpd(&["workflow", "edit", "b", "--base", "main", "--plan", "pay-1"])
        .ok();
    assert_eq!(base_of(&fx, "b"), "main");

    let spec = render(&fx, &job);
    assert_names_with(&spec, "main", "develop", "up to date with its base");
    assert_eq!(stored(&fx, &job).0, before.0);
}
