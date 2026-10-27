//! E3 and the description of a PR (contract §3.7 E3, §5.2 `error`; Plan desc-gate): an `open` Job whose PR has no stored
//! description and no `head_ref` is never started; E3 raises one `description_required` error Event for it, once, and starts it at the
//! first tick after the PR is described. An adopted PR (`head_ref` set) and an `update` Job need no description.

use cpd_core::ports::Fakes;
use serde_json::json;

use super::{open, schedule};
use super::testkit::Fx;

/// One tick of E3 on the fake ports: a started Attempt records its rows and launches nothing real.
fn tick(fx: &mut Fx) {
    let fakes = Fakes::new();
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    fx.engine.ports = fakes.ports.clone();
    schedule::run(&mut fx.engine);
}

fn ticks(fx: &mut Fx, n: usize) {
    for _ in 0..n {
        tick(fx);
    }
}

fn attempts(fx: &mut Fx) -> i64 {
    fx.query("SELECT count(*) FROM attempts")
}

fn texts(fx: &mut Fx, kind: &str) -> Vec<String> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT text FROM events WHERE kind = ?1 ORDER BY id")?;
            Ok(stmt.query_map([kind], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// PR `repo` with Job `api` merged and the PR's Job `key` of `kind` queued: Ready. The PR's description and head_ref are NULL.
fn with_open_job(key: &str, kind: &str) -> Fx {
    let mut fx = Fx::with_plan("");
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    fx.fixture(&format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
         VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');"
    ));
    fx.lock_job(key, "");
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM prs WHERE description IS NULL AND head_ref IS NULL"), 1);
    fx
}

#[test]
fn e3_does_not_start_an_open_job_whose_pr_has_no_description_raises_one_error_and_starts_it_once_described() {
    let mut fx = with_open_job("open-repo", "open");

    ticks(&mut fx, 3);

    assert_eq!(attempts(&mut fx), 0, "no Attempt for an undescribed PR");
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan.as_deref(), job.as_deref(), attempt.as_deref()), (Some("pay"), Some("open-repo"), None));
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("description_required")), "{data}");
    assert!(texts(&mut fx, "error")[0].contains("cpd pr describe --pr repo"), "{:?}", texts(&mut fx, "error"));
    assert_eq!(fx.job_status("open-repo"), "queued");

    fx.fixture("UPDATE prs SET description = 'A description';");
    tick(&mut fx);

    assert_eq!(attempts(&mut fx), 1, "described: started");
    assert_eq!(fx.count("error"), 1, "no further error: {:?}", texts(&mut fx, "error"));
}

/// Guard: an adopted PR keeps its own pull request body; its open Job starts.
#[test]
fn e3_starts_the_open_job_of_an_adopted_pr_without_a_stored_description() {
    let mut fx = with_open_job("open-repo", "open");
    fx.fixture("DROP TRIGGER prs_head_ref_fixed; UPDATE prs SET head_ref = 'feature';");
    ticks(&mut fx, 2);
    assert_eq!(attempts(&mut fx), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}

/// Guard: a described PR starts its open Job.
#[test]
fn e3_starts_the_open_job_of_a_described_pr() {
    let mut fx = with_open_job("open-repo", "open");
    fx.fixture("UPDATE prs SET description = 'A description';");
    ticks(&mut fx, 2);
    assert_eq!(attempts(&mut fx), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}

/// Pin (Plan bifrost, PR haiku55): the refused open Job stays queued, so E4 adds no second open Job.
#[test]
fn an_undescribed_pr_keeps_one_open_job_and_no_attempt_over_eleven_e3_and_e4_ticks() {
    let mut fx = with_open_job("open-repo", "open");

    for _ in 0..11 {
        tick(&mut fx);
        open::run(&mut fx.engine);
    }

    assert_eq!(fx.query::<i64>("SELECT count(*) FROM jobs WHERE plan = 'pay' AND kind = 'open'"), 1);
    assert_eq!(attempts(&mut fx), 0);
    assert_eq!(fx.job_status("open-repo"), "queued");
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].3["code"], json!("description_required"));
}
