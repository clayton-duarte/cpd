//! The ticketless rows of the `jira` source (swr-jira.md §7): a Workflow with no ticket queues rows that wait with no key,
//! and setting the ticket queues them again under it.

use super::jira;
use crate::clock::{Clock, ManualClock};
use crate::swr::outbox;
use crate::swr::outbox_tests::store;

const FIXTURE: &str = "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
       VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
       VALUES ('pay', 'api', '/nowhere', 'main', 'dispatch', 'cpd/pay/api', '/nowhere/api', cpd_txn(), 't', 't');";

#[test]
fn a_ticketless_row_waits_and_is_queued_again_under_the_ticket_when_it_is_set() {
    let (_dir, mut store) = store();
    let now = ManualClock::new().utc();
    store
        .write("fixture", |tx| {
            tx.execute_batch(FIXTURE)?;
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
    store
        .write("sites", |tx| {
            jira::enqueue_for_workflow(tx, "pay", "api", "in_progress")?;
            jira::enqueue_for_workflow(tx, "pay", "api", "in_progress")?;
            jira::enqueue_for_workflow(tx, "pay", "api", "in_review")
        })
        .unwrap();
    assert!(
        outbox::claim_next(&mut store, now, "jira")
            .unwrap()
            .is_none(),
        "a row with no ticket is never claimed"
    );

    store
        .write("ticket", |tx| {
            tx.execute("UPDATE workflows SET jira = 'ABC-1' WHERE key = 'api'", [])?;
            jira::adopt_ticket(tx, "pay", "api", "ABC-1")
        })
        .unwrap();
    let claimed = outbox::claim_next(&mut store, now, "jira")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.target_key, "jira:issue:ABC-1");
    assert!(claimed.vars_json.contains("in_review"), "{claimed:?}");
    assert!(
        outbox::claim_next(&mut store, now, "jira")
            .unwrap()
            .is_none(),
        "the lower rank was superseded by the forward rule: one row runs"
    );
}
