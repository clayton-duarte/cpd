//! Red tests for the `guard` Event kind (contract §5.2, Plan `agent-refusals`): the
//! kind that records what an agent tried and cpd refused, from the adapter guard
//! refusals and the agent-caused CLI refusals (commands.md §10.18, §11, §14.7).
//!
//! The `ev-impl` Job adds the kind (a `Kind` row and a `spec()` row in
//! `events/kind.rs`, the `event_kinds` migration and the typed constructor); until
//! then these fail on their assertions, which is the point.

use std::str::FromStr;

use rusqlite::OptionalExtension;
use serde_json::{Value, json};

use super::{Kind, NewEvent, Set, append};
use crate::home::CpdHome;
use crate::ids::SessionId;
use crate::store::Store;

/// The kind's name (§5.2: `guard`).
const GUARD: &str = "guard";

/// The `data` fields every guard Event carries (§5.2): the refusal's own fields, so a
/// ranking per guard, role, model and day is an aggregation over these rows.
const FIELDS: [&str; 6] = ["code", "role", "tier", "model", "tool", "excerpt"];

/// A fresh store in its own temporary home (the migration runner seeds `event_kinds`).
fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, store)
}

/// The kind named `guard`, or a panic naming the registry that is missing it.
fn guard_kind() -> Kind {
    Kind::from_str(GUARD)
        .unwrap_or_else(|e| panic!("the `guard` kind is not registered in Kind (§5.2): {e:?}"))
}

#[test]
fn guard_kind_is_registered() {
    let kind = guard_kind();
    assert_eq!(kind.as_str(), GUARD);
    assert!(
        Kind::ALL.iter().any(|k| k.as_str() == GUARD),
        "Kind::ALL lists the `guard` kind"
    );

    // A kind exists when its `event_kinds` row does (§4.4 K1): one additive migration of one statement.
    let (_dir, mut store) = store();
    let listed: Option<String> = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT name FROM event_kinds WHERE name = ?1",
                [GUARD],
                |r| r.get(0),
            )
            .optional()?)
        })
        .unwrap();
    assert_eq!(
        listed.as_deref(),
        Some(GUARD),
        "a `guard` row in event_kinds"
    );
}

#[test]
fn guard_event_carries_the_refusal_fields() {
    let kind = guard_kind();
    let spec = kind.spec();
    assert_eq!(
        (
            spec.context,
            spec.plan,
            spec.job,
            spec.attempt,
            spec.session
        ),
        (Set::Yes, Set::Maybe, Set::Maybe, Set::Maybe, Set::Yes),
        "a guard Event is about the refusing Session, with its Plan, Job and Attempt when it has one"
    );
    assert_eq!(spec.required, FIELDS, "the guard Event's data (§5.2)");

    let (_dir, mut store) = store();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                   VALUES ('sess', 'worker', 'ctx', 'Worker', '/w', 'H3', 'working', 't', 't');",
            )?;
            Ok(())
        })
        .unwrap();

    let data = json!({
        "code": "explore network write",
        "role": "worker",
        "tier": "H3",
        "model": "github-copilot/gpt-5",
        "tool": "bash",
        "excerpt": "git fetch origin agent-refusals/miner",
    });
    let session = SessionId::parse("sess").unwrap();
    let event = NewEvent::new(
        kind,
        "Worker sess refused: explore network write: scout has no network: git fetch",
        data.clone(),
    )
    .session(session);
    let id = store.write("guard_tests", |tx| append(tx, event)).unwrap();

    let (stored_kind, stored_session, stored_data): (String, Option<String>, String) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT kind, session, data FROM events WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(stored_kind, GUARD);
    assert_eq!(stored_session.as_deref(), Some("sess"));
    let stored: Value = serde_json::from_str(&stored_data).unwrap();
    for field in FIELDS {
        assert_eq!(
            stored.get(field),
            data.get(field),
            "the guard Event carries {field:?}"
        );
    }
}
