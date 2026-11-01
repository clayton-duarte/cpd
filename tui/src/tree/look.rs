//! The tree column's section look v3, as exact rows (commands.md §13.2 "Rows", "Clicks"; §17
//! "Collapsible blocks", "Section titles", "Scroll cues"; hs-d344 to hs-d349). The
//! Context blocks fold only their archived rows and close with an arrow row (or the terminator `╰󰬪` when no rows; hs-d574).
//!
//! The view is drawn on a `TestBackend` and read back cell by cell, so the rows and the styles that matter are pinned
//! together. Every row is found by what it says, not by a count of rows from the top, where the look allows.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::{Color, Modifier};

use super::fixture::{archived, sample};
use super::view::{Effect, View};
use crate::palette::{self, IDLE, Meaning, NEEDS_YOU, SPINNER, STOPPED};
use crate::test_text::rows;
use cpd_core::inbox::model::Urgency;

/// U+F0B26 md-arrow_down_drop_circle_outline: the folded block's arrow.
const DOWN: &str = "\u{F0B26}";
/// U+F0B2C md-arrow_up_drop_circle_outline: the expanded block's arrow.
const UP: &str = "\u{F0B2C}";
const WORKING: &str = SPINNER[0];

fn muted() -> Color {
    palette::color(Meaning::Nothing)
}

fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    terminal.backend().buffer().clone()
}

fn pointer(view: &mut View, kind: MouseEventKind, column: u16, row: u16) -> Effect {
    view.mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE })
}

fn click(view: &mut View, column: u16, row: u16) -> Effect {
    pointer(view, MouseEventKind::Down(MouseButton::Left), column, row)
}

/// The row of `rows` that holds `what`.
fn find(rows: &[String], what: &str) -> usize {
    rows.iter().position(|r| r.contains(what)).unwrap_or_else(|| panic!("no row holds {what:?}: {rows:#?}"))
}

/// `rows` start with `expected`, one each, in order: the rows that carry counters are only prefixes.
fn assert_prefixes(rows: &[String], first: usize, expected: &[String]) {
    for (i, want) in expected.iter().enumerate() {
        assert!(rows[first + i].starts_with(want.as_str()), "row {}: want {want:?}, got {:?}\n{rows:#?}", first + i, rows[first + i]);
    }
}

fn is_muted_line(buf: &Buffer, y: u16, from: u16, to: u16) {
    let mut seen = false;
    for x in from..to {
        let cell = &buf[(x, y)];
        if cell.symbol() != " " {
            seen = true;
            // the rule glyphs are muted (8); the toggle's arrow and word are read-only text (7)
            let want = if "╭├╰│─󰬪".contains(cell.symbol()) { muted() } else { palette::toggle_text().fg.unwrap() };
            assert_eq!(cell.fg, want, "column {x} of row {y} ({:?}) is muted, or the toggle's 7", cell.symbol());
            assert!(!cell.modifier.contains(Modifier::BOLD), "column {x} of row {y} is not bold");
        }
    }
    assert!(seen, "row {y} is blank");
}

// ---------- the Context blocks: the toggle shows and hides only the archived rows (hs-d344) ----------

/// `alpha` of `archived()`: three live rows (ship, sketch and the fork ship·f-id), then two archived ones (the Plan
/// old and the fork ship·f-bl).
fn alpha_live() -> Vec<String> {
    vec![
        format!(" ╭{NEEDS_YOU} alpha"),
        format!(" ├─{WORKING} ship"),
        format!(" ├─{IDLE} sketch"),
        format!(" ├─{IDLE} ship·f-id"),
    ]
}

fn alpha_archived() -> Vec<String> {
    vec![format!(" ├─{STOPPED} old"), format!(" ├─{NEEDS_YOU} ship·f-bl")]
}

#[test]
fn an_expanded_context_shows_its_live_rows_then_its_archived_rows_and_closes_with_the_up_arrow() {
    let mut view = View::new(archived(), true);
    let buf = draw(&mut view, 44, 24);
    let got = rows(&buf);
    let want = [alpha_live(), alpha_archived()].concat();
    assert_prefixes(&got, 4, &want);
    assert_eq!(got[4 + want.len()], format!(" ╰{UP} collapse"), "one closing row, the arrow and the word: {got:#?}");
    assert_eq!(got[5 + want.len()], "", "one empty row between two Contexts: {got:#?}");
    assert!(got[6 + want.len()].starts_with(&format!(" ╭{STOPPED} gamma")), "the next Context follows: {got:#?}");
}

#[test]
fn the_archived_rows_are_muted_the_live_rows_are_not_and_the_closing_row_is_all_muted() {
    let mut view = View::new(archived(), true);
    let buf = draw(&mut view, 44, 24);
    let got = rows(&buf);
    for (title, is_archived) in [("sketch", false), ("ship·f-id", false), ("old", true), ("ship·f-bl", true)] {
        let y = find(&got, title) as u16;
        let glyph = &buf[(3, y)];
        assert_eq!(glyph.fg == muted(), is_archived, "{title}: the glyph {:?} is {:?}", glyph.symbol(), glyph.fg);
        let name = &buf[(5, y)];
        assert_eq!(name.fg == muted(), is_archived, "{title}: its name starts {:?} in {:?}", name.symbol(), name.fg);
    }
    let close = find(&got, "collapse") as u16;
    assert_eq!((buf[(1, close)].symbol(), buf[(2, close)].symbol()), ("╰", UP), "the corner and the up arrow");
    is_muted_line(&buf, close, 1, 14);
}

#[test]
fn folding_a_context_hides_only_its_archived_rows_and_the_live_ones_stay_above_the_down_arrow() {
    let mut view = View::new(archived(), true);
    let got = rows(&draw(&mut view, 44, 24));
    let toggle = find(&got, "collapse") as u16;
    assert_eq!(click(&mut view, 3, toggle), Effect::None, "a view-state change, no command");
    let got = rows(&draw(&mut view, 44, 24));
    assert_prefixes(&got, 4, &alpha_live());
    assert_eq!(got[8], format!(" ╰{DOWN} expand"), "the live rows, then the toggle: {got:#?}");
    assert!(!got.iter().any(|r| r.contains(" old") || r.contains("f-bl")), "the archived rows are hidden: {got:#?}");
    assert_eq!(got[9], "", "{got:#?}");
    assert!(got[10].starts_with(&format!(" ╭{STOPPED} gamma")), "{got:#?}");
    // and the same click unfolds it
    assert_eq!(click(&mut view, 3, 8), Effect::None);
    assert_prefixes(&rows(&draw(&mut view, 44, 24)), 4, &[alpha_live(), alpha_archived()].concat());
}

#[test]
fn the_down_arrow_row_is_muted_and_only_it_toggles() {
    let mut view = View::new(archived(), true);
    let got = rows(&draw(&mut view, 44, 24));
    click(&mut view, 3, find(&got, "collapse") as u16);
    let buf = draw(&mut view, 44, 24);
    is_muted_line(&buf, 8, 1, 12);
    assert_eq!(buf[(2, 8)].symbol(), DOWN);
    // the title row and a live row keep their own actions and fold nothing
    click(&mut view, 3, 4);
    assert_eq!(rows(&draw(&mut view, 44, 24))[8], format!(" ╰{DOWN} expand"), "still folded after a click on a live row");
}

#[test]
fn an_archived_context_is_all_archived_rows_folded_it_shows_its_title_and_the_down_arrow() {
    let mut view = View::new(archived(), true);
    let got = rows(&draw(&mut view, 44, 24));
    click(&mut view, 3, find(&got, " archive") as u16);
    let got = rows(&draw(&mut view, 44, 24));
    let zeta = find(&got, "zeta");
    assert_eq!(got[zeta + 1], format!(" ╰{DOWN} expand"), "{got:#?}");
    click(&mut view, 3, zeta as u16 + 1);
    let got = rows(&draw(&mut view, 44, 24));
    let zeta = find(&got, "zeta");
    let close = zeta + got[zeta..].iter().position(|r| r.contains("collapse")).expect("zeta's closing row");
    assert_eq!(got[close], format!(" ╰{UP} collapse"));
    assert!(close - zeta >= 2, "its archived rows are drawn between the title and the closing row: {got:#?}");
}

#[test]
fn a_context_with_no_archived_rows_ends_with_its_last_rows_corner_and_has_no_toggle() {
    let mut view = View::new(sample(), true);
    let buf = draw(&mut view, 44, 24);
    let got = rows(&buf);
    // alpha (five live rows), beta (one) and gamma (none): no archived row anywhere, so no closing row but gamma's
    assert!(got[9].starts_with(&format!(" ╰─{IDLE} ship·f-id")), "alpha's last row ends the block: {got:#?}");
    assert!(got[2].starts_with(&format!(" ╰─{NEEDS_YOU} rel")), "beta's: {got:#?}");
    assert_eq!(got[12], " ╰󰬪 new", "gamma, with no row at all, ends with the clickable new row: {got:#?}");
    assert!(!got.iter().any(|r| r.contains("expand") || r.contains("collapse") || r.contains(DOWN) || r.contains(UP)), "no arrow: {got:#?}");
    for y in [9, 2] {
        is_muted_line(&buf, y, 1, 3);
    }
    is_muted_line(&buf, 12, 1, 3);
    // a live empty Context's new row opens the New Plan popup and folds nothing
    assert!(matches!(click(&mut view, 2, 12), Effect::Ask(_)), "the new row asks for the New Plan title");
    assert_eq!(rows(&draw(&mut view, 44, 24)), got, "nothing folded");
}

// ---------- the Inbox's tier glyphs (hs-d483) ----------

/// The six tiers, red to white (the order of `cpd inbox list`): urgency, glyph (code point, never a copied glyph) and the
/// colour of its urgency, through the palette.
fn tiers() -> [(Urgency, &'static str, Color); 6] {
    [
        (Urgency::Red, "\u{F0362}", palette::color_clickable(Meaning::Failed)),
        (Urgency::Yellow, "\u{F069A}", palette::color_clickable(Meaning::Review)),
        (Urgency::Magenta, "\u{F173A}", palette::color_clickable(Meaning::Planning)),
        (Urgency::Blue, "\u{F0368}", palette::color_clickable(Meaning::External)),
        (Urgency::Cyan, "\u{F06F1}", palette::color_clickable(Meaning::Notice)),
        (Urgency::White, "\u{F0366}", palette::color_clickable(Meaning::NotStarted)),
    ]
}

#[test]
fn the_tier_glyphs_are_red_f0362_yellow_f069a_magenta_f173a_cyan_f06f1_white_f0366() {
    assert_eq!(
        [palette::TIER_RED, palette::TIER_YELLOW, palette::TIER_MAGENTA, palette::TIER_CYAN, palette::TIER_WHITE],
        ["\u{F0362}", "\u{F069A}", "\u{F173A}", "\u{F06F1}", "\u{F0366}"]
    );
    for (urgency, glyph, colour) in tiers() {
        assert_eq!(palette::tier_span(urgency), ratatui::text::Span::styled(glyph, ratatui::style::Style::new().fg(colour)), "{urgency:?}");
    }
}

#[test]
fn the_terminator_of_a_block_that_cannot_fold_is_colour_8_corner_and_glyph_with_no_word() {
    const TERMINATOR: &str = "\u{F0B2A}";
    // an ARCHIVED empty Context keeps the plain terminator (`plan new` is refused there; hs-d688)
    let mut gamma = super::fixture::context("gamma", vec![]);
    gamma.archived_at = Some(super::fixture::WHEN.into());
    let mut empty = View::new(super::fixture::tree(vec![gamma]), true);
    let closed = rows(&draw(&mut empty, 44, 10));
    click(&mut empty, 3, find(&closed, " archive") as u16);
    let archived_empty = ("an archived Context with no row", draw(&mut empty, 44, 10), 3u16);
    for (name, buf, y) in [archived_empty] {
        let width = buf.area.width;
        let cells: Vec<(u16, &str)> = (0..width).map(|x| (x, buf[(x, y)].symbol())).filter(|(_, s)| *s != " ").collect();
        assert_eq!(cells, [(1, "╰"), (2, TERMINATOR)], "{name}: only the corner and the glyph, no word, no `─`: {:#?}", rows(&buf));
        for (x, _) in cells {
            let cell = &buf[(x, y)];
            assert_eq!(cell.fg, palette::muted_text().fg.unwrap(), "{name}: column {x} is colour 8");
            assert_eq!(cell.fg, muted(), "{name}: column {x} is the muted colour");
            assert!(!cell.modifier.contains(Modifier::BOLD), "{name}: column {x} is not bold");
        }
    }
}
