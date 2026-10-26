//! The tree column's section look v3, as exact rows (commands.md §13.2 "Rows", "Inbox section", "Clicks"; §17
//! "Collapsible blocks", "Section titles", "Scroll cues"; hs-d344 to hs-d349). Red until the tree draws them: the
//! Context blocks fold only their archived rows and close with an arrow row (or the terminator `╰󰬪` when no rows; hs-d574), the Inbox's title is the
//! pill under one empty row, its item rows keep their icon and colour, and its scroll cues are right-aligned.
//!
//! The view is drawn on a `TestBackend` and read back cell by cell, so the rows and the styles that matter are pinned
//! together. Every row is found by what it says, not by a count of rows from the top, where the look allows.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::{Color, Modifier};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::fixture::{archived, sample};
use super::inbox::item;
use super::view::{Effect, View};
use crate::palette::{self, IDLE, Meaning, NEEDS_YOU, SPINNER, STOPPED};
use crate::test_text::rows;
use cpd_core::inbox::model::{Item, Kind, Urgency};

/// U+F0B26 md-arrow_down_drop_circle_outline: the folded block's arrow.
const DOWN: &str = "\u{F0B26}";
/// U+F0B2C md-arrow_up_drop_circle_outline: the expanded block's arrow.
const UP: &str = "\u{F0B2C}";
/// The pill's caps, U+E0B6 and U+E0B4.
const CAP_LEFT: &str = "\u{E0B6}";
const CAP_RIGHT: &str = "\u{E0B4}";
const WORKING: &str = SPINNER[0];

fn muted() -> Color {
    palette::color(Meaning::Nothing)
}

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &Rfc3339).unwrap()
}

fn draw(view: &mut View, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw_at(f, 0, now())).unwrap();
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

// ---------- the Inbox section ----------

/// Three open items with their kind's urgency colour: red, yellow, magenta, then white, with no link in them.
fn inbox_items(n: usize) -> Vec<Item> {
    let mut dialog = item(1, Kind::Dialog, Urgency::Red, "f-blocked: Worker question", "2026-01-02T23:59:18.000Z");
    dialog.session = Some(cpd_core::ids::SessionId::parse("f-blocked").unwrap());
    let mut stuck = item(2, Kind::JobStuck, Urgency::Yellow, "ship: Job c is stuck", "2026-01-02T21:00:00.000Z");
    stuck.plan = Some(cpd_core::ids::PlanId::parse("ship").unwrap());
    let mut error = item(3, Kind::EngineError, Urgency::Magenta, "engine: store busy", "2026-01-01T00:00:00.000Z");
    error.event = Some(9);
    let mut unread = item(4, Kind::Unread, Urgency::White, "ship lead has a reply", "2026-01-02T23:50:00.000Z");
    unread.session = Some(cpd_core::ids::SessionId::parse("l-ship").unwrap());
    [dialog, stuck, error, unread].into_iter().take(n).collect()
}

fn with_inbox(items: Vec<Item>) -> View {
    let mut view = View::new(sample(), true);
    view.tree.inbox = items;
    view
}

/// The pill title row at `inner` columns (the pane has one blank column on each side): `╭─`, the capped title, a muted
/// line, the count as a pill (hs-d480).
fn pill(inner: usize, count: usize) -> String {
    format!(" ╭─{CAP_LEFT}Inbox{CAP_RIGHT}{}{CAP_LEFT}{count}{CAP_RIGHT}", "─".repeat(inner - 2 - 7 - (count.to_string().len() + 2)))
}

/// An item row at 41 inner columns: `│`, one space, the tier glyph, a space, the label, the age at column 38 (the rail's
/// padding takes the last two inner columns).
fn item_row(glyph: &str, label: &str, age: &str) -> String {
    format!(" │ {glyph} {label}{}{age}", " ".repeat(39 - 4 - label.chars().count() - age.chars().count()))
}

#[test]
fn the_inbox_title_is_the_pill_under_one_empty_row_and_the_rows_follow_at_once() {
    let mut view = with_inbox(inbox_items(3));
    let got = rows(&draw(&mut view, 43, 24));
    let want = [
        String::new(),
        pill(41, 3),
        item_row(palette::TIER_RED, "f-blocked: Worker question", "42s"),
        item_row(palette::TIER_YELLOW, "ship: Job c is stuck", "3h"),
        item_row(palette::TIER_MAGENTA, "engine: store busy", "2d"),
        " ╰󰬪".to_owned(),
        String::new(),
    ];
    assert_eq!(&got[got.len() - want.len()..], &want[..], "one empty row, the title, three rows, the terminator (nothing to fold) and one empty row: {got:#?}");
}

#[test]
fn the_empty_row_above_the_inbox_title_is_never_a_tree_row_in_a_short_pane() {
    let mut view = with_inbox(inbox_items(3));
    let got = rows(&draw(&mut view, 43, 14));
    let title = find(&got, "Inbox");
    assert_eq!(got[title - 1], "", "the empty row sits between the tree and the title: {got:#?}");
    assert_ne!(got[title - 2], "", "and the tree is above it: {got:#?}");
    assert_eq!(got[title], pill(41, 3));
}

#[test]
fn with_no_item_the_section_is_the_empty_row_and_the_pill_with_a_zero() {
    let mut view = with_inbox(Vec::new());
    let got = rows(&draw(&mut view, 43, 16));
    assert_eq!(&got[12..], [String::new(), pill(41, 0), " ╰\u{F0B2A} up to date, drink some water".to_owned(), String::new()], "the empty row, the pill with a zero, the closing row, the empty row: {got:#?}");
}

#[test]
fn the_pill_is_grey_the_caps_and_lines_muted_and_the_text_inverted() {
    let mut view = with_inbox(inbox_items(3));
    let buf = draw(&mut view, 43, 24);
    let y = find(&rows(&buf), "Inbox") as u16;
    assert_eq!((buf[(1, y)].symbol(), buf[(1, y)].fg), ("╭", muted()));
    assert_eq!((buf[(2, y)].symbol(), buf[(2, y)].fg), ("─", muted()));
    crate::section::check::grey_pill(&buf, 3, y, "Inbox");
    crate::section::check::grey_pill(&buf, 39, y, "3"); // the count is a grey pill too (hs-d480), ending at column 41
    assert_eq!((buf[(10, y)].symbol(), buf[(10, y)].fg), ("─", muted()), "the line to the right edge");
}

#[test]
fn four_items_show_three_and_the_down_arrow_a_click_expands_to_the_up_arrow_and_folds_again() {
    let mut view = with_inbox(inbox_items(4));
    let buf = draw(&mut view, 43, 24);
    let got = rows(&buf);
    assert_eq!(got[got.len() - 2], format!(" ╰{DOWN} expand"), "{got:#?}");
    assert_eq!(got[got.len() - 1], "", "the empty row below the closing row");
    assert_eq!(got[got.len() - 5], item_row(palette::TIER_RED, "f-blocked: Worker question", "42s"));
    assert_eq!(got[got.len() - 3], item_row(palette::TIER_MAGENTA, "engine: store busy", "2d"), "the third is the last shown");
    assert!(!got.iter().any(|r| r.contains("ship lead has a reply")), "the fourth is behind the toggle: {got:#?}");
    is_muted_line(&buf, 22, 1, 12);
    assert_eq!(click(&mut view, 3, 22), Effect::None, "view state only");
    let got = rows(&draw(&mut view, 43, 24));
    assert_eq!(got[got.len() - 2], format!(" ╰{UP} collapse"), "{got:#?}");
    assert_eq!(got[got.len() - 3], item_row(palette::TIER_WHITE, "ship lead has a reply", "10m"), "the fourth row is shown, at the bottom");
    assert_eq!(click(&mut view, 3, 22), Effect::None);
    assert_eq!(rows(&draw(&mut view, 43, 24))[22], format!(" ╰{DOWN} expand"), "folded again");
}

// ---------- scroll cues in the Inbox: right-aligned, and the line on their left unbroken (hs-d348) ----------

/// An expanded Inbox of `n` items in a 43 × 40 pane (a section capped at half the column).
fn expanded(n: usize) -> View {
    let items = (0..n)
        .map(|i| {
            let mut it = item(i as i64 + 1, Kind::Unread, Urgency::White, &format!("item {i:02}"), "2026-01-02T00:00:00.000Z");
            it.session = Some(cpd_core::ids::SessionId::parse("m-alpha").unwrap());
            it
        })
        .collect();
    let mut view = with_inbox(items);
    let got = rows(&draw(&mut view, 43, 40));
    click(&mut view, 3, find(&got, "expand") as u16);
    view
}

/// A cue row at 41 inner columns: the line `│` kept on the left, the cue right-aligned at the last column.
fn cue_row(arrow: &str, n: usize) -> String {
    format!(" │{:>38}", format!("{arrow} {n} more"))
}

/// The cue rows of `got`: their arrow and count.
fn cues(got: &[String]) -> Vec<(char, usize)> {
    got.iter()
        .filter_map(|r| {
            let arrow = r.chars().find(|c| matches!(c, '▲' | '▼'))?;
            let n = r.split_whitespace().rev().nth(1)?.parse().ok()?;
            Some((arrow, n))
        })
        .collect()
}

#[test]
fn an_overflowing_inbox_draws_its_cue_at_the_right_end_of_its_row_with_the_line_still_on_the_left() {
    let mut view = expanded(50);
    let buf = draw(&mut view, 43, 40);
    let got = rows(&buf);
    let [(arrow, n)] = cues(&got)[..] else { panic!("one cue (the first page): {got:#?}") };
    assert_eq!(arrow, '▼');
    let y = find(&got, "▼");
    assert_eq!(got[y], cue_row("▼", n), "right-aligned, after the muted `│`");
    assert_eq!((buf[(1, y as u16)].symbol(), buf[(1, y as u16)].fg), ("│", muted()), "the line is not cut by the cue");
    assert_eq!(buf[(39, y as u16)].symbol(), "e", "the cue ends at column W-4");
    let width = format!("▼ {n} more").chars().count() as u16;
    assert!((40 - width..40).all(|x| crate::palette::check::is_muted(&buf[(x, y as u16)])), "and is dim");
}

#[test]
fn a_scrolled_inbox_has_both_cues_right_aligned_and_every_row_of_the_block_keeps_its_line() {
    let mut view = expanded(50);
    let got = rows(&draw(&mut view, 43, 40));
    let title = find(&got, "Inbox");
    for _ in 0..2 {
        assert_eq!(pointer(&mut view, MouseEventKind::ScrollDown, 10, 30), Effect::None);
    }
    let buf = draw(&mut view, 43, 40);
    let got = rows(&buf);
    let found = cues(&got);
    let [(up, above), (down, below)] = found[..] else { panic!("both cues: {got:#?}") };
    assert_eq!((up, down), ('▲', '▼'));
    assert_eq!(above, 6, "two notches of three items");
    assert_eq!(got[find(&got, "▲")], cue_row("▲", above));
    assert_eq!(got[find(&got, "▼")], cue_row("▼", below));
    let close = find(&got, "collapse");
    for y in title + 1..close {
        assert_eq!((buf[(1, y as u16)].symbol(), buf[(1, y as u16)].fg), ("│", muted()), "row {y} keeps the line: {:?}", got[y]);
    }
    assert_eq!(got[close], format!(" ╰{UP} collapse"), "the closing row stays in view");
}

// ---------- the Inbox's tier glyphs (hs-d483): red until the rows draw them ----------

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

/// A view whose Inbox holds the one item `kind` and `urgency`, drawn in 43 columns: its buffer and the row of its label.
fn one_item(kind: Kind, urgency: Urgency) -> (Buffer, u16) {
    let mut view = with_inbox(vec![item(1, kind, urgency, "the label", "2026-01-02T23:59:18.000Z")]);
    let buf = draw(&mut view, 43, 24);
    let y = find(&rows(&buf), "the label") as u16;
    (buf, y)
}

#[test]
fn an_item_row_is_the_muted_line_one_space_its_tier_glyph_in_its_colour_a_space_and_the_text() {
    for (urgency, glyph, colour) in tiers() {
        let (buf, y) = one_item(Kind::Unread, urgency);
        assert_eq!((buf[(1, y)].symbol(), buf[(1, y)].fg), ("│", muted()), "{urgency:?}: the line");
        assert_eq!((buf[(3, y)].symbol(), buf[(3, y)].fg), (glyph, colour), "{urgency:?}: the tier glyph in its colour");
        assert_eq!(buf[(2, y)].symbol(), " ", "{urgency:?}: one space after the line");
        assert_eq!(buf[(4, y)].symbol(), " ", "{urgency:?}");
        assert_eq!(buf[(5, y)].symbol(), "t", "{urgency:?}: the text starts at column 5");
        assert_ne!(buf[(5, y)].fg, muted(), "{urgency:?}: the text is not muted");
        assert_eq!(buf[(38, y)].fg, muted(), "{urgency:?}: the age");
        assert_eq!(rows(&buf)[y as usize], format!(" │ {glyph} the label{}42s", " ".repeat(39 - 4 - "the label".len() - 3)), "{urgency:?}");
    }
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
fn the_glyph_is_its_urgencys_never_its_kinds() {
    let kinds = [
        Kind::Dialog,
        Kind::Unread,
        Kind::EngineError,
        Kind::EngineDown,
        Kind::JobStuck,
        Kind::Question,
        Kind::PlanReview,
        Kind::PrReady,
        Kind::PrFailing,
        Kind::PrChanges,
        Kind::ReviewRequested,
    ];
    for (urgency, glyph, colour) in tiers() {
        for kind in kinds {
            let (buf, y) = one_item(kind, urgency);
            assert_eq!((buf[(3, y)].symbol(), buf[(3, y)].fg), (glyph, colour), "{kind:?} at {urgency:?}");
        }
    }
}

#[test]
fn no_inbox_row_draws_a_state_dot() {
    let items = inbox_items(4);
    let mut view = with_inbox(items);
    let got = rows(&draw(&mut view, 43, 24));
    let section = &got[find(&got, "f-blocked: Worker question")..find(&got, "engine: store busy") + 1];
    assert_eq!(section.len(), 3, "{got:#?}");
    for row in section {
        for frame in SPINNER {
            assert!(!row.contains(frame), "{frame:?} is a state dot, reserved for the Context tree: {row:?}");
        }
    }
    for (urgency, _, _) in tiers() {
        let (buf, y) = one_item(Kind::Dialog, urgency);
        let row = &rows(&buf)[y as usize];
        assert!(SPINNER.iter().all(|frame| !row.contains(frame)), "{urgency:?}: {row:?}");
    }
}

#[test]
fn a_long_label_is_cut_with_an_ellipsis_before_the_age_and_after_the_tier_glyph() {
    let mut items = inbox_items(1);
    items[0].label = "x".repeat(60);
    let mut view = with_inbox(items);
    let got = rows(&draw(&mut view, 43, 24));
    // 41 columns: `│`, one space, the glyph and a space (4), the label, a space, `42s` (3): 31 for the label
    assert_eq!(got[find(&got, "xxx")], format!(" │ \u{F0362} {}… 42s", "x".repeat(30)));
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
    let mut with_three = with_inbox(inbox_items(3));
    let archived_empty = ("an archived Context with no row", draw(&mut empty, 44, 10), 3u16);
    let inbox = ("the Inbox with 3 items", draw(&mut with_three, 43, 22), 20u16);
    for (name, buf, y) in [archived_empty, inbox] {
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
