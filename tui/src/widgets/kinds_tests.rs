//! The Dispatch column follows a Lead's `session-state` Event (its `lead_working` dot): the defect of
//! panel-sync-facts.md, trigger A of panel-sync-latency.md.

use super::KINDS;
use crate::read;
use cpd_core::events::{self, Kind, NewEvent};
use cpd_core::home::CpdHome;
use cpd_core::ids::SessionId;
use cpd_core::store::Store;

#[test]
fn the_dispatch_kinds_include_session_state() {
    assert!(KINDS.contains(&Kind::SessionState), "KINDS lacks Kind::SessionState");
}

#[test]
fn a_session_state_event_makes_the_dispatch_follow_report_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let cursor = store.read(|conn| events::highest_id(conn)).unwrap();
    let mut follow = read::Follow::new(&mut store, KINDS, cursor).unwrap();
    // The first poll always says the pull request rows changed: take it out of the way.
    follow.poll_changes(&mut store).unwrap();
    assert!(!follow.poll_changes(&mut store).unwrap().any(), "nothing was appended yet");

    // Control: a kind in neither the Dispatch list nor the tree's.
    store
        .write("test", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead-1', 'lead', 'ctx', NULL, 'Lead', '/', 'O3', 'idle', 't', 't');",
            )?;
            Ok(())
        })
        .unwrap();
    // The fixture rows are no Event of the Dispatch's; take the cursor past them.
    follow.poll_changes(&mut store).unwrap();
    let session = SessionId::parse("lead-1").unwrap();
    store
        .write("test", |tx| events::append(tx, NewEvent::session_titled("Session titled.", "t").session(session.clone())))
        .unwrap();
    assert!(!follow.poll_changes(&mut store).unwrap().any(), "a session-titled Event is not the Dispatch's");

    store
        .write("test", |tx| events::append(tx, NewEvent::session_state("Session working.", "idle", "working", None, false).session(session.clone())))
        .unwrap();
    assert!(follow.poll_changes(&mut store).unwrap().any(), "a session-state Event must make the Dispatch follow report a change");
}

#[test]
fn the_dispatch_kinds_include_inbox_claimed() {
    assert!(KINDS.contains(&Kind::InboxClaimed), "KINDS lacks Kind::InboxClaimed");
}
