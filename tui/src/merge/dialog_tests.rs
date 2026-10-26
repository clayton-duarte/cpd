//! The merge popup is the dialog kit's confirm (hs-d621; commands.md §13.2, contract.md §6.1.1): one rounded frame,
//! 60 x 8, pill buttons. Coordinates are popup columns and rows, 0-based; every glyph and column is a literal.
//!
//! The seams these tests use, which the merge view must KEEP: `super::popup_args`, `super::draw`,
//! `super::on_mouse`, `super::Pull::parse` and `super::Action`.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::{Action, Pull, draw, on_key, on_mouse, popup_args};
use crate::palette;

fn render(width: u16, height: u16, title: Option<&str>) -> Buffer {
    let pull = Pull::parse("owner/repo#12").unwrap();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, Rect::new(0, 0, width, height), &pull, title)).unwrap();
    terminal.backend().buffer().clone()
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn click(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }
}

fn inside(text: &str, width: usize) -> String {
    format!("│{text:<width$}│")
}

#[test]
fn the_popup_is_the_kits_frame_60_by_8_without_a_tmux_border_or_title() {
    let args = popup_args("c0", (200, 50), "a/b#1");
    let has = |a: &str| args.iter().any(|x| x == a);
    let after = |flag: &str| args.iter().position(|x| x == flag).map(|i| args[i + 1].as_str());
    assert!(has("display-popup"), "{args:?}");
    assert!(has("-E"), "{args:?}");
    assert!(has("-B"), "-B (no tmux border): {args:?}");
    assert_eq!(after("-c"), Some("c0"), "{args:?}");
    assert_eq!(after("-w"), Some("60"), "{args:?}");
    assert_eq!(after("-h"), Some("8"), "{args:?}");
    assert!(!has("-T"), "no tmux title: {args:?}");
    assert!(!has(" merge "), "no tmux title text: {args:?}");
    assert_eq!(args.last().unwrap(), "cpd tui view merge 'a/b#1' --client 'c0'", "{args:?}");
}

#[test]
fn the_popup_is_capped_by_the_client_and_never_empty() {
    for (client, w, h) in [((30, 6), "28", "6"), ((1, 1), "1", "1"), ((62, 8), "60", "8"), ((61, 9), "59", "8")] {
        let args = popup_args("c0", client, "a/b#1");
        let after = |flag: &str| args.iter().position(|x| x == flag).map(|i| args[i + 1].clone());
        assert_eq!((after("-w"), after("-h")), (Some(w.to_owned()), Some(h.to_owned())), "client {client:?}: {args:?}");
    }
}

#[test]
fn the_dialog_is_one_rounded_frame_with_the_title_pill_the_pull_request_its_title_the_method_and_two_pill_buttons() {
    let buf = render(60, 8, Some("Fix the thing"));
    let top = format!("╭─\u{E0B6}merge\u{E0B4}{}╮", "─".repeat(60 - 2 - 7 - 1));
    assert_eq!(row(&buf, 0), top);
    assert_eq!(row(&buf, 1), inside("", 58));
    assert_eq!(row(&buf, 2), inside(" owner/repo#12", 58));
    assert_eq!(row(&buf, 3), inside(" Fix the thing", 58));
    assert_eq!(row(&buf, 4), inside(" method: squash", 58));
    assert_eq!(row(&buf, 5), inside("", 58));
    let buttons = format!("│ \u{E0B6}Merge\u{E0B4}{}\u{E0B6}Cancel\u{E0B4} │", " ".repeat(41));
    assert_eq!(row(&buf, 6), buttons);
    assert_eq!(row(&buf, 7), format!("╰{}╯", "─".repeat(58)));
    // text starts at column 2, the pull request in bold
    for x in 2..15 {
        assert!(buf[(x, 2)].modifier.contains(Modifier::BOLD), "bold at {x}");
    }
    // frame colour 8, the title pill is pill_static()
    for (x, y) in [(0, 0), (1, 0), (59, 0), (0, 1), (59, 1), (0, 6), (59, 6), (0, 7), (30, 7), (59, 7), (50, 0)] {
        assert_eq!(buf[(x, y)].fg, palette::muted_text().fg.unwrap(), "frame colour at ({x}, {y})");
    }
    let pill = palette::pill_static();
    for x in 2..=8 {
        assert_eq!((buf[(x, 0)].fg, buf[(x, 0)].bg), (pill.fg.unwrap(), pill.bg.unwrap()), "title pill at {x}");
    }
    for y in 0..8 {
        assert!(!row(&buf, y).contains("[Merge]"), "no plain-text button, row {y}");
    }
}

#[test]
fn a_missing_title_leaves_its_row_blank_and_a_long_one_is_cut_with_an_ellipsis() {
    let none = render(60, 8, None);
    assert_eq!(row(&none, 3), inside("", 58));
    assert_eq!(row(&none, 2), inside(" owner/repo#12", 58));
    assert_eq!(row(&none, 4), row(&render(60, 8, Some("x")), 4));
    assert_eq!(row(&none, 4), inside(" method: squash", 58));
    let long = "a".repeat(100);
    let buf = render(60, 8, Some(&long));
    let r = row(&buf, 3);
    let cells: Vec<char> = r.chars().collect();
    assert_eq!(cells.len(), 60, "{r}");
    assert_eq!(cells[59], '│', "{r}");
    assert_eq!(cells[0], '│', "{r}");
    let content: String = cells[2..58].iter().collect();
    assert_eq!(content, format!("{}…", "a".repeat(55)), "{r}");
    assert_eq!(cells[1], ' ', "{r}");
}

#[test]
fn a_left_click_on_a_pill_acts_and_nothing_else_does() {
    for (x0, y0) in [(0u16, 0u16), (5, 3)] {
        let area = Rect::new(x0, y0, 60, 8);
        let at = |kind, c: u16, r: u16| on_mouse(area, click(kind, x0 + c, y0 + r));
        let l = MouseEventKind::Down(MouseButton::Left);
        for c in [2, 8] {
            assert_eq!(at(l, c, 6), Some(Action::Merge), "merge at {c} (origin {x0},{y0})");
        }
        for c in [50, 57] {
            assert_eq!(at(l, c, 6), Some(Action::Cancel), "cancel at {c} (origin {x0},{y0})");
        }
        for (c, r) in [(9, 6), (49, 6), (0, 6), (59, 6), (3, 5), (3, 7)] {
            assert_eq!(at(l, c, r), None, "({c}, {r}) (origin {x0},{y0})");
        }
        assert_eq!(at(MouseEventKind::Down(MouseButton::Right), 3, 6), None);
        assert_eq!(at(MouseEventKind::Up(MouseButton::Left), 3, 6), None);
    }
}

#[test]
fn a_dialog_in_a_tiny_area_draws_without_panicking() {
    let pull = Pull::parse("owner/repo#12").unwrap();
    for w in 0..=12u16 {
        for h in 0..=9u16 {
            let mut terminal = Terminal::new(TestBackend::new(w.max(1), h.max(1))).unwrap();
            terminal.draw(|f| draw(f, Rect::new(0, 0, w, h), &pull, Some("Fix the thing"))).unwrap();
        }
    }
    // and at exactly 60 x 8 the kit's drawing is the one drawn
    let buf = render(60, 8, Some("Fix the thing"));
    assert_eq!(row(&buf, 7), format!("╰{}╯", "─".repeat(58)));
    assert!(row(&buf, 6).contains("\u{E0B6}Merge\u{E0B4}"), "{}", row(&buf, 6));
}

// ---------- the view's pane tests of `cli/tests/tui/tui_merge.rs`, as units (the pane only forwards keys and clicks to
// `on_key` and `on_mouse` and draws with `draw`; what a real tmux made of it is tmux's) ----------

const REF: &str = "o/r#7";
/// The client the view was opened on, for the tests that have none attached.
const CLIENT: &str = "/dev/pts/9";

/// A pill: its label between the caps U+E0B6 and U+E0B4.
fn pill(label: &str) -> String {
    format!("\u{E0B6}{label}\u{E0B4}")
}

/// The dialog of `reference` as the pane shows it, 60 x 8 (the dialog draws its own frame): the rows, without their trailing blanks.
fn screen(reference: &str, title: Option<&str>) -> Vec<String> {
    let pull = Pull::parse(reference).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
    terminal.draw(|f| draw(f, Rect::new(0, 0, 60, 8), &pull, title)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..8).map(|y| row(&buf, y).trim_end().to_owned()).collect()
}

#[test]
fn the_dialog_shows_the_pull_request_its_title_the_method_and_both_buttons() {
    // `draw` takes no way to run anything, so the old pane test's "opening the dialog runs nothing" (no `gh` call) holds by its signature
    let rows = screen(REF, Some("Add the thing"));
    assert_eq!(rows[2..5], [inside(" o/r#7", 58), inside(" Add the thing", 58), inside(" method: squash", 58)]);
    assert!(rows[6].starts_with(&format!("│ {}", pill("Merge"))) && rows[6].ends_with(&format!("{} │", pill("Cancel"))), "{rows:?}");
}

#[test]
fn n_q_and_esc_close_it_and_start_nothing() {
    // the view starts `run-shell` only on `Action::Merge` (`run`), so a key that gives `Cancel` closes it and starts nothing
    for code in [KeyCode::Char('n'), KeyCode::Char('q'), KeyCode::Esc] {
        assert_eq!(on_key(KeyEvent::new(code, KeyModifiers::NONE)), Some(Action::Cancel), "{code:?}");
    }
}

#[test]
fn a_click_on_cancel_closes_it_and_starts_nothing() {
    // the Cancel pill is at 0-based columns 50 to 57, row 6: the pane's 1-based (54, 7)
    let at = |column, row| on_mouse(Rect::new(0, 0, 60, 8), click(MouseEventKind::Down(MouseButton::Left), column, row));
    assert_eq!(at(53, 6), Some(Action::Cancel));
    assert_ne!(at(53, 6), Some(Action::Merge), "a click on Cancel starts nothing");
}

#[test]
fn the_popup_opens_on_the_named_client_and_its_command_reaches_cpd_unchanged() {
    // commands.md §13.2: `display-popup -E -B -c <name> -w <width> -h <height> -- cpd tui view merge <ref> --client <name>`
    let args = popup_args(CLIENT, (200, 60), REF);
    let (command, flags) = args.split_last().unwrap();
    assert_eq!(flags, ["display-popup", "-E", "-B", "-c", CLIENT, "-w", "60", "-h", "8", "--"]);
    // the string the popup's shell splits into `cpd`, `tui`, `view`, `merge`, `o/r#7`, `--client`, `/dev/pts/9`: each argument in single quotes
    assert_eq!(command, "cpd tui view merge 'o/r#7' --client '/dev/pts/9'");
    // at most the client's size
    let small = popup_args(CLIENT, (50, 6), REF);
    let after = |flag: &str| small.iter().position(|a| a == flag).map(|i| small[i + 1].as_str());
    assert_eq!((after("-w"), after("-h")), (Some("48"), Some("6")), "{small:?}");
}
