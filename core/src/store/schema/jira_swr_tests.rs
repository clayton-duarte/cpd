//! The migration `jira-swr` (docs/architecture/swr-jira.md §12; breaking): the open rows of `jira_transitions` become `jira`
//! `transition` rows of `swr_mutations`, and the table is dropped.

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

const PLAN: &str =
    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
       VALUES ('pay', 'ctx', 'Pay', 'deploy', 'pay-lead', 't', 't');";

fn workflow(key: &str, jira: &str) -> String {
    format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, jira)
           VALUES ('pay', '{key}', '/nowhere', 'main', 'dispatch', 'cpd/pay/{key}', '/nowhere/lead', cpd_txn(), 't', 't', {jira});"
    )
}

/// Workflow `a` has a ticket, `b` has none, `c` has one and is parked.
fn rows() -> String {
    format!(
        "{PLAN}{}{}{}
         INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', 'a', 'in_progress', 'q1');
         INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', 'a', 'in_review', 'q2');
         INSERT INTO jira_transitions (plan, pr, target, queued_at, settled_at, outcome) VALUES ('pay', 'a', 'done', 'q3', 's', 'moved');
         INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('pay', 'b', 'in_progress', 'q4');
         INSERT INTO jira_transitions (plan, pr, target, queued_at, refused_at, refusal, refused_with)
           VALUES ('pay', 'c', 'done', 'q5', 'r', 'no_transition', 'ABC-3|Done');",
        workflow("a", "'ABC-1'"),
        workflow("b", "NULL"),
        workflow("c", "'ABC-3'"),
    )
}

fn before() -> (tempfile::TempDir, CpdHome) {
    let (dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-jira-swr"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "jira-swr is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    write(&mut store, &rows()).unwrap();
    drop(store);
    (dir, home)
}

type Row = (Option<String>, String, String, Option<String>, String, Option<String>);

#[test]
fn a_database_before_jira_swr_keeps_its_open_transitions_as_outbox_rows_and_loses_the_table() {
    let (_dir, home) = before();
    let mut store = Store::open(&home, 5000).unwrap();
    assert!(
        store.schema_mismatch().is_none(),
        "{:?}",
        store.schema_mismatch()
    );
    let tables: i64 = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'jira_transitions'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(tables, 0);
    let rows: Vec<Row> = store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT target_key, json_extract(vars_json, '$.target'), status, json_extract(context_json, '$.refused_with'),
                        json_extract(context_json, '$.workflow'), error
                 FROM swr_mutations WHERE system = 'jira' AND op_kind = 'transition' ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    let key = |t: &str| Some(format!("jira:issue:{t}"));
    assert_eq!(
        rows,
        vec![
            (key("ABC-1"), "in_progress".into(), "superseded".into(), None, "a".into(), None),
            (key("ABC-1"), "in_review".into(), "pending".into(), None, "a".into(), None),
            (
                key("ABC-3"),
                "done".into(),
                "failed".into(),
                Some("ABC-3|Done".into()),
                "c".into(),
                Some("no_transition".into())
            ),
            (None, "in_progress".into(), "pending".into(), None, "b".into(), None),
        ],
        "the settled row is not copied; the ticketless one waits with no key; the lower pending rank is superseded"
    );
}
