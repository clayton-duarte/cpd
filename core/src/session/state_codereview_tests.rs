//! Decision pro-d391 (A): a Session of Context `codereview` going `idle --reply` on a `done` Plan records no reply.

use super::row::State;
use super::state::set_reply;
use crate::ids::SessionId;
use crate::store::Store;

/// A Store with Session `s1` (`working`) leading Plan `pay` in phase `phase` of Context `context`.
fn store(context: &str, phase: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home =
        crate::home::CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let locked = if phase == "grounding" { "NULL" } else { "'t'" };
    let done = if phase == "done" { "'t'" } else { "NULL" };
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('{context}', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at)
                   VALUES ('s1', 'lead', '{context}', 'pay', 'One', '/', 'O3', '%5', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
                   VALUES ('pay', '{context}', 'Pay', '{phase}', 's1', 't', {locked}, {done});"
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// Whether the `session-state` Event of going idle with `--reply` carries `reply: true`.
fn idle_reply_recorded(context: &str, phase: &str) -> bool {
    let (_dir, mut store) = store(context, phase);
    let id = SessionId::parse("s1").unwrap();
    set_reply(&mut store, "test", &id, State::Idle, None, true).unwrap();
    let data: String = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT data FROM events WHERE kind = 'session-state' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let data: serde_json::Value = serde_json::from_str(&data).unwrap();
    data["reply"] == serde_json::Value::Bool(true)
}

#[test]
fn idle_reply_on_a_done_codereview_plan_records_no_reply() {
    assert!(!idle_reply_recorded("codereview", "done"));
}

#[test]
fn idle_reply_on_a_working_codereview_plan_still_records_reply() {
    assert!(idle_reply_recorded("codereview", "grounding"));
}

#[test]
fn idle_reply_on_a_done_plan_of_another_context_still_records_reply() {
    assert!(idle_reply_recorded("cpd", "done"));
}
