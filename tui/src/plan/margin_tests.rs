//! The chart's boxes keep one column of margin from the pane's edge (THE BOX RULE, hs-d578), through the production path:
//! [`snapshot`] builds the chart, [`Ui::draw`] draws it.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use super::*;

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn session_row(id: &str, role: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', 'ctx', NULL, NULL, 'n', '/s/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    )
}

/// The Centre of a Plan of one Workflow `api` (repo `api-repo`, base `main`, `dispatch`, no Jobs), as the view reads it.
fn centre() -> Centre {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         {}
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('p1', 'ctx', 'P', 'dispatch', 'lead', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
         VALUES ('p1', 'api', '/api-repo', 'main', 'dispatch', 'cpd/p1', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
         UPDATE sessions SET plan = 'p1' WHERE id = 'lead';
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-10-18T15:00:00.000Z', 'session-moved', 'ctx', NULL, 'lead', 'f',
                 '{{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}}');",
        session_row("lead", "lead")
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    snapshot(&mut store, 40, 1_792_336_050_000).unwrap().centre.expect("the Lead is in the slot")
}

#[test]
fn the_chart_boxes_are_drawn_one_column_from_the_edge_of_the_plan_view() {
    let centre = centre();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let mut ui = Ui::default();
    terminal.draw(|f| ui.draw(f, Some(&centre))).unwrap();
    let buf = terminal.backend().buffer();
    // row 0 is the chart's blank row of vertical margin; the first box's top starts one column in and ends one before the edge
    assert_eq!(row(buf, 0).trim_end(), "");
    assert_eq!(row(buf, 1).trim_end(), " ╭─ api · api-repo 󱦰 main · dispatch ─╮");
}
