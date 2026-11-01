//! Red tests: a merged or cancelled Workflow's box title draws no Lead dot and no red loader (docs/commands.md §13.2 "Workflow boxes",
//! §17 "The Lead dot rule"; decisions pro-d217, pro-d219). The open box of the same Plan still draws its dot. Which open
//! box shows the dot, and its colour, is the next Workflow's and is not asserted here.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::Store;

use crate::palette;
use crate::plan::boxes;
use crate::read::{self, Dot, PlanBoxes, WorkflowNode};

const AT: &str = "'2026-01-02T00:00:00.000Z'";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// One Plan `pay` with a working Lead and Workflows `old` (merged), `new` (open) and `gone` (cancelled); a failed Job in `new`
/// when `failed`. In-process store; no tmux, git or gh.
fn plan_boxes(failed: bool) -> (tempfile::TempDir, PlanBoxes) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let mut sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', NULL, 'lead1', '/repo', 'O3', 'working', {AT}, '2026-01-01T00:00:00.000Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', '2026-01-01T00:00:00.000Z', {AT}, NULL);
         UPDATE sessions SET plan = 'pay' WHERE id = 'lead1';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state, authored,
                                    base_ref, default_branch, created_at, changed_at, merged_at)
           VALUES ('acme/repo', 1, 'https://github.com/acme/repo/pull/1', 'T', 0, 'merged', 'none', 'passed', 'clean', 1,
                   'main', 'main', {AT}, {AT}, {AT}),
                  ('acme/repo', 2, 'https://github.com/acme/repo/pull/2', 'T', 0, 'open', 'none', 'passed', 'clean', 1,
                   'main', 'main', {AT}, {AT}, NULL);
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
           VALUES ('pay', 'old', '/repo', 'main', 'merged', 'pay/old', '/wt-old', 1, 'https://github.com/acme/repo/pull/1', {AT},
                   (SELECT id FROM pull_requests WHERE number = 1), cpd_txn(), '2026-01-01T00:00:00.000Z', {AT}, {AT});
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at)
           VALUES ('pay', 'new', '/repo', 'main', 'open', 'pay/new', '/wt-new', 2, 'https://github.com/acme/repo/pull/2', {AT},
                   (SELECT id FROM pull_requests WHERE number = 2), cpd_txn(), '2026-01-01T00:00:00.000Z', {AT});
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'gone', '/repo', 'main', 'cancelled', 'pay/gone', cpd_txn(), '2026-01-01T00:00:00.000Z');"
    );
    if failed {
        sql.push_str(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, failed_cause, merged_sha)
               VALUES ('pay', 'new', 'j1', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'failed', 0, '{FP}', cpd_txn(), {AT}, 'crashed', NULL);"
        ));
    }
    sql.push_str("UPDATE sessions SET working_pr = 'new' WHERE id = 'lead1';");
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    let boxes = read::plan_boxes(&mut store, &PlanId::parse("pay").unwrap()).unwrap();
    (dir, boxes)
}

fn node<'a>(b: &'a PlanBoxes, key: &str) -> &'a WorkflowNode {
    b.workflows.iter().find(|p| p.key.as_str() == key).unwrap()
}

/// The foreground of the spinner cell in row 0 of the box of `pr` drawn alone among all the Plan's Jobs.
fn title_dot(b: &PlanBoxes, workflow: &WorkflowNode, frame: usize) -> Option<ratatui::style::Color> {
    let chart = boxes::layout(std::slice::from_ref(workflow), &b.jobs, &b.needs_user, frame).chart;
    (0..chart.buf.area.width)
        .map(|x| &chart.buf[(x, 0)])
        .find(|cell| palette::SPINNER.iter().any(|glyph| cell.symbol().contains(glyph)))
        .map(|cell| cell.fg)
}

#[test]
fn lead_dot_box_merged_box_title_draws_no_dot_and_the_open_box_title_does() {
    let (_dir, b) = plan_boxes(false);
    for frame in [0, 5, 27] {
        assert_eq!(title_dot(&b, node(&b, "old"), frame), None, "merged box, frame {frame}");
        assert!(title_dot(&b, node(&b, "new"), frame).is_some(), "open box, frame {frame}");
    }
}

#[test]
fn lead_dot_box_merged_box_draws_no_red_loader_when_a_sibling_job_failed() {
    let (_dir, b) = plan_boxes(true);
    for frame in [0, 5, 27] {
        let red = palette::dot_span(Dot::Fixing, frame, false).style.fg;
        let dot = title_dot(&b, node(&b, "old"), frame);
        assert_eq!(dot, None, "merged box, frame {frame} (red loader is {red:?})");
    }
}

#[test]
fn lead_dot_box_cancelled_box_draws_no_dot() {
    let (_dir, b) = plan_boxes(false);
    for frame in [0, 5, 27] {
        assert_eq!(title_dot(&b, node(&b, "gone"), frame), None, "cancelled box, frame {frame}");
    }
}
