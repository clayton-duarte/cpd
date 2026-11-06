//! Contract §3.6: `blocked` is for human reasons only; the agent labels belong to `waiting`.

use super::row::State;
use super::state::{Label, set};
use crate::error::ErrorCode;
use crate::ids::SessionId;
use crate::store::Store;

const AGENT_LABELS: [&str; 2] = ["φ: Worker question", "φ: question to the Lead"];

/// A Store with Session `s1` (`working`) leading Plan `pay`.
fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home =
        crate::home::CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at)
                   VALUES ('s1', 'lead', 'cpd', 'pay', 'One', '/', 'O3', '%5', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
                   VALUES ('pay', 'cpd', 'Pay', 'grounding', 's1', 't', NULL, NULL);",
            )?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

fn row(store: &mut Store) -> (String, Option<String>) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, state_label FROM sessions WHERE id = 's1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

fn events(store: &mut Store) -> i64 {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM events WHERE kind = 'session-state'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn blocked_with_the_worker_question_label_is_refused_usage() {
    let (_dir, mut store) = store();
    let id = SessionId::parse("s1").unwrap();
    for text in AGENT_LABELS {
        let label: Label = text.parse().unwrap();
        let result = set(&mut store, "test", &id, State::Blocked, Some(&label));
        let code = result.as_ref().err().map(|e| e.code());
        assert_eq!(code, Some(ErrorCode::Usage), "{text}");
        assert_eq!(row(&mut store), ("working".to_owned(), None), "{text}");
        assert_eq!(events(&mut store), 0, "{text}");
    }
}

#[test]
fn waiting_with_the_agent_labels_is_accepted() {
    let (_dir, mut store) = store();
    let id = SessionId::parse("s1").unwrap();
    for text in AGENT_LABELS {
        let label: Label = text.parse().unwrap();
        assert!(set(&mut store, "test", &id, State::Waiting, Some(&label)).is_ok());
        assert_eq!(
            row(&mut store),
            ("waiting".to_owned(), Some(text.to_owned()))
        );
    }
}

#[test]
fn blocked_with_a_human_label_is_still_accepted() {
    let (_dir, mut store) = store();
    let id = SessionId::parse("s1").unwrap();
    for text in ["dialog", "φ: plan review", "φ: loop guard"] {
        let label: Label = text.parse().unwrap();
        assert!(set(&mut store, "test", &id, State::Blocked, Some(&label)).is_ok());
        assert_eq!(
            row(&mut store),
            ("blocked".to_owned(), Some(text.to_owned()))
        );
    }
}
