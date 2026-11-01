//! The `loop-guard` Inbox item in the tree view (commands.md §13.2, §21; hs-d552, hs-d571) over a prepared store, drawn
//! on a `TestBackend`: the glyph and the dot are read from the cells. The click and the dot are unit-tested in `inbox.rs`.

use ratatui::buffer::Buffer;

use super::prepared_tests::{Env, draw, text_rows};
use crate::palette::NEEDS_YOU;
use crate::palette::numbered::C9;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};

/// Context `alpha` with its Plan `sketch`, whose Lead `l-sketch` is blocked by the loop guard, and the open `kind` item the
/// engine's E8 would open for it: red, linked to that Session. An engine runs (`engine-down` is no row of the Inbox).
fn tripped(kind: Kind) -> Env {
    let mut env = Env::new();
    env.sql(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('alpha', 'Title of alpha', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('sketch', 'alpha', 'Plan sketch', 'plan', 'l-sketch', '2026-01-02T00:00:01.000Z', NULL, NULL);
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
         VALUES ('sketch', 'repo', '/repo/sketch', 'main', 'waiting', 'cpd/sketch', NULL, NULL, NULL, NULL,
                 NULL, cpd_txn(), '2026-01-02T00:00:01.000Z', NULL, NULL);
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('l-sketch', 'lead', 'alpha', 'sketch', NULL, 'sketch', '/sessions/l-sketch.jsonl', '/repo', 'O3',
                 NULL, 'blocked', 'φ: loop guard', '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:01.000Z');",
    );
    let item = Condition {
        kind,
        urgency: Urgency::Red,
        subject: "session:l-sketch".into(),
        label: "sketch: loop guard tripped".into(),
        url: None,
        context: Some(ContextId::parse("alpha").unwrap()),
        plan: Some(PlanId::parse("sketch").unwrap()),
        job: None,
        session: Some(SessionId::parse("l-sketch").unwrap()),
        record: None,
        event: None,
        opened_at: None,
    };
    reconcile(env.store(), &[item], "2026-01-03T00:00:00.000Z").expect("the store holds the item");
    env
}

/// The tree in a 60 x 14 pane.
fn screen_of(env: &mut Env) -> Buffer {
    draw(&mut env.view(), 60, 14)
}

#[test]
fn a_loop_guard_item_fills_its_sessions_tree_dot_red() {
    let mut env = tripped(Kind::LoopGuard);
    let buf = screen_of(&mut env);
    let rows = text_rows(&buf);
    assert!(
        rows.iter()
            .any(|l| l.contains(&format!("{NEEDS_YOU} sketch"))),
        "the Lead's row: {rows:?}"
    );
    let y = rows
        .iter()
        .position(|l| l.contains("sketch") && !l.contains("loop guard"))
        .expect("the Lead's row") as u16;
    let dot = (0..buf.area.width)
        .find(|&x| buf[(x, y)].symbol() == NEEDS_YOU)
        .expect("the filled dot");
    assert_eq!(
        buf[(dot, y)].fg,
        C9,
        "the filled dot in red (colour 9): {:?}",
        rows[y as usize]
    );
}
