//! Archive on a Context row whose Manager holds the chat slot (commands.md §13.2 "Archive of the chat slot's Lead", hs-d90):
//! the tree frees the slot (opens the next live Session, else `session park`), then runs `context archive`. In the test
//! process on a `Tree` (`common/tree.rs`) over a prepared store, with a fake tmux whose popup confirms, and a fake `cpd` that
//! records its calls and keeps the CLI's refusal: `context archive` fails with `in_chat_slot` until a `session open` or
//! `session park` ran before it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::common::tree::{self as drive, Tree, engine, fx};
use crate::common::{TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::store::Store;

fn session(id: &str, role: &str, context: &str, plan: Option<&str>, created: &str) -> String {
    let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {plan}, NULL, 'name of {id}', '/sessions/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

fn context(id: &str, plan: &str, lead: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('{plan}', '{id}', 'Plan {plan}', 'grounding', '{lead}', '2026-01-02T00:00:00.000Z');"
    )
}

/// Context `alpha` (Plan `ship`, Lead `l-ship`) whose Manager `m-alpha` holds the chat slot; with `with_beta`, a Context `beta`
/// after it (Plan `rel`, Lead `l-rel`): the next live Session.
fn populate(env: &TestEnv, with_beta: bool) {
    let mut store = Store::open(
        &CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap();
    let mut s = context("alpha", "ship", "l-ship");
    s += &session(
        "l-ship",
        "lead",
        "alpha",
        Some("ship"),
        "2026-01-02T00:00:00.000Z",
    );
    s += &session(
        "m-alpha",
        "manager",
        "alpha",
        None,
        "2026-01-02T00:00:07.000Z",
    );
    if with_beta {
        s += &context("beta", "rel", "l-rel");
        s += &session(
            "l-rel",
            "lead",
            "beta",
            Some("rel"),
            "2026-01-02T00:00:02.000Z",
        );
    }
    s += "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
          VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-moved', 'alpha', NULL, 'm-alpha', 'fixture',
                  '{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}');";
    sql(&mut store, &s);
}

/// The fake `cpd`: `context archive` is refused with `in_chat_slot` unless `session open` or `session park` ran first; its
/// success sets `archived`.
#[derive(Default)]
struct Slot {
    freed: AtomicBool,
    archived: AtomicBool,
}

impl Handler for Slot {
    fn handle(&self, call: &Call) -> Reply {
        match call
            .args
            .iter()
            .take(2)
            .map(String::as_str)
            .collect::<Vec<_>>()[..]
        {
            ["session", "open" | "park"] => {
                self.freed.store(true, Ordering::SeqCst);
                Reply::ok(r#"{"ok":true}"#)
            }
            ["context", "archive"] if self.freed.load(Ordering::SeqCst) => {
                self.archived.store(true, Ordering::SeqCst);
                Reply::ok(r#"{"ok":true}"#)
            }
            ["context", "archive"] => Reply::exit(
                3,
                r#"{"ok":false,"error":{"code":"in_chat_slot","message":"Lead m-alpha is in the chat slot","details":{}}}"#,
                "",
            ),
            _ => Reply::ok(r#"{"ok":true}"#),
        }
    }
}

/// Archive on the Context `alpha`'s row, confirmed; returns once its `context archive` call has been made.
fn archive_alpha(fx: &crate::common::fx::Fx, tree: &mut Tree) {
    tree.until("the tree", |s| s.iter().any(|r| r.contains("alpha")));
    tree.right_click(5, 1); // the Context `alpha`: Archive is the fourth item of its menu
    tree.until("the menu", |s| s.iter().any(|r| r.contains("New Plan…")));
    tree.click(7, 5);
    tree.until_that("the archive call", |_| {
        drive::calls(fx)
            .iter()
            .any(|c| c[..2] == drive::words(&["context", "archive"])[..])
    });
}

/// Archives `alpha` (with a Context `beta` after it when `with_beta`) in a tree with the [`Slot`] for `cpd` and a confirming
/// popup; the `cpd` calls, whether the Context ended archived, and the tree's log.
fn archived_alpha(with_beta: bool) -> (Vec<Vec<String>>, bool, String) {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx.env, with_beta);
    let slot = Arc::new(Slot::default());
    let _ = (*fx.fakes.runner).clone().route("cpd", slot.clone());
    let dialogs = drive::dialogs(&fx);
    dialogs.confirm();
    let mut tree = Tree::start(&fx, 60, 14);
    archive_alpha(&fx, &mut tree);
    tree.until_that("the Context to be archived", |_| {
        slot.archived.load(Ordering::SeqCst)
    });
    (
        drive::calls(&fx),
        slot.archived.load(Ordering::SeqCst),
        tree.log(),
    )
}

#[test]
fn context_archive_frees_the_slot_held_by_its_manager() {
    let (calls, archived, log) = archived_alpha(true);
    assert_eq!(
        calls,
        [
            drive::words(&["session", "open", "l-rel"]),
            drive::words(&["context", "archive", "alpha"])
        ],
        "the next live Session after the Context, then the archive; log: {log}"
    );
    assert!(
        archived,
        "the Context ends archived (not refused in_chat_slot); log: {log}"
    );
}

#[test]
fn context_archive_parks_when_no_other_session_is_live() {
    let (calls, archived, log) = archived_alpha(false);
    assert_eq!(
        calls,
        [
            drive::words(&["session", "park"]),
            drive::words(&["context", "archive", "alpha"])
        ],
        "no other live Session: the slot is parked, then the archive; log: {log}"
    );
    assert!(
        archived,
        "the Context ends archived (not refused in_chat_slot); log: {log}"
    );
}
