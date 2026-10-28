//! E3 and the Jira ticket of a PR (contract §3.7 E3, §5.2 `error`; Plan pr-ticket): when the PR's policy sets `pr.jira_required`, an
//! `open` Job whose PR has no `jira` and no `head_ref` is never started; E3 raises one `jira_required` error Event for it, once, and
//! starts it at the first tick after the key is set. An adopted PR and a policy that leaves the key false need none.

use cpd_core::ports::Fakes;
use serde_json::json;

use super::schedule;
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

/// The owner `acme`'s policy file (the Context's owner), its `[pr]` table holding `pr_body`.
fn owner_policy(fx: &Fx, pr_body: &str) {
    let dir = fx.engine.home.root().join("policy/acme");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("owner.toml"), format!("[pr]\n{pr_body}")).unwrap();
}

/// PR `repo` with Job `api` merged and the PR's Job `key` of `kind` queued: Ready. The PR is described and titled; its `jira` and
/// head_ref are NULL; the owner's policy holds `policy` in its `[pr]` table.
fn with_open_job(key: &str, kind: &str, policy: &str) -> Fx {
    let mut fx = Fx::with_plan("");
    fx.add_job("api", &[], "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'api';");
    fx.fixture(&format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
         VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');
         UPDATE prs SET description = 'A description';"
    ));
    fx.lock_job(key, "");
    owner_policy(&fx, policy);
    fx
}

#[test]
fn e3_does_not_start_an_open_job_whose_pr_has_no_jira_when_required_raises_one_error_and_starts_it_once_set() {
    let mut fx = with_open_job("open-repo", "open", "jira_required = true\n");

    ticks(&mut fx, 3);

    assert_eq!(attempts(&mut fx), 0, "no Attempt for a PR with no ticket");
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let (plan, job, attempt, data) = &errors[0];
    assert_eq!((plan.as_deref(), job.as_deref(), attempt.as_deref()), (Some("pay"), Some("open-repo"), None));
    assert_eq!((&data["source"], &data["code"]), (&json!("watch"), &json!("jira_required")), "{data}");
    assert!(texts(&mut fx, "error")[0].contains("cpd pr edit repo --jira"), "{:?}", texts(&mut fx, "error"));
    assert_eq!(fx.job_status("open-repo"), "queued");

    fx.fixture("UPDATE prs SET jira = 'ABC-1';");
    tick(&mut fx);

    assert_eq!(attempts(&mut fx), 1, "ticketed: started");
    assert_eq!(fx.count("error"), 1, "no further error: {:?}", texts(&mut fx, "error"));
}

/// Guard: a policy that leaves the key false never needs a ticket.
#[test]
fn e3_starts_the_open_job_of_a_pr_with_no_jira_when_the_policy_does_not_require_one() {
    let mut fx = with_open_job("open-repo", "open", "jira_required = false\n");
    ticks(&mut fx, 2);
    assert_eq!(attempts(&mut fx), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}

/// Guard: a policy without the key is the same as false.
#[test]
fn e3_starts_the_open_job_of_a_pr_with_no_jira_when_the_policy_has_no_key() {
    let mut fx = with_open_job("open-repo", "open", "");
    ticks(&mut fx, 2);
    assert_eq!(attempts(&mut fx), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}

/// Guard: an adopted PR (`head_ref` set) needs no ticket.
#[test]
fn e3_starts_the_open_job_of_an_adopted_pr_with_no_jira_when_required() {
    let mut fx = with_open_job("open-repo", "open", "jira_required = true\n");
    fx.fixture("DROP TRIGGER prs_head_ref_fixed; UPDATE prs SET head_ref = 'feature';");
    ticks(&mut fx, 2);
    assert_eq!(attempts(&mut fx), 1);
    assert_eq!(fx.count("error"), 0, "{:?}", texts(&mut fx, "error"));
}
