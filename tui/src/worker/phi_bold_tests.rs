//! The worker view's foot draws cpd's mark φ bold (contract §6.1.1 rule (4), hs-d708): only the glyph is bold, never
//! the colon or the text that follows it.

use ratatui::style::Modifier;

use super::screen_tests::{draw, env, populate, session_file};
use crate::test_text::rows as buffer_rows;

/// The column of `needle` in `row`, in cells (the same unit as a buffer coordinate).
fn column(row: &str, needle: char) -> u16 {
    row.char_indices().find(|(_, c)| *c == needle).map(|(i, _)| row[..i].chars().count() as u16).unwrap_or_else(|| panic!("no {needle:?} in {row:?}"))
}

#[test]
fn the_worker_foot_draws_a_bold_phi() {
    let mut env = env();
    let file = session_file(&env);
    populate(&mut env, Some(&file));
    env.sql("UPDATE sessions SET state = 'waiting', state_label = 'φ: question to the Lead' WHERE id = 'w1';");
    let mut view = env.attempt();
    let buf = draw(&mut view, 80, 12);
    let rows = buffer_rows(&buf);
    let at = rows.iter().position(|r| r.contains("φ: question to the Lead")).unwrap_or_else(|| panic!("no foot with the state label: {rows:?}"));
    let (y, row) = (at as u16, &rows[at]);
    let phi = column(row, 'φ');
    let colon = column(row, ':');
    assert!(buf[(phi, y)].modifier.contains(Modifier::BOLD), "the φ cell is bold: {row:?}");
    assert!(!buf[(colon, y)].modifier.contains(Modifier::BOLD), "the `:` after φ is not bold: {row:?}");
    for x in column(row, 'q')..buf.area.width {
        assert!(!buf[(x, y)].modifier.contains(Modifier::BOLD), "the text after `φ:` is not bold at column {x}: {row:?}");
    }
}
