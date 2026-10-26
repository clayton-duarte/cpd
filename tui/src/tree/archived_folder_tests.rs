//! The tree's archive row and its fold (commands.md §13.2 "Archive folder row"; contract §6.1.1; hs-d698, hs-d707):
//! one clickable row at the tree root, after the last live Context, `󰬦 archive` folded and `󰬬 archive` expanded,
//! every cell colour 8, present only when at least one Context is archived, folded by default, its fold persisted
//! under `"archived"` in `run/tui/tree-folds.json`.
//!
//! Red until the tree draws them: today's tree has no archive row, so every test here fails at runtime on an
//! assertion, never at build time.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;

use super::folds::FoldsFile;
use super::fixture::{archived, sample};
use super::rows as tree_rows;
use super::view::{Effect, View};
use crate::palette::{self, Meaning, STOPPED};
use crate::test_text::rows;

/// U+F0B26 md-arrow_down_drop_circle_outline: the archive row folded.
const DOWN: &str = "\u{F0B26}";
/// U+F0B2C md-arrow_up_drop_circle_outline: the archive row expanded.
const UP: &str = "\u{F0B2C}";

/// Colour 8, the archive row's colour on every mark.
fn muted() -> Color {
    palette::color(Meaning::Nothing)
}

fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    terminal.backend().buffer().clone()
}

fn click(view: &mut View, column: u16, row: u16) -> Effect {
    view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE })
}

fn find(screen: &[String], what: &str) -> usize {
    screen.iter().position(|r| r.contains(what)).unwrap_or_else(|| panic!("no row holds {what:?}: {screen:#?}"))
}

/// The archive row's path (the impl's synthetic id, `rows::ARCHIVE`): the tests use the literal, never the private
/// constant, so they build before the row exists.
fn archive_path() -> Vec<String> {
    vec!["Archive".to_owned()]
}

/// The row `y` reads `expected` and every drawn cell of it is colour 8 (`palette::Meaning::Nothing`).
fn assert_muted_row(buf: &Buffer, y: u16, expected: &str) {
    let screen = rows(buf);
    let line = &screen[usize::from(y)];
    assert_eq!(line.trim(), expected, "row {y} is {line:?}");
    let start = line.chars().take_while(|c| *c == ' ').count();
    for (i, c) in expected.chars().enumerate() {
        let x = start + i;
        let cell = &buf[(u16::try_from(x).unwrap(), y)];
        assert_eq!(cell.symbol().chars().next(), Some(c), "cell {x} of row {y}");
        assert_eq!(cell.fg, muted(), "cell {x} of row {y} ({c:?}) is colour 8");
    }
}

fn file_in(dir: &tempfile::TempDir) -> FoldsFile {
    FoldsFile::at(&cpd_core::home::CpdHome::resolve(Some(dir.path().into()), None).unwrap())
}

#[test]
fn the_archive_row_reads_archive_with_the_folded_and_expanded_chevrons_in_colour_8() {
    let mut view = View::new(archived(), true);
    let buf = draw(&mut view, 44, 24);
    let y = find(&rows(&buf), " archive");
    assert_muted_row(&buf, y as u16, &format!("{DOWN} archive"));
    assert_eq!(click(&mut view, 3, y as u16), Effect::None, "a click toggles the fold and runs nothing");
    let buf = draw(&mut view, 44, 24);
    let y = find(&rows(&buf), " archive");
    assert_muted_row(&buf, y as u16, &format!("{UP} archive"));
}

#[test]
fn archived_contexts_render_under_the_archive_row_and_none_at_the_root() {
    let mut view = View::new(archived(), true);
    let screen = rows(&draw(&mut view, 44, 24));
    // folded: no zeta block anywhere, and the archive row is the last tree block
    assert!(!screen.iter().any(|r| r.contains("zeta") || r.contains("zp") || r.contains("f-zp")), "the folder starts folded: {screen:#?}");
    let archive = find(&screen, " archive");
    assert!(find(&screen, "gamma") < archive, "{screen:#?}");
    assert!(screen[archive + 1..].iter().all(String::is_empty), "the archive row is the last tree block: {screen:#?}");
    // a click shows zeta's block, folded as today, and the live Contexts above are unchanged
    assert_eq!(click(&mut view, 3, archive as u16), Effect::None);
    let buf = draw(&mut view, 44, 24);
    let screen = rows(&buf);
    let archive = find(&screen, " archive");
    let zeta = find(&screen, "zeta");
    let expand = find(&screen, "expand");
    assert!(archive < zeta && zeta < expand, "{screen:#?}");
    assert_eq!(screen[zeta].trim(), format!("╭{STOPPED} zeta"), "{screen:#?}");
    assert_eq!(screen[expand].trim(), format!("╰{DOWN} expand"), "{screen:#?}");
    assert_eq!((buf[(2, zeta as u16)].fg, buf[(4, zeta as u16)].fg), (muted(), muted()), "zeta's title is muted");
    assert!(!screen.iter().any(|r| r.contains("zp·f-zp")), "zeta's own toggle is folded: {screen:#?}");
    // zeta's own toggle shows its Plan and its fork, then its own closing toggle
    assert_eq!(click(&mut view, 3, expand as u16), Effect::None);
    let buf = draw(&mut view, 44, 24);
    let screen = rows(&buf);
    let zeta = find(&screen, "zeta");
    let plan = screen.iter().position(|r| r.contains(" zp")).expect("zp's Plan row");
    let fork = find(&screen, "zp·f-zp");
    let collapse = fork + screen[fork..].iter().position(|r| r.contains("collapse")).expect("zeta's closing row");
    assert!(zeta < plan && plan < fork && fork < collapse, "{screen:#?}");
    assert_eq!(screen[collapse].trim(), format!("╰{UP} collapse"), "{screen:#?}");
    for y in [plan, fork] {
        assert_eq!((buf[(3, y as u16)].fg, buf[(5, y as u16)].fg), (muted(), muted()), "row {y} is muted");
    }
}

#[test]
fn a_click_on_the_archive_row_toggles_the_archived_contexts_and_each_keeps_its_own_toggle() {
    let mut view = View::new(archived(), true);
    let screen = rows(&draw(&mut view, 44, 24));
    assert!(!screen.iter().any(|r| r.contains("zeta")), "the folder starts folded: {screen:#?}");
    // a click on the archive row shows zeta's block, still folded
    let archive = find(&screen, " archive");
    assert_eq!(click(&mut view, 3, archive as u16), Effect::None);
    let screen = rows(&draw(&mut view, 44, 24));
    let archive = find(&screen, " archive");
    assert_eq!(screen[archive].trim(), format!("{UP} archive"), "{screen:#?}");
    let expand = find(&screen, "expand");
    assert_eq!(screen[expand].trim(), format!("╰{DOWN} expand"), "{screen:#?}");
    assert!(!screen.iter().any(|r| r.contains("zp·f-zp")), "zeta is folded: {screen:#?}");
    // zeta's own toggle shows its Plan rows while the archive row stays expanded
    assert_eq!(click(&mut view, 3, expand as u16), Effect::None);
    let screen = rows(&draw(&mut view, 44, 24));
    assert_eq!(screen[find(&screen, " archive")].trim(), format!("{UP} archive"), "{screen:#?}");
    assert!(screen.iter().any(|r| r.contains("zp·f-zp")), "zeta's Plan rows show: {screen:#?}");
    // a second click on the archive row hides them and folds it again
    let archive = find(&screen, " archive");
    assert_eq!(click(&mut view, 3, archive as u16), Effect::None);
    let screen = rows(&draw(&mut view, 44, 24));
    assert_eq!(screen[find(&screen, " archive")].trim(), format!("{DOWN} archive"), "{screen:#?}");
    assert!(!screen.iter().any(|r| r.contains("zeta") || r.contains("zp") || r.contains("f-zp")), "{screen:#?}");
}

#[test]
fn the_archive_row_appears_only_when_a_context_is_archived() {
    let mut view = View::new(sample(), true);
    let screen = rows(&draw(&mut view, 44, 24));
    assert!(!screen.iter().any(|r| r.trim().ends_with(" archive")), "no archived Context, no archive row: {screen:#?}");
    let mut view = View::new(archived(), true);
    let screen = rows(&draw(&mut view, 44, 24));
    let archive: Vec<usize> = (0..screen.len()).filter(|&i| screen[i].trim().ends_with(" archive")).collect();
    assert_eq!(archive.len(), 1, "exactly one archive row: {screen:#?}");
    for live in ["alpha", "beta", "gamma"] {
        assert!(find(&screen, live) < archive[0], "the archive row follows the live Context {live}: {screen:#?}");
    }
}

#[test]
fn the_archive_row_is_clickable_and_never_selectable() {
    let mut view = View::new(archived(), true);
    let screen = rows(&draw(&mut view, 44, 24));
    let archive = find(&screen, " archive");
    // the whole row is a hit target that toggles it and runs no command
    assert_eq!(click(&mut view, 1, archive as u16), Effect::None);
    assert_eq!(rows(&draw(&mut view, 44, 24))[archive].trim(), format!("{UP} archive"));
    assert_eq!(click(&mut view, 40, archive as u16), Effect::None);
    assert_eq!(rows(&draw(&mut view, 44, 24))[archive].trim(), format!("{DOWN} archive"));
    // the cursor never rests on the archive path
    for delta in [1isize, -1] {
        for _ in 0..40 {
            tree_rows::move_cursor(&mut view.state, &view.tree, delta);
            assert_ne!(view.state.selected(), archive_path().as_slice(), "the cursor moved onto the archive path");
            if let Some(row) = tree_rows::row(&view.tree, view.state.selected()) {
                assert_ne!(row.label().trim(), "archive", "the selected row is the archive row");
            }
        }
    }
    // the archive row is neither a Session nor a Plan
    let row = tree_rows::row(&view.tree, &archive_path()).expect("the archive row exists");
    assert!(row.session().is_none(), "the archive row is no Session");
    assert!(row.plan().is_none(), "the archive row is no Plan");
}

#[test]
fn the_archive_fold_defaults_folded_and_persists_under_the_archived_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut view = View::new(archived(), true);
    view.keep_folds(file_in(&dir));
    let screen = rows(&draw(&mut view, 44, 24));
    assert!(!screen.iter().any(|r| r.contains("zeta")), "folded by default: {screen:#?}");
    let archive = find(&screen, " archive");
    assert_eq!(click(&mut view, 3, archive as u16), Effect::None);
    view.save_folds();
    let text = std::fs::read_to_string(dir.path().join("run/tui/tree-folds.json")).unwrap();
    assert!(text.contains("\"archived\":true"), "the fold is written under the archived key: {text}");
    let mut again = View::new(archived(), true);
    again.keep_folds(file_in(&dir));
    let screen = rows(&draw(&mut again, 44, 24));
    assert!(screen.iter().any(|r| r.contains("zeta")), "the folder comes back expanded: {screen:#?}");
}
