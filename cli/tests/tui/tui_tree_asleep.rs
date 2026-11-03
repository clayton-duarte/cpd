//! Asleep Sessions in `cpd tui view tree` (commands.md §13.2; contract §2.2 "Asleep"; hs-d515, hs-d520): the view runs in
//! the test process on a `Tree` (`common/tree.rs`) over a prepared store, with a fake `cpd` that records its command line.

use crate::common;

use common::fx::Fx;
use common::tree::{self as drive, Tree, engine, fx};

const WHEN: &str = "2026-02-01T00:00:00.000Z";

fn fork(id: &str, extra_column: &str, extra_value: &str, created: &str) -> String {
    // `l-ship` is the Plan's one Lead; the others fork it (contract §4.4)
    let forked_from = if id == "l-ship" { "NULL" } else { "'l-ship'" };
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at, forked_from{extra_column})
         VALUES ('{id}', 'lead', 'alpha', 'ship', NULL, 'name of {id}', '/sessions/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '{created}', {forked_from}{extra_value});"
    )
}

/// Context `alpha` with Plan `ship` (its Lead `l-ship`, idle) and three forks of it: `f-slp` (asleep, the oldest), `f-live` and
/// `f-arc` (archived), all idle. `asleep_at` is the column the Job's migration adds.
fn populate(fx: &Fx) {
    let mut s = String::from(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('alpha', 'Title of alpha', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('ship', 'alpha', 'Plan ship', 'grounding', 'l-ship', '2026-01-02T00:00:00.000Z');",
    );
    s += &fork("l-ship", "", "", "2026-01-02T00:00:00.000Z");
    s += &fork(
        "f-slp",
        ", asleep_at",
        &format!(", '{WHEN}'"),
        "2026-01-02T00:00:01.000Z",
    );
    s += &fork("f-live", "", "", "2026-01-02T00:00:02.000Z");
    s += &fork(
        "f-arc",
        ", archived_at",
        &format!(", '{WHEN}'"),
        "2026-01-02T00:00:03.000Z",
    );
    fx.env.sql(&s);
}

/// The index of the row of the fork labelled `ship·<suffix>`.
fn row_of(screen: &[String], suffix: &str) -> usize {
    screen
        .iter()
        .position(|r| r.contains(&format!("ship·{suffix}")))
        .unwrap_or_else(|| panic!("no row of {suffix}: {screen:?}"))
}

/// A tree of the three forks over a fake `cpd`.
fn start(fx: &Fx) -> Tree {
    drive::fake_cpd(
        fx,
        r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#,
    );
    let tree = Tree::start(fx, 60, 20);
    let screen = tree.screen();
    assert!(
        ["f-sl", "f-li", "f-ar"]
            .iter()
            .all(|f| screen.iter().any(|r| r.contains(f))),
        "the three forks: {screen:?}"
    );
    tree
}

#[test]
fn click_on_asleep_row_resumes_it_in_the_chat_slot() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx);
    let mut tree = start(&fx);
    let row = row_of(&tree.screen(), "f-sl");
    tree.click(8, row as u16);
    assert_eq!(
        drive::calls(&fx),
        [drive::words(&["session", "open", "f-slp"])]
    );
    // no second command follows the click (one action): the clock goes on and nothing runs
    tree.advance(std::time::Duration::from_millis(500));
    assert_eq!(
        drive::calls(&fx),
        [drive::words(&["session", "open", "f-slp"])],
        "one action: no unarchive, no history"
    );
}

#[test]
fn click_on_archived_session_unarchives_and_resumes_it() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx);
    let mut tree = start(&fx);
    let row = row_of(&tree.screen(), "f-ar");
    tree.click(8, row as u16);
    assert_eq!(
        drive::calls(&fx),
        [
            drive::words(&["session", "unarchive", "f-arc"]),
            drive::words(&["session", "open", "f-arc"])
        ],
        "the archived Session is unarchived, then opened: not `session history`"
    );
}
