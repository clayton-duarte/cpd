//! Red tests of THE RULE (pro-d88) on the TUI side: a failed Job is being worked on when its Plan's Lead works, or a
//! queued or running Job of its Workflow was created after its `job-failed` Event. `plan_boxes` lists those Jobs
//! (`NeedsUser::worked_on`), the failed pill turns for them, and the tree's Lead dot is the red loader for a Plan that
//! has one. The store is in-process, built with SQL in explicit transactions; no tmux, git or gh.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::inbox::model::Urgency;
use cpd_core::session::State;
use cpd_core::store::Store;

use crate::plan::boxes;
use crate::read::{self, Dot, Work};

const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CLOSE_CIRCLE: &str = "\u{F0159}";
const MOONS: [&str; 28] = [
    "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}", "\u{E3CE}",
    "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}", "\u{E3D5}", "\u{E3D6}",
    "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}", "\u{E3DC}", "\u{E3DD}", "\u{E3DE}",
    "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
];

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

/// Context `ctx`, Plan `pay` (Lead `l-pay`, in `lead_state`) with Workflows `p1`, `p2`, `p3`.
fn fixture(lead_state: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture { _dir: dir, store };
    let label = if lead_state == "waiting" || lead_state == "blocked" {
        "'Which?'"
    } else {
        "NULL"
    };
    let mut sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_label, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', '{lead_state}', {label}, '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'P', 'dispatch', 'l-pay', '{T}', '{T}');"
    );
    for workflow in ["p1", "p2", "p3"] {
        sql.push_str(&format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{workflow}', '/r', 'main', 'dispatch', 'cpd/pay/{workflow}', '/lw', 1, '{T}', '{T}');"
        ));
    }
    f.sql(&sql);
    f
}

impl Fixture {
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
    }

    /// A Job in its own transaction (so `created_txn` grows with each call).
    fn job(&mut self, workflow: &str, key: &str, status: &str) {
        let cause = if status == "failed" { "'crashed'" } else { "NULL" };
        let sha = if status == "merged" { "'abc'" } else { "NULL" };
        self.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               fingerprint, created_txn, created_at, failed_cause, merged_sha)
               VALUES ('pay', '{workflow}', '{key}', 'J', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{FP}', cpd_txn(), '{T}', {cause}, {sha});"
        ));
    }

    /// A failed Job and its `job-failed` Event, in two transactions.
    fn failed(&mut self, workflow: &str, key: &str) {
        self.job(workflow, key, "failed");
        self.sql(&format!(
            "INSERT INTO events (txn, at, kind, context, plan, job, text, data)
             VALUES (cpd_txn(), '{T}', 'job-failed', 'ctx', 'pay', '{key}', 'x', '{{}}');"
        ));
    }

    fn set(&mut self, key: &str, status: &str) {
        let sha = if status == "merged" { "'abc'" } else { "NULL" };
        let cause = if status == "failed" {
            "'crashed'"
        } else {
            "NULL"
        };
        if status == "merged" {
            // queued -> running -> merged: the schema allows no queued -> merged
            self.sql(&format!(
                "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = '{key}'"
            ));
        }
        self.sql(&format!("UPDATE jobs SET status = '{status}', merged_sha = {sha}, failed_cause = {cause} WHERE plan = 'pay' AND key = '{key}'"));
    }

    /// `j-c` (p3), `j-a` (p1) and `j-b` (p2) fail, in that order; then `f-a` (p1) is queued and `f-c` (p3) runs.
    fn failures_and_fixes(&mut self) {
        self.failed("p3", "j-c");
        self.failed("p1", "j-a");
        self.failed("p2", "j-b");
        self.job("p1", "f-a", "queued");
        self.job("p3", "f-c", "running");
    }

    fn worked_on(&mut self) -> Vec<String> {
        let plan = PlanId::parse("pay").unwrap();
        let boxes = read::plan_boxes(&mut self.store, &plan).unwrap();
        boxes
            .needs_user
            .worked_on
            .iter()
            .map(|k| k.as_str().to_owned())
            .collect()
    }

    fn lead_dot(&mut self) -> Dot {
        let tree = read::tree_at(&mut self.store, None).unwrap();
        tree.contexts[0].plans[0]
            .lead()
            .expect("the Lead's row")
            .dot
    }
}

#[test]
fn plan_boxes_lists_the_failed_jobs_being_worked_on() {
    let mut idle = fixture("idle");
    idle.failures_and_fixes();
    assert_eq!(idle.worked_on(), ["j-a", "j-c"], "idle Lead: the failed Jobs with a queued or running fix of their Workflow, sorted");

    let mut working = fixture("working");
    working.failures_and_fixes();
    assert_eq!(
        working.worked_on(),
        ["j-a", "j-b", "j-c"],
        "working Lead: every failed Job of the Plan, sorted"
    );

    let mut neither = fixture("idle");
    neither.failures_and_fixes();
    neither.set("f-a", "merged");
    neither.set("f-c", "failed");
    assert_eq!(
        neither.worked_on(),
        ["j-b"; 0],
        "idle Lead, no fix underway: nothing (f-c failed too, with no event)"
    );
}

#[test]
fn a_failed_pill_turns_while_a_fix_job_of_its_pr_is_underway_and_its_lead_is_idle() {
    let mut f = fixture("idle");
    f.failed("p1", "j-a");
    f.job("p1", "f-a", "queued");
    let plan = PlanId::parse("pay").unwrap();
    let b = read::plan_boxes(&mut f.store, &plan).unwrap();
    for frame in 0..30 {
        let chart = boxes::layout(&b.workflows, &b.jobs, &b.needs_user, frame).chart;
        let pill = chart.pills.iter().find(|p| p.key == "j-a").expect("the failed Job's pill");
        let cell = &chart.buf[(pill.rect.x + 1, pill.rect.y)];
        assert_eq!(cell.symbol(), MOONS[frame % 28], "frame {frame}");
        assert_ne!(cell.symbol(), CLOSE_CIRCLE);
        let red = crate::palette::job_pill(crate::read::JobStatus::Failed, false, true);
        assert_eq!(
            (Some(cell.fg), Some(cell.bg)),
            (red.fg, red.bg),
            "red, frame {frame}"
        );
    }
}

#[test]
fn the_tree_shows_the_red_loader_for_an_idle_lead_whose_plan_has_a_failed_job_with_a_fix_underway()
{
    let mut fix = fixture("idle");
    fix.failed("p1", "j-a");
    fix.job("p1", "f-a", "queued");
    assert_eq!(fix.lead_dot(), Dot::Fixing, "idle Lead, a fix underway");

    let mut working = fixture("working");
    working.failed("p1", "j-a");
    assert_eq!(working.lead_dot(), Dot::Fixing, "working Lead");

    let mut none = fixture("idle");
    none.failed("p1", "j-a");
    assert_eq!(
        none.lead_dot(),
        Dot::Idle,
        "idle Lead, failed Job, no fix underway"
    );
}

#[test]
fn the_dot_of_a_lead_with_a_failed_job_being_worked_on_is_red_loader_or_red_item_whatever_its_state()
 {
    let phase = Some(Work::Review);
    let states = [
        State::Working,
        State::Idle,
        State::Waiting,
        State::Blocked,
        State::Stopped,
    ];
    for state in states {
        assert_eq!(
            Dot::of(state, None, true, phase),
            Dot::Fixing,
            "{state:?}, no item"
        );
        assert_eq!(
            Dot::of(state, Some(Urgency::Red), true, phase),
            Dot::Red,
            "{state:?}, red item"
        );
        for item in [Urgency::Yellow, Urgency::Magenta, Urgency::White] {
            assert_eq!(
                Dot::of(state, Some(item), true, phase),
                Dot::Fixing,
                "{state:?}, {item:?} item"
            );
        }
        let plain = if state == State::Working {
            Dot::Working(phase)
        } else {
            Dot::glyph(state)
        };
        assert_eq!(
            Dot::of(state, None, false, phase),
            plain,
            "{state:?}, not failed"
        );
        let item = if state == State::Working {
            Dot::Working(phase)
        } else {
            Dot::Yellow
        };
        assert_eq!(
            Dot::of(state, Some(Urgency::Yellow), false, phase),
            item,
            "{state:?}, yellow item, not failed"
        );
    }
}
