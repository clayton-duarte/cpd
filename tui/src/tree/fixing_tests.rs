//! The red loader (hs-d581, commands.md §13.2 and §17, contract §6.1.1): a Plan Lead's dot, `Dot::Fixing`, turns the 28 moon_alt
//! frames (50 ms) in red 9 while the Lead works on a Plan with a failed Job; the tree redraws with the clock and the legend lists it.

use std::collections::HashSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use tui_tree_widget::TreeState;

use super::fixture::{context, plan, session, tree};
use super::rows::{Scroll, draw, open_new};
use super::view::View;
use crate::palette::dot_span;
use crate::palette::numbered::{C9, C15};
use crate::read::Work;
use crate::read::{Dot, Place, Tree};
use cpd_core::plan::Phase;
use cpd_core::session::State;

const MOONS: [&str; 28] = [
    "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}", "\u{E3CE}",
    "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}", "\u{E3D5}", "\u{E3D6}",
    "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}", "\u{E3DC}", "\u{E3DD}", "\u{E3DE}",
    "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
];

/// A tree of one Plan whose Lead shows `dot`, one failed Job.
fn with_dot(dot: Dot) -> Tree {
    let mut lead = session("s-pp", "x", State::Working, None, Place::Parked);
    lead.dot = dot;
    tree(vec![context(
        "cc",
        vec![plan(
            "pp",
            "cc",
            Phase::Dispatch,
            [0, 0, 0, 0, 1, 0],
            lead,
            vec![],
        )],
    )])
}

/// The dot's cell (column 2 of row 2, under the ▲ line) of the tree drawn at `frame`: its glyph and foreground.
fn dot_cell(t: &Tree, frame: usize) -> (String, Color) {
    let mut state: TreeState<String> = TreeState::default();
    open_new(&mut state, &mut HashSet::new(), t);
    state.select(vec![]);
    let mut terminal = Terminal::new(TestBackend::new(30, 7)).unwrap();
    terminal
        .draw(|f| {
            draw(
                f,
                f.area(),
                t,
                &mut state,
                false,
                frame,
                false,
                &mut Scroll::default(),
            )
        })
        .unwrap();
    let cell = &terminal.backend().buffer()[(2, 2)];
    (cell.symbol().to_owned(), cell.fg)
}

#[test]
fn a_leads_red_loader_turns_with_the_frame_on_its_row() {
    for frame in 0..58 {
        assert_eq!(
            dot_cell(&with_dot(Dot::Fixing), frame),
            (MOONS[frame % 28].to_owned(), C9),
            "frame {frame}"
        );
        assert_eq!(
            dot_cell(&with_dot(Dot::Working(None)), frame),
            (MOONS[frame % 28].to_owned(), C15),
            "working, frame {frame}"
        );
    }
}

#[test]
fn the_tree_is_animated_while_a_live_red_loader_is_in_it() {
    let mut t = with_dot(Dot::Fixing);
    assert!(
        View::new(t.clone(), true).animated(),
        "a live red loader turns"
    );
    t.contexts[0].plans[0].sessions[0].archived_at = Some("2026-02-01T00:00:00.000Z".into());
    assert!(
        !View::new(t, true).animated(),
        "an archived Session's dot never turns"
    );
}

#[test]
fn the_legend_lists_the_red_loader_after_the_working_one() {
    let sections = crate::widgets::legend::sections();
    let dots = sections
        .iter()
        .find(|s| s.title == "Session Dots")
        .expect("the Session Dots section");
    let first = |row: &Vec<ratatui::text::Line<'static>>| {
        row.first().map(|l| l.spans.clone()).unwrap_or_default()
    };
    let red: Vec<_> = (0..28).map(|f| dot_span(Dot::Fixing, f, false)).collect();
    let at = dots
        .rows
        .iter()
        .position(|r| first(r) == red)
        .expect("the row of the 28 red frames");
    let row = &dots.rows[at];
    let white: Vec<_> = (0..28)
        .map(|f| dot_span(Dot::Working(None), f, false))
        .collect();
    assert!(
        dots.rows.iter().any(|r| first(r) == white),
        "the white loader row is in the legend"
    );
    let after = dots.rows.get(at + 1).expect("a row after the red one");
    let yellow: Vec<_> = (0..28)
        .map(|f| dot_span(Dot::Working(Some(Work::Review)), f, false))
        .collect();
    assert_eq!(
        first(after),
        yellow,
        "the yellow loader follows the red one"
    );
    for (i, r) in red.iter().enumerate() {
        assert_eq!(r.content.as_ref(), MOONS[i], "frame {i}");
    }
    let text: String = row[1].spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(
        text,
        "the Lead is fixing a failed Job or answering a Worker's question (one frame every 50 ms)"
    );
}

/// A store of Plan `px` in dispatch whose working Lead `l-x` has a running Job, its Attempt and one Worker
/// Escalation in `status` (`open`, `answered` or `expired`); no failed Job. No tmux, git or gh.
fn escalated(status: &str) -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let home =
        cpd_core::home::CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = cpd_core::store::Store::open(&home, 5000).unwrap();
    let t = "'2026-01-02T01:00:00.000Z'";
    let (answer, answered, expired) = match status {
        "answered" => ("'a'", t, "NULL"),
        "expired" => ("NULL", "NULL", t),
        _ => ("NULL", "NULL", "NULL"),
    };
    let fp = "a".repeat(64);
    let sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-x', 'lead', 'ctx', NULL, 'l-x', '/repo', 'O3', 'working', {t}, '2026-01-02T00:00:00.000Z');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('px', 'ctx', 'X', 'dispatch', 'l-x', {t}, {t}, NULL);
         UPDATE sessions SET plan = 'px' WHERE id = 'l-x';
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('px', 'work', '/repo', 'main', 'dispatch', 'cpd/px/work', '/wt', cpd_txn(), {t}, {t});
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, created_at)
           VALUES ('px', 'work', 'j1', 'J', 'S1', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'running', 0, '{fp}', cpd_txn(), {t});
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('px', 'j1-a1', 'j1', 1, 'S1', 'ladder', 'running', 'cpd/px--j1-a1', '/w/j1', {t});
         INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at)
           VALUES ('px', 'j1-a1-q1', 'j1-a1', 1, 'which?', '{status}', {answer}, 1, {t}, {answered}, {expired});"
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();
    crate::read::tree(&mut store).unwrap()
}

/// `dots_tree_open_escalation_is_red_loader`: a working Lead whose Plan has an open Worker Escalation (no failed
/// Job) shows the red loader; once the question is answered or expired it shows its Work dot again.
#[test]
fn dots_tree_open_escalation_is_red_loader() {
    let lead = |t: &Tree| t.contexts[0].plans[0].lead().expect("the Lead").dot;
    assert_eq!(lead(&escalated("open")), Dot::Fixing, "an open escalation");
    for status in ["answered", "expired"] {
        assert_eq!(
            lead(&escalated(status)),
            Dot::Working(Some(crate::read::Work::Dispatch)),
            "{status}"
        );
    }
}
