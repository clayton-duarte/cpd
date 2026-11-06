//! The migration `swr-slack` (swr-slack.md §9): the open rows of the retired `slack_reactions` queue become `reaction` mutations of
//! the outbox, a parked one a `failed` mutation, a reacted one is dropped; the queue is left empty.

use super::*;
use crate::error::CpdError;
use crate::store::Store;

#[test]
fn open_reactions_move_to_the_outbox_and_the_queue_is_emptied() {
    let (_dir, home) = crate::store::tests::tmp_home();
    let earlier: Vec<Named> = NAMED
        .iter()
        .copied()
        .take_while(|n| !n.name.ends_with("-swr-slack"))
        .collect();
    assert!(
        earlier.len() < NAMED.len(),
        "swr-slack is a built-in migration"
    );
    let mut store = crate::store::tests::open_named(&home, &earlier).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'deploy', 'pay-lead', 't', 't');
                 INSERT INTO slack_reactions (channel, ts, emoji, queued_at) VALUES ('C0POST', '1.1', 'eyes', 'q1');
                 INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at) VALUES ('C0POST', '2.2', 'eyes', 'q2', 'r2');
                 INSERT INTO slack_reactions (channel, ts, emoji, queued_at, plan, workflow) VALUES ('C0POST', '3.3', 'git-merge', 'q3', 'pay', 'api');
                 INSERT INTO slack_reactions (channel, ts, emoji, queued_at, plan, workflow, refused_at, refusal)
                   VALUES ('C0POST', '4.4', 'nope', 'q4', 'pay', 'b', 'f4', 'invalid_name');",
            )?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    drop(store);

    let mut store = Store::open(&home, 5000).unwrap();

    let rows: Vec<(String, String, String, String, Option<String>)> = store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT target_key, op_kind, vars_json, status, error FROM swr_mutations WHERE system = 'slack' ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    let key = "slack:channel:C0POST".to_owned();
    assert_eq!(
        rows,
        vec![
            (key.clone(), "reaction".into(), r#"{"channel":"C0POST","emoji":"eyes","ts":"1.1"}"#.into(), "pending".into(), None),
            (key.clone(), "reaction".into(), r#"{"channel":"C0POST","emoji":"git-merge","plan":"pay","ts":"3.3","workflow":"api"}"#.into(), "pending".into(), None),
            (key, "reaction".into(), r#"{"channel":"C0POST","emoji":"nope","plan":"pay","ts":"4.4","workflow":"b"}"#.into(), "failed".into(), Some("invalid_name".into())),
        ],
        "open rows are pending, the refused one failed with its refusal, the reacted one is gone"
    );
    let left: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM slack_reactions", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(left, 0, "the retired queue is empty");
}
