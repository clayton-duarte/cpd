//! The reaction queue's link to its PR and its refusal (contract §4.4 `slack_reactions`; migration `slack-reaction-refusal`):
//! `plan` and `pr` name the PR whose merge a reaction is, `refused_at` and `refusal` park a reaction Slack refused.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;
use serde_json::Value;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn message(result: Result<(), CpdError>, what: &str) -> String {
    result.expect_err(what).details()["sqlite_message"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

const PLAN: &str =
    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
       VALUES ('pay', 'ctx', 'Pay', 'deploy', 'pay-lead', 't', 't');";

fn pr(key: &str, number: u32, reaction: &str) -> String {
    format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', {number}, 'https://github.com/acme/pay/pull/{number}', 'PR {key}', 0, 'merged', 'none', 'none', 't', '2026-10-01T10:00:00.000Z', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
           VALUES ('pay', '{key}', '/nowhere', 'main', 'merged', 'cpd/pay/{key}', '/nowhere/lead', {number}, 'https://github.com/acme/pay/pull/{number}', 't',
                   (SELECT id FROM pull_requests WHERE number = {number}), cpd_txn(), 't', 't', '2026-10-01T10:00:00.000Z');
         INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction)
           VALUES ('pay', '{key}', 'C0POST', '1793040000.00000{number}', 'https://acme.slack.com/p{number}', 't', {reaction});"
    )
}

fn before_refusal() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-slack-reaction-refusal"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "slack-reaction-refusal is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    let sql = format!(
        "{PLAN}{}{}{}
         INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at) VALUES ('C0POST', '1793040000.000001', 'git-merge', 'q1', NULL);
         INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at) VALUES ('C0POST', '1793040000.000002', 'git-merge', 'q2', 'r2');
         INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at) VALUES ('C0POST', '1793040000.000001', 'eyes', 'q3', NULL);
         INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at) VALUES ('C0POST', '1793040000.000099', 'git-merge', 'q4', NULL);",
        pr("a", 1, "'git-merge'"),
        pr("b", 2, "'git-merge'"),
        pr("c", 3, "NULL"),
    );
    write(&mut store, &sql).unwrap();
    drop(store);
    (dir, home)
}

fn rows(store: &mut Store) -> Vec<Value> {
    store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT json_object('ts', ts, 'emoji', emoji, 'queued_at', queued_at, 'reacted_at', reacted_at,
                                    'plan', plan, 'pr', pr, 'refused_at', refused_at, 'refusal', refusal)
                   FROM slack_reactions ORDER BY id",
            )?;
            Ok(stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
        .iter()
        .map(|r| serde_json::from_str(r).unwrap())
        .collect()
}

#[test]
fn a_database_with_queued_merge_reactions_migrates_to_linked_rows() {
    let (_dir, home) = before_refusal();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    let row = |ts: &str, emoji: &str, q: &str, reacted: Value, plan: Value, pr: Value| serde_json::json!({"ts": ts, "emoji": emoji, "queued_at": q, "reacted_at": reacted, "plan": plan, "pr": pr, "refused_at": null, "refusal": null});
    let expected = vec![
        row(
            "1793040000.000001",
            "git-merge",
            "q1",
            Value::Null,
            "pay".into(),
            "a".into(),
        ),
        row(
            "1793040000.000002",
            "git-merge",
            "q2",
            "r2".into(),
            "pay".into(),
            "b".into(),
        ),
        row(
            "1793040000.000001",
            "eyes",
            "q3",
            Value::Null,
            Value::Null,
            Value::Null,
        ),
        row(
            "1793040000.000099",
            "git-merge",
            "q4",
            Value::Null,
            Value::Null,
            Value::Null,
        ),
    ];
    assert_eq!(
        rows(&mut store),
        expected,
        "the rows matching a post by channel, ts and emoji are linked; the others stay unlinked"
    );
    drop(store);
    let mut again = Store::open(&home, 5000).unwrap();
    assert_eq!(
        rows(&mut again),
        expected,
        "applying the migrations again changes nothing"
    );
}

#[test]
fn slack_reactions_refuses_a_half_linked_or_half_refused_row() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    write(&mut store, &format!("{PLAN}{}", pr("a", 1, "'git-merge'"))).unwrap();
    let insert = |columns: &str, values: &str| {
        format!(
            "INSERT INTO slack_reactions (channel, ts, emoji, queued_at, {columns}) VALUES ('C0X', '1793040000.000001', 'e', 'q', {values});"
        )
    };
    for (columns, values, what) in [
        ("plan", "'pay'", "plan without pr"),
        ("pr", "'a'", "pr without plan"),
        ("refused_at", "'t'", "refused_at without refusal"),
        ("refusal", "'invalid_name'", "refusal without refused_at"),
        (
            "reacted_at, refused_at, refusal",
            "'r', 't', 'invalid_name'",
            "reacted and refused",
        ),
    ] {
        let text = message(write(&mut store, &insert(columns, values)), what);
        assert!(text.contains("CHECK"), "{what}: {text}");
    }
    write(&mut store, &insert("plan, pr", "'pay', 'a'")).expect("a linked row");
    write(
        &mut store,
        &insert("refused_at, refusal", "'t', 'invalid_name'").replace("'e'", "'f'"),
    )
    .expect("a refused row");
}
