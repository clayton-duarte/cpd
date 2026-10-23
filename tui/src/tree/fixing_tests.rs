//! The red loader (hs-d581, commands.md §13.2 and §17, contract §6.1.1): a Plan Lead's dot, `Dot::Fixing`, turns the eight moon
//! frames in red 9 while the Lead works on a Plan with a failed Job; the tree redraws with the clock and the legend lists it.

use std::collections::HashSet;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use tui_tree_widget::TreeState;

use super::fixture::{context, plan, session, tree};
use super::rows::{Scroll, draw, open_new};
use super::view::View;
use crate::palette::dot_span;
use crate::palette::numbered::{C14, C9};
use crate::read::{Dot, Place, Tree};
use cpd_core::plan::Phase;
use cpd_core::session::State;

const MOONS: [&str; 8] =
    ["\u{F0F64}", "\u{F0F67}", "\u{F0F61}", "\u{F0F68}", "\u{F0F62}", "\u{F0F66}", "\u{F0F63}", "\u{F0F65}"];

/// A tree of one Plan whose Lead shows `dot`, one failed Job.
fn with_dot(dot: Dot) -> Tree {
    let mut lead = session("s-pp", "x", State::Working, None, Place::Parked);
    lead.dot = dot;
    tree(vec![context("cc", vec![plan("pp", "cc", Phase::Dispatch, [0, 0, 0, 0, 1, 0], lead, vec![])])])
}

/// The dot's cell (column 2 of row 1) of the tree drawn at `frame`: its glyph and foreground.
fn dot_cell(t: &Tree, frame: usize) -> (String, Color) {
    let mut state: TreeState<String> = TreeState::default();
    open_new(&mut state, &mut HashSet::new(), t);
    state.select(vec![]);
    let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
    terminal.draw(|f| draw(f, f.area(), t, &mut state, false, frame, &mut Scroll::default())).unwrap();
    let cell = &terminal.backend().buffer()[(2, 1)];
    (cell.symbol().to_owned(), cell.fg)
}

#[test]
fn a_leads_red_loader_turns_with_the_frame_on_its_row() {
    for frame in 0..16 {
        assert_eq!(dot_cell(&with_dot(Dot::Fixing), frame), (MOONS[frame % 8].to_owned(), C9), "frame {frame}");
        assert_eq!(dot_cell(&with_dot(Dot::Working), frame), (MOONS[frame % 8].to_owned(), C14), "working, frame {frame}");
    }
}

#[test]
fn the_tree_is_animated_while_a_live_red_loader_is_in_it() {
    let mut t = with_dot(Dot::Fixing);
    assert!(View::new(t.clone(), true).animated(), "a live red loader turns");
    t.contexts[0].plans[0].sessions[0].archived_at = Some("2026-02-01T00:00:00.000Z".into());
    assert!(!View::new(t, true).animated(), "an archived Session's dot never turns");
}

#[test]
fn the_legend_lists_the_red_loader_after_the_working_one() {
    let sections = crate::widgets::legend::sections();
    let dots = sections.iter().find(|s| s.title == "Session Dots").expect("the Session Dots section");
    let first = |row: &Vec<ratatui::text::Line<'static>>| row.first().map(|l| l.spans.clone()).unwrap_or_default();
    let cyan: Vec<_> = (0..8).map(|f| dot_span(Dot::Working, f, false)).collect();
    let at = dots.rows.iter().position(|r| first(r) == cyan).expect("the row of the 8 cyan frames");
    let row = dots.rows.get(at + 1).expect("a row after the working one");
    let red: Vec<_> = (0..8).map(|f| dot_span(Dot::Fixing, f, false)).collect();
    assert_eq!(first(row), red, "the 8 red frames");
    let text: String = row[1].spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text, "the Lead is working on a Plan with a failed Job (one frame every 100 ms)");
}
