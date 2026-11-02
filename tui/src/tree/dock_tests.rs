//! The docked group on small panes (commands.md §13.2 "The Archive section", pro-d349): Standing and Archive together are
//! capped at half the pane, and a pane too short for every fixed row cuts the group at the bottom. Whatever the pane, the
//! frame is drawn without a panic or an underflow, the group is never taller than half the pane, and the wheel of one
//! section never moves the other.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::fixture::{context, tree};
use super::rows::{ARCHIVE, STANDING};
use super::view::View;
use crate::test_text::rows;

/// Standing: `cpd`; the project tree: `gamma`; Archive: `zeta`.
fn both() -> View {
    let mut zeta = context("zeta", vec![]);
    zeta.archived_at = Some(super::fixture::WHEN.into());
    View::new(
        tree(vec![context("cpd", vec![]), context("gamma", vec![]), zeta]),
        true,
    )
}

fn draw(view: &mut View, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(30, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    rows(terminal.backend().buffer())
}

fn wheel(view: &mut View, down: bool, row: u16) {
    view.mouse(MouseEvent {
        kind: if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        column: 5,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn headers(screen: &[String]) -> Vec<usize> {
    (0..screen.len())
        .filter(|&y| screen[y].starts_with("───"))
        .collect()
}

#[test]
fn panes_of_3_5_and_8_rows_draw_both_sections_open_or_folded_without_a_panic() {
    for height in [3u16, 5, 8] {
        for standing_open in [true, false] {
            for archive_open in [true, false] {
                let mut view = both();
                if !standing_open {
                    view.state.open(vec![STANDING.to_owned()]);
                }
                if archive_open {
                    view.state.open(vec![ARCHIVE.to_owned()]);
                }
                let screen = draw(&mut view, height);
                assert_eq!(screen.len(), usize::from(height));
                let first = screen.len() - usize::from(height / 2);
                assert!(
                    headers(&screen).iter().all(|&y| y >= first),
                    "{height} rows, standing {standing_open}, archive {archive_open}: a header above the group's half: {screen:#?}"
                );
                if height / 2 < 2 {
                    assert!(
                        headers(&screen).is_empty(),
                        "no room, no group: {screen:#?}"
                    );
                }
                // a wheel over every row, both ways, and a second frame: no panic, no underflow
                for y in 0..height {
                    wheel(&mut view, true, y);
                    wheel(&mut view, false, y);
                }
                draw(&mut view, height);
            }
        }
    }
}

#[test]
fn a_pane_of_8_rows_draws_standing_first_and_cuts_archive_at_the_bottom() {
    let mut view = both();
    view.state.open(vec![ARCHIVE.to_owned()]);
    let screen = draw(&mut view, 8);
    // the group is 4 rows: the blank row, Standing's header, its `▲` row and its first list row; Archive is cut
    assert_eq!(headers(&screen), [5], "{screen:#?}");
    assert!(screen[5].contains("Standing"), "{screen:#?}");
}
