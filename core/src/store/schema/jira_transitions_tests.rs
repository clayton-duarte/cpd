//! The Jira transition queue (contract §4.4 `jira_transitions`; migration `jira-transitions`):
//! one row per PR and target, PENDING, SETTLED or PARKED, never a history table.

use super::*;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

fn write(store: &mut Store, sql: &str) -> Result<(), CpdError> {
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok::<(), CpdError>(())
    })
}

fn message(result: Result<(), CpdError>, what: &str) -> String {
    result.expect_err(what).details()["sqlite_message"].as_str().unwrap_or_default().to_owned()
}

const PLAN: &str = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
       VALUES ('pay', 'ctx', 'Pay', 'deploy', 'pay-lead', 't', 't');";

/// A dispatched object, after `workflow-rename`: the table is `workflows`.
fn workflow(key: &str, jira: &str) -> String {
    format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, jira)
           VALUES ('pay', '{key}', '/nowhere', 'main', 'dispatch', 'cpd/pay/{key}', '/nowhere/lead', cpd_txn(), 't', 't', {jira});"
    )
}

fn two_prs() -> String {
    format!("{PLAN}{}{}", workflow("a", "'ABC-1'"), workflow("b", "NULL"))
}

/// The same two objects before `workflow-rename`: the table is `prs`.
fn workflow_before(key: &str, jira: &str) -> String {
    format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, jira)
           VALUES ('pay', '{key}', '/nowhere', 'main', 'dispatch', 'cpd/pay/{key}', '/nowhere/lead', cpd_txn(), 't', 't', {jira});"
    )
}

fn two_prs_before() -> String {
    format!("{PLAN}{}{}", workflow_before("a", "'ABC-1'"), workflow_before("b", "NULL"))
}

fn count(store: &mut Store, sql: &str) -> i64 {
    store.read(|c| Ok(c.query_row(sql, [], |r| r.get::<_, i64>(0))?)).unwrap()
}

fn before_jira_transitions() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED.iter().copied().take_while(|n| !n.name.ends_with("-jira-transitions")).collect();
    assert!(earlier.len() < NAMED.len(), "jira-transitions is a built-in migration");
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(&mut store, &two_prs_before()).unwrap();
    drop(store);
    (dir, home)
}

fn insert(columns: &str, values: &str) -> String {
    format!("INSERT INTO jira_transitions (plan, pr, target, queued_at, {columns}) VALUES ('pay', 'a', 'in_progress', 'q', {values});")
}

#[test]
fn a_database_before_jira_transitions_migrates_to_an_empty_queue() {
    let (_dir, home) = before_jira_transitions();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    assert_eq!(count(&mut store, "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'jira_transitions'"), 1);
    assert_eq!(count(&mut store, "SELECT count(*) FROM jira_transitions"), 0);
    assert_eq!(count(&mut store, "SELECT count(*) FROM workflows WHERE plan = 'pay'"), 2);
    assert_eq!(count(&mut store, "SELECT count(*) FROM workflows WHERE key = 'a' AND jira = 'ABC-1'"), 1);
    assert_eq!(count(&mut store, "SELECT count(*) FROM workflows WHERE key = 'b' AND jira IS NULL"), 1);
    assert_eq!(count(&mut store, "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'jira_transitions'"), 0, "not a history table");
    assert_eq!(count(&mut store, "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE '%jira_transitions%'"), 0);
}

#[test]
fn jira_transitions_refuses_an_unknown_target_a_half_settled_row_a_half_parked_row_and_a_row_both_settled_and_parked() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    write(&mut store, &two_prs()).unwrap();
    let bad = |sql: String, what: &str, store: &mut Store| {
        let text = message(write(store, &sql), what);
        assert!(text.contains("CHECK") || text.contains("FOREIGN KEY"), "{what}: {text}");
    };
    bad("INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', 'a', 'merged', 'q');".into(), "unknown target", &mut store);
    bad(insert("settled_at", "'s'"), "settled_at without outcome", &mut store);
    bad(insert("outcome", "'moved'"), "outcome without settled_at", &mut store);
    bad(insert("refused_at, refusal", "'t', 'no_transition'"), "refused_at without refused_with", &mut store);
    bad(insert("refused_at, refused_with", "'t', 'ABC-1|Done'"), "refused_at without refusal", &mut store);
    bad(insert("refusal, refused_with", "'no_transition', 'ABC-1|Done'"), "refusal without refused_at", &mut store);
    bad(insert("refused_with", "'ABC-1|Done'"), "refused_with alone", &mut store);
    bad(insert("settled_at, outcome, refused_at, refusal, refused_with", "'s', 'moved', 't', 'r', 'ABC-1|Done'"), "settled and parked", &mut store);
    bad(insert("settled_at, outcome", "'s', 'deleted'"), "unknown outcome", &mut store);
    bad(
        "INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', 'nope', 'done', 'q');".into(),
        "no such PR",
        &mut store,
    );
    assert_eq!(count(&mut store, "SELECT count(*) FROM jira_transitions"), 0);
    write(&mut store, "INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', 'a', 'in_progress', 'q');").expect("a PENDING row");
    write(&mut store, &insert("settled_at, outcome", "'s', 'moved'").replace("'in_progress'", "'in_review'")).expect("a SETTLED row");
    write(&mut store, &insert("refused_at, refusal, refused_with", "'t', 'no_transition', 'ABC-1|Done'").replace("'in_progress'", "'done'")).expect("a PARKED row");
    assert_eq!(count(&mut store, "SELECT count(*) FROM jira_transitions"), 3);
}

#[test]
fn jira_transitions_keeps_one_row_per_pr_and_target() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(store.schema_mismatch().is_none(), "{:?}", store.schema_mismatch());
    write(&mut store, &two_prs()).unwrap();
    let row = |pr: &str, target: &str| format!("INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', '{pr}', '{target}', 'q');");
    write(&mut store, &row("a", "done")).expect("first row");
    let text = message(write(&mut store, &row("a", "done")), "same plan, pr and target");
    assert!(text.contains("UNIQUE"), "{text}");
    let again = row("a", "done").replace("'q'", "'later'").replace(';', " ON CONFLICT DO NOTHING;");
    write(&mut store, &again).expect("ON CONFLICT DO NOTHING");
    assert_eq!(count(&mut store, "SELECT count(*) FROM jira_transitions"), 1);
    assert_eq!(count(&mut store, "SELECT count(*) FROM jira_transitions WHERE queued_at = 'q'"), 1, "the first row is untouched");
    write(&mut store, &row("b", "done")).expect("same target, another PR");
    write(&mut store, &row("a", "in_review")).expect("another target, same PR");
    assert_eq!(count(&mut store, "SELECT count(*) FROM jira_transitions"), 3);
}
