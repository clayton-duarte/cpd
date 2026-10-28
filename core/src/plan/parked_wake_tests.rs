//! The wake state of parked drafts (contract §3.7 E20, hs-d731) over an in-memory store.

use super::parked_wake::{prefix, wake_state};
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::store::Store;

fn plan() -> PlanId {
    PlanId::parse("pl").unwrap()
}

/// A store with Plan `pl` in `dispatch`, its Lead `lead` and one stub draft `k`, then `extra` SQL.
fn store(extra: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('pl', 'ctx', 'P', 'dispatch', 'lead', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pl', 'r', '/r', 'main', 'dispatch', 'cpd/pl', '/w', cpd_txn(), 't', 't');
                 INSERT INTO jobs (plan, pr, key, title, kind, status, appended, created_txn, created_at)
                   VALUES ('pl', 'r', 'k', 'T', 'work', 'draft', 1, cpd_txn(), 't');
                 {extra}"
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// SQL for an Event of `kind` of Plan `pl` (and Session `lead`), with `data`.
fn event(kind: &str, data: &str) -> String {
    format!("INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES (cpd_txn(), 't', '{kind}', 'ctx', 'pl', 'lead', 'x', '{data}');")
}

fn wake_message() -> String {
    event("session-message", &format!(r#"{{"from": null, "text": "{}k (x)", "wake": true}}"#, prefix(&plan())))
}

#[test]
fn no_event_no_wake() {
    let (_d, mut s) = store("");
    let w = s.read(|c| wake_state(c, &plan(), "lead")).unwrap();
    assert_eq!((w.parked, w.t, w.wake, w.finished_turn_after_wake), (vec!["k".to_owned()], 0, None, false));
}

#[test]
fn t_is_the_newest_changing_event_and_w_the_newest_wake() {
    let (_d, mut s) = store(&format!("{}{}{}", event("job-added", "{}"), wake_message(), event("inbox-opened", "{}")));
    let w = s.read(|c| wake_state(c, &plan(), "lead")).unwrap();
    let (t, wake) = (w.t, w.wake.unwrap());
    assert!(t > 0 && wake > t, "t {t}, wake {wake}: a message and an Event of another kind do not move T");
}

#[test]
fn a_message_from_a_session_or_with_another_text_is_no_wake() {
    let other = event("session-message", r#"{"from": "x", "text": "Drafts of Plan pl are parked: k", "wake": true}"#);
    let text = event("session-message", r#"{"from": null, "text": "hello", "wake": true}"#);
    let (_d, mut s) = store(&format!("{other}{text}"));
    assert_eq!(s.read(|c| wake_state(c, &plan(), "lead")).unwrap().wake, None);
}

#[test]
fn a_finished_turn_counts_only_after_the_wake() {
    let before = event("session-state", r#"{"from": "working", "to": "idle", "label": null}"#);
    let (_d, mut s) = store(&format!("{before}{}", wake_message()));
    assert!(!s.read(|c| wake_state(c, &plan(), "lead")).unwrap().finished_turn_after_wake);
    let (_d, mut s) = store(&format!("{}{before}", wake_message()));
    assert!(s.read(|c| wake_state(c, &plan(), "lead")).unwrap().finished_turn_after_wake);
    let stopped = event("session-state", r#"{"from": "stopped", "to": "idle", "label": null}"#);
    let (_d, mut s) = store(&format!("{}{stopped}", wake_message()));
    assert!(!s.read(|c| wake_state(c, &plan(), "lead")).unwrap().finished_turn_after_wake);
}
