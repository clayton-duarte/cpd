//! Red tests: the Lead's dot draws on the one box `sessions.working_pr` names, and is red only for that Workflow's own
//! failure or open Escalation (docs/commands.md §13.2, §17; decisions pro-d217, pro-d219, pro-d259). Rendering only.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::Store;
use ratatui::style::Color;

use crate::palette;
use crate::plan::boxes;
use crate::read::{self, Dot, PlanBoxes, WorkflowNode};

const AT: &str = "'2026-01-02T00:00:00.000Z'";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FRAMES: [usize; 3] = [0, 5, 27];
const ALL: [&str; 5] = ["api", "dev", "web", "old", "gone"];

#[derive(Clone, Copy, PartialEq)]
enum Extra {
    None,
    FailedJobInApi,
    OpenEscalationInApi,
}

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

impl Fixture {
    fn new(extra: Extra) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
        let store = Store::open(&home, 5000).unwrap();
        let mut f = Fixture { _dir: dir, store };
        f.sql(&format!(
            "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pay', 'lead1', '/repo', 'O3', 'working', {AT}, '2026-01-01T00:00:00.000Z');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', '2026-01-01T00:00:00.000Z', {AT}, NULL);
             UPDATE sessions SET plan = 'pay' WHERE id = 'lead1';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state, authored,
                                        base_ref, default_branch, created_at, changed_at, merged_at)
               VALUES ('acme/repo', 1, 'https://github.com/acme/repo/pull/1', 'T', 0, 'merged', 'none', 'passed', 'clean', 1, 'main', 'main', {AT}, {AT}, {AT}),
                      ('acme/repo', 2, 'https://github.com/acme/repo/pull/2', 'T', 0, 'open', 'none', 'passed', 'clean', 1, 'main', 'main', {AT}, {AT}, NULL),
                      ('acme/repo', 3, 'https://github.com/acme/repo/pull/3', 'T', 0, 'open', 'none', 'passed', 'clean', 1, 'main', 'main', {AT}, {AT}, NULL);
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                              pull_request, created_txn, created_at, started_at, merged_at)
               VALUES ('pay', 'old', '/repo', 'main', 'merged', 'pay/old', '/wt-old', 1, 'https://github.com/acme/repo/pull/1', {AT},
                       (SELECT id FROM pull_requests WHERE number = 1), cpd_txn(), '2026-01-01T00:00:00.000Z', {AT}, {AT});
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                              pull_request, created_txn, created_at, started_at)
               VALUES ('pay', 'api', '/repo', 'main', 'open', 'pay/api', '/wt-api', 2, 'https://github.com/acme/repo/pull/2', {AT},
                       (SELECT id FROM pull_requests WHERE number = 2), cpd_txn(), '2026-01-01T00:00:00.000Z', {AT}),
                      ('pay', 'dev', '/repo', 'main', 'open', 'pay/dev', '/wt-dev', 3, 'https://github.com/acme/repo/pull/3', {AT},
                       (SELECT id FROM pull_requests WHERE number = 3), cpd_txn(), '2026-01-01T00:00:00.000Z', {AT});
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', 'web', '/repo', 'main', 'dispatch', 'pay/web', '/wt-web', cpd_txn(), '2026-01-01T00:00:00.000Z', {AT});
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pay', 'gone', '/repo', 'main', 'cancelled', 'pay/gone', cpd_txn(), '2026-01-01T00:00:00.000Z');"
        ));
        match extra {
            Extra::None => {}
            Extra::FailedJobInApi => f.sql(&format!(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                   fingerprint, created_txn, created_at, failed_cause, merged_sha)
                   VALUES ('pay', 'api', 'j1', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'failed', 0, '{FP}', cpd_txn(), {AT}, 'crashed', NULL);"
            )),
            Extra::OpenEscalationInApi => f.sql(&format!(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                   fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'j1', 'J', 'S1', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{FP}', cpd_txn(), {AT});
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                   VALUES ('pay', 'j1-a1', 'j1', 1, 'S1', 'ladder', 'running', 'cpd/pay--j1-a1', '/w/j1', {AT});
                 INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
                   VALUES ('pay', 'j1-a1-q1', 'j1-a1', 1, 'which?', 'open', 1, {AT});"
            )),
        }
        f
    }

    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
    }

    fn name(&mut self, key: &str) {
        self.sql(&format!(
            "UPDATE sessions SET working_pr = '{key}' WHERE id = 'lead1'"
        ));
    }

    fn boxes(&mut self) -> PlanBoxes {
        read::plan_boxes(&mut self.store, &PlanId::parse("pay").unwrap()).unwrap()
    }
}

fn node<'a>(b: &'a PlanBoxes, key: &str) -> &'a WorkflowNode {
    b.workflows.iter().find(|p| p.key.as_str() == key).unwrap()
}

/// The foreground of the spinner cell in row 0 of the box of `key` drawn alone.
fn title_dot(b: &PlanBoxes, key: &str, frame: usize) -> Option<Color> {
    let workflow = node(b, key);
    let chart = boxes::layout(
        std::slice::from_ref(workflow),
        &b.jobs,
        &b.needs_user,
        frame,
    )
    .chart;
    (0..chart.buf.area.width)
        .map(|x| &chart.buf[(x, 0)])
        .find(|cell| {
            palette::SPINNER
                .iter()
                .any(|glyph| cell.symbol().contains(glyph))
        })
        .map(|cell| cell.fg)
}

fn red(frame: usize) -> Option<Color> {
    palette::dot_span(Dot::Fixing, frame, false).style.fg
}

fn phase(b: &PlanBoxes, frame: usize) -> Option<Color> {
    palette::dot_span(Dot::working(false, b.needs_user.lead_work), frame, false)
        .style
        .fg
}

fn only(b: &PlanBoxes, named: &str) {
    for frame in FRAMES {
        for key in ALL {
            let dot = title_dot(b, key, frame);
            if key == named {
                assert!(dot.is_some(), "{key} must draw the dot, frame {frame}");
            } else {
                assert_eq!(dot, None, "{key} must draw no dot, frame {frame}");
            }
        }
    }
}

#[test]
fn working_pr_dot_draws_on_the_named_open_box_only() {
    let mut f = Fixture::new(Extra::None);
    f.name("api");
    only(&f.boxes(), "api");
}

#[test]
fn working_pr_dot_draws_on_a_box_with_no_pull_request() {
    let mut f = Fixture::new(Extra::None);
    f.name("web");
    only(&f.boxes(), "web");
}

#[test]
fn working_pr_dot_null_draws_nowhere() {
    let mut f = Fixture::new(Extra::None);
    only(&f.boxes(), "");
}

#[test]
fn working_pr_dot_is_red_only_when_its_own_workflow_has_a_failed_job() {
    let mut f = Fixture::new(Extra::FailedJobInApi);
    f.name("dev");
    let b = f.boxes();
    for frame in FRAMES {
        let dot = title_dot(&b, "dev", frame);
        assert!(dot.is_some(), "dev draws the dot, frame {frame}");
        assert_eq!(
            dot,
            phase(&b, frame),
            "dev is the phase colour, frame {frame}"
        );
        assert_ne!(dot, red(frame), "dev is not red, frame {frame}");
    }
    f.name("api");
    let b = f.boxes();
    for frame in FRAMES {
        let dot = title_dot(&b, "api", frame);
        assert!(dot.is_some(), "api draws the dot, frame {frame}");
        assert_eq!(dot, red(frame), "api is the red loader, frame {frame}");
    }
}

#[test]
fn working_pr_dot_is_red_for_an_open_escalation_of_its_own_workflow() {
    let mut f = Fixture::new(Extra::OpenEscalationInApi);
    f.name("api");
    let b = f.boxes();
    for frame in FRAMES {
        let dot = title_dot(&b, "api", frame);
        assert!(dot.is_some(), "api draws the dot, frame {frame}");
        assert_eq!(dot, red(frame), "api is red, frame {frame}");
    }
    f.name("dev");
    let b = f.boxes();
    for frame in FRAMES {
        let dot = title_dot(&b, "dev", frame);
        assert!(dot.is_some(), "dev draws the dot, frame {frame}");
        assert_eq!(
            dot,
            phase(&b, frame),
            "dev is the phase colour, frame {frame}"
        );
        assert_ne!(dot, red(frame), "dev is not red, frame {frame}");
    }
}

#[test]
fn working_pr_dot_goes_when_the_lead_leaves_working() {
    let mut f = Fixture::new(Extra::None);
    f.name("api");
    f.sql("UPDATE sessions SET state = 'idle' WHERE id = 'lead1'");
    let named: Option<String> = f
        .store
        .write("read", |tx| {
            Ok::<_, CpdError>(tx.query_row(
                "SELECT working_pr FROM sessions WHERE id = 'lead1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(named, None, "leaving working clears the name");
    only(&f.boxes(), "");
}

#[test]
fn working_pr_dot_on_a_merged_or_cancelled_named_box_draws_none() {
    let mut f = Fixture::new(Extra::None);
    for key in ["old", "gone"] {
        f.name(key);
        only(&f.boxes(), "");
    }
}
