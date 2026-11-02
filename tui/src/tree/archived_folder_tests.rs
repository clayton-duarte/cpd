//! The tree's docked Archive section and its fold (commands.md §13.2 "The Archive section"; contract §6.1.1; pro-d348,
//! pro-d349, amending hs-d698 and hs-d707): the archived Contexts are no row of the project tree (it has no `archive` row);
//! they are the list of a collapsible section `Archive` docked at the bottom of the pane, below Standing. Its header is
//! `───Archive────…<glyph>─`, the left pill `Archive` and the right pill the fold glyph (U+F0B26 open, U+F0B2C folded), both
//! static grey pills, the lines colour 8. It is folded by default, drawn only when at least one Context is archived, its
//! fold is persisted under `"archived"` in `run/tui/tree-folds.json`; its archived Contexts keep their own closing toggle
//! and are never cursor stops. The group is ONE blank row, then the section: open, a header, one reserved row, the blocks
//! and one reserved row; folded, the header only (the pane's last row).
//!
//! Red until the tree docks the section: today's tree draws the row `󰬦 archive` instead, so every test here fails at
//! runtime on an assertion, never at build time. Every pane is 30 columns wide and 16 rows high (the group is capped at
//! half of it, 8 rows, and every group below fits), and every expected row is built here from the docs.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;

use super::fixture::{archived, tree};
use super::folds::FoldsFile;
use super::rows as tree_rows;
use super::view::{Effect, View};
use crate::palette::{self, Meaning, STOPPED};
use crate::read::Tree;
use crate::section::check::grey_pill;
use crate::test_text::rows;

/// U+F0B26 md-arrow_down_drop_circle_outline: the section open (and a Context's closing row `expand`).
const DOWN: &str = "\u{F0B26}";
/// U+F0B2C md-arrow_up_drop_circle_outline: the section folded (and a Context's closing row `collapse`).
const UP: &str = "\u{F0B2C}";
/// The closing row `╰ new` of a Context with no Plan.
const NEW: &str = "\u{F0B2A}";
const CAP_LEFT: &str = "\u{E0B6}";
const CAP_RIGHT: &str = "\u{E0B4}";
/// The pane's size.
const W: u16 = 30;
const H: u16 = 16;

/// Colour 8, the archived rows' and the lines' colour.
fn muted() -> Color {
    palette::color(Meaning::Nothing)
}

fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    terminal.backend().buffer().clone()
}

fn pane(view: &mut View) -> Buffer {
    draw(view, W, H)
}

fn click(view: &mut View, column: u16, row: u16) -> Effect {
    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn right_click(view: &mut View, column: u16, row: u16) -> Effect {
    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

fn find(screen: &[String], what: &str) -> usize {
    screen
        .iter()
        .position(|r| r.contains(what))
        .unwrap_or_else(|| panic!("no row holds {what:?}: {screen:#?}"))
}

fn file_in(dir: &tempfile::TempDir) -> FoldsFile {
    FoldsFile::at(&cpd_core::home::CpdHome::resolve(Some(dir.path().into()), None).unwrap())
}

// ---------- the fixture and the rows ----------

/// The live Context `gamma` (no Plan) and the archived Context `zeta` (its Plan `zp` with a Lead and a fork), the
/// smallest tree with an Archive section.
fn small() -> Tree {
    let mut t = archived();
    t.contexts.drain(..2);
    assert_eq!(t.contexts.len(), 2, "gamma and zeta");
    t
}

/// The same tree with `zeta` unarchived: no Archive section.
fn live_only() -> Tree {
    tree(vec![super::fixture::context("gamma", vec![])])
}

/// The project tree's rows for `small()`: the block of `gamma` (its Plan-less closing row is `╰ new`).
fn gamma() -> Vec<String> {
    vec![format!(" ╭{STOPPED} gamma"), format!(" ╰{NEW} new")]
}

/// `zeta` folded: the title and the closing row `expand` (its archived Plan and fork hidden).
fn zeta_folded() -> Vec<String> {
    vec![format!(" ╭{STOPPED} zeta"), format!(" ╰{DOWN} expand")]
}

/// A row with `count` right-aligned to end at column `W - 2`, as the tree draws a Plan's counters.
fn counted(left: &str, count: &str) -> String {
    format!(
        "{left}{count:>0$}",
        usize::from(W) - 1 - left.chars().count()
    )
}

/// `zeta` with its own toggle open: its Plan and its fork, then the closing row `collapse`.
fn zeta_open() -> Vec<String> {
    vec![
        format!(" ╭{STOPPED} zeta"),
        counted(&format!(" ├─{STOPPED} zp"), "2"),
        format!(" ├─{STOPPED} zp·f-zp"),
        format!(" ╰{UP} collapse"),
    ]
}

/// The header row of the Archive section: `───`, the pill `Archive`, the lines, the pill of `glyph` and `─` at the last column.
fn header(glyph: &str) -> String {
    let fill = usize::from(W) - 3 - ("Archive".len() + 2) - 3 - 1;
    format!(
        "───{CAP_LEFT}Archive{CAP_RIGHT}{}{CAP_LEFT}{glyph}{CAP_RIGHT}─",
        "─".repeat(fill)
    )
}

/// The group of a FOLDED Archive section: the blank row and the header, the pane's last row.
fn folded_group() -> Vec<String> {
    vec![String::new(), header(UP)]
}

/// The group of an OPEN Archive section: the blank row, the header, the reserved row, the blocks, the reserved row.
fn open_group(list: &[String]) -> Vec<String> {
    let mut g = vec![String::new(), header(DOWN), String::new()];
    g.extend(list.iter().cloned());
    g.push(String::new());
    g
}

/// The whole screen of the pane: the project tree's `▲` cue row (blank), its `live` rows, blank rows (the middle, ending
/// with its `▼` cue row), then the `group` at the bottom, whose height the project tree does not take.
fn screen(live: &[String], group: &[String]) -> Vec<String> {
    let mut rows = vec![String::new()];
    rows.extend(live.iter().cloned());
    rows.resize(usize::from(H) - group.len(), String::new());
    rows.extend(group.iter().cloned());
    assert_eq!(rows.len(), usize::from(H));
    rows
}

/// The row `y` of the group: from the header at `y`, rows are counted from the pane's top.
fn header_row(group: &[String]) -> u16 {
    H - u16::try_from(group.len()).unwrap() + 1
}

/// Every drawn mark of the row `y` in `columns` is colour 8.
fn assert_muted(buf: &Buffer, y: u16, columns: &[u16]) {
    for &x in columns {
        assert_eq!(
            buf[(x, y)].fg,
            muted(),
            "cell {x} of row {y} ({:?}) is colour 8",
            buf[(x, y)].symbol()
        );
    }
}

// ---------- the tests ----------

#[test]
fn the_archive_header_reads_archive_with_the_down_and_up_glyphs_in_the_right_pill() {
    let mut view = View::new(small(), true);
    // folded: the pane's last row, the glyph U+F0B2C in the right pill
    let buf = pane(&mut view);
    let last = H - 1;
    assert_eq!(rows(&buf)[usize::from(last)], header(UP));
    assert_eq!(header(UP).chars().count(), usize::from(W), "30 cells");
    // `Archive` is the left grey pill (columns 3 to 11), the glyph the right grey pill (26 to 28)
    grey_pill(&buf, 3, last, "Archive");
    grey_pill(&buf, 26, last, UP);
    // the lines (`───` before, the fill between, `─` at the last column) are colour 8
    let lines: Vec<u16> = (0..3).chain(12..26).chain([29]).collect();
    assert_muted(&buf, last, &lines);
    assert_eq!(
        click(&mut view, 5, last),
        Effect::None,
        "a click folds, it runs nothing"
    );
    // open: the header moves up with the section, the glyph is U+F0B26
    let buf = pane(&mut view);
    let at = header_row(&open_group(&zeta_folded()));
    assert_eq!(rows(&buf)[usize::from(at)], header(DOWN));
    grey_pill(&buf, 3, at, "Archive");
    grey_pill(&buf, 26, at, DOWN);
    assert_muted(&buf, at, &lines);
}

#[test]
fn archived_contexts_render_in_the_archive_section_and_none_in_the_project_tree() {
    // the whole screen, folded: the project tree holds only `gamma`, no `archive` row, no `zeta`
    let mut view = View::new(small(), true);
    assert_eq!(
        rows(&pane(&mut view)),
        screen(&gamma(), &folded_group()),
        "folded: gamma at the top, the header at the bottom"
    );
    // open: zeta's block (folded as an archived Context starts) between the two reserved rows
    click(&mut view, 5, H - 1);
    let buf = pane(&mut view);
    let group = open_group(&zeta_folded());
    assert_eq!(rows(&buf), screen(&gamma(), &group));
    let title = header_row(&group) + 2;
    assert_muted(&buf, title, &[1, 2, 4]);
    // the closing row keeps the fold toggle's own colour: the corner is colour 8, the glyph and the word colour 7
    assert_muted(&buf, title + 1, &[1]);
    for x in [2, 4] {
        assert_eq!(
            buf[(x, title + 1)].fg,
            palette::toggle_text().fg.unwrap(),
            "cell {x} of the closing row ({:?}) is the toggle's colour",
            buf[(x, title + 1)].symbol()
        );
    }
    // the fixture with archived objects in a live Context: no row above the group holds the archived Context, none reads `archive`
    let mut view = View::new(archived(), true);
    draw(&mut view, W, 40);
    click(&mut view, 5, 39);
    let screen = rows(&draw(&mut view, W, 40));
    let header_at = find(&screen, "Archive");
    for (y, row) in screen[..header_at].iter().enumerate() {
        assert!(
            !row.contains("zeta") && !row.contains("zp") && !row.contains("f-zp"),
            "row {y} above the Archive header holds an archived Context: {screen:#?}"
        );
        assert!(
            !row.ends_with(" archive"),
            "the project tree has no archive row: {screen:#?}"
        );
    }
    assert!(
        find(&screen, "gamma") < header_at,
        "the live Contexts are above: {screen:#?}"
    );
    assert_eq!(
        screen[header_at + 2].trim(),
        format!("╭{STOPPED} zeta"),
        "{screen:#?}"
    );
    assert_eq!(
        screen[header_at + 3].trim(),
        format!("╰{DOWN} expand"),
        "{screen:#?}"
    );
}

#[test]
fn a_click_on_the_archive_header_toggles_the_archived_contexts_and_each_keeps_its_own_toggle() {
    let mut view = View::new(small(), true);
    let folded = screen(&gamma(), &folded_group());
    assert_eq!(rows(&pane(&mut view)), folded, "folded by default");
    // a click on the header opens it: zeta's block, still folded
    assert_eq!(click(&mut view, 5, H - 1), Effect::None);
    let closed = screen(&gamma(), &open_group(&zeta_folded()));
    assert_eq!(rows(&pane(&mut view)), closed);
    // zeta's own closing toggle (`expand`, at row 14) shows its Plan and fork while the section stays open
    assert_eq!(click(&mut view, 3, 14), Effect::None);
    let open = screen(&gamma(), &open_group(&zeta_open()));
    assert_eq!(rows(&pane(&mut view)), open);
    // its closing row (`collapse`, now at row 14 again) hides them again, the section stays open
    assert_eq!(click(&mut view, 3, 14), Effect::None);
    assert_eq!(rows(&pane(&mut view)), closed);
    // the header (row 11) folds the section
    assert_eq!(click(&mut view, 5, 11), Effect::None);
    assert_eq!(rows(&pane(&mut view)), folded);
    // the reserved rows and the blank rows are no hit: opened again, a click on one changes nothing
    click(&mut view, 5, H - 1);
    assert_eq!(rows(&pane(&mut view)), closed);
    for y in [10, 12, 15] {
        assert_eq!(click(&mut view, 5, y), Effect::None);
        assert_eq!(rows(&pane(&mut view)), closed, "row {y} is no hit");
    }
}

#[test]
fn the_archive_section_appears_only_when_a_context_is_archived() {
    let mut view = View::new(live_only(), true);
    assert_eq!(
        rows(&pane(&mut view)),
        screen(&gamma(), &[]),
        "no archived Context, no Archive section: the project tree alone"
    );
    let mut view = View::new(small(), true);
    let all = rows(&pane(&mut view));
    let headers: Vec<usize> = (0..all.len())
        .filter(|&i| all[i].contains("Archive"))
        .collect();
    assert_eq!(
        headers,
        [usize::from(H) - 1],
        "exactly one Archive header: {all:#?}"
    );
}

#[test]
fn the_archive_header_is_clickable_and_never_selectable() {
    let mut view = View::new(small(), true);
    let folded = screen(&gamma(), &folded_group());
    let open = screen(&gamma(), &open_group(&zeta_folded()));
    // a View has hit geometry from its last frame only
    pane(&mut view);
    // the whole header row is one hit target that toggles it and runs no command
    assert_eq!(click(&mut view, 1, H - 1), Effect::None);
    assert_eq!(rows(&pane(&mut view)), open);
    let header_at = header_row(&open_group(&zeta_folded()));
    assert_eq!(click(&mut view, 28, header_at), Effect::None);
    assert_eq!(rows(&pane(&mut view)), folded);
    // a right click on the header opens no menu
    assert_eq!(right_click(&mut view, 5, H - 1), Effect::None);
    assert_eq!(rows(&pane(&mut view)), folded, "no menu");
    // open again: the cursor crosses the rows but never stops on the header or on an archived Context
    click(&mut view, 5, H - 1);
    assert_eq!(rows(&pane(&mut view)), open);
    for delta in [1isize, -1] {
        for _ in 0..40 {
            tree_rows::move_cursor(&mut view.state, &view.tree, delta);
            let selected = view.state.selected();
            assert_ne!(
                selected,
                ["Archive".to_owned()].as_slice(),
                "the cursor moved onto the Archive header"
            );
            assert_ne!(
                selected.first().map(String::as_str),
                Some("zeta"),
                "the cursor moved onto an archived Context: {selected:?}"
            );
        }
    }
    // the header is neither a Session nor a Plan
    let row = tree_rows::row(&view.tree, &["Archive".to_owned()])
        .expect("the Archive header's row exists");
    assert!(row.session().is_none(), "the header is no Session");
    assert!(row.plan().is_none(), "the header is no Plan");
    assert_eq!(right_click(&mut view, 5, header_at), Effect::None);
    assert_eq!(
        rows(&pane(&mut view)),
        open,
        "no menu on an open header either"
    );
}

#[test]
fn the_archive_fold_defaults_folded_and_persists_under_the_archived_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut view = View::new(small(), true);
    view.keep_folds(file_in(&dir));
    assert_eq!(
        rows(&pane(&mut view)),
        screen(&gamma(), &folded_group()),
        "folded by default"
    );
    assert_eq!(click(&mut view, 5, H - 1), Effect::None);
    view.save_folds();
    let text = std::fs::read_to_string(dir.path().join("run/tui/tree-folds.json")).unwrap();
    assert!(
        text.contains("\"archived\":true"),
        "the fold is written under the archived key: {text}"
    );
    let mut again = View::new(small(), true);
    again.keep_folds(file_in(&dir));
    assert_eq!(
        rows(&pane(&mut again)),
        screen(&gamma(), &open_group(&zeta_folded())),
        "a fresh View over that file starts open"
    );
    // folding it again writes the default: the key goes away, the file format is unchanged
    click(&mut again, 5, header_row(&open_group(&zeta_folded())));
    again.save_folds();
    let text = std::fs::read_to_string(dir.path().join("run/tui/tree-folds.json")).unwrap();
    assert!(
        !text.contains("archived"),
        "folded is the default, not written: {text}"
    );
}
