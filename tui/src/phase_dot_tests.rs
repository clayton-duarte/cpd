//! Red tests: the phase-coloured working dot, from docs/commands.md §17 ("The tree's dot rule" and "The Lead dot
//! rule"), §13.2 (the legend) and docs/contract.md §6.1.1, and the accepted decisions hs-d622 and hs-d624: a
//! working Plan Lead's spinner is the bright pair of its Plan's phase colour (grounding grey 15, plan magenta 13,
//! dispatch cyan 14, deploy yellow 11, done green 10); a working Session that leads no Plan stays cyan 14. The
//! stubs make these tests red until the implementation reads the phase. Expected glyphs are codepoints and expected
//! colours the palette's numbers, never read back from the code under test.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::SessionId;
use cpd_core::inbox::model::Urgency;
use cpd_core::plan::Phase;
use cpd_core::session::State;
use cpd_core::store::Store;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::palette::numbered::{C8, C10, C11, C13, C14, C15};
use crate::palette::dot_span;
use crate::read::{self, Dot, Tree};
use crate::tree::View;
use crate::widgets::legend;

const MOONS: [&str; 28] = [
    "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}",
    "\u{E3CE}", "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}",
    "\u{E3D5}", "\u{E3D6}", "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}",
    "\u{E3DC}", "\u{E3DD}", "\u{E3DE}", "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
];

const PHASES: [(Phase, &str, usize); 5] = [
    (Phase::Grounding, "grounding", 0),
    (Phase::Plan, "plan", 1),
    (Phase::Dispatch, "dispatch", 2),
    (Phase::Deploy, "deploy", 3),
    (Phase::Done, "done", 4),
];

/// `a_working_lead_is_drawn_in_its_plans_status_colour`: the 28 frames turn in the phase's bright colour, no Plan
/// is cyan, and an archived row keeps the hollow ring in colour 8 whatever the phase.
#[test]
fn a_working_lead_is_drawn_in_its_plans_status_colour() {
    let colours = [C15, C13, C14, C11, C10];
    for (phase, _, i) in PHASES {
        let colour = colours[i];
        for frame in 0..56 {
            let span = dot_span(Dot::Working(Some(phase)), frame, false);
            assert_eq!(span.content, MOONS[frame % 28], "{phase:?} frame {frame}");
            assert_eq!(span.style, Style::new().fg(colour), "{phase:?} frame {frame}: no other modifier");
        }
    }
    for frame in 0..56 {
        let span = dot_span(Dot::Working(None), frame, false);
        assert_eq!(span.style, Style::new().fg(C14), "a working Session that leads no Plan is cyan, frame {frame}");
    }
    for (phase, _, _) in PHASES {
        for frame in 0..56 {
            let span = dot_span(Dot::Working(Some(phase)), frame, true);
            assert_eq!(span.content, "\u{E3D5}", "archived {phase:?} frame {frame}: the hollow ring");
            assert_eq!(span.style, Style::new().fg(C8), "archived {phase:?} frame {frame}: colour 8");
        }
    }
}

/// `the_rule_gives_a_working_lead_the_dot_of_its_plans_phase`: `Dot::of` and `Dot::working` choose the phase dot for
/// a working Session, giving own state first, the red loader when the Plan has a failed Job, and the phase never for
/// a Session that is not working.
#[test]
fn the_rule_gives_a_working_lead_the_dot_of_its_plans_phase() {
    for phase in Phase::ALL {
        assert_eq!(Dot::of(State::Working, None, false, Some(phase)), Dot::Working(Some(phase)), "{phase:?}");
        for urgency in [Urgency::Red, Urgency::Yellow, Urgency::Magenta, Urgency::Cyan, Urgency::White] {
            assert_eq!(
                Dot::of(State::Working, Some(urgency), false, Some(phase)),
                Dot::Working(Some(phase)),
                "{phase:?} {urgency:?}: own state first (hs-d183)"
            );
        }
        assert_eq!(Dot::working(false, Some(phase)), Dot::Working(Some(phase)), "{phase:?}");
    }
    for phase in Phase::ALL {
        assert_eq!(Dot::working(true, Some(phase)), Dot::Fixing, "{phase:?}");
        assert_eq!(Dot::of(State::Working, None, true, Some(phase)), Dot::Fixing, "{phase:?}");
        for urgency in [Urgency::Yellow, Urgency::Magenta, Urgency::Cyan, Urgency::White] {
            assert_eq!(Dot::of(State::Working, Some(urgency), true, Some(phase)), Dot::Fixing, "{phase:?} {urgency:?}");
        }
        assert_eq!(Dot::of(State::Working, Some(Urgency::Red), true, Some(phase)), Dot::Red, "the Lead's own red item (hs-d581): {phase:?}");
    }
    for state in [State::Idle, State::Waiting, State::Blocked, State::Stopped] {
        for phase in Phase::ALL {
            for item in [None, Some(Urgency::Red), Some(Urgency::White)] {
                for failed in [false, true] {
                    assert_eq!(
                        Dot::of(state, item, failed, Some(phase)),
                        Dot::of(state, item, failed, None),
                        "{state:?} {phase:?} {item:?} failed={failed}: only a working Session uses the phase"
                    );
                }
            }
        }
    }
    for phase in Phase::ALL {
        assert!(!Dot::Working(Some(phase)).needs_you());
        assert!(!Dot::of(State::Working, None, false, Some(phase)).needs_you(), "{phase:?}");
    }
}

fn session(id: &str, role: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
         VALUES ('{id}', '{role}', 'ctx', NULL, '{id}', '/repo', 'O3', 'working',
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    )
}

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A store of one Context: five Plans, one per phase, each with a working Lead; a Plan with a failed Job and a
/// working Lead; a working fork of a Plan; a working Manager; and an archived working Lead. No tmux, git or gh.
fn fixture_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let t = "'2026-01-02T01:00:00.000Z'";
    let sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         {g} {p} {d} {y} {n} {f} {a} {fork} {mgr}
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at) VALUES
           ('pg', 'ctx', 'G', 'grounding', 'l-grounding', {t}, NULL, NULL),
           ('pp', 'ctx', 'P', 'plan', 'l-plan', {t}, NULL, NULL),
           ('pd', 'ctx', 'D', 'dispatch', 'l-dispatch', {t}, {t}, NULL),
           ('py', 'ctx', 'Y', 'deploy', 'l-deploy', {t}, {t}, NULL),
           ('pn', 'ctx', 'N', 'done', 'l-done', {t}, {t}, {t}),
           ('pf', 'ctx', 'F', 'dispatch', 'l-failed', {t}, {t}, NULL),
           ('pa', 'ctx', 'A', 'dispatch', 'l-arch', {t}, {t}, NULL);
         UPDATE sessions SET plan = 'pg' WHERE id = 'l-grounding';
         UPDATE sessions SET plan = 'pp' WHERE id = 'l-plan';
         UPDATE sessions SET plan = 'pd' WHERE id = 'l-dispatch';
         UPDATE sessions SET plan = 'py' WHERE id = 'l-deploy';
         UPDATE sessions SET plan = 'pn' WHERE id = 'l-done';
         UPDATE sessions SET plan = 'pf' WHERE id = 'l-failed';
         UPDATE sessions SET plan = 'pa' WHERE id = 'l-arch';
         UPDATE sessions SET plan = 'pd' WHERE id = 'l-fork';
         UPDATE sessions SET archived_at = '2026-01-05T00:00:00.000Z' WHERE id = 'l-arch';
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pf', 'work', '/repo', 'main', 'dispatch', 'cpd/pf/work', '/wt', cpd_txn(), {t}, {t});
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status,
                           appended, fingerprint, failed_cause, created_txn, created_at)
           VALUES ('pf', 'work', 'j1', 'J', 'S1', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'failed',
                   0, '{FP}', 'boom', cpd_txn(), {t});",
        g = session("l-grounding", "lead"),
        p = session("l-plan", "lead"),
        d = session("l-dispatch", "lead"),
        y = session("l-deploy", "lead"),
        n = session("l-done", "lead"),
        f = session("l-failed", "lead"),
        a = session("l-arch", "lead"),
        fork = session("l-fork", "lead"),
        mgr = session("mgr", "manager"),
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    (dir, store)
}

/// The dot of the Plan `plan`'s Lead.
fn lead_dot(tree: &Tree, plan: &str) -> Dot {
    tree.contexts
        .iter()
        .flat_map(|c| &c.plans)
        .find(|p| p.head.id.as_str() == plan)
        .and_then(|p| p.lead())
        .unwrap_or_else(|| panic!("the Lead of Plan {plan}"))
        .dot
}

/// The dot of the Session `id`, wherever the tree puts it.
fn session_dot(tree: &Tree, id: &str) -> Dot {
    let id = SessionId::parse(id).unwrap();
    tree.contexts
        .iter()
        .flat_map(|c| &c.plans)
        .flat_map(|p| &p.sessions)
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("Session {id}"))
        .dot
}

/// `the_tree_reads_each_working_leads_dot_from_its_plans_phase`: `read::tree` gives each working Lead the dot of its
/// Plan's phase, the red loader for a Plan with a failed Job, cyan for a fork and a Manager, and its static glyph for
/// an archived Lead.
#[test]
fn the_tree_reads_each_working_leads_dot_from_its_plans_phase() {
    let (_dir, mut store) = fixture_store();
    let tree = read::tree(&mut store).unwrap();
    for (phase, plan) in [
        (Phase::Grounding, "pg"),
        (Phase::Plan, "pp"),
        (Phase::Dispatch, "pd"),
        (Phase::Deploy, "py"),
        (Phase::Done, "pn"),
    ] {
        assert_eq!(lead_dot(&tree, plan), Dot::Working(Some(phase)), "Plan {plan}");
    }
    assert_eq!(lead_dot(&tree, "pf"), Dot::Fixing, "the Lead of a Plan with a failed Job");
    assert_eq!(session_dot(&tree, "l-fork"), Dot::Working(None), "a working fork leads no Plan: cyan");
    assert_eq!(tree.contexts[0].dot, Dot::Working(None), "the working Manager leads no Plan: cyan");
    assert_eq!(session_dot(&tree, "l-arch"), Dot::Working(None), "an archived working Lead keeps its glyph");
}

/// Sets the dot of every Session the tree draws under a Plan.
fn every_dot(tree: &mut Tree, dot: Dot) {
    for s in tree.contexts.iter_mut().flat_map(|c| &mut c.plans).flat_map(|p| &mut p.sessions) {
        s.dot = dot;
    }
}

/// `the_tree_animates_a_working_dot_of_every_phase`: a tree whose Sessions all carry `Dot::Working(Some(p))` is
/// animated, and not when every such Session is archived.
#[test]
fn the_tree_animates_a_working_dot_of_every_phase() {
    let (_dir, mut store) = fixture_store();
    let base = read::tree(&mut store).unwrap();
    for phase in Phase::ALL {
        let mut tree = base.clone();
        every_dot(&mut tree, Dot::Working(Some(phase)));
        assert!(View::new(tree, true).animated(), "{phase:?}");
    }
    let mut tree = base;
    every_dot(&mut tree, Dot::Working(Some(Phase::Dispatch)));
    for s in tree.contexts.iter_mut().flat_map(|c| &mut c.plans).flat_map(|p| &mut p.sessions) {
        s.archived_at = Some("2026-02-01T00:00:00.000Z".into());
    }
    for c in &mut tree.contexts {
        c.dot = Dot::Stopped;
    }
    assert!(!View::new(tree, true).animated(), "an archived Session working dot never turns");
}

/// The spans of a row's first cell.
fn first_cell(row: &[Line<'static>]) -> Vec<Span<'static>> {
    row.first().map(|l| l.spans.clone()).unwrap_or_default()
}

/// `the_legend_shows_the_working_loader_once_per_phase`: the `Session Dots` section has, for each phase, a row of the
/// 28 frames drawn by `dot_span(Dot::Working(Some(p)), f, false)` and naming the phase, and the red loader row stays.
#[test]
fn the_legend_shows_the_working_loader_once_per_phase() {
    let sections = legend::sections();
    let dots = sections.iter().find(|s| s.title == "Session Dots").expect("the Session Dots section");
    for (phase, name, _) in PHASES {
        let frames: Vec<Span> = (0..28).map(|f| dot_span(Dot::Working(Some(phase)), f, false)).collect();
        let row = dots.rows.iter().find(|r| first_cell(r) == frames);
        assert!(row.is_some(), "no `Session Dots` row of the 28 {name} frames");
        let text: String = row
            .unwrap()
            .iter()
            .skip(1)
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.to_lowercase().contains(name), "the row names the phase {name}: {text:?}");
    }
    let red: Vec<Span> = (0..28).map(|f| dot_span(Dot::Fixing, f, false)).collect();
    assert!(dots.rows.iter().any(|r| first_cell(r) == red), "the red loader row stays");
}
