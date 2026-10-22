//! A new `plan review` closes the Plan's older pending reviews as `superseded` (contract §2.2, §4.5
//! `review-superseded`; commands.md §4.6, §4.7), run as the built binary against a temporary `$CPD_HOME`.

use crate::common;

use std::path::PathBuf;

use common::{TestEnv, WATCH_OK, refused};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

/// Plan `pay` in phase `plan` with one draft Job `a` and an outline, ready to be reviewed.
fn planned(env: &TestEnv) {
    env.write_tmux_config("");
    env.fake_cpd_panes(WATCH_OK);
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let repo: PathBuf = env.keyed_repo("pay");
    env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok();
    env.github_origin(&repo);
    env.cpd(&["pr", "add", "pay", "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"]).ok();
    env.cpd(&["job", "cancel", "pay-config", "--reason", "a stub of the template", "--plan", "pay"]).ok();
    env.drop_unreachable_origin(&repo);
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    store(env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, created_txn, created_at)
                     VALUES ('pay', 'pay', 'a', 'Job a', 'S2', 'Goal of a.', 'Context of a.', '[\"src/a.rs\"]',
                             '[\"cargo test\"]', '[]', 'draft', 0, {}, 't');",
                    tx.id()
                ),
            )
        })
        .unwrap();
}

fn review(env: &TestEnv) -> Value {
    env.outline_for_review("pay");
    env.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]).ok()
}

fn edit_goal(env: &TestEnv, goal: &str) {
    env.sql(&format!("UPDATE jobs SET goal = '{goal}' WHERE plan = 'pay' AND key = 'a';"));
}

/// `(status, decided_at is set, feedback)` of review `id` of `pay`.
fn row(env: &TestEnv, id: &str) -> (String, bool, Option<String>) {
    (
        env.query(&format!("SELECT status FROM plan_reviews WHERE plan = 'pay' AND id = '{id}'")),
        env.query::<Option<String>>(&format!("SELECT decided_at FROM plan_reviews WHERE plan = 'pay' AND id = '{id}'")).is_some(),
        env.query(&format!("SELECT feedback FROM plan_reviews WHERE plan = 'pay' AND id = '{id}'")),
    )
}

// ---------- tests ----------

#[test]
fn a_new_review_supersedes_the_older_pending_one_with_an_event() {
    let env = TestEnv::new();
    planned(&env);
    let r1 = review(&env);
    assert_eq!(r1["review"]["id"], "r1");
    edit_goal(&env, "A new goal.");
    let r2 = review(&env);
    assert_eq!((&r2["review"]["id"], &r2["review"]["status"]), (&json!("r2"), &json!("pending")));

    assert_eq!(row(&env, "r1"), ("superseded".to_owned(), true, None));
    assert_eq!(row(&env, "r2"), ("pending".to_owned(), false, None));
    let listed = events(&env, "plan-review");
    let data: Vec<&Value> = listed.iter().map(|e| &e["data"]).collect();
    let fingerprint = &r1["review"]["fingerprint"];
    assert_eq!(
        data,
        [
            &json!({"review": "r1", "fingerprint": fingerprint, "status": "pending"}),
            &json!({"review": "r1", "fingerprint": fingerprint, "status": "superseded"}),
            &json!({"review": "r2", "fingerprint": r2["review"]["fingerprint"], "status": "pending"}),
        ]
    );
    let text = listed[1]["text"].as_str().unwrap();
    assert!(text.contains("r1") && text.contains("r2"), "{text}");
    assert_eq!(env.cpd(&["plan", "show", "--plan", "pay"]).ok()["review"]["id"], "r2");
}

#[test]
fn a_new_review_closes_every_older_pending_review() {
    let env = TestEnv::new();
    planned(&env);
    env.outline_for_review("pay");
    // A store as before the fix: two pending reviews.
    env.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at)
         VALUES ('pay', 'r1', 1, '{fp}', 'pending', 't'), ('pay', 'r2', 2, '{fp}', 'pending', 't');",
        fp = "e".repeat(64)
    ));
    let r3 = env.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]).ok();
    assert_eq!((&r3["review"]["id"], &r3["review"]["status"]), (&json!("r3"), &json!("pending")));

    assert_eq!(row(&env, "r1"), ("superseded".to_owned(), true, None));
    assert_eq!(row(&env, "r2"), ("superseded".to_owned(), true, None));
    let listed = events(&env, "plan-review");
    assert_eq!(listed.len(), 3, "{listed:?}");
    let mut closed: Vec<(String, String)> =
        listed[..2].iter().map(|e| (e["data"]["review"].as_str().unwrap().to_owned(), e["data"]["status"].as_str().unwrap().to_owned())).collect();
    closed.sort();
    assert_eq!(closed, [("r1".to_owned(), "superseded".to_owned()), ("r2".to_owned(), "superseded".to_owned())]);
    assert_eq!((&listed[2]["data"]["review"], &listed[2]["data"]["status"]), (&json!("r3"), &json!("pending")));
    let txns: Vec<i64> = env.read(|c| {
        c.prepare("SELECT txn FROM events WHERE kind = 'plan-review' ORDER BY id")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()
    });
    assert_eq!(txns.len(), 3);
    assert!(txns.iter().all(|t| *t == txns[0]), "one transaction: {txns:?}");
}

#[test]
fn a_repeated_request_closes_nothing() {
    let env = TestEnv::new();
    planned(&env);
    review(&env);
    let counts = |env: &TestEnv| (env.query::<i64>("SELECT count(*) FROM txns"), env.query::<i64>("SELECT count(*) FROM events"), env.query::<i64>("SELECT count(*) FROM plan_reviews"));
    let again_of = |env: &TestEnv, id: &str, before: (i64, i64, i64)| {
        let again = env.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]).ok();
        assert_eq!((&again["already"], &again["review"]["id"]), (&json!(true), &json!(id)));
        assert_eq!(counts(env), before, "no row, no Event, no transaction");
    };
    // The same drafts: r1 stays pending.
    let before = counts(&env);
    again_of(&env, "r1", before);
    assert_eq!(row(&env, "r1"), ("pending".to_owned(), false, None));

    // The same drafts again after r2 closed r1: r2 stays pending and r1 stays superseded.
    edit_goal(&env, "A new goal.");
    review(&env);
    assert_eq!(row(&env, "r1").0, "superseded");
    let before = counts(&env);
    again_of(&env, "r2", before);
    assert_eq!((row(&env, "r1").0, row(&env, "r2").0), ("superseded".to_owned(), "pending".to_owned()));
}

#[test]
fn a_superseded_review_cannot_be_decided() {
    let env = TestEnv::new();
    planned(&env);
    review(&env);
    edit_goal(&env, "A new goal.");
    review(&env);
    assert_eq!(row(&env, "r1").0, "superseded");
    let before = (row(&env, "r1"), env.query::<i64>("SELECT count(*) FROM events"));
    for flag in ["--approve", "--reject"] {
        let error = refused(&env.cpd(&["plan", "decide", "r1", flag, "--plan", "pay"]), "review_decided", 3);
        assert_eq!(error["details"]["status"], "superseded");
    }
    assert_eq!((row(&env, "r1"), env.query::<i64>("SELECT count(*) FROM events")), before);
}
